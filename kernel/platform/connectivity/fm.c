// SPDX-License-Identifier: GPL-2.0-only
/* MT6627 FM receiver in the MT6582 CONSYS, on this owner's WMT/STP transport.
 * FM on = VCN28 (co-clock software mode) + WMT function 1 (stock
 * wmt_func_fm_on); every register access is an STP channel-1 packet. The
 * sequences are the stock Y2 kernel's (see fm-protocol.h); audio leaves the
 * chip as 32 kHz master I2S into the AFE, which captures it (mt6582-afe.c).
 * Interface: root-only sysfs group "fm" on the connectivity device. One
 * mutex owns every chip operation; a scan or seek is a bounded loop of
 * soft-mute tunes and can be aborted without that mutex. No RDS yet. */
#include <linux/delay.h>
#include <linux/firmware.h>
#include <linux/jiffies.h>
#include <linux/slab.h>
#include <linux/spinlock.h>
#include <linux/wait.h>
#include "conn.h"
#include "fm-protocol.h"

#define Y2_FM_STATIONS 64
#define Y2_FM_WAIT_S 3
#define Y2_FM_SM_TUNE_S 6

struct y2_fm {
	struct y2_conn *c;
	struct mutex lock;
	spinlock_t events_lock;
	wait_queue_head_t wait;
	unsigned events, fspi, cspi;
	unsigned char cqi[Y2_FM_CQI_MAX];
	struct y2_fm_pkt tx;
	bool powered, lost;
	unsigned khz, chip, rom, patch;
	int error;
	atomic_t abort, progress;
	unsigned stations, station_khz[Y2_FM_STATIONS];
	int station_dbm[Y2_FM_STATIONS];
};

/* Called by STP with its receive lock held (BTIF threaded IRQ). */
void y2_fm_receive(struct y2_conn *c, const unsigned char *p, unsigned size)
{
	struct y2_fm *fm = c->fm;
	struct y2_fm_event e;
	unsigned long flags;
	unsigned used;

	if (!fm)
		return;
	spin_lock_irqsave(&fm->events_lock, flags);
	while (size && (used = y2_fm_event_next(p, size, &e))) {
		if (e.op == Y2_FM_OP_FSPI_READ && e.size >= 2)
			fm->fspi = e.payload[0] | (e.payload[1] << 8);
		else if ((e.op == Y2_FM_OP_HOST_READ || e.op == Y2_FM_OP_CSPI_READ) && e.size >= 4)
			fm->cspi = y2_conn_le32(e.payload);
		else if (e.op == Y2_FM_OP_SOFT_MUTE_TUNE && e.bits) {
			memset(fm->cqi, 0, sizeof(fm->cqi));
			memcpy(fm->cqi, e.payload, min_t(unsigned, e.size, sizeof(fm->cqi)));
		}
		fm->events |= e.bits;
		p += used; size -= used;
	}
	spin_unlock_irqrestore(&fm->events_lock, flags);
	wake_up_all(&fm->wait);
}

/* Transport failure or whole-chip recovery: wake every waiter at once. */
void y2_fm_lost(struct y2_conn *c)
{
	struct y2_fm *fm = c->fm;

	if (!fm)
		return;
	WRITE_ONCE(fm->lost, true);
	atomic_set(&fm->abort, 1);
	wake_up_all(&fm->wait);
}

static bool fm_done(struct y2_fm *fm, unsigned mask)
{
	unsigned long flags;
	bool done;

	spin_lock_irqsave(&fm->events_lock, flags);
	done = (fm->events & mask) == mask;
	spin_unlock_irqrestore(&fm->events_lock, flags);
	return done || READ_ONCE(fm->lost) || READ_ONCE(fm->c->failure);
}

/* Send fm->tx and wait for every bit of mask (vendor fm_cmd_tx semantics). */
static int fm_cmd(struct y2_fm *fm, unsigned mask, unsigned seconds)
{
	unsigned long flags;
	int ret;

	if (READ_ONCE(fm->lost) || READ_ONCE(fm->c->failure))
		return -EIO;
	spin_lock_irqsave(&fm->events_lock, flags);
	fm->events &= ~mask;
	spin_unlock_irqrestore(&fm->events_lock, flags);
	ret = y2_stp_send(fm->c, Y2_FM_CHANNEL, fm->tx.data, fm->tx.size);
	if (ret)
		return ret;
	if (!wait_event_timeout(fm->wait, fm_done(fm, mask), seconds * HZ)) {
		dev_err(fm->c->dev, "FM opcode=%02x timeout\n", fm->tx.data[1]);
		return -ETIMEDOUT;
	}
	if (READ_ONCE(fm->lost) || READ_ONCE(fm->c->failure))
		return -EIO;
	spin_lock_irqsave(&fm->events_lock, flags);
	fm->events &= ~mask;
	spin_unlock_irqrestore(&fm->events_lock, flags);
	return 0;
}

