// SPDX-License-Identifier: GPL-2.0-only
/* STP power saving (PSM) for the MT6582 CONSYS link, after MediaTek GPL
 * psm_core.c / wmt_core.c opfunc_pwr_sv / mtk_btif.c (Y2 power plan 2026-11,
 * phase 7; brief in docs/knowledge/stp-psm.md). With the link idle for
 * idle_ms the host sends WMT SLEEP (01 03 01 00 01 -> 02 03 02 00 00 01),
 * arms the CONN2AP wake line and gates the BTIF and AP_DMA clocks, which
 * otherwise refuse every SoC idle entry while Bluetooth, Wi-Fi or FM is on.
 * Before any send, and when the chip pulls the wake line, the host ungates,
 * pulses BTIF_WAK (0, >1/32 kHz, 1) and waits for WAKEUP (02 03 02 00 00 03);
 * stock wakes for the chip the same way (wmt_lib_ps_do_host_awake).
 *
 * Differences from stock, deliberately: no hold FIFO (a sender blocks until
 * the chip is awake; stock drops held data when its 8 KiB FIFO fills), no
 * per-packet "quick sleep" while dark, and a failed handshake never gates
 * (stock gates anyway and asserts the firmware); it fails the transport into
 * the existing recovery instead.
 *
 * Off unless stp_psm.enable=Y when the connectivity core starts.
 * Lock order: lifecycle -> psm_lock -> command -> stp_lock -> dma_tx/dma_rx. */
#include <linux/delay.h>
#include <linux/interrupt.h>
#include <linux/io.h>
#include <linux/ktime.h>
#include <linux/math64.h>
#include <linux/module.h>
#include "conn.h"

static bool enable;
module_param(enable, bool, 0644);
MODULE_PARM_DESC(enable, "STP power saving from the next connectivity start (default N)");
static unsigned idle_ms = 5000;
static int idle_ms_set(const char *val, const struct kernel_param *kp)
{
	unsigned ms;
	int ret = kstrtouint(val, 0, &ms);
	if (ret) return ret;
	if (ms < 100 || ms > 20000) return -ERANGE;
	WRITE_ONCE(idle_ms, ms);
	return 0;
}
static const struct kernel_param_ops idle_ms_ops = { .set = idle_ms_set, .get = param_get_uint };
module_param_cb(idle_ms, &idle_ms_ops, &idle_ms, 0644);
MODULE_PARM_DESC(idle_ms, "idle link time before the chip is put to sleep, 100..20000 ms (5000)");

#define BTIF_LSR 0x14
#define BTIF_WAK 0x64
#define LSR_DR BIT(0)
#define LSR_TEMT BIT(6)
#define DMA_WRITE_PTR 0x2c
#define DMA_INT_BUF 0x38
#define DMA_VALID 0x3c
#define DMA_FLUSH 0x14
#define WAIT_MS 2000

static const unsigned char sleep_cmd[] = {1, 3, 1, 0, 1};
static const unsigned char sleep_evt[] = {2, 3, 2, 0, 0, 1};
static const unsigned char wakeup_evt[] = {2, 3, 2, 0, 0, 3};

/* The wake line's enable state, balanced however it changes (handler,
 * host wake, stop). Its depth must be 0 again whenever y2_btif_stop runs. */
static void wake_arm(struct y2_conn *c)
{
	unsigned long flags;
	spin_lock_irqsave(&c->psm_irq_lock, flags);
	if (!c->psm_wake_enabled) { c->psm_wake_enabled = true; enable_irq(c->irq[3]); }
	spin_unlock_irqrestore(&c->psm_irq_lock, flags);
}
static void wake_disarm(struct y2_conn *c)
{
	unsigned long flags;
	spin_lock_irqsave(&c->psm_irq_lock, flags);
	if (c->psm_wake_enabled) { c->psm_wake_enabled = false; disable_irq_nosync(c->irq[3]); }
	spin_unlock_irqrestore(&c->psm_irq_lock, flags);
}
static void data_irqs(struct y2_conn *c, bool on)
{
	for (unsigned i = 0; i < 3; i++) {
		if (on) enable_irq(c->irq[i]);
		else disable_irq(c->irq[i]);
	}
}
static void rearm(struct y2_conn *c)
{
	if (c->psm_state == Y2_PSM_ACT && !c->psm_users)
		mod_delayed_work(system_wq, &c->psm_idle, msecs_to_jiffies(READ_ONCE(idle_ms)));
}
/* Clocks and data IRQs on (no chip handshake); under psm_lock. */
static void ungate(struct y2_conn *c)
{
	if (!c->psm_gated) return;
	if (clk_bulk_enable(2, &c->clocks[1])) {
		/* Prepared clocks only fail to enable on a broken provider. */
		c->psm_wake_failures++;
		return;
	}
	WRITE_ONCE(c->psm_gated, false);
	data_irqs(c, true);
	/* The lost-RX detector: nothing may have reached the RX ring while gated. */
	if ((readl(c->rxdma + DMA_WRITE_PTR) & 0x1ffff) != c->rxptr ||
	    (readl(c->btif + BTIF_LSR) & LSR_DR)) c->psm_rx_while_gated++;
}
/* TX drained, nothing received or arriving: stock _btif_is_tx_complete plus
 * the RX side. Data IRQs are already off; under dma_tx and dma_rx. */
