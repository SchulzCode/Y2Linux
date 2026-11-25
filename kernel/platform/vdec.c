// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 hardware video decoder of the Innioasis Y2: /dev/y2-vdec.
 *
 * Power, clocks, memory and the one-picture-at-a-time contract of y2-vdec.h. Base, interrupt, clock
 * and power sequence are the stock FM kernel's (videocodec driver, vdec_power_on,
 * spm_mtcmos_ctrl_vdec; docs/knowledge/video-decoder.md). The decoder is powered only while pictures
 * are being decoded: it goes off 200 ms after the last one, on close and before every system sleep,
 * and every picture programs the hardware from reset, so nothing is lost with the power. */
#include <linux/clk.h>
#include <linux/debugfs.h>
#include <linux/dma-mapping.h>
#include <linux/fs.h>
#include <linux/interrupt.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/platform_device.h>
#include <linux/pm_runtime.h>
#include <linux/seq_file.h>
#include <linux/uaccess.h>
#include "clocks.h"
#include "spm.h"
#include "vdec.h"

#define VDEC_IDLE_MS 200
#define VDEC_PICTURE_MS 500
/* SYS block: clock enable set/clear for the decoder and for its SMI larb. */
#define SYS_CKEN_SET 0x0
#define SYS_CKEN_CLR 0x4
#define SYS_LARB_SET 0x8
#define SYS_LARB_CLR 0xc
#define MISC_IRQ 0xa4

static irqreturn_t y2_vdec_irq(int irq, void *data)
{
	struct y2_vdec *v = data;
	u32 status = vdec_read(v, VDEC_MISC(MISC_IRQ));
	if (!(status & BIT(16))) return IRQ_NONE;
	/* Stock dec_isr: acknowledge by raising bits 0 and 4, then dropping bit 4. */
	vdec_write(v, VDEC_MISC(MISC_IRQ), status | 0x11);
	vdec_write(v, VDEC_MISC(MISC_IRQ), vdec_read(v, VDEC_MISC(MISC_IRQ)) & ~0x10U);
	complete(&v->done);
	return IRQ_HANDLED;
}

static void y2_vdec_power_off(struct y2_vdec *v)
{
	if (!v->powered) return;
	disable_irq(v->irq);
	writel(1, v->sys + SYS_CKEN_CLR);
	writel(1, v->sys + SYS_LARB_CLR);
	if (y2_spm_vde_power(0)) v->broken = true;
	y2_ccf_vdec_source(false, NULL);
	clk_disable_unprepare(v->bus);
	pm_runtime_put(v->dev);
	v->powered = false;
}

/* Stock order: SMI common, the domain, then the decoder's own two gates. */
static int y2_vdec_power_on(struct y2_vdec *v)
{
	int ret;
	if (v->powered) return 0;
	if (v->broken) return -EIO;
	ret = pm_runtime_resume_and_get(v->dev);
	if (ret) return ret;
	ret = clk_prepare_enable(v->bus);
	if (ret) goto put;
	ret = y2_ccf_vdec_source(true, &v->last_mux);
	if (ret) goto bus;
	ret = y2_spm_vde_power(1);
	if (ret) { v->broken = true; goto source; }
	writel(1, v->sys + SYS_CKEN_SET);
	writel(1, v->sys + SYS_LARB_SET);
	v->powered = true;
	v->power_ons++;
	enable_irq(v->irq);
	return 0;
source:
	y2_ccf_vdec_source(false, NULL);
bus:
	clk_disable_unprepare(v->bus);
put:
	pm_runtime_put(v->dev);
	return ret;
}

static void y2_vdec_idle(struct work_struct *work)
{
	struct y2_vdec *v = container_of(work, struct y2_vdec, idle.work);
	mutex_lock(&v->lock);
	if (!v->debug_hold) y2_vdec_power_off(v);
	mutex_unlock(&v->lock);
}

static void y2_vdec_buf_free(struct y2_vdec *v, struct y2_vdec_buf *b)
{
	if (b->page) dma_free_pages(v->dev, b->size, b->page, b->dma, DMA_BIDIRECTIONAL);
	*b = (struct y2_vdec_buf){ 0 };
}

