"""F56: the kernel-fed SoC watchdog, the real patched mtk_wdt functions and the
real Linux 6.18 watchdog core paths against a modelled RGU counter.
Synthetic evidence, never a device receipt."""
from pathlib import Path
import re
import unittest
from test_audio import function
from test_cpu_fix02 import overlay
from test_power import run_c
ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT/'.cache/sources/linux-6.18'

FIXTURE = r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
#define BIT(n) (1UL<<(n))
#define WRITE_ONCE(x, v) ((x) = (v))
#define READ_ONCE(x) (x)
#define THIS_MODULE NULL
#define __iomem
#define container_of(p, t, m) ((t *)((char *)(p) - offsetof(t, m)))
#define trace_watchdog_start(w, e) ((void)(e))
#define trace_watchdog_stop(w, e) ((void)(e))
#define watchdog_hrtimer_pretimeout_start(w) ((void)(w))
#define watchdog_hrtimer_pretimeout_stop(w) ((void)(w))
#define pr_info(...) ((void)0)
#define pr_crit(...) ((void)0)
#define mutex_lock(l) ((void)(l))
#define mutex_unlock(l) ((void)(l))
#define module_put(o) ((void)(o))
#define put_device(d) ((void)(d))
#define ktime_get() (now_ms)
#define NOTIFY_DONE 0
#define NOTIFY_BAD 0x8002
#define SYS_DOWN 1
#define SYS_HALT 2
#define SYS_POWER_OFF 3
#define WDIOF_MAGICCLOSE 0x0100
#define WDOG_ACTIVE 0
#define WDOG_NO_WAY_OUT 1
#define WDOG_STOP_ON_REBOOT 2
#define WDOG_HW_RUNNING 3
#define _WDOG_DEV_OPEN 0
#define _WDOG_ALLOW_RELEASE 1
#define _WDOG_KEEPALIVE 2
typedef long long ktime_t;
typedef unsigned u32;
static ktime_t now_ms;
static void set_bit(int n, unsigned long *p) { *p |= BIT(n); }
static void clear_bit(int n, unsigned long *p) { *p &= ~BIT(n); }
static bool test_bit(int n, const unsigned long *p) { return *p & BIT(n); }
static bool test_and_clear_bit(int n, unsigned long *p) { bool v = test_bit(n, p); clear_bit(n, p); return v; }
struct notifier_block { int unused; };
struct device { void *drvdata; };
struct inode { int unused; };
struct file { void *private_data; };
struct watchdog_info { unsigned options; };
struct watchdog_device;
struct watchdog_ops {
	void *owner;
	int (*start)(struct watchdog_device *);
	int (*stop)(struct watchdog_device *);
	int (*ping)(struct watchdog_device *);
	int (*set_timeout)(struct watchdog_device *, unsigned);
	int (*set_pretimeout)(struct watchdog_device *, unsigned);
	int (*restart)(struct watchdog_device *, unsigned long, void *);
};
struct watchdog_core_data {
	struct device dev; struct { void *owner; } cdev; struct watchdog_device *wdd; int lock;
	ktime_t last_keepalive, last_hw_keepalive, open_deadline; unsigned long status;
};
struct watchdog_device {
	int id; const struct watchdog_info *info; const struct watchdog_ops *ops;
	unsigned timeout, pretimeout, max_hw_heartbeat_ms, min_hw_heartbeat_ms;
	struct notifier_block reboot_nb; void *driver_data; struct watchdog_core_data *wd_data;
	unsigned long status;
};
struct mtk_wdt_dev { struct watchdog_device wdt_dev; void *wdt_base; bool disable_wdt_extrst, reset_by_toprgu; };
static void *watchdog_get_drvdata(struct watchdog_device *w) { return w->driver_data; }
static void *dev_get_drvdata(struct device *d) { return d->drvdata; }
/* RGU model: keyed MODE/LENGTH writes, RST reload; the counter runs while EN. */
static unsigned regs[8], mode, length_ms, left_ms, resets;
static unsigned readl(const void *p) { return ((const unsigned *)p - regs) == 0 ? mode : *(const unsigned *)p; }
static void writel(unsigned v, void *p)
{
	unsigned off = (unsigned)((unsigned *)p - regs) * 4;
	if (off == 0x00) {
		assert((v & 0xff000000U) == 0x22000000U);
		if (!(mode & 1) && (v & 1)) left_ms = length_ms;
		mode = v & 0x00ffffffU;
	} else if (off == 0x04) {
		assert((v & 0x1f) == 8);
		length_ms = (v >> 5) * 1000 / 64;
	} else if (off == 0x08) {
		assert(v == 0x1971);
		left_ms = length_ms;
	}
}
#define ioread32 readl
#define iowrite32 writel
static int mtk_wdt_set_pretimeout(struct watchdog_device *w, unsigned t) { (void)w; (void)t; return -EINVAL; }
static int mtk_wdt_restart(struct watchdog_device *w, unsigned long a, void *d) { (void)w; (void)a; (void)d; return 0; }
/* The core's hrtimer: armed for the next keepalive (15.5 s) or cancelled. */
static ktime_t timer_at = -1;
static bool watchdog_need_worker(struct watchdog_device *wdd);
static void watchdog_update_worker(struct watchdog_device *wdd) { timer_at = watchdog_need_worker(wdd) ? now_ms + 15500 : -1; }
static int __watchdog_ping(struct watchdog_device *wdd)
{
	int err = wdd->ops->ping(wdd);
	watchdog_update_worker(wdd);
	return err;
}
static bool watchdog_past_open_deadline(struct watchdog_core_data *d) { (void)d; return false; }
struct y2_pm_backstop_ops { int (*start)(unsigned); void (*ping)(void); void (*stop)(void); void (*end)(void); };
'''

HARNESS = r'''
static struct watchdog_core_data core;
static struct mtk_wdt_dev wdt;
static struct device dev = { &wdt };
static struct watchdog_device *wdd = &wdt.wdt_dev;
static const struct watchdog_info info = { WDIOF_MAGICCLOSE };
static struct file file = { &core };
/* Half-second steps; watchdogd runs when the timer fires (not when frozen). */
static void run(unsigned ms, bool kernel_alive, unsigned user_every)
{
	for (unsigned t = 0; t < ms; t += 500) {
		now_ms += 500;
		if (mode & 1) { if (left_ms <= 500) { resets++; left_ms = length_ms; } else left_ms -= 500; }
		if (kernel_alive && timer_at >= 0 && now_ms >= timer_at) {
			timer_at = -1;
			if (watchdog_worker_should_ping(&core)) __watchdog_ping(wdd);
		}
		if (user_every && now_ms % user_every == 0 && watchdog_active(wdd))
			watchdog_ping(wdd);
	}
}
static void open_dev(void) { assert(!watchdog_start(wdd)); set_bit(_WDOG_DEV_OPEN, &core.status); }
static void probe(bool keepalive)
{
	memset(&wdt, 0, sizeof(wdt)); memset(&core, 0, sizeof(core));
	mode = 0x2; resets = 0; timer_at = -1;   /* stopped by the zImage entry, EXT_POL kept */
	wdt.wdt_base = regs; wdd->info = &info; wdd->timeout = 31; wdd->max_hw_heartbeat_ms = 31000;
	wdd->driver_data = &wdt; wdd->wd_data = &core; core.wdd = wdd;
	set_bit(WDOG_STOP_ON_REBOOT, &wdd->status);
	y2_keepalive = keepalive; y2_keepalive_wdt = NULL; y2_backstop = &wdt; y2_backstop_owns = false;
	if (keepalive) y2_keepalive_arm(&wdt);
	/* watchdog_cdev_register: a running watchdog gets an immediate core ping. */
	if (watchdog_hw_running(wdd)) __watchdog_ping(wdd);
}
int main(void)
{
	/* Probe: running at 31 s with external reset and the bypass-Power-key
	 * restart (LK boot_reason 4), no IRQ/dual mode; the core feeds it. */
	probe(true);
	assert((mode & 0x5f) == (0x1 | 0x2 | 0x4 | 0x10) && length_ms == 31000);
	assert(watchdog_hw_running(wdd) && !watchdog_active(wdd) && watchdog_need_worker(wdd) && wdd->ops == &y2_wdt_ops);
	run(3600000, true, 0); assert(!resets);
	/* A hard freeze (no watchdogd) resets within 31 s. */
	run(31000, false, 0); assert(resets == 1);

	/* Recovery keeper: while it holds /dev/watchdog the core never pings;
	 * no progress means no writes and the reset comes 31 s later. */
	probe(true); open_dev();
	assert(watchdog_active(wdd) && !watchdog_need_worker(wdd));
	run(600000, true, 5000); assert(!resets);
	run(31000, true, 0); assert(resets == 1);
	/* A keeper that never writes: the core's timer armed before the open
	 * pings once more at most, then the reset follows. */
	probe(true); run(7000, true, 0); open_dev(); run(15500 + 31000, true, 0); assert(resets == 1);
	/* Its normal stop ('V' then close) hands the RGU back to the kernel. */
	probe(true); open_dev(); run(20000, true, 5000);
	set_bit(_WDOG_ALLOW_RELEASE, &core.status); watchdog_release(NULL, &file);
	assert((mode & 1) && watchdog_hw_running(wdd) && !watchdog_active(wdd) && watchdog_need_worker(wdd));
	run(3600000, true, 0); assert(!resets);
	run(31000, false, 0); assert(resets == 1);
	/* ...and it can be opened again (the installer's keeper after the early one). */
	probe(true); open_dev(); set_bit(_WDOG_ALLOW_RELEASE, &core.status); watchdog_release(NULL, &file);
	open_dev(); assert(watchdog_active(wdd) && !watchdog_need_worker(wdd));
	run(15500 + 31000, true, 0); assert(resets == 1);

	/* power.py: armed, closed without 'V' at exit, fed no more: reset in
	 * 31 s; reboot(2)'s notifier really stops it (no reset in the final path). */
	probe(true); open_dev(); watchdog_release(NULL, &file);
	assert(watchdog_active(wdd) && !watchdog_need_worker(wdd));
	run(31000, true, 0); assert(resets == 1);
	probe(true); open_dev(); watchdog_release(NULL, &file); run(10000, true, 0);
	watchdog_reboot_notifier(&wdd->reboot_nb, SYS_POWER_OFF, NULL);
	assert(!(mode & 1) && !watchdog_hw_running(wdd));
	run(600000, false, 0); assert(!resets);
	/* Restart/power-off from the kernel-fed state stops it as well. */
	probe(true); watchdog_reboot_notifier(&wdd->reboot_nb, SYS_DOWN, NULL);
	assert(!(mode & 1)); run(600000, false, 0); assert(!resets);

	/* Sleep without the backstop: stopped for the whole sleep, as stock
	 * mtk_wd_suspend, and running at 31 s again after resume. */
	probe(true); run(10000, true, 0);
	assert(!mtk_wdt_suspend(&dev) && !(mode & 1));
	run(1800000, false, 0); assert(!resets);
	assert(!mtk_wdt_resume(&dev) && (mode & 1) && length_ms == 31000 && left_ms == 31000);
	run(3600000, true, 0); assert(!resets);

	/* Product sleep with the 30-s backstop: it owns the RGU from the request
	 * to the exit; watchdogd's pings no longer hide a stall. */
	probe(true); run(10000, true, 0);
	assert(!y2_backstop_start(30) && y2_backstop_owns && length_ms == 30000 && wdd->timeout == 31);
	run(29000, true, 0); assert(!resets);           /* stall: only watchdogd runs */
	run(1500, true, 0); assert(resets == 1);
	probe(true); assert(!y2_backstop_start(30));
	assert(!mtk_wdt_suspend(&dev) && (mode & 1));     /* the backstop keeps counting */
	y2_backstop_ping(); y2_backstop_stop(); assert(!(mode & 1));    /* SPM entry */
	run(1800000, true, 0); assert(!resets);
	assert(!y2_backstop_start(30) && (mode & 1));    /* after SPM return */
	assert(!mtk_wdt_resume(&dev) && length_ms == 30000);
	y2_backstop_stop(); y2_backstop_end();           /* y2_pm_backstop_end */
	assert(!y2_backstop_owns && (mode & 1) && length_ms == 31000);
	run(3600000, true, 0); assert(!resets);
	/* A refused restart after SPM (paused, never restarted) still hands back. */
	probe(true); assert(!y2_backstop_start(30)); y2_backstop_stop(); y2_backstop_end();
	assert((mode & 1) && length_ms == 31000);
	/* Userspace owns it: the backstop is refused and the keepalive untouched. */
	probe(true); open_dev(); assert(y2_backstop_start(30) == -EBUSY && !y2_backstop_owns);

	/* mtk_wdt.y2_keepalive=0: stopped as before; suspend leaves it alone. */
	probe(false);
	assert(!(mode & 1) && !watchdog_hw_running(wdd) && timer_at < 0 && !y2_keepalive_running(wdd));
	assert(!mtk_wdt_suspend(&dev) && !mtk_wdt_resume(&dev) && !(mode & 1));
	assert(!y2_backstop_start(30)); y2_backstop_stop(); y2_backstop_end(); assert(!(mode & 1));
	return 0;
}
'''


class KernelFedWatchdog(unittest.TestCase):
    def source(self):
        driver = overlay('drivers/watchdog/mtk_wdt.c')
        header = (CORE/'include/linux/watchdog.h').read_text()
        dev = (CORE/'drivers/watchdog/watchdog_dev.c').read_text()
        core = (CORE/'drivers/watchdog/watchdog_core.c').read_text()
        defines = '\n'.join(re.findall(r'^#define WDT_\w+.*$', driver, re.M))
        state = '\n'.join(re.findall(r'^static (?:bool |struct mtk_wdt_dev \*)y2_\w+(?: = true)?;$', driver, re.M))
        ops = re.search(r'^static const struct watchdog_ops y2_wdt_ops = \{.*?^\};$', driver, re.M | re.S).group(0)
        return (FIXTURE + defines + '\n' + state + '\n' +
                ''.join(function(header, n) for n in ('watchdog_active', 'watchdog_hw_running')) +
                ''.join(function(dev, n) for n in ('watchdog_need_worker', 'watchdog_worker_should_ping', 'watchdog_ping',
                                                    'watchdog_start', 'watchdog_stop', 'watchdog_release')) +
                function(core, 'watchdog_reboot_notifier') +
                ''.join(function(driver, n) for n in ('mtk_wdt_ping', 'mtk_wdt_set_timeout', 'mtk_wdt_stop', 'mtk_wdt_start',
                                                       'y2_keepalive_running', 'y2_backstop_start', 'y2_backstop_ping',
                                                       'y2_backstop_stop', 'y2_backstop_end', 'y2_wdt_ping', 'y2_wdt_stop')) +
                ops + '\n' + function(driver, 'y2_keepalive_arm') +
                function(driver, 'mtk_wdt_suspend') + function(driver, 'mtk_wdt_resume'))

    def test_boot_open_close_reboot_and_sleep_against_the_real_core(self):
        run_c(self.source() + HARNESS)

    def test_probe_arms_before_registration_and_the_core_handles_boot_enabled(self):
        driver = overlay('drivers/watchdog/mtk_wdt.c')
        probe = function(driver, 'mtk_wdt_probe')
        # Registered running: the core pings it from registration on.
        self.assertLess(probe.index('y2_keepalive_arm(mtk_wdt);'), probe.index('devm_watchdog_register_device'))
        self.assertLess(probe.index('mtk_wdt_init(&mtk_wdt->wdt_dev);'), probe.index('y2_keepalive_arm(mtk_wdt);'))
        self.assertIn('module_param(y2_keepalive, bool, 0444);', driver)
        self.assertIn('.end = y2_backstop_end,', driver)
        config = (ROOT/'kernel/config/production.config').read_text()
        self.assertIn('CONFIG_WATCHDOG_HANDLE_BOOT_ENABLED=y\n', config)
        self.assertIn('CONFIG_WATCHDOG_OPEN_TIMEOUT=0\n', config)
        self.assertIn('CONFIG_WATCHDOG_NOWAYOUT=n\n', config)
        # The core semantics the stop op relies on (Linux 6.18): a userspace
        # stop clears HW_RUNNING before ops->stop; the reboot notifier does not.
        dev = function((CORE/'drivers/watchdog/watchdog_dev.c').read_text(), 'watchdog_stop')
        self.assertLess(dev.index('clear_bit(WDOG_HW_RUNNING'), dev.index('err = wdd->ops->stop(wdd);'))
        notifier = function((CORE/'drivers/watchdog/watchdog_core.c').read_text(), 'watchdog_reboot_notifier')
        self.assertNotIn('clear_bit', notifier)
        self.assertIn('if (watchdog_hw_running(wdd)) {', notifier)

    def test_backstop_end_hands_the_rgu_back(self):
        journal = (ROOT/'kernel/platform/pm-journal.c').read_text()
        end = journal[journal.index('void y2_pm_backstop_end(void)'):]
        end = end[:end.index('\n}\n')]
        self.assertLess(end.index('ops->stop();'), end.index('ops->end();'))
        self.assertIn('if (ops && ops->end)', end)


if __name__ == '__main__': unittest.main()
