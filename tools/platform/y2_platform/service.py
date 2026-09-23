"""Bounded readiness/maintenance work, separate from the battery deadline owner."""
# SPDX-License-Identifier: GPL-2.0-only
import fcntl
import os
import signal
import time
from .boot import private_directory
from .common import atomic_json
from .network import EventMonitor, probe_dns
from .observe import wifi
from .timekeeping import status as time_status


def serve(ctx):
    runtime = private_directory(ctx.path('/run/y2'))
    fd = os.open(runtime / 'service.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    monitor = EventMonitor(ctx)
    running = [True]
    signal.signal(signal.SIGTERM, lambda *_: running.__setitem__(0, False))
    signal.signal(signal.SIGINT, lambda *_: running.__setitem__(0, False))
    next_probe = next_maintenance = next_ntp = 0
    associating_since = None
    try:
        while running[0]:
            before = time.monotonic()
            monitor.poll()
            value = wifi(ctx)
            if value['state'] == 'Associating':
                associating_since = before if associating_since is None else associating_since
                if before - associating_since > 30:
                    value.update(state='Failed', reason='authentication_timeout')
            else:
                associating_since = None
            if before >= next_probe and value['association'] == 'COMPLETED':
                next_probe = before + 30
                probe_dns(ctx, value)
                value = wifi(ctx)
            value.update(schema=1, boot_id=ctx.read('/proc/sys/kernel/random/boot_id'), monotonic_s=time.monotonic())
            atomic_json(runtime / 'network.json', value)
            atomic_json(runtime / 'time-status.json', time_status(ctx))
            ctx.command(['/usr/sbin/y2-platform', 'media', 'reconcile'], timeout=7)
            if value['state'] == 'Online' and before >= next_ntp:
                next_ntp = before + 60
                ctx.command(['/etc/init.d/S43y2-time', 'start'], timeout=1)
            if before >= next_maintenance:
                next_maintenance = before + 30
                # Run slow fs/stat/cleanup outside this readiness process. A
                # kernel D-state does not block Wi-Fi or the power daemon.
                ctx.command(['/usr/sbin/y2-platform', 'space', '--cleanup'], timeout=2)
                ctx.command(['/usr/sbin/y2-platform', 'update', 'health-ack'], timeout=8)
                ready = ctx.json('/run/y2/application-ready.json', {})
                journal = ctx.json('/data/system/platform/boot.json', {})
                if (ready.get('boot_id') == ctx.read('/proc/sys/kernel/random/boot_id') and
                        ready.get('first_frame') is True and journal.get('last_stage') in ('platform_start', 'services_started')):
                    ctx.command(['/usr/sbin/y2-platform', 'boot-stage', 'application_ready'], timeout=2)
            time.sleep(max(.05, 3 - (time.monotonic() - before)))
    finally:
        monitor.close()
        os.close(fd)