static bool quiet(struct y2_conn *c)
{
	unsigned lsr = readl(c->btif + BTIF_LSR);
	return !readl(c->txdma + DMA_VALID) && !readl(c->txdma + DMA_INT_BUF) &&
		!(readl(c->txdma + DMA_FLUSH) & 1) && (lsr & LSR_TEMT) && !(lsr & LSR_DR) &&
		(readl(c->rxdma + DMA_WRITE_PTR) & 0x1ffff) == c->rxptr && !readl(c->rxdma + DMA_VALID);
}
/* After SLEEP_EVT: the data IRQs off (this also waits for the RX thread that
 * sent our STP ACK of the event), then the clocks off once all is quiet. */
static void gate(struct y2_conn *c)
{
	bool idle = false;
	data_irqs(c, false);
	mutex_lock(&c->dma_tx);
	mutex_lock(&c->dma_rx);
	for (unsigned i = 0; i < 50 && !(idle = quiet(c)); i++) usleep_range(1000, 1200);
	if (idle) {
		clk_bulk_disable(2, &c->clocks[1]);
		WRITE_ONCE(c->psm_gated, true);
	}
	mutex_unlock(&c->dma_rx);
	mutex_unlock(&c->dma_tx);
	if (!idle) {
		/* The chip sleeps, the host stays clocked: correct, only not saving. */
		c->psm_gate_busy++;
		data_irqs(c, true);
	}
}
static const char *sleep_refusal(struct y2_conn *c)
{
	if (c->psm_users) return "busy";
	if (READ_ONCE(c->failure) || READ_ONCE(c->recovering) || !READ_ONCE(c->transport_on) ||
	    !c->full_stp) return "transport";
	if (READ_ONCE(c->stp_pending) || READ_ONCE(c->rx_used)) return "stp";
	if (!skb_queue_empty(&c->hci_tx)) return "queue";
	return NULL;
}
static void idle_work(struct work_struct *work)
{
	struct y2_conn *c = container_of(to_delayed_work(work), struct y2_conn, psm_idle);
	int ret = 0;
	mutex_lock(&c->psm_lock);
	if (c->psm_state != Y2_PSM_ACT) goto out;
	if (sleep_refusal(c)) { c->psm_refused++; rearm(c); goto out; }
	c->psm_state = Y2_PSM_ACT_INACT;
	WRITE_ONCE(c->psm_expect, 1);
	ret = y2_conn_wmt_awake(c, sleep_cmd, sizeof(sleep_cmd), sleep_evt, sizeof(sleep_evt));
	WRITE_ONCE(c->psm_expect, 0);
	if (ret) { c->psm_sleep_failures++; c->psm_state = Y2_PSM_ACT; goto out; }
	/* Level-low: an assertion from here on is seen once the line is armed. */
	wake_arm(c);
	gate(c);
	c->psm_state = Y2_PSM_INACT;
	c->psm_sleeps++;
	c->psm_since = ktime_get();
out:
	mutex_unlock(&c->psm_lock);
	if (ret) y2_conn_failed(c, ret);
}
/* Wake the chip (for a send, or because it pulled the wake line); under psm_lock. */
static int host_wake(struct y2_conn *c)
{
	ktime_t start = ktime_get();
	bool awake = false;
	c->psm_state = Y2_PSM_INACT_ACT;
	wake_disarm(c);
	ungate(c);
	if (c->psm_gated) { c->psm_state = Y2_PSM_INACT; return -EIO; }
	for (unsigned attempt = 0; attempt < 2 && !awake; attempt++) {
		mutex_lock(&c->command);
		reinit_completion(&c->response); c->response_size = 0;
		WRITE_ONCE(c->psm_expect, 3);
		writel(0, c->btif + BTIF_WAK);
		usleep_range(64, 96); /* longer than one 32 kHz period, as stock */
		writel(1, c->btif + BTIF_WAK);
		if (wait_for_completion_timeout(&c->response, msecs_to_jiffies(WAIT_MS)))
			awake = !READ_ONCE(c->failure) && c->response_size == sizeof(wakeup_evt) &&
				!memcmp(c->response_data, wakeup_evt, sizeof(wakeup_evt));
		WRITE_ONCE(c->psm_expect, 0);
		mutex_unlock(&c->command);
		if (!awake) c->psm_wake_retries++;
	}
	if (!awake) {
		c->psm_wake_failures++;
		c->psm_state = Y2_PSM_ACT; /* recovery follows; nothing may wait on a sleeping state */
		return -ETIMEDOUT;
	}
	c->psm_asleep_ns += ktime_to_ns(ktime_sub(start, c->psm_since));
	c->psm_wake_latency_us = ktime_us_delta(ktime_get(), start);
	c->psm_wake_latency_max_us = max(c->psm_wake_latency_max_us, c->psm_wake_latency_us);
	c->psm_state = Y2_PSM_ACT;
	return 0;
}
static void wake_work(struct work_struct *work)
{
	struct y2_conn *c = container_of(work, struct y2_conn, psm_wake);
	int ret = 0;
	mutex_lock(&c->psm_lock);
	if (c->psm_state == Y2_PSM_INACT) {
		c->psm_chip_wakes++;
		ret = host_wake(c);
		if (!ret) rearm(c);
	} else {
		c->psm_wake_spurious++;
	}
	mutex_unlock(&c->psm_lock);
	if (ret) y2_conn_failed(c, ret);
}
/* The wake line (IRQ thread): mask it and let the work wake the chip. */
void y2_psm_wake_irq(struct y2_conn *c)
{
	wake_disarm(c);
	schedule_work(&c->psm_wake);
}
int y2_psm_get(struct y2_conn *c)
{
	int ret = 0;
	mutex_lock(&c->psm_lock);
	if (c->psm_state == Y2_PSM_INACT) {
		c->psm_host_wakes++;
		ret = host_wake(c);
	}
	if (!ret) c->psm_users++;
	mutex_unlock(&c->psm_lock);
	if (ret) y2_conn_failed(c, ret);
	return ret;
}
void y2_psm_put(struct y2_conn *c)
{
	mutex_lock(&c->psm_lock);
	if (c->psm_users) c->psm_users--;
	rearm(c);
	mutex_unlock(&c->psm_lock);
}
void y2_psm_init(struct y2_conn *c)
{
	mutex_init(&c->psm_lock);
	spin_lock_init(&c->psm_irq_lock);
	INIT_DELAYED_WORK(&c->psm_idle, idle_work);
	INIT_WORK(&c->psm_wake, wake_work);
}
/* After y2_btif_start (wake line enabled with the data IRQs) and a verified
 * full-STP boot, under lifecycle. */
