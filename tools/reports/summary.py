#!/usr/bin/env python3
"""The facts a first look at an error report needs, a few lines per report.

Answers what reporters are otherwise asked: which release, whether the radio
firmware is installed, whether Wi-Fi and Bluetooth run (and are switched on),
the battery and sleep, Internal Storage and the SD card, and why Reborn last
failed to start. Reads the .tar.gz in place (nothing is extracted).

  summary.py REPORT.tar.gz ...      the named reports
  summary.py --pending [--into DIR]  every report REVIEWED.md does not name yet
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
import re
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fetch  # noqa: E402  (sibling module: pending() and the default folder)

STATUS = 'commands/platform_status.txt'
STARTUP_LOG = 'files/data/reborn/logs/startup-last.jsonl'
SYSTEM_LOG = 'files/data/logs/system.log'
# BlueALSA's line for each radio stall it bounds (patch 0009, from Beta 9.3.34).
STALL = re.compile(r'Y2 stall: stream (\d+) ms behind')
# Time and charge per state (power plan 2026-11, phase 2), this boot.
ENERGY = 'files/data/system/platform/energy.json'
SETTINGS = {'wifi': 'files/data/network/enabled', 'bluetooth': 'files/data/bluetooth/enabled'}
MEMBER_LIMIT = 4 * 1024 * 1024


def members(path):
    """Name → text of the members a summary reads; missing ones are absent."""
    wanted = {'report.json', STATUS, STARTUP_LOG, SYSTEM_LOG, ENERGY, *SETTINGS.values()}
    found = {}
    with tarfile.open(path, 'r:gz') as archive:
        for info in archive:
            if info.name in wanted and info.isfile() and info.size <= MEMBER_LIMIT:
                found[info.name] = archive.extractfile(info).read().decode('utf-8', 'replace')
    return found


def json_or_empty(text):
    try:
        value = json.loads(text) if text else {}
    except ValueError:
        return {}
    return value if isinstance(value, dict) else {}


def get(value, *keys):
    """value[k1][k2]…, or None where a level is missing or not a mapping."""
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def readiness(status, name):
    state = get(status, 'readiness', name, 'state')
    reason = get(status, 'readiness', name, 'reason')
    return '%s (%s)' % (state, reason) if reason else str(state)


def switched(texts, name):
    value = texts.get(SETTINGS[name], '').strip()
    return {'1': 'on', '0': 'off'}.get(value, 'unknown')


def startup_failure(text):
    """The last fatal startup line Reborn wrote, or None."""
    fatal = None
    for line in (text or '').splitlines():
        record = json_or_empty(line)
        if record.get('event') == 'fatal' or record.get('level') == 'ERROR':
            fatal = record.get('message') or line
    return fatal


def stalls(text):
    """How many Bluetooth stalls the system log names, and the longest in ms (None without any)."""
    lengths = [int(m.group(1)) for m in STALL.finditer(text or '')]
    return (len(lengths), max(lengths)) if lengths else None


def energy(text):
    """One line from energy.json: hours per state and the gauge's mAh per state (None without one)."""
    record = json_or_empty(text)
    seconds = record.get('seconds')
    if not isinstance(seconds, dict) or not seconds:
        return None
    gauge = record.get('gauge_mah') if isinstance(record.get('gauge_mah'), dict) else {}
    parts = []
    for state, value in sorted(seconds.items(), key=lambda item: -float(item[1] or 0)):
        part = '%s %.1f h' % (state, float(value or 0) / 3600)
        if state in gauge:
            part += ' %.0f mAh' % float(gauge[state] or 0)
        parts.append(part)
    return '  energy (gauge estimate): ' + ', '.join(parts)


def summarize(path):
    texts = members(path)
    report = json_or_empty(texts.get('report.json'))
    status = json_or_empty(texts.get(STATUS))
    wifi = get(status, 'wifi') or {}
    power = get(status, 'power') or {}
    lines = [
        '%s  install %s  %s  %s  %s  uptime %ss' % (
            str(report.get('report_id'))[:8], str(report.get('install_id'))[:8],
            get(report, 'release', 'release_version'), ','.join(report.get('triggers') or []),
            report.get('created_utc'), report.get('uptime_seconds')),
        '  radio firmware: %s; core: %s' % (
            get(status, 'system', 'radio_firmware', 'state'), get(status, 'system', 'radio_core')),
        '  Wi-Fi: switched %s, %s, %s, %s dBm' % (
            switched(texts, 'wifi'), readiness(status, 'wifi'), wifi.get('association'), wifi.get('rssi_dbm')),
        '  Bluetooth: switched %s, %s' % (switched(texts, 'bluetooth'), readiness(status, 'bluetooth')),
        '  battery: %s %% %s, sleep %s' % (
            power.get('soc_percent'), get(power, 'battery', 'status'), get(power, 'sleep', 'state')),
        '  storage: %s, internal setup %s, SD %s' % (
            readiness(status, 'storage'), get(status, 'storage', 'internal_setup', 'state'),
            get(status, 'storage', 'media', 'lifecycle', 'reason')
            or get(status, 'storage', 'media', 'lifecycle', 'state')),
    ]
    spent = energy(texts.get(ENERGY))
    if spent:
        lines.append(spent)
    stalled = stalls(texts.get(SYSTEM_LOG))
    if stalled:
        lines.append('  Bluetooth stalls in the system log: %d, longest %d ms' % stalled)
    failure = startup_failure(texts.get(STARTUP_LOG))
    if failure:
        lines.append('  Reborn startup failure: %s' % failure)
    if not status:
        lines.append('  (no readable %s)' % STATUS)
    return '\n'.join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('reports', nargs='*', type=Path)
    parser.add_argument('--pending', action='store_true', help='every report %s does not name' % fetch.LEDGER)
    parser.add_argument('--into', type=Path, default=Path(__file__).resolve().parents[2] / 'out/reports')
    args = parser.parse_args(argv)
    paths = list(args.reports) + (fetch.pending(args.into) if args.pending else [])
    if not paths:
        parser.error('name reports or use --pending')
    failed = 0
    for path in paths:
        try:
            print(summarize(path))
        except (OSError, tarfile.TarError) as error:
            failed += 1
            print('%s: unreadable (%s)' % (path, error), file=sys.stderr)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
