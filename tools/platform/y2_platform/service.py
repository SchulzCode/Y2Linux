"""Bounded readiness/maintenance work, separate from the battery deadline owner."""
# SPDX-License-Identifier: GPL-2.0-only
import contextlib
import fcntl
import os
import signal
import time
from .boot import private_directory
import json
from .common import atomic_json, kill_tree, read
from .media import inventory
from .network import DnsProbeSchedule, EventMonitor, WifiRecovery, probe_dns
from .observe import wifi
from .timekeeping import remember as remember_time, status as time_status


# Fix02 background attribution: every iteration used to start a complete
# Python interpreter for `media reconcile` (every 3 s) plus two more every
# 30 s. On the Y2 that recurring start-up work drove schedutil to 1040 MHz
# and >10% samples during screen-off idle. The reconcile subprocess is still
# the only owner of mount/unmount work; it is now started only when its own
# no-op condition (unchanged SD inventory, no retry) is false.
INTERVAL_S = 3
DARK_INTERVAL_S = 10  # Reborn: network.json stale after 15 s (an Online record after 45 s)
MAINTENANCE_S = 30
DARK_MAINTENANCE_S = 300
# A card's read-only admission check alone may take 60 s (media.read_only_check);
# a run that is still busy after this is stopped, and the same card is tried
# again only after RECONCILE_BACKOFF_S.
RECONCILE_LIMIT_S = 150
RECONCILE_BACKOFF_S = 600
# A background check (`media verify`, F49) that ended without a verdict (the
# player slept, the card was busy) starts again after this.
VERIFY_RETRY_S = 30
# Error reports (ERROR-REPORTS-PLAN): look for new triggers this often, the
# first time shortly after start (a crash record of the previous boot); a run
# that builds and sends reports is stopped after REPORT_LIMIT_S.
REPORT_FIRST_S = 120
REPORT_CHECK_S = 900
REPORT_LIMIT_S = 600
REPORT_SEND_GAP_S = 300   # a blocked upload (battery, clock) is not retried every pass


def display_dark(ctx):
    """Same meaning as the kernel's backlight_is_blank(): Reborn blanks with
    bl_power=4 (FB_BLANK_POWERDOWN) and leaves brightness unchanged."""
    devices = ctx.glob('/sys/class/backlight/*')
    states = [(read(path / 'bl_power'), read(path / 'brightness')) for path in devices]
    return bool(states) and all(power not in (None, '0') or brightness == '0' for power, brightness in states)


def media_reconcile_needed(ctx):
    """Mirror media.reconcile's own early return without mounting anything."""
    lifecycle = ctx.json('/run/y2/media-lifecycle.json', {})
    current = [list(v) for v in inventory(ctx)]
    return not isinstance(lifecycle, dict) or lifecycle.get('inventory') != current or bool(lifecycle.get('retry'))


def health_settled(answer):
    """True once health-ack reports a non-pending state for this boot."""
    if not answer or not answer.get('ok'):
        return False
    try:
        value = json.loads(answer.get('output') or '')
    except (ValueError, TypeError):
        return False
    return isinstance(value, dict) and value.get('state') not in (None, 'PendingHealth', 'Failed')