void y2_psm_start(struct y2_conn *c)
{
	unsigned long flags;
	spin_lock_irqsave(&c->psm_irq_lock, flags);
	c->psm_wake_enabled = true;
	spin_unlock_irqrestore(&c->psm_irq_lock, flags);
	if (!READ_ONCE(enable)) return;
	mutex_lock(&c->psm_lock);
	wake_disarm(c);
	c->psm_users = 0;
	c->psm_state = Y2_PSM_ACT;
	rearm(c);
	mutex_unlock(&c->psm_lock);
	dev_info(c->dev, "STP power saving on (idle %u ms)\n", READ_ONCE(idle_ms));
}
/* Before the transport stops (power off, recovery, a failed start), under
 * lifecycle: clocks, data IRQs and the wake line back to how y2_btif_start
 * left them, so y2_btif_stop/abort balance exactly. No chip handshake. */
void y2_psm_stop(struct y2_conn *c)
{
	mutex_lock(&c->psm_lock);
	c->psm_state = Y2_PSM_OFF;
	mutex_unlock(&c->psm_lock);
	cancel_delayed_work_sync(&c->psm_idle);
	cancel_work_sync(&c->psm_wake);
	mutex_lock(&c->psm_lock);
	if (c->psm_gated) {
		if (c->psm_since) c->psm_asleep_ns += ktime_to_ns(ktime_sub(ktime_get(), c->psm_since));
		ungate(c);
	}
	if (c->dma_active) wake_arm(c);
	c->psm_users = 0;
	mutex_unlock(&c->psm_lock);
}
ssize_t y2_psm_show(struct y2_conn *c, char *buf)
{
	static const char *const names[] = {"off", "act", "act_inact", "inact", "inact_act"};
	ssize_t n;
	mutex_lock(&c->psm_lock);
	n = sysfs_emit(buf, "state=%s enable=%u idle_ms=%u gated=%u sleeps=%u refused=%u sleep_failures=%u gate_busy=%u host_wakes=%u chip_wakes=%u wake_spurious=%u wake_retries=%u wake_failures=%u wake_latency_us=%u wake_latency_max_us=%u asleep_ms=%llu rx_while_gated=%u send_while_gated=%u unsolicited=%u\n",
		names[c->psm_state], READ_ONCE(enable), READ_ONCE(idle_ms), c->psm_gated, c->psm_sleeps,
		c->psm_refused, c->psm_sleep_failures, c->psm_gate_busy, c->psm_host_wakes,
		c->psm_chip_wakes, c->psm_wake_spurious, c->psm_wake_retries, c->psm_wake_failures,
		c->psm_wake_latency_us, c->psm_wake_latency_max_us,
		(unsigned long long)div_u64(c->psm_asleep_ns, NSEC_PER_MSEC), READ_ONCE(c->psm_rx_while_gated),
		READ_ONCE(c->psm_send_while_gated), READ_ONCE(c->psm_unsolicited));
	mutex_unlock(&c->psm_lock);
	return n;
}