/* Contiguous, cached, zeroed and written back, so that no stale cache line lands on what the
 * hardware writes later. */
static int y2_vdec_buf_alloc(struct y2_vdec *v, struct y2_vdec_buf *b, size_t size)
{
	b->size = PAGE_ALIGN(size);
	b->page = dma_alloc_pages(v->dev, b->size, &b->dma, DMA_BIDIRECTIONAL,
				  GFP_KERNEL | __GFP_NOWARN | __GFP_RETRY_MAYFAIL);
	if (!b->page) { b->size = 0; return -ENOMEM; }
	b->va = page_address(b->page);
	memset(b->va, 0, b->size);
	dma_sync_single_for_device(v->dev, b->dma, b->size, DMA_BIDIRECTIONAL);
	return 0;
}

static void y2_vdec_release_buffers(struct y2_vdec *v)
{
	unsigned i;
	for (i = 0; i < Y2_VDEC_MAX_FRAMES; i++) {
		y2_vdec_buf_free(v, &v->frame[i]);
		y2_vdec_buf_free(v, &v->mv[i]);
	}
	y2_vdec_buf_free(v, &v->bits);
	y2_vdec_buf_free(v, &v->work);
	y2_vdec_buf_free(v, &v->work_a);
	y2_vdec_buf_free(v, &v->work_b);
	v->cfg.frames = 0;
	v->last_y = 0;
}

static int y2_vdec_configure(struct y2_vdec *v, struct y2_vdec_config *c)
{
	bool mpeg4 = c->codec == Y2_VDEC_CODEC_MPEG4;
	unsigned i, tall, vectors;
	int ret;
	/* The stock library's limits: 64..1920x1088 luma samples. */
	if ((c->codec != Y2_VDEC_CODEC_H264 && !mpeg4) || !c->frames || c->frames > Y2_VDEC_MAX_FRAMES ||
	    (c->width | c->height) & 15 || c->width < 64 || c->height < 64 || c->width > 1920 ||
	    c->height > 1920 || c->width * c->height > 1920 * 1088)
		return -EINVAL;
	/* A mapping of the old buffers would outlive them. */
	if (atomic_read(&v->maps)) {
		dev_warn(v->dev, "configure refused: buffers of the last stream are still mapped\n");
		return -EBUSY;
	}
	y2_vdec_power_off(v);
	y2_vdec_release_buffers(v);
	v->mbs_w = c->width / 16;
	v->mbs_h = c->height / 16;
	tall = ALIGN(c->height, 32);
	c->y_size = v->mbs_w * (tall / 32) * 512;
	c->c_size = ALIGN(c->y_size / 2, 512);
	c->bitstream_size = VDEC_BITSTREAM_SIZE;
	c->reserved = 0;
	vectors = v->mbs_w * (tall / 16) * 64;
	ret = y2_vdec_buf_alloc(v, &v->bits, VDEC_BITSTREAM_SIZE);
	if (!ret) ret = y2_vdec_buf_alloc(v, &v->work, mpeg4 ? VDEC_MPEG4_WORK_SIZE : VDEC_WORK_SIZE);
	if (!ret) ret = y2_vdec_buf_alloc(v, &v->work_a, VDEC_WORK_A_SIZE);
	if (!ret) ret = y2_vdec_buf_alloc(v, &v->work_b, VDEC_WORK_B_SIZE);
	for (i = 0; !ret && i < c->frames; i++) {
		ret = y2_vdec_buf_alloc(v, &v->frame[i], c->y_size + c->c_size);
		if (!ret && (!mpeg4 || i < 2)) ret = y2_vdec_buf_alloc(v, &v->mv[i], vectors);
	}
	if (!ret && mpeg4 && c->frames < 2) ret = y2_vdec_buf_alloc(v, &v->mv[1], vectors);
	if (ret) {
		/* Y2Linux #145: a film fell back to software and the report could not say why. */
		dev_warn(v->dev, "configure %ux%u, %u frames of %u bytes: %d after %u frames\n",
			 c->width, c->height, c->frames, c->y_size + c->c_size, ret, i);
		y2_vdec_release_buffers(v);
		return ret;
	}
	v->cfg = *c;
	return 0;
}

