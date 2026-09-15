// SPDX-License-Identifier: GPL-2.0-only
/* Bounded MD1 calibration lifecycle. Protocol reconstructed from this Y2's
 * stock runtime builder and retained CCCI boot behavior. No cellular stack,
 * captured RAM replay, userspace MMIO, or writable factory partitions. */
#include <linux/fs.h>
#include <linux/bitmap.h>
#include <linux/firmware.h>
#include <linux/interrupt.h>
#include <linux/iopoll.h>
#include <linux/miscdevice.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include "conn.h"
#include "calibration.h"
#include "md-runtime.h"
#include "../spm.h"

struct y2_md {
	struct y2_conn *conn;
	struct miscdevice service;
	struct mutex packet_lock;
	wait_queue_head_t wait;
	atomic_t opened;
	void __iomem *ccif, *rom, *smem, *wdt, *key, *vector, *enable;
	int irq, error;
	bool active, request, delivered, replied;
	unsigned tx, rx, stage, served, generation, serial, packet_size, reply_size, opcode;
	unsigned last_fs_op;
	int last_fs_status;
	unsigned long ready_at, activity;
	DECLARE_BITMAP(handles,128);
	bool restore_seen, read_seen, close_seen;
	unsigned char packet[Y2_CAL_MESSAGE], reply[Y2_FS_STRIDE];
};
static int service_open(struct inode *inode, struct file *file)
{
	struct y2_md *m = container_of(file->private_data, struct y2_md, service);
	if (atomic_cmpxchg(&m->opened, 0, 1)) return -EBUSY;
	file->private_data = m;
	return nonseekable_open(inode, file);
}
static int service_close(struct inode *inode, struct file *file)
{
	struct y2_md *m = file->private_data;
	atomic_set(&m->opened, 0);
	if (READ_ONCE(m->active)) {
		WRITE_ONCE(m->error,-EPIPE); WRITE_ONCE(m->active,false);
	}
	wake_up_all(&m->wait);
	return 0;
}
static ssize_t service_read(struct file *file, char __user *user, size_t size, loff_t *pos)
{
	struct y2_md *m = file->private_data; int ret;
	if (size < Y2_CAL_MESSAGE) return -EMSGSIZE;
	ret = wait_event_interruptible(m->wait, READ_ONCE(m->request) && !READ_ONCE(m->delivered));
	if (ret) return ret;
	mutex_lock(&m->packet_lock);
	if (!m->request || m->delivered) ret = -EAGAIN;
	else if (copy_to_user(user, m->packet, m->packet_size)) ret = -EFAULT;
	else { m->delivered = true; ret = m->packet_size; }
	mutex_unlock(&m->packet_lock);
	return ret;
}
static ssize_t service_write(struct file *file, const char __user *user, size_t size, loff_t *pos)
{
	struct y2_md *m = file->private_data;
	struct y2_fs_packet parsed;
	unsigned char *data; int ret;
	if (size < Y2_CAL_HEADER + 8 || size > Y2_CAL_MESSAGE) return -EMSGSIZE;
	data = memdup_user(user, size);
	if (IS_ERR(data)) return PTR_ERR(data);
	mutex_lock(&m->packet_lock);
	if (!m->active || !m->request || !m->delivered || m->replied ||
	    y2_conn_le32(data) != Y2_CAL_VERSION ||
	    y2_conn_le32(data + 4) != m->generation ||
	    y2_conn_le32(data + 8) != m->serial ||
	    y2_conn_le32(data + 12) != size - Y2_CAL_HEADER ||
	    y2_fs_parse(data + Y2_CAL_HEADER, size - Y2_CAL_HEADER, &parsed) ||
	    parsed.op != (m->opcode | 0xffff0000U)) ret = -EPROTO;
	else {
		m->reply_size = size - Y2_CAL_HEADER;
		memcpy(m->reply, data + Y2_CAL_HEADER, m->reply_size);
		m->replied = true; ret = size;
	}
	mutex_unlock(&m->packet_lock);
	kfree_sensitive(data); wake_up_all(&m->wait);
	return ret;
}
static const struct file_operations service_ops = {
	.owner = THIS_MODULE, .open = service_open, .release = service_close,
	.read = service_read, .write = service_write,
};
static int md_send(struct y2_md *m, u32 address, u32 size, u32 channel, u32 index)
{
	u32 busy, bit = BIT(m->tx);
	int ret = readl_poll_timeout(m->ccif + 4, busy, !(busy & bit), 50, 100000);
	if (ret) return ret;
	writel(bit, m->ccif + 4);
	writel(address, m->ccif + 0x100 + m->tx*16);
	writel(size, m->ccif + 0x104 + m->tx*16);
	writel(channel, m->ccif + 0x108 + m->tx*16);
	writel(index, m->ccif + 0x10c + m->tx*16);
	wmb(); writel(m->tx, m->ccif + 0xc);
	m->tx = (m->tx + 1) & 7;
	return 0;
}
static int md_fs(struct y2_md *m, u32 address, u32 size, u32 index)
{
	struct y2_fs_packet parsed; int ret, used;
	if (index >= 5 || address != Y2_MD_VIEW + Y2_MD_FS_OFFSET + index*Y2_FS_STRIDE ||
	    size < 8 || size > Y2_FS_STRIDE || m->served >= 4096) return -EPROTO;
	mutex_lock(&m->packet_lock);
	memcpy_fromio(m->packet + Y2_CAL_HEADER, m->smem + Y2_MD_FS_OFFSET + index*Y2_FS_STRIDE, size);
	used = y2_fs_request_size(m->packet + Y2_CAL_HEADER, size, &parsed);
	if (used < 0 || parsed.op < 0x1001 || parsed.op > 0x1021) {
		/* Header structure only: never dump request contents or filenames. */
		dev_err(m->conn->dev, "MD FS framing rejected: bytes=%u known_op=%04x args=%u\n",
			size, parsed.op >= 0x1001 && parsed.op <= 0x1021 ? parsed.op : 0,
			min(parsed.count, (unsigned)Y2_FS_ARGS + 1));
		memzero_explicit(m->packet, sizeof(m->packet));
		mutex_unlock(&m->packet_lock); return -EPROTO;
	}
	size = used;
	m->opcode = parsed.op; m->serial++;
	y2_conn_put32(m->packet, Y2_CAL_VERSION);
	y2_conn_put32(m->packet + 4, m->generation);
	y2_conn_put32(m->packet + 8, m->serial);
	y2_conn_put32(m->packet + 12, size);
	m->packet_size = size + Y2_CAL_HEADER;
	m->delivered = false; m->replied = false; m->request = true;
	mutex_unlock(&m->packet_lock); wake_up_all(&m->wait);
	ret = wait_event_timeout(m->wait, READ_ONCE(m->replied) || !READ_ONCE(m->active) ||
				!atomic_read(&m->opened), msecs_to_jiffies(5000)) ? 0 : -ETIMEDOUT;
	mutex_lock(&m->packet_lock);
	if (!m->replied || !m->active) ret = ret ? ret : -EPIPE;
	if (!ret) {
		struct y2_fs_packet reply;
		int status;
		if (y2_fs_parse(m->reply,m->reply_size,&reply) || !reply.count || reply.args[0].size!=4) {
			ret=-EPROTO; goto replied;
		}
		status=(int)y2_conn_le32(reply.args[0].data);
		if (parsed.op==0x101c && !parsed.count && !status) m->restore_seen=true;
		if ((parsed.op==0x1001 || parsed.op==0x1011 || parsed.op==0x1012) && status>0) {
			if (status>=128 || test_and_set_bit(status,m->handles)) { ret=-EPROTO; goto replied; }
		}
		if ((parsed.op==0x1005 || parsed.op==0x1014) && !status && parsed.count && parsed.args[0].size==4) {
			unsigned handle=y2_conn_le32(parsed.args[0].data);
			if (handle>=128 || !test_and_clear_bit(handle,m->handles)) { ret=-EPROTO; goto replied; }
			m->close_seen=true;
		}
		if ((parsed.op==0x1006 || parsed.op==0x1017) && !status) bitmap_zero(m->handles,128);
		if (parsed.op==0x1003 && !status) m->read_seen=true;
		memcpy_toio(m->smem + Y2_MD_FS_OFFSET + index*Y2_FS_STRIDE, m->reply, m->reply_size);
		wmb(); ret = md_send(m, address, m->reply_size, 15, index);
		if (!ret) { m->last_fs_op = parsed.op; m->last_fs_status = status; }
	}
replied:
	m->request = false;
	memzero_explicit(m->packet, sizeof(m->packet));
	memzero_explicit(m->reply, sizeof(m->reply));
	mutex_unlock(&m->packet_lock);
	if (!ret) m->served++;
	return ret;
}
static irqreturn_t md_irq(int irq, void *arg)
{
	struct y2_md *m = arg;
	u32 pending = readl(m->ccif + 0x10) & 255;
	if (!pending) return IRQ_NONE;
	for (unsigned i = 0; i < 8; i++) {
		unsigned slot = m->rx; u32 a, n, ch, index; int ret = 0;
		m->rx = (m->rx + 1) & 7;
		if (!(pending & BIT(slot))) continue;
		a = readl(m->ccif + 0x180 + slot*16); n = readl(m->ccif + 0x184 + slot*16);
		ch = readl(m->ccif + 0x188 + slot*16); index = readl(m->ccif + 0x18c + slot*16);
		writel(BIT(slot), m->ccif + 0x14);
		if (!READ_ONCE(m->active) || READ_ONCE(m->error)) continue;
		if (ch == 0 && !n && !m->stage && index == 0x5555ffff) {
			const u32 tag[] = {Y2_MD_MAGIC,0x3536544d,0x31453238,0x20121001,Y2_MD_VIEW,280,Y2_MD_MAGIC};
			for (unsigned j = 0; j < ARRAY_SIZE(tag); j++) writel(tag[j], m->ccif + 0x140 + 4*j);
			ret = md_send(m, 0xffffffff, 0, 1, 0x5555ffff); m->stage = 1;
		} else if (ch == 0 && !n && m->stage == 1) {
			m->stage = 2; m->ready_at = jiffies;
		} else if (ch == 0 && a == 0xffffffff && n == 4 && index == 0x45584350) {
			/* On the control channel n is a message ID, not a byte count.
			 * MD_EX is a firmware exception, never successful readiness.
			 * Record operation/status only; no private arguments or names. */
			dev_err(m->conn->dev, "MD firmware exception: stage=%u FS=%u last_op=%04x last_status=%d\n",
				m->stage, m->served, m->last_fs_op, m->last_fs_status);
			ret = -EPROTO;
		} else if (ch == 14) ret = md_fs(m, a, n, index);
		else if ((ch == 23 || ch == 27 || ch == 31) && a == 0xffffffff && !n) { }
		else if (ch == 4 && a == 0xffffffff && n == 0xaf700000 && !index) { }
		else if (ch == 10 && a == 0xffffffff && !n && index == 1) { }
		else ret = -EPROTO;
		WRITE_ONCE(m->activity, jiffies);
		if (ret) {
			dev_err(m->conn->dev, "MD message failed: stage=%u FS=%u channel=%u bytes=%u slot=%u buffer=%u error=%d\n",
				m->stage, m->served, ch, n, slot, index, ret);
			WRITE_ONCE(m->error, ret);
		}
		wake_up_all(&m->wait);
	}
	return IRQ_HANDLED;
}
int y2_md_calibrate(struct y2_conn *c)
{
	struct y2_md *m = c->md;
	const struct firmware *fw;
	unsigned char runtime[280], misc[288]; unsigned rtc;
	unsigned long deadline; int ret, stopped;
	if (!atomic_read(&m->opened)) return -ENXIO;
	ret = y2_spm_radio_status(1);
	if (ret < 0) return ret;
	/* Normalize only the explicitly unstarted LK state. Any inherited
	 * running firmware needs the ordinary acknowledged isolation path. */
	if (ret && !c->md_owned && y2_ccf_md_unconfigured() == 1 &&
	    !readl(m->enable) && !readl(m->ccif) && !readl(m->ccif+4) &&
	    !readl(m->ccif+0x10)) ret = y2_spm_unstarted_md_off();
	else ret = y2_spm_radio_power(1, 0);
	if (ret) return ret;
	c->md_owned = false;
	ret = y2_conn_request_firmware(c->dev, Y2_CONN_FW "modem_1_2g_n.img", &fw);
	if (ret) return ret;
	if (fw->size != 2424376) { ret = -ENOEXEC; goto release; }
	ret = regmap_read(c->pmic, 0x8030, &rtc);
	if (ret) goto release;
	memset_io(m->rom, 0, Y2_MD_ROM_SIZE);
	memcpy_toio(m->rom, fw->data, fw->size);
	memset_io(m->smem, 0, Y2_MD_SMEM_SIZE);
	y2_md_runtime(runtime, misc, rtc & BIT(6));
	memcpy_toio(m->smem, runtime, sizeof(runtime));
	memcpy_toio(m->smem + 0x924, misc, sizeof(misc));
	wmb();
	ret = y2_ccf_radio_remap(1);
release:
	release_firmware(fw);
	if (ret) return ret;
	writel(1, m->ccif); writel(255, m->ccif + 0x14);
	memset_io(m->ccif + 0x100, 0, 0x100);
	m->generation++; m->serial = m->tx = m->rx = m->stage = m->served = 0;
	m->last_fs_op = 0; m->last_fs_status = 0;
	bitmap_zero(m->handles,128); m->restore_seen=m->read_seen=m->close_seen=false;
	m->error = 0; m->request = false; m->activity = jiffies;
	WRITE_ONCE(m->active, true);
	c->md_owned = true; /* Also covers a partial power-on failure. */
	ret = y2_spm_radio_power(1, 1);
	if (ret) goto stop;
	enable_irq(m->irq);
	writel(0x2200, m->wdt); writel(0x3567c766, m->key);
	writel(0, m->vector); writel(0xa3b66175, m->enable);
	deadline = jiffies + msecs_to_jiffies(90000);
	ret = -ETIMEDOUT;
	while (time_before(jiffies, deadline)) {
		if (READ_ONCE(m->error)) { ret = READ_ONCE(m->error); break; }
		if (!atomic_read(&m->opened)) { ret = -EPIPE; break; }
		if (READ_ONCE(m->stage) == 2 && READ_ONCE(m->restore_seen) &&
		    READ_ONCE(m->read_seen) && READ_ONCE(m->close_seen) && bitmap_empty(m->handles,128) &&
		    time_after_eq(jiffies, READ_ONCE(m->ready_at) + msecs_to_jiffies(5000)) &&
		    time_after_eq(jiffies, READ_ONCE(m->activity) + msecs_to_jiffies(5000)) &&
		    !READ_ONCE(m->request) && !(readl(m->ccif + 0x10) & 255)) { ret = 0; break; }
		wait_event_timeout(m->wait, READ_ONCE(m->error), msecs_to_jiffies(100));
	}
	WRITE_ONCE(m->active, false); wake_up_all(&m->wait); disable_irq(m->irq);
stop:
	WRITE_ONCE(m->active, false);
	mutex_lock(&m->packet_lock);
	m->request = m->delivered = m->replied = false;
	memzero_explicit(m->packet, sizeof(m->packet));
	memzero_explicit(m->reply, sizeof(m->reply));
	mutex_unlock(&m->packet_lock);
	stopped = y2_spm_radio_power(1, 0);
	if (!stopped) c->md_owned = false;
	if (stopped) ret = stopped;
	dev_info(c->dev, "MD1 calibration boot: stage=%u FS=%u restore=%u open=%u result=%d poweroff=%d\n",
		 m->stage, m->served, m->restore_seen, bitmap_weight(m->handles,128), ret, stopped);
	return ret;
}
int y2_md_probe(struct platform_device *pdev, struct y2_conn *c)
{
	struct y2_md *m = devm_kzalloc(c->dev, sizeof(*m), GFP_KERNEL); int ret;
	if (!m) return -ENOMEM;
	m->conn = c; c->md = m; mutex_init(&m->packet_lock); init_waitqueue_head(&m->wait);
	const char *names[] = {"ccif","md-rom","md-smem","md-wdt","md-key","md-vector","md-enable"};
	void __iomem **maps[] = {&m->ccif,&m->rom,&m->smem,&m->wdt,&m->key,&m->vector,&m->enable};
	for (unsigned i = 0; i < ARRAY_SIZE(names); i++) {
		*maps[i] = devm_platform_ioremap_resource_byname(pdev, names[i]);
		if (IS_ERR(*maps[i])) return PTR_ERR(*maps[i]);
	}
	m->irq = platform_get_irq_byname(pdev, "ccif");
	if (m->irq < 0) return m->irq;
	ret = devm_request_threaded_irq(c->dev, m->irq, NULL, md_irq,
		IRQF_ONESHOT | IRQF_NO_AUTOEN, "y2-calibration", m);
	if (ret) return ret;
	m->service = (struct miscdevice){ .minor=MISC_DYNAMIC_MINOR, .name="y2-calibration",
		.fops=&service_ops, .parent=c->dev, .mode=0600 };
	return misc_register(&m->service);
}
void y2_md_unregister(struct y2_conn *c)
{
	/* The connectivity core is built-in, with bind/unbind suppressed. There
	 * is no module removal that could free DMA-visible memory or an open fd. */
	misc_deregister(&c->md->service);
}
void y2_md_abort(struct y2_conn *c)
{
	WRITE_ONCE(c->md->error,-ECANCELED);
	WRITE_ONCE(c->md->active,false);
	wake_up_all(&c->md->wait);
}
