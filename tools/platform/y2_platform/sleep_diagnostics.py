"""Persistent sleep and USB diagnostics that survive a hold-Power restart.

POWER-FINAL-14. 13's counters (VPROC sleep, PHY save-current, wheel sleep,
INFRA power-down, the golden capture at sleep entry) live only in kernel memory,
and the USB-after-wake defect is only recoverable by a restart, which erased
them twice. Each row here is read-only sysfs/procfs plus a filtered kernel-log
tail, appended to a bounded private file on Y2DATA. A failure is counted and
never blocks, delays or changes sleep.
"""
# SPDX-License-Identifier: GPL-2.0-only
import errno
import json
import os
import re
import time
from .common import read

PATH = '/data/system/platform/sleep-diagnostics.jsonl'
LIMIT = 2097152
ROW_LIMIT = 65536
SPM = '/sys/devices/platform/10006000.power-controller/'
USB_SUMMARY = ('stage', 'error', 'configured', 'detached', 'monitor_parked', 'cable_events',
               'runtime_status', 'runtime_held', 'pm_suspends', 'pm_restores', 'irqs',
               'dma_errors', 'dma_aborts', 'reconnect_failures', 'monitor_failures',
               'dma_tx_programs', 'dma_irqs', 'dma_unmask_rearms', 'dma_unmask_lost',
               'dma_intr_before', 'dma_intr_after')
# Kernel messages kept in a row: USB, gadget, network and the sleep path.
KERNEL_TOKENS = ('Y2USB', 'Y2NET', 'Y2SPM', 'Y2PM', 'Y2INFRA', 'Y2VPROC', 'Y2DVFS', 'Y2_DPM',
                 'y2-usb', 'y2-spm', 'y2-pmic', 'apt32f-wheel', 'musb', 'udc', 'configfs',
                 'gadget', 'usb0', 'PM: ')
PROCESSES = ('umtprd', 'dropbear')
FOLLOW_UP_S = 30
MIN_INTERVAL_S = 10


def data_ready(ctx):
    """Never create persistent state on an unverified or missing data mount."""
    from .observe import mountinfo
    mounts = [m for m in mountinfo(ctx.read('/proc/self/mountinfo')) if m['path'] == '/data']
    return (ctx.read('/data/.y2data-schema') == '1' and len(mounts) == 1 and
            mounts[0]['source'] == ctx.read('/run/y2-data-device'))


def key_values(text, keys=None):
    found = dict(re.findall(r'\b([a-z0-9_]+)=(\S+)', text or ''))
    return found if keys is None else {key: found.get(key) for key in keys}


def ipv4(ctx, interface):
    """Local IPv4 addresses of one interface from /proc/net/fib_trie: no subprocess, no ioctl."""
    routes = ctx.read('/proc/net/route', limit=65536) or ''
    networks = set()
    for line in routes.splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 8 and fields[0] == interface:
            networks.add((int(fields[1], 16), int(fields[7], 16)))
    found, previous = set(), None
    for line in (ctx.read('/proc/net/fib_trie', limit=262144) or '').splitlines():
        text = line.strip()
        if '/32 host LOCAL' in text and previous:
            octets = [int(x) for x in previous.split('.')]
            value = octets[0] | octets[1] << 8 | octets[2] << 16 | octets[3] << 24
            if any(value & mask == network for network, mask in networks):
                found.add(previous)
        match = re.match(r'[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$', text)
        previous = match.group(1) if match else previous if '/' in text else None
    return sorted(found)


def processes(ctx):
    running = {name: False for name in PROCESSES}
    try:
        entries = os.scandir(ctx.path('/proc'))
    except OSError:
        return running
    with entries:
        for entry in entries:
            if entry.name.isdigit():
                name = read(os.path.join(entry.path, 'comm'), limit=64)
                if name in running:
                    running[name] = True
    return running