/* Before a picture: power, and what the caller wrote, with `tail` and zeros behind it, goes out to
 * memory. The frame needs nothing: nobody can write its mapping, so no cached line of it is waiting
 * to land on the picture. */
static int y2_vdec_begin(struct y2_vdec *v, u32 size, const u8 *tail, unsigned tail_size,
			 unsigned reach)
{
	int ret;
	if (!v->cfg.frames || size > v->bits.size - reach) return -EINVAL;
	cancel_delayed_work(&v->idle);
	ret = y2_vdec_power_on(v);
	if (ret) return ret;
	memset(v->bits.va + size, 0, reach);
	memcpy(v->bits.va + size, tail, tail_size);
	dma_sync_single_for_device(v->dev, v->bits.dma, PAGE_ALIGN(size + reach), DMA_TO_DEVICE);
	reinit_completion(&v->done);
	return 0;
}

/* After the start: wait for the interrupt. True when the hardware never answered. */
static bool y2_vdec_wait(struct y2_vdec *v, int started)
{
	if (started || wait_for_completion_timeout(&v->done, msecs_to_jiffies(VDEC_PICTURE_MS)))
		return false;
	v->timeouts++;
	return true;
}

/* A picture the hardware never finished is reported in the result, not as a failed call: the caller
 * shows or conceals it and goes on. A dead decoder is switched off and starts from reset. */
static void y2_vdec_end(struct y2_vdec *v, unsigned frame, bool silent, u32 *error, u32 *complete)
{
	dma_sync_single_for_cpu(v->dev, v->frame[frame].dma, v->frame[frame].size, DMA_FROM_DEVICE);
	v->pictures++;
	if (silent) {
		y2_vdec_power_off(v);
		*complete = 0;
		if (!*error) *error = ~0U;
	}
	if (*error || !*complete) { v->errors++; v->last_error = *error; }
	schedule_delayed_work(&v->idle, msecs_to_jiffies(VDEC_IDLE_MS));
}

static int y2_vdec_h264(struct y2_vdec *v, struct y2_vdec_h264 *p)
{
	/* The picture ends with a start code; the hardware is told about 0x100 bytes more. */
	static const u8 tail[4] = { 0, 0, 0, 1 };
	bool silent;
	int ret;
	if (p->frame >= v->cfg.frames) return -EINVAL;
	ret = y2_vdec_begin(v, p->size, tail, sizeof(tail), VDEC_BITSTREAM_TAIL);
	if (ret) return ret;
	ret = y2_vdec_h264_start(v, p);
	silent = y2_vdec_wait(v, ret);
	y2_vdec_h264_result(v, p);
	y2_vdec_end(v, p->frame, silent, &p->error, &p->complete);
	return ret;
}

static int y2_vdec_mpeg4(struct y2_vdec *v, struct y2_vdec_mpeg4 *p)
{
	/* The stock library ends the picture with three VOP start codes. */
	static const u8 tail[12] = { 0, 0, 1, 0xb6, 0, 0, 1, 0xb6, 0, 0, 1, 0xb6 };
	bool silent;
	int ret;
	if (p->frame >= v->cfg.frames) return -EINVAL;
	ret = y2_vdec_begin(v, p->size, tail, sizeof(tail), VDEC_MPEG4_TAIL);
	if (ret) return ret;
	ret = y2_vdec_mpeg4_start(v, p);
	silent = y2_vdec_wait(v, ret);
	y2_vdec_mpeg4_result(v, p);
	y2_vdec_end(v, p->frame, silent, &p->error, &p->complete);
	return ret;
}