static int fm_read(struct y2_fm *fm, unsigned addr, unsigned *value)
{
	int ret;

	y2_fm_read_pkt(&fm->tx, addr);
	ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_FSPI_READ), Y2_FM_WAIT_S);
	if (!ret)
		*value = READ_ONCE(fm->fspi);
	return ret;
}
static int fm_write(struct y2_fm *fm, unsigned addr, unsigned value)
{
	y2_fm_write_pkt(&fm->tx, addr, value);
	return fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_FSPI_WRITE), Y2_FM_WAIT_S);
}
static int fm_set_bits(struct y2_fm *fm, unsigned addr, unsigned bits, unsigned mask)
{
	y2_fm_set_bits_pkt(&fm->tx, addr, bits, mask);
	return fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_SET_BITS), Y2_FM_WAIT_S);
}
static int fm_top_write(struct y2_fm *fm, unsigned addr, unsigned value)
{
	y2_fm_top_write_pkt(&fm->tx, addr, value);
	return fm_cmd(fm, Y2_FM_EV_CSPI_WRITE, Y2_FM_WAIT_S);
}
static int fm_host_write(struct y2_fm *fm, unsigned addr, unsigned value)
{
	y2_fm_host_write_pkt(&fm->tx, addr, value);
	return fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_HOST_WRITE), Y2_FM_WAIT_S);
}
static int fm_host_read(struct y2_fm *fm, unsigned addr, unsigned *value)
{
	int ret;

	y2_fm_host_read_pkt(&fm->tx, addr);
	ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_HOST_READ), Y2_FM_WAIT_S);
	if (!ret)
		*value = READ_ONCE(fm->cspi);
	return ret;
}

/* mt6627_RampDown: mute the audio ramp, interrupts masked around it. */
static int fm_rampdown(struct y2_fm *fm)
{
	int ret = fm_top_write(fm, 0x50, 0x7);

	if (!ret) ret = fm_set_bits(fm, 0x0f, 0x0000, 0xf800);
	if (!ret) ret = fm_top_write(fm, 0x50, 0xf);
	if (!ret) ret = fm_write(fm, Y2_FM_REG_INTRMASK, 0);
	if (!ret) ret = fm_write(fm, Y2_FM_REG_EXTINTRMASK, 0);
	if (!ret) {
		y2_fm_rampdown_pkt(&fm->tx);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_RAMPDOWN), Y2_FM_WAIT_S);
	}
	if (!ret) ret = fm_write(fm, Y2_FM_REG_EXTINTRMASK, 0x21);
	if (!ret) ret = fm_write(fm, Y2_FM_REG_INTRMASK, 0x21);
	return ret;
}

static int fm_download(struct y2_fm *fm, unsigned op, const struct firmware *fw)
{
	unsigned count = y2_fm_segments(fw->size), at = 0;
	int ret = 0;

	for (unsigned i = 0; !ret && i < count; i++) {
		unsigned size = y2_fm_segment_size(fw->size, i);

		if (!y2_fm_segment_pkt(&fm->tx, op, count, i, fw->data + at, size))
			return -EINVAL;
		ret = fm_cmd(fm, Y2_FM_EV(op), Y2_FM_WAIT_S);
		at += size;
	}
	return ret;
}

/* mt6627_get_rom_version, bounded: stock polls 0x84 forever. */
static int fm_rom_version(struct y2_fm *fm, unsigned *rom)
{
	unsigned value = 0, tries = 0;
	int ret = fm_set_bits(fm, 0x61, 0x8000, 0x7fff);

	if (!ret) ret = fm_set_bits(fm, 0x61, 0x0002, 0xfffd);
	if (!ret) ret = fm_set_bits(fm, 0x61, 0x0000, 0xfffe);
	while (!ret && value != 1) {
		if (++tries > 100)
			return -ETIMEDOUT;
		usleep_range(1000, 1200);
		ret = fm_read(fm, 0x84, &value);
	}
	if (!ret) ret = fm_read(fm, 0x83, &value);
	if (!ret) *rom = value >> 8;
	if (!ret) ret = fm_set_bits(fm, 0x61, 0x0000, 0x7fff);
	if (!ret) ret = fm_set_bits(fm, 0x61, 0x0001, 0xfffc);
	return ret;
}