def media_reconcile(ctx, run, now):
    """SD insertion/removal work (`media reconcile`) in its own process. It
    used to run inside this loop with a 7 s deadline: a card whose check took
    longer was never mounted, and every pass started another checker while the
    orphaned ones kept running (V1 code review B5). `run` is this function's
    previous answer: {'process', 'started'} while one runs, {'retry_at'} after
    one was stopped. Returns the new state."""
    run = run or {}
    process = run.get('process')
    if process is not None:
        if process.poll() is None:
            if now - run['started'] < RECONCILE_LIMIT_S:
                return run
            kill_tree(process.pid)
            with contextlib.suppress(Exception):
                process.wait(timeout=1)
            return {'retry_at': now + RECONCILE_BACKOFF_S}
        run = {}
    if now < run.get('retry_at', 0) or not media_reconcile_needed(ctx):
        return run
    import subprocess
    try:
        process = subprocess.Popen(['/usr/sbin/y2-platform', 'media', 'reconcile'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
    except OSError:
        return {}
    return {'process': process, 'started': now}


def media_verify_needed(ctx):
    claim = ctx.json('/run/y2/media-mount.json', {})
    return (isinstance(claim, dict) and claim.get('read_only') == 'checking' and
            claim.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id'))


def media_verify(ctx, run, now):
    """The check of a card mounted while its check ran (media.verify) in its
    own process; it bounds its checker itself (media.CHECK_SECONDS). `run`
    as for media_reconcile. Returns the new state."""
    run = run or {}
    process = run.get('process')
    if process is not None:
        if process.poll() is None:
            return run
        run = {'retry_at': now + VERIFY_RETRY_S}
    if now < run.get('retry_at', 0) or not media_verify_needed(ctx):
        return run
    import subprocess
    try:
        process = subprocess.Popen(['/usr/sbin/y2-platform', 'media', 'verify'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
    except OSError:
        return {'retry_at': now + VERIFY_RETRY_S}
    return {'process': process, 'started': now}


def daily_update_check(ctx, network, child):
    """Start the daily release check (update.automatic_check) in its own
    process when Wi-Fi is online and it is due; the download may take seconds
    and must never stall this loop. Returns the still-running child, if any."""
    if child is not None:
        if child.poll() is None:
            return child
        child = None
    if network.get('state') != 'Online':
        return None
    last = ctx.json('/data/updates/last-check.json', {}) or {}
    last = last if isinstance(last, dict) else {}
    now = time.time()
    for key, period in (('time', 24 * 3600), ('attempt', 3600)):
        stamp = last.get(key)
        # Only a check that reached the server starts the next day; ≤ 8.14 also
        # stamped `time` on a failed one (report ef22a23d: no check for a day).
        if key == 'time' and last.get('result') != 'checked':
            continue
        if type(stamp) in (int, float) and 0 <= now - stamp < period:
            return None
    import subprocess
    try:
        return subprocess.Popen(['/usr/sbin/y2-platform', 'update', 'auto-check'],
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
    except OSError:
        return None


def error_reports(ctx, network, run, now, wall=None):
    """Start `report work` (new triggers, then the upload queue) in its own
    process: building a report takes ≈10 s of every core and an upload waits on
    the network, so neither may stall this loop. Only with reports on and an
    upload key in this build; due every REPORT_CHECK_S, or sooner when a queued
    report may be sent and Wi-Fi is online. `run` is the previous answer:
    {'next_check', 'process', 'started'}. Returns the new state."""
    from . import report
    run = dict(run or {'next_check': now + REPORT_FIRST_S})
    process = run.pop('process', None)
    started = run.pop('started', None)
    if process is not None:
        if process.poll() is None:
            if now - started < REPORT_LIMIT_S:
                return dict(run, process=process, started=started)
            kill_tree(process.pid)
            with contextlib.suppress(Exception):
                process.wait(timeout=1)
    try:
        if report.upload_key(ctx) is None or report.state(ctx)['enabled'] is not True:
            return run
        due = report.next_upload(ctx)
    except (OSError, ValueError, TypeError, AttributeError):
        return run                # an unreadable record never stops this loop
    check = now >= run['next_check']
    send = (due is not None and network.get('state') == 'Online' and now >= run.get('send_after', 0) and
            due <= (time.time() if wall is None else wall))
    if not (check or send):
        return run
    if check:
        run['next_check'] = now + REPORT_CHECK_S
    run['send_after'] = now + REPORT_SEND_GAP_S
    import subprocess
    try:
        process = subprocess.Popen(['/usr/sbin/y2-platform', 'report', 'work'],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
    except OSError:
        return run
    return dict(run, process=process, started=now)


def radio_firmware(ctx, child, ready, maintenance):
    """While the player has no radio firmware, look for the owner's
    Firmware/system.img (and keep the folder visible) in its own process: the
    extraction reads a 518 MB image and must never stall this loop."""
    from .radiofw import candidates, leftover_waiting, present
    # Re-checked every maintenance pass: firmware removed while running is
    # noticed too (found by the owner test of 49, archive 154), and so is a
    # system.img copied after the firmware was installed: the scan deletes it.
    if ready and not (maintenance and (not present(ctx) or leftover_waiting(ctx))):
        return child, True
    if child is not None:
        if child.poll() is None:
            return child, False
        child = None
    waiting = any(path.exists() for path in candidates(ctx))
    if present(ctx) and not (maintenance and leftover_waiting(ctx)):
        return None, True
    if not (waiting or maintenance):
        return None, False
    import subprocess
    try:
        return subprocess.Popen(['/usr/sbin/y2-platform', 'firmware', 'scan'],
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True), False
    except OSError:
        return None, False


def cadence(dark, settled):
    return (DARK_INTERVAL_S if dark else INTERVAL_S,
            DARK_MAINTENANCE_S if dark and settled else MAINTENANCE_S)


class SignalWake:
    """Wake the same select as supplicant events; retain signals until drained."""
    def __init__(self, running):
        self.read_fd, self.write_fd = os.pipe2(os.O_NONBLOCK | os.O_CLOEXEC)
        try:
            self.previous_fd = signal.set_wakeup_fd(self.write_fd, warn_on_full_buffer=False)
        except BaseException:
            os.close(self.read_fd)
            os.close(self.write_fd)
            raise
        def stop(*_):
            running[0] = False
        self.previous_handlers = {
            sig: signal.signal(sig, stop if sig != signal.SIGUSR1 else lambda *_: None)
            for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGUSR1)
        }

    def drain(self):
        try:
            os.read(self.read_fd, 4096)
        except BlockingIOError:
            pass

    def close(self):
        signal.set_wakeup_fd(self.previous_fd)
        for sig, handler in self.previous_handlers.items():
            signal.signal(sig, handler)
        os.close(self.read_fd)
        os.close(self.write_fd)


def serve(ctx):
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'service.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    monitor = EventMonitor(ctx)
    recovery = WifiRecovery(ctx)
    running = [True]
    wake = usb = None
    try:
        wake = SignalWake(running)
        dns_schedule = DnsProbeSchedule()
        next_maintenance = next_ntp = 0
        associating_since = None
        from .radio_policy import Coexistence, wifi_policy
        coexistence = Coexistence()
        next_radio_policy = 0
        settled = False
        from .usb import USBService
        usb = USBService(ctx)
        previous = ctx.json('/run/y2/network.json', {})
        query_failures = previous.get('status_query_failures', 0) if previous.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') else 0
        query_failures = query_failures if isinstance(query_failures, int) and query_failures >= 0 else 0
        update_check = None
        reports = None
        reconcile = verify = None
        firmware_scan, firmware_ready = None, False
        while running[0]:
            wake.drain()
            before = time.monotonic()
            dark = display_dark(ctx)
            interval, maintenance = cadence(dark, settled)
            monitor.poll()
            usb.tick()
            value = wifi(ctx)
            if value['state'] == 'Associating':
                associating_since = before if associating_since is None else associating_since
                if before - associating_since > 30:
                    value.update(state='Failed', reason='authentication_timeout')
            else:
                associating_since = None
            if dns_schedule.due(value, ctx.read('/run/y2/wifi-epoch'), before):
                if (probe_dns(ctx, value) or {}).get('ok') is True:
                    dns_schedule.confirmed(before)
                value = wifi(ctx, signal_cache=value)
            if value.get('radio_present') and value.get('supplicant_observation', {}).get('ok') is False:
                query_failures += 1
            recovery.observe(value, before, monitor)
            value.update(status_query_failures=query_failures, schema=1, boot_id=ctx.read('/proc/sys/kernel/random/boot_id'), monotonic_s=time.monotonic(),
                         supplicant_events=monitor.summary())
            atomic_json(runtime / 'network.json', value)
            activity = coexistence.observe(ctx, value, before)
            if before >= next_radio_policy and value['association'] == 'COMPLETED':
                next_radio_policy = before + 30
                wifi_policy(ctx, activity)
            atomic_json(runtime / 'time-status.json', time_status(ctx))
            reconcile = media_reconcile(ctx, reconcile, before)
            verify = media_verify(ctx, verify, before)
            if value['state'] == 'Online' and before >= next_ntp:
                next_ntp = before + 60
                ctx.command(['/etc/init.d/S43y2-time', 'start'], timeout=1)
            due = before >= next_maintenance
            firmware_scan, firmware_ready = radio_firmware(ctx, firmware_scan, firmware_ready, due)
            if due:
                next_maintenance = before + maintenance
                # Run slow fs/stat/cleanup outside this readiness process. A
                # kernel D-state does not block Wi-Fi or the power daemon.
                ctx.command(['/usr/sbin/y2-platform', 'space', '--cleanup'], timeout=2)
                # A restart without Wi-Fi resumes near this clock (#53); hourly.
                with contextlib.suppress(OSError):
                    remember_time(ctx)
                update_check = daily_update_check(ctx, value, update_check)
                reports = error_reports(ctx, value, reports, before)
                settled = settled or health_settled(
                    ctx.command(['/usr/sbin/y2-platform', 'update', 'health-ack'], timeout=8))
                ready = ctx.json('/run/y2/application-ready.json', {})
                journal = ctx.json('/data/system/platform/boot.json', {})
                if (ready.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
                        ready.get('first_frame') is True and journal.get('last_stage') in ('platform_start', 'services_started')):
                    ctx.command(['/usr/sbin/y2-platform', 'boot-stage', 'application_ready'], timeout=2)
            if value['state'] in ('Scanning', 'Associating', 'AcquiringIP'):
                interval = min(interval, 1)
            monitor.wait(max(.05, interval - (time.monotonic() - before)), wake_fd=wake.read_fd)
    finally:
        try:
            if usb is not None:
                usb.stop()
        finally:
            if wake is not None:
                wake.close()
            monitor.close()
            os.close(fd)