static long y2_vdec_ioctl(struct file *file, unsigned int cmd, unsigned long arg)
{
	struct y2_vdec *v = container_of(file->private_data, struct y2_vdec, misc);
	void __user *user = (void __user *)arg;
	long ret;
	if (cmd == Y2_VDEC_CONFIGURE) {
		struct y2_vdec_config c;
		if (copy_from_user(&c, user, sizeof(c))) return -EFAULT;
		mutex_lock(&v->lock);
		ret = y2_vdec_configure(v, &c);
		mutex_unlock(&v->lock);
		if (!ret && copy_to_user(user, &c, sizeof(c))) ret = -EFAULT;
		return ret;
	}
	if (cmd == Y2_VDEC_H264_DECODE || cmd == Y2_VDEC_MPEG4_DECODE) {
		size_t size = _IOC_SIZE(cmd);
		void *p = memdup_user(user, size);
		if (IS_ERR(p)) return PTR_ERR(p);
		mutex_lock(&v->lock);
		ret = cmd == Y2_VDEC_H264_DECODE ? y2_vdec_h264(v, p) : y2_vdec_mpeg4(v, p);
		mutex_unlock(&v->lock);
		if (!ret && copy_to_user(user, p, size)) ret = -EFAULT;
		kfree(p);
		return ret;
	}
	return -ENOTTY;
}

static void y2_vdec_vm_open(struct vm_area_struct *vma)
{
	atomic_inc(&((struct y2_vdec *)vma->vm_private_data)->maps);
}
static void y2_vdec_vm_close(struct vm_area_struct *vma)
{
	atomic_dec(&((struct y2_vdec *)vma->vm_private_data)->maps);
}
static const struct vm_operations_struct y2_vdec_vm_ops = {
	.open = y2_vdec_vm_open, .close = y2_vdec_vm_close,
};

/* The bitstream buffer is the caller's to write; frames are read only. A mapping stays valid until
 * the buffers are sized again, which the single owner does only between streams. */
static int y2_vdec_mmap(struct file *file, struct vm_area_struct *vma)
{
	struct y2_vdec *v = container_of(file->private_data, struct y2_vdec, misc);
	unsigned long which = vma->vm_pgoff, size = vma->vm_end - vma->vm_start;
	struct y2_vdec_buf *b;
	int ret = -EINVAL;
	mutex_lock(&v->lock);
	if (!v->cfg.frames || which > v->cfg.frames) goto out;
	b = which == Y2_VDEC_MAP_BITSTREAM ? &v->bits : &v->frame[which - 1];
	if (size > b->size) goto out;
	if (which != Y2_VDEC_MAP_BITSTREAM) {
		if (vma->vm_flags & VM_WRITE) { ret = -EPERM; goto out; }
		vm_flags_clear(vma, VM_MAYWRITE);
	}
	vma->vm_pgoff = 0;
	ret = dma_mmap_pages(v->dev, vma, size, b->page);
	if (!ret) {
		vma->vm_private_data = v;
		vma->vm_ops = &y2_vdec_vm_ops;
		atomic_inc(&v->maps);
	}
out:
	mutex_unlock(&v->lock);
	return ret;
}

static int y2_vdec_open(struct inode *inode, struct file *file)
{
	struct y2_vdec *v = container_of(file->private_data, struct y2_vdec, misc);
	if (test_and_set_bit(0, &v->opened)) {
		dev_warn(v->dev, "open refused: the decoder is already in use\n");
		return -EBUSY;
	}
	return 0;
}

static int y2_vdec_release(struct inode *inode, struct file *file)
{
	struct y2_vdec *v = container_of(file->private_data, struct y2_vdec, misc);
	cancel_delayed_work_sync(&v->idle);
	mutex_lock(&v->lock);
	y2_vdec_power_off(v);
	y2_vdec_release_buffers(v);
	mutex_unlock(&v->lock);
	clear_bit(0, &v->opened);
	return 0;
}

static const struct file_operations y2_vdec_fops = {
	.owner = THIS_MODULE,
	.open = y2_vdec_open,
	.release = y2_vdec_release,
	.unlocked_ioctl = y2_vdec_ioctl,
	.mmap = y2_vdec_mmap,
};

static ssize_t state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_vdec *v = dev_get_drvdata(dev);
	return sysfs_emit(buf,
		"open=%d powered=%d broken=%d size=%ux%u frames=%u pictures=%u errors=%u timeouts=%u power_ons=%u last_error=%#x mux=%#x domain=%d\n",
		test_bit(0, &v->opened), v->powered, v->broken, v->cfg.frames ? v->cfg.width : 0,
		v->cfg.frames ? v->cfg.height : 0, v->cfg.frames, v->pictures, v->errors, v->timeouts,
		v->power_ons, v->last_error, v->last_mux, y2_spm_vde_status());
}
static DEVICE_ATTR_RO(state);
static struct attribute *y2_vdec_attrs[] = { &dev_attr_state.attr, NULL };
ATTRIBUTE_GROUPS(y2_vdec);