/* mt6627_PowerUp. Only ROM version 0 has a real patch in the stock image
 * (v2..v5 are empty files there), so any other version is refused. */
static int fm_power_up_seq(struct y2_fm *fm, const struct firmware *patch,
			   const struct firmware *coeff)
{
	unsigned value = 0;
	int ret = fm_host_write(fm, Y2_FM_HOST_CSPI, 0x801f);

	if (!ret) {
		y2_fm_clock_on_pkt(&fm->tx, 0);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_ENABLE), Y2_FM_WAIT_S);
	}
	if (!ret) ret = fm_read(fm, Y2_FM_REG_CHIP_ID, &value);
	if (ret) return ret;
	fm->chip = value;
	if (value != 0x6627 && value != 0x6625) return -ENODEV;
	ret = fm_rom_version(fm, &fm->rom);
	if (!ret && fm->rom) ret = -ENOEXEC;
	if (!ret) ret = fm_download(fm, Y2_FM_OP_PATCH, patch);
	if (!ret) {
		fm->patch = coeff->data[38] | (coeff->data[39] << 8);
		ret = fm_download(fm, Y2_FM_OP_COEFF, coeff);
	}
	if (!ret) ret = fm_write(fm, 0x92, 0x0000);
	if (!ret) ret = fm_write(fm, 0x90, 0x0040);
	if (!ret) ret = fm_write(fm, 0x90, 0x0000);
	if (!ret) {
		y2_fm_digital_init_pkt(&fm->tx);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_ENABLE), Y2_FM_WAIT_S);
	}
	if (!ret) {
		y2_fm_fine_tune_pkt(&fm->tx);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_ENABLE), Y2_FM_WAIT_S);
	}
	/* I2S Tx (FM master, 32 kHz) and the CONSYS I2S Rx mode. */
	if (!ret) ret = fm_write(fm, Y2_FM_REG_I2S_TX, 0xf9ab);
	if (!ret) ret = fm_host_write(fm, Y2_FM_HOST_I2S, 0x3f35);
	return ret;
}

/* mt6627_PowerDown, with a correctly assembled host register read. */
static int fm_power_down_seq(struct y2_fm *fm)
{
	unsigned value = 0;
	int ret = fm_read(fm, Y2_FM_REG_INTR, &value);

	if (!ret && (value & 1)) ret = fm_write(fm, Y2_FM_REG_INTR, value);
	if (!ret) ret = fm_rampdown(fm);
	if (!ret) ret = fm_host_read(fm, Y2_FM_HOST_I2S, &value);
	if (!ret) ret = fm_host_write(fm, Y2_FM_HOST_I2S, value & 0xffff9fff);
	if (!ret) {
		y2_fm_power_down_pkt(&fm->tx);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_ENABLE), Y2_FM_WAIT_S);
	}
	if (!ret) ret = fm_write(fm, Y2_FM_REG_EXTINTRMASK, 0);
	return ret;
}