def usb_net(ctx):
    """Compact USB gadget and USB network state (about 1 KiB)."""
    net = '/sys/class/net/usb0/'
    statistics = {name: ctx.integer(net + 'statistics/' + name)
                  for name in ('rx_packets', 'tx_packets', 'rx_bytes', 'tx_bytes',
                               'rx_errors', 'tx_errors', 'rx_dropped', 'tx_dropped')}
    return {'udc': {p.name: read(p / 'state', limit=64) for p in ctx.glob('/sys/class/udc/*')},
            'gadget_udc': ctx.read('/sys/kernel/config/usb_gadget/y2/UDC', limit=128),
            'controller': {p.parent.name: key_values(read(p, limit=8192), USB_SUMMARY)
                           for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/status')},
            'out_path': {p.parent.name: (read(p, limit=4096) or '').splitlines()[:1]
                         for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/rxdiag')},
            'usb0': {'operstate': ctx.read(net + 'operstate', limit=64),
                     'carrier': ctx.read(net + 'carrier', limit=8),
                     'flags': ctx.read(net + 'flags', limit=32),
                     'ipv4': ipv4(ctx, 'usb0'), 'statistics': statistics},
            'transfer': ctx.json('/run/y2/usb-transfer.json'),
            'processes': processes(ctx)}


def features(ctx, golden=False):
    """The 13 sleep-parity switches and counters, the INFRA and SPM state."""
    value = {'vproc_sleep': ctx.read('/sys/module/pwrap/parameters/vproc_sleep', limit=16),
             'vproc_sleep_state': ctx.read('/sys/module/pwrap/parameters/vproc_sleep_state', limit=4096),
             'pmic_sleep_state': ctx.read('/sys/module/pwrap/parameters/pmic_sleep_state', limit=4096),
             'lp_sel': ctx.read('/sys/firmware/y2_pmic/lp_sel', limit=4096),
             'phy_savecurrent': {p.parent.name: read(p, limit=4096)
                                 for p in ctx.glob('/sys/bus/platform/drivers/y2-usb/*/phy_savecurrent')},
             'wheel_sleep': {p.parent.name: read(p, limit=4096)
                             for p in ctx.glob('/sys/bus/i2c/devices/*-0051/sleep')},
             'infra': ctx.read('/sys/firmware/y2_infra/state', limit=4096),
             'spm': ctx.read(SPM + 'state', limit=8192),
             # 27: stock panel power-down and the idle VGP2 rail (ledger F20/F22).
             'panel_stock_suspend': ctx.read('/sys/module/panel/parameters/stock_suspend', limit=16),
             'panel_power_down': ctx.read('/sys/module/panel/parameters/power_down', limit=16),
             'vgp2_idle_off': ctx.read('/sys/module/y2_cs43131/parameters/vgp2_idle_off', limit=16),
             # 32: deep idle during playback and how often it was admitted.
             'deep_idle_audio': ctx.read('/sys/module/clocks/parameters/deep_idle_audio', limit=16),
             'deep_audio_entries': ctx.read('/sys/module/clocks/parameters/deep_audio_entries', limit=32)}
    if golden:
        # Read-only always-on/INFRA registers; the entry column is the last
        # suspend-entry capture, so read it once Linux has returned.
        value['golden'] = ctx.read(SPM + 'golden', limit=16384)
        value['pmic_golden'] = ctx.read('/sys/module/pwrap/parameters/pmic_golden', limit=16384)
    return value


def kernel_tail(ctx, lines=80, width=240):
    """The last relevant kernel messages, read non-blocking from /dev/kmsg."""
    try:
        fd = os.open(ctx.path('/dev/kmsg'), os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    except OSError:
        return None
    kept = []
    try:
        for _ in range(16384):
            try:
                block = os.read(fd, 8192)
            except OSError as error:
                if error.errno == errno.EPIPE:
                    continue
                break
            if not block:
                break
            for record in block.decode(errors='replace').splitlines():
                if record.startswith(' '):
                    continue
                header, _, message = record.partition(';')
                fields = header.split(',')
                if any(token in message[:96] for token in KERNEL_TOKENS):
                    stamp = fields[2] if len(fields) > 2 else '?'
                    kept.append(f'{stamp} {message}'[:width])
                    del kept[:-lines]
    finally:
        os.close(fd)
    return kept


class Diagnostics:
    """Rows on suspend stages and around USB state changes."""

    def __init__(self, ctx):
        self.ctx = ctx
        self.failures = 0
        self.usb_signature = None
        self.follow_up = None
        self.last_row = None

    def record(self, stage, now=None, golden=False, usb_reason=None):
        now = time.monotonic() if now is None else now
        try:
            if not data_ready(self.ctx):
                return False
            ctx = self.ctx
            row = {'schema': 1, 'stage': stage, 'usb_reason': usb_reason,
                   'wall_time': time.time(), 'monotonic_s': now,
                   'boottime_s': time.clock_gettime(time.CLOCK_BOOTTIME),
                   'boot_id': ctx.read('/proc/sys/kernel/random/boot_id'),
                   'build_id': (ctx.json('/etc/y2linux/versions.json', {}) or {}).get('build_id'),
                   'features': features(ctx, golden=golden), 'usb_net': usb_net(ctx),
                   'kernel_pm': ctx.read('/sys/firmware/y2_pm/state', limit=8192),
                   'kernel_tail': kernel_tail(ctx)}
            encoded = (json.dumps(row, separators=(',', ':')) + '\n').encode()
            while len(encoded) > ROW_LIMIT and row['kernel_tail']:
                del row['kernel_tail'][:max(1, len(row['kernel_tail']) // 4)]
                encoded = (json.dumps(row, separators=(',', ':')) + '\n').encode()
            if len(encoded) > ROW_LIMIT:
                raise ValueError('sleep_diagnostics_row_too_large')
            from .power_journal import append_bounded
            append_bounded(ctx, PATH, encoded, LIMIT, durable=True)
            self.last_row = now
            return True
        except (OSError, ValueError, TypeError):
            self.failures += 1
            return False

    def observe_usb(self, now=None):
        """Called on the existing one-second power sample; cheap when nothing changed."""
        now = time.monotonic() if now is None else now
        signature = tuple(sorted((p.name, read(p / 'state', limit=64))
                                 for p in self.ctx.glob('/sys/class/udc/*')))
        reason = None
        if self.usb_signature is not None and signature != self.usb_signature:
            reason = 'udc_change'
            self.follow_up = now + FOLLOW_UP_S
        elif self.follow_up is not None and now >= self.follow_up:
            reason = 'udc_follow_up'
            self.follow_up = None
        self.usb_signature = signature
        if reason is None:
            return False
        if self.last_row is not None and now - self.last_row < MIN_INTERVAL_S:
            # Rate-limited: a change still gets its follow-up, a follow-up is postponed.
            self.follow_up = now + FOLLOW_UP_S if reason == 'udc_change' else self.last_row + MIN_INTERVAL_S
            return False
        return self.record('usb', now=now, usb_reason=reason)


def record(ctx, stage):
    """One row for a suspend stage; the golden table only after Linux returned."""
    return Diagnostics(ctx).record(stage, golden=stage == 'kernel_resumed')