/* Bring-up aid (debugfs, root): `power` switches the decoder by hand, `reg` reads and writes one
 * register of a powered decoder: "<offset>" selects, "<offset> <value>" writes. Offsets are those of
 * the decoder block; 0x100000 + n is the SYS block. Not used by the product. */
static int y2_vdec_debug_power_set(void *data, u64 on)
{
	struct y2_vdec *v = data;
	int ret = 0;
	cancel_delayed_work_sync(&v->idle);
	mutex_lock(&v->lock);
	v->debug_hold = on;
	if (on) ret = y2_vdec_power_on(v);
	else y2_vdec_power_off(v);
	mutex_unlock(&v->lock);
	return ret;
}
static int y2_vdec_debug_power_get(void *data, u64 *on)
{
	*on = ((struct y2_vdec *)data)->powered;
	return 0;
}
DEFINE_DEBUGFS_ATTRIBUTE(y2_vdec_debug_power, y2_vdec_debug_power_get, y2_vdec_debug_power_set,
			 "%llu\n");

static void __iomem *y2_vdec_debug_address(struct y2_vdec *v)
{
	u32 at = v->debug_reg & ~3U;
	if (at >= 0x100000) return at - 0x100000 < 0x1000 ? v->sys + (at - 0x100000) : NULL;
	return at < 0x9000 ? v->regs + at : NULL;
}
static ssize_t y2_vdec_debug_reg_read(struct file *file, char __user *to, size_t n, loff_t *pos)
{
	struct y2_vdec *v = file->private_data;
	void __iomem *at;
	char text[32];
	int len = -ENODEV;
	mutex_lock(&v->lock);
	at = y2_vdec_debug_address(v);
	if (v->powered && at) len = scnprintf(text, sizeof(text), "%#x %#x\n", v->debug_reg, readl(at));
	mutex_unlock(&v->lock);
	return len < 0 ? len : simple_read_from_buffer(to, n, pos, text, len);
}
static ssize_t y2_vdec_debug_reg_write(struct file *file, const char __user *from, size_t n,
				       loff_t *pos)
{
	struct y2_vdec *v = file->private_data;
	void __iomem *at;
	char text[32] = { 0 };
	u32 reg, value;
	int fields;
	if (n >= sizeof(text) || copy_from_user(text, from, n)) return -EINVAL;
	fields = sscanf(text, "%x %x", &reg, &value);
	if (fields < 1) return -EINVAL;
	mutex_lock(&v->lock);
	v->debug_reg = reg;
	at = y2_vdec_debug_address(v);
	if (fields == 2 && v->powered && at) writel(value, at);
	else if (fields == 2) n = -ENODEV;
	mutex_unlock(&v->lock);
	return n;
}
/* Where the buffers are, and what the last picture's start read back. */
static int y2_vdec_debug_buffers_show(struct seq_file *m, void *unused)
{
	struct y2_vdec *v = m->private;
	unsigned i;
	mutex_lock(&v->lock);
	seq_printf(m, "bits %pad %zu\nwork %pad\nwork_a %pad\nwork_b %pad\n", &v->bits.dma,
		   v->bits.size, &v->work.dma, &v->work_a.dma, &v->work_b.dma);
	for (i = 0; i < v->cfg.frames; i++)
		seq_printf(m, "frame %u %pad mv %pad %zu\n", i, &v->frame[i].dma, &v->mv[i].dma,
			   v->mv[i].size);
	seq_printf(m, "trace %#x %#x %#x %#x %#x %#x %#x %#x\n", v->trace[0], v->trace[1],
		   v->trace[2], v->trace[3], v->trace[4], v->trace[5], v->trace[6], v->trace[7]);
	mutex_unlock(&v->lock);
	return 0;
}
DEFINE_SHOW_ATTRIBUTE(y2_vdec_debug_buffers);