/* mt6627_SetFreq (no 6625/6592 I2S hopping branch: chip 6627 on 6582). */
static int fm_set_freq(struct y2_fm *fm, unsigned unit)
{
	unsigned para = y2_fm_chan_para(unit);
	int ret = fm_top_write(fm, 0x50, 0x7);

	if (!ret) ret = fm_set_bits(fm, 0x0f, 0x0455, 0xf800);
	if (!ret) ret = fm_set_bits(fm, 0x30, y2_fm_tdd(unit) ? 0x0008 : 0, 0xfff3);
	if (!ret) ret = fm_top_write(fm, 0x50, 0xf);
	if (!ret) ret = fm_set_bits(fm, Y2_FM_REG_CHANNEL, y2_fm_channel_reg(unit), 0xfc00);
	if (!ret) ret = fm_set_bits(fm, Y2_FM_REG_CHANNEL, para << 12, 0x0fff);
	if (!ret) {
		y2_fm_tune_pkt(&fm->tx);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_TUNE) | Y2_FM_EV_TUNE_DONE, Y2_FM_WAIT_S);
	}
	return ret;
}
/* fm_tune: rampdown, then the frequency. */
static int fm_tune(struct y2_fm *fm, unsigned khz)
{
	int ret = fm_rampdown(fm);

	if (!ret) ret = fm_set_freq(fm, y2_fm_unit(khz));
	if (!ret) fm->khz = khz;
	return ret;
}
static int fm_mute(struct y2_fm *fm, bool mute)
{
	unsigned value = 0;
	int ret = fm_read(fm, Y2_FM_REG_MUTE, &value);

	return ret ? ret : fm_write(fm, Y2_FM_REG_MUTE, (value & 0xfffc) | (mute ? 3 : 0));
}
static int fm_volume(struct y2_fm *fm, unsigned level)
{
	return fm_write(fm, Y2_FM_REG_VOLUME, y2_fm_volume[min(level, 15U)]);
}
/* mt6627_pre_search / restore_search: audio out off during a search. */
static int fm_pre_search(struct y2_fm *fm)
{
	int ret = fm_rampdown(fm);

	if (!ret) ret = fm_host_write(fm, Y2_FM_HOST_I2S, 0);
	if (!ret) ret = fm_write(fm, Y2_FM_REG_I2S_TX, 0);
	return ret;
}
static int fm_restore_search(struct y2_fm *fm)
{
	int ret = fm_rampdown(fm);

	if (!ret) ret = fm_write(fm, Y2_FM_REG_I2S_TX, 0xf9ab);
	if (!ret) ret = fm_host_write(fm, Y2_FM_HOST_I2S, 0x3f35);
	return ret;
}
/* mt6627_soft_mute_tune: CQI at one channel; raw RSSI and the decision. */
static int fm_soft_mute_tune(struct y2_fm *fm, unsigned unit, int *raw, bool *valid)
{
	struct y2_fm_cqi r;
	unsigned char cqi[Y2_FM_CQI_MAX];
	unsigned long flags;
	int ret = fm_set_bits(fm, Y2_FM_REG_CHANNEL, y2_fm_chan_para(unit) == 2 ? 0x2000 : 0, 0x0fff);

	if (!ret) {
		y2_fm_soft_mute_tune_pkt(&fm->tx, unit);
		ret = fm_cmd(fm, Y2_FM_EV(Y2_FM_OP_SOFT_MUTE_TUNE), Y2_FM_SM_TUNE_S);
	}
	if (ret)
		return ret;
	spin_lock_irqsave(&fm->events_lock, flags);
	memcpy(cqi, fm->cqi, sizeof(cqi));
	spin_unlock_irqrestore(&fm->events_lock, flags);
	r = y2_fm_cqi_decide(cqi);
	*raw = r.rssi;
	*valid = r.valid && !y2_fm_desense(unit, r.rssi);
	return 0;
}

static void fm_release(struct y2_fm *fm)
{
	if (y2_conn_put(fm->c, Y2_CONN_FM))
		dev_warn(fm->c->dev, "FM function release failed\n");
	fm->powered = false;
}

static int fm_power_on(struct y2_fm *fm)
{
	const struct firmware *patch = NULL, *coeff = NULL;
	int ret;

	ret = y2_conn_request_firmware(fm->c->dev, Y2_CONN_FW "mt6627_fm_v1_patch.bin", &patch);
	if (!ret)
		ret = y2_conn_request_firmware(fm->c->dev, Y2_CONN_FW "mt6627_fm_v1_coeff.bin", &coeff);
	if (ret)
		goto out;
	WRITE_ONCE(fm->lost, false);
	ret = y2_conn_get(fm->c, Y2_CONN_FM);
	if (ret)
		goto out;
	fm->powered = true;
	ret = fm_power_up_seq(fm, patch, coeff);
	if (!ret) ret = fm_tune(fm, fm->khz);
	if (!ret) ret = fm_volume(fm, 15);
	if (!ret) ret = fm_mute(fm, false);
	if (ret) {
		dev_err(fm->c->dev, "FM power-up failed: %d chip=%04x rom=%u\n", ret, fm->chip, fm->rom);
		if (!READ_ONCE(fm->lost) && fm_power_down_seq(fm))
			dev_warn(fm->c->dev, "FM power-down after failure incomplete\n");
		fm_release(fm);
	} else {
		dev_info(fm->c->dev, "FM on: chip=%04x rom=%u patch=%04x %u kHz\n",
			 fm->chip, fm->rom, fm->patch, fm->khz);
	}
out:
	release_firmware(coeff);
	release_firmware(patch);
	return ret;
}

static int fm_power_off(struct y2_fm *fm)
{
	int ret = 0;

	if (!fm->powered)
		return 0;
	if (!READ_ONCE(fm->lost)) {
		ret = fm_mute(fm, true);
		if (!ret) ret = fm_power_down_seq(fm);
	}
	fm_release(fm);
	return READ_ONCE(fm->lost) ? 0 : ret;
}

/* A lost chip (recovery) still holds its function reference until here. */
static int fm_usable(struct y2_fm *fm)
{
	if (fm->powered && READ_ONCE(fm->lost)) {
		fm_release(fm);
		return -EIO;
	}
	return fm->powered ? 0 : -ENETDOWN;
}

/* Soft-mute search from start (exclusive) in steps of step kHz, wrapping.
 * Returns the first valid channel or 0. Audio is off while it runs. */
static unsigned fm_search(struct y2_fm *fm, unsigned start, int step, int *dbm)
{
	unsigned span = (Y2_FM_KHZ_MAX - Y2_FM_KHZ_MIN) / abs(step) + 1, khz = start;
	int raw;
	bool valid;

	for (unsigned i = 1; i < span && !atomic_read(&fm->abort); i++) {
		int next = (int)khz + step;

		if (next > Y2_FM_KHZ_MAX) next = Y2_FM_KHZ_MIN;
		if (next < Y2_FM_KHZ_MIN) next = Y2_FM_KHZ_MAX - (Y2_FM_KHZ_MAX - Y2_FM_KHZ_MIN) % abs(step);
		khz = next;
		if (fm_soft_mute_tune(fm, y2_fm_unit(khz), &raw, &valid))
			return 0;
		if (valid) {
			*dbm = y2_fm_rssi_dbm(raw & 0x3ff);
			return khz;
		}
	}
	return 0;
}

static ssize_t power_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_conn *c = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%u\n", c->fm->powered && !READ_ONCE(c->fm->lost));
}
static ssize_t power_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	bool on;
	int ret;

	if (kstrtobool(buf, &on))
		return -EINVAL;
	atomic_set(&fm->abort, 1);
	mutex_lock(&fm->lock);
	atomic_set(&fm->abort, 0);
	if (fm->powered && READ_ONCE(fm->lost))
		fm_release(fm);
	ret = on ? (fm->powered ? 0 : fm_power_on(fm)) : fm_power_off(fm);
	fm->error = ret;
	mutex_unlock(&fm->lock);
	return ret ? ret : n;
}
static DEVICE_ATTR_RW(power);

static ssize_t frequency_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	return sysfs_emit(buf, "%u\n", ((struct y2_conn *)dev_get_drvdata(dev))->fm->khz);
}
static ssize_t frequency_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	unsigned khz;
	int ret;

	if (kstrtouint(buf, 10, &khz) || !y2_fm_khz_valid(khz))
		return -EINVAL;
	mutex_lock(&fm->lock);
	ret = fm->powered ? fm_usable(fm) : 0;
	if (!ret && fm->powered)
		ret = fm_tune(fm, khz);
	else if (!ret)
		fm->khz = khz;      /* applied at the next power-on */
	fm->error = ret;
	mutex_unlock(&fm->lock);
	return ret ? ret : n;
}
static DEVICE_ATTR_RW(frequency);

static ssize_t signal_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	unsigned value = 0;
	int ret;

	mutex_lock(&fm->lock);
	ret = fm_usable(fm);
	if (!ret)
		ret = fm_read(fm, Y2_FM_REG_RSSI, &value);
	mutex_unlock(&fm->lock);
	return ret ? ret : sysfs_emit(buf, "rssi_dbm=%d stereo=%u\n",
				      y2_fm_rssi_dbm(value), y2_fm_stereo(value));
}
static DEVICE_ATTR_RO(signal);

/* "up" / "down": next valid station from the current one, then tuned. */
static ssize_t seek_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	int step = sysfs_streq(buf, "up") ? 100 : sysfs_streq(buf, "down") ? -100 : 0, dbm = 0, ret;
	unsigned found, from;

	if (!step)
		return -EINVAL;
	mutex_lock(&fm->lock);
	atomic_set(&fm->abort, 0);
	ret = fm_usable(fm);
	from = fm->khz;
	if (!ret) ret = fm_pre_search(fm);
	found = ret ? 0 : fm_search(fm, from, step, &dbm);
	if (!ret) ret = fm_restore_search(fm);
	if (!ret) ret = fm_tune(fm, found ? found : from);
	if (!ret && !found) ret = atomic_read(&fm->abort) ? -ECANCELED : -ENOENT;
	fm->error = ret;
	mutex_unlock(&fm->lock);
	return ret ? ret : n;
}
static DEVICE_ATTR_WO(seek);