static const struct file_operations y2_vdec_debug_reg = {
	.owner = THIS_MODULE, .open = simple_open, .read = y2_vdec_debug_reg_read,
	.write = y2_vdec_debug_reg_write,
};

static int y2_vdec_probe(struct platform_device *pdev)
{
	struct device *dev = &pdev->dev;
	struct y2_vdec *v = devm_kzalloc(dev, sizeof(*v), GFP_KERNEL);
	int ret;
	if (!v) return -ENOMEM;
	v->dev = dev;
	mutex_init(&v->lock);
	init_completion(&v->done);
	INIT_DELAYED_WORK(&v->idle, y2_vdec_idle);
	v->sys = devm_platform_ioremap_resource(pdev, 0);
	if (IS_ERR(v->sys)) return PTR_ERR(v->sys);
	v->regs = devm_platform_ioremap_resource(pdev, 1);
	if (IS_ERR(v->regs)) return PTR_ERR(v->regs);
	v->bus = devm_clk_get(dev, "bus");
	if (IS_ERR(v->bus)) return PTR_ERR(v->bus);
	v->irq = platform_get_irq(pdev, 0);
	if (v->irq < 0) return v->irq;
	ret = dma_set_mask_and_coherent(dev, DMA_BIT_MASK(32));
	if (ret) return ret;
	/* The interrupt is enabled only while the domain is on: its status lives behind the domain. */
	ret = devm_request_irq(dev, v->irq, y2_vdec_irq, IRQF_NO_AUTOEN, "y2-vdec", v);
	if (ret) return ret;
	platform_set_drvdata(pdev, v);
	ret = devm_pm_runtime_enable(dev);
	if (ret) return ret;
	v->misc = (struct miscdevice){ .minor = MISC_DYNAMIC_MINOR, .name = "y2-vdec",
				       .fops = &y2_vdec_fops, .parent = dev, .mode = 0600 };
	ret = misc_register(&v->misc);
	if (ret) return ret;
	v->debug = debugfs_create_dir("y2_vdec", NULL);
	debugfs_create_file("power", 0600, v->debug, v, &y2_vdec_debug_power);
	debugfs_create_file("reg", 0600, v->debug, v, &y2_vdec_debug_reg);
	debugfs_create_file("buffers", 0400, v->debug, v, &y2_vdec_debug_buffers_fops);
	return 0;
}

static void y2_vdec_remove(struct platform_device *pdev)
{
	struct y2_vdec *v = platform_get_drvdata(pdev);
	debugfs_remove_recursive(v->debug);
	misc_deregister(&v->misc);
	cancel_delayed_work_sync(&v->idle);
	mutex_lock(&v->lock);
	y2_vdec_power_off(v);
	y2_vdec_release_buffers(v);
	mutex_unlock(&v->lock);
}

/* System sleep: the decoder is off before the SPM takes the domains down. Userspace has stopped
 * asking for pictures by then (tasks are frozen); the next picture powers it on again. */
static int y2_vdec_suspend(struct device *dev)
{
	struct y2_vdec *v = dev_get_drvdata(dev);
	cancel_delayed_work_sync(&v->idle);
	mutex_lock(&v->lock);
	y2_vdec_power_off(v);
	mutex_unlock(&v->lock);
	return 0;
}
static DEFINE_SIMPLE_DEV_PM_OPS(y2_vdec_pm, y2_vdec_suspend, NULL);

static const struct of_device_id y2_vdec_of[] = { { .compatible = "innioasis,y2-vdec" }, {} };
MODULE_DEVICE_TABLE(of, y2_vdec_of);
static struct platform_driver y2_vdec_driver = {
	.probe = y2_vdec_probe,
	.remove = y2_vdec_remove,
	.driver = { .name = "y2-vdec", .of_match_table = y2_vdec_of, .dev_groups = y2_vdec_groups,
		    .pm = pm_sleep_ptr(&y2_vdec_pm) },
};
module_platform_driver(y2_vdec_driver);
MODULE_DESCRIPTION("Innioasis Y2 MT6582 hardware video decoder");
MODULE_LICENSE("GPL");