/* Write a step in kHz (50 or 100) to scan the band; "0" aborts a scan
 * that is running. Read: one "kHz dBm" line per station found. */
static ssize_t scan_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t n)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	unsigned step, khz, count = 0, from;
	int raw, ret;
	bool valid;

	if (kstrtouint(buf, 10, &step) || (step && step != 50 && step != 100))
		return -EINVAL;
	if (!step) {
		atomic_set(&fm->abort, 1);
		return n;
	}
	mutex_lock(&fm->lock);
	atomic_set(&fm->abort, 0);
	atomic_set(&fm->progress, 0);
	ret = fm_usable(fm);
	from = fm->khz;
	if (!ret) ret = fm_pre_search(fm);
	for (khz = Y2_FM_KHZ_MIN; !ret && khz <= Y2_FM_KHZ_MAX; khz += step) {
		if (atomic_read(&fm->abort)) {
			ret = -ECANCELED;
			break;
		}
		ret = fm_soft_mute_tune(fm, y2_fm_unit(khz), &raw, &valid);
		if (!ret && valid && count < Y2_FM_STATIONS) {
			fm->station_khz[count] = khz;
			fm->station_dbm[count++] = y2_fm_rssi_dbm(raw & 0x3ff);
		}
		atomic_set(&fm->progress, (khz - Y2_FM_KHZ_MIN) * 100 / (Y2_FM_KHZ_MAX - Y2_FM_KHZ_MIN));
	}
	if (!READ_ONCE(fm->lost) && fm->powered) {
		int restore = fm_restore_search(fm);

		if (!restore) restore = fm_tune(fm, from);
		if (!ret) ret = restore;
	}
	if (ret != -ECANCELED && !ret)
		fm->stations = count;
	atomic_set(&fm->progress, 100);
	fm->error = ret;
	mutex_unlock(&fm->lock);
	return ret ? ret : n;
}
static ssize_t scan_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;
	int len = 0;

	mutex_lock(&fm->lock);
	for (unsigned i = 0; i < fm->stations; i++)
		len += sysfs_emit_at(buf, len, "%u %d\n", fm->station_khz[i], fm->station_dbm[i]);
	mutex_unlock(&fm->lock);
	return len;
}
static DEVICE_ATTR_RW(scan);

/* Lock-free: readable while a scan or seek holds the chip. */
static ssize_t state_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct y2_fm *fm = ((struct y2_conn *)dev_get_drvdata(dev))->fm;

	return sysfs_emit(buf, "powered=%u lost=%u khz=%u chip=%04x rom=%u patch=%04x progress=%d error=%d\n",
			  READ_ONCE(fm->powered), READ_ONCE(fm->lost), READ_ONCE(fm->khz),
			  READ_ONCE(fm->chip), READ_ONCE(fm->rom), READ_ONCE(fm->patch),
			  atomic_read(&fm->progress), READ_ONCE(fm->error));
}
static DEVICE_ATTR_RO(state);

static struct attribute *fm_attrs[] = {
	&dev_attr_power.attr, &dev_attr_frequency.attr, &dev_attr_signal.attr,
	&dev_attr_seek.attr, &dev_attr_scan.attr, &dev_attr_state.attr, NULL,
};
static umode_t fm_mode(struct kobject *kobj, struct attribute *attr, int index)
{
	return attr->mode & 0600;
}
static const struct attribute_group fm_group = {
	.name = "fm", .attrs = fm_attrs, .is_visible = fm_mode,
};

int y2_fm_probe(struct platform_device *pdev, struct y2_conn *c)
{
	struct y2_fm *fm = devm_kzalloc(&pdev->dev, sizeof(*fm), GFP_KERNEL);

	if (!fm)
		return -ENOMEM;
	fm->c = c;
	mutex_init(&fm->lock);
	spin_lock_init(&fm->events_lock);
	init_waitqueue_head(&fm->wait);
	fm->khz = Y2_FM_KHZ_MIN;
	c->fm = fm;
	return devm_device_add_group(&pdev->dev, &fm_group);
}

/* Shutdown: power the tuner down while the transport still works. */
void y2_fm_shutdown(struct y2_conn *c)
{
	struct y2_fm *fm = c->fm;

	if (!fm)
		return;
	atomic_set(&fm->abort, 1);
	mutex_lock(&fm->lock);
	fm_power_off(fm);
	mutex_unlock(&fm->lock);
}
