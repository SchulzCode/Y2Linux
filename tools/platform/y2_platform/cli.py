"""Command line entry point for the versioned platform contract."""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
import sys
from .common import Context
from .health import check
from .observe import snapshot


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    status = sub.add_parser('status')
    status.add_argument('section', nargs='?', choices=['cpu', 'memory', 'thermal', 'power', 'storage',
                                                      'wifi', 'bluetooth', 'system', 'audio', 'readiness', 'usb'])
    status.add_argument('--json', action='store_true', help='JSON is the default output')
    status.add_argument('--pid', action='append', type=int, default=[])
    status.add_argument('--pss', action='store_true')
    status.add_argument('--reborn', action='store_true', help='observe current Reborn PID and bounded metrics')
    status.add_argument('--interval', type=float, default=0)
    health = sub.add_parser('health')
    health.add_argument('--json', action='store_true')
    health.add_argument('--full', action='store_true')
    caps = sub.add_parser('capabilities')
    caps.add_argument('--json', action='store_true')
    sub.add_parser('boot-evidence')
    boot = sub.add_parser('boot-stage')
    boot.add_argument('stage')
    idle_policy = sub.add_parser('cpu-idle-policy')
    idle_policy.add_argument('action', choices=['start', 'stop'])
    bt_power = sub.add_parser('bluetooth-power')
    bt_power.add_argument('value', choices=['on', 'off'])
    sleep = sub.add_parser('sleep')
    sleep.add_argument('action', choices=['request', 'status', 'prepare'])
    sub.add_parser('power-daemon')
    sub.add_parser('service-daemon')
    sub.add_parser('connectivity-daemon')
    usb_mode = sub.add_parser('usb-mode')
    usb_mode.add_argument('mode', choices=['mtp', 'developer'])
    codec_runtime = sub.add_parser('codec-runtime')
    codec_runtime.add_argument('--record', action='store_true')
    codec_settings = sub.add_parser('codec-settings')
    codec_settings.add_argument('--sbc-quality', choices=['high', 'xq', 'xq+'])
    codec_settings.add_argument('--sbc-abr', choices=['on', 'off'])
    codec_settings.add_argument('--ldac-quality', choices=['mobile', 'standard', 'high'])
    codec_settings.add_argument('--ldac-abr', choices=['on', 'off'])
    codec_settings.add_argument('--aac-quality', choices=['standard', 'high'])
    codec_settings.add_argument('--aac-abr', choices=['on', 'off'])
    sub.add_parser('wifi-power-policy')
    sub.add_parser('ssh-daemon')
    transfer = sub.add_parser('transfer')
    transfer.add_argument('action', choices=['begin', 'commit', 'status', 'discard'])
    transfer.add_argument('--id')
    transfer.add_argument('--name')
    transfer.add_argument('--bytes', type=int)
    transfer.add_argument('--sha256')
    reset = sub.add_parser('reset')
    reset.add_argument('action', choices=['plan', 'execute', 'resume', 'purge'])
    reset.add_argument('--scope', choices=['settings','network','bluetooth-bonds','library','caches','full-user'])
    reset.add_argument('--confirm')
    reset.add_argument('--id')
    reset.add_argument('--erase-user-music', action='store_true')
    sub.add_parser('export-diagnostics')
    error_report = sub.add_parser('report', help='opt-in error reports (docs/planning/ERROR-REPORTS-PLAN.md)')
    error_report.add_argument('action', choices=['status', 'enable', 'disable', 'new-id', 'send', 'save', 'check',
                                                 'upload', 'work'])
    error_report.add_argument('--to', choices=['internal', 'sd'], help='save only: Internal Storage or the SD card')
    error_report.add_argument('--topic', choices=['sound', 'display', 'battery', 'bluetooth', 'wifi', 'crash', 'other'])
    export = sub.add_parser('export-state')
    export.add_argument('--include-database', action='store_true')
    export.add_argument('--include-network', action='store_true')
    firmware = sub.add_parser('firmware', help="radio firmware from the owner's own copy")
    firmware.add_argument('action', choices=['status', 'scan', 'install'])
    firmware.add_argument('path', nargs='?', help='install: stock system.img or a directory of the reviewed files')
    update = sub.add_parser('update')
    update.add_argument('action', choices=['status', 'check', 'stage', 'apply', 'rollback', 'cancel', 'health-ack',
                                           'auto-check', 'probe-recovery', 'recovery-status'])
    update.add_argument('--url', help='HTTPS manifest.json URL (default: the release channel)')
    update.add_argument('--package', help='owner-supplied local signed package directory')
    sub.add_parser('time-bootstrap')
    sub.add_parser('time')
    time_set = sub.add_parser('time-set', help="the owner's date and time (#53); network time replaces it")
    time_set.add_argument('epoch', type=int, help='UTC seconds since 1970, within the build-date range')
    sub.add_parser('time-remember', help='write the durable clock floor now (orderly shutdown)')
    rtc_alarm = sub.add_parser('rtc-alarm')
    rtc_alarm.add_argument('seconds', type=int, help='seconds from RTC now; zero disables; does not suspend')
    ntp = sub.add_parser('ntp-event')
    ntp.add_argument('event', choices=['step', 'stratum', 'periodic', 'unsync'])
    wifi_event = sub.add_parser('wifi-event')
    wifi_event.add_argument('event', choices=['CONNECTED', 'DISCONNECTED', 'SERVICE_RESTART'])
    dhcp_event = sub.add_parser('dhcp-event')
    dhcp_event.add_argument('event', choices=['deconfig', 'leasefail', 'nak', 'bound', 'renew'])
    network_check = sub.add_parser('network-check')
    network_check.add_argument('--peer', required=True)
    network_check.add_argument('--seconds', type=int, default=10)
    network_check.add_argument('--throughput', action='store_true')
    network_check.add_argument('--interface', choices=['wlan0', 'usb0'], default='wlan0')
    shutdown = sub.add_parser('shutdown')
    shutdown.add_argument('action', choices=['poweroff', 'reboot'])
    shutdown.add_argument('--reason', choices=['user', 'service', 'update'], default='user')
    media = sub.add_parser('media')
    media.add_argument('action', choices=['mount', 'unmount', 'check', 'status', 'reconcile', 'format', 'grow'])
    media.add_argument('--json', action='store_true')
    media.add_argument('--confirm-cid', help='format only: the CID shown by `media status`; formats exactly that card')
    internal = sub.add_parser('internal-media')
    internal.add_argument('action', choices=['status', 'mount', 'unmount', 'provision', 'setup'])
    internal.add_argument('--confirm-serial', help='provision/setup: the stock FAT serial from `status`; that filesystem is replaced')
    internal.add_argument('--automatic', action='store_true',
                          help='setup only: decide by itself at start (keeps a few files, waits for a large library)')
    internal.add_argument('--confirm-shown', action='store_true',
                          help='setup only: the listener confirmed the old storage this boot reported as waiting')
    internal.add_argument('--no-migrate', action='store_true', help='provision only: leave /data/music untouched')
    bench = sub.add_parser('bench-storage')
    bench.add_argument('--volume', choices=['/data', '/media/sd', '/media/internal'], default='/data')
    bench.add_argument('--size-mib', type=int, default=16)
    bench.add_argument('--operations', type=int, default=64)
    bench.add_argument('--seconds', type=int, default=300)
    space = sub.add_parser('space')
    space.add_argument('--cleanup', action='store_true')
    library = sub.add_parser('bench-library')
    library.add_argument('--volume', choices=['/data', '/media/sd', '/media/internal'], default='/data')
    library.add_argument('--tracks', type=int, choices=[1000, 10000, 20000], default=1000)
    library.add_argument('--scan', action='store_true')
    collect = sub.add_parser('collect')
    collect.add_argument('--seconds', type=int, default=60)
    collect.add_argument('--interval', type=int, default=5)
    collect.add_argument('--warmup', type=int, default=0)
    collect.add_argument('--workload', default='idle')
    collect.add_argument('--pid', action='append', type=int, default=[])
    collect.add_argument('--pss', action='store_true')
    collect.add_argument('--reborn', action='store_true')
    args = parser.parse_args()
    ctx = Context()
    if args.command == 'usb-mode':
        from .usb import request_mode
        print(json.dumps(request_mode(ctx, args.mode)))
        return 0
    if args.command == 'cpu-idle-policy':
        from .cpu_idle import apply
        result = apply(ctx, args.action)
        print(json.dumps(result, sort_keys=True))
        return 1 if result['state'] == 'Failed' else 0
    if args.command == 'sleep':
        from . import sleep
        result = {'request': sleep.request, 'prepare': sleep.prepare, 'status': sleep.status}[args.action](ctx)
        print(json.dumps(result, sort_keys=True))
        return 0
    if args.command == 'rtc-alarm':
        from .timekeeping import alarm
        print(json.dumps(alarm(ctx, args.seconds)))
        return 0
    if args.command == 'codec-settings':
        from .codec_controls import configure
        try:
            result = configure(ctx, sbc_quality=args.sbc_quality,
                               sbc_abr=None if args.sbc_abr is None else args.sbc_abr == 'on',
                               ldac_quality=args.ldac_quality,
                               ldac_abr=None if args.ldac_abr is None else args.ldac_abr == 'on',
                               aac_quality=args.aac_quality,
                               aac_abr=None if args.aac_abr is None else args.aac_abr == 'on')
        except (ValueError, OSError) as error:
            print(json.dumps({'schema': 'org.y2linux.codec-settings/v1', 'failure': str(error)}))
            return 1
        print(json.dumps(result))
        return 0
    if args.command == 'codec-runtime':
        from .radio_policy import codec_runtime
        print(' '.join(codec_runtime(ctx, record=args.record)))
        return 0
    if args.command == 'wifi-power-policy':
        from .radio_policy import wifi_policy
        result = wifi_policy(ctx)
        print(json.dumps(result))
        return 0 if result.get('state') == 'Ready' else 1
    if args.command == 'boot-evidence':
        from .boot import capture_previous
        print(json.dumps(capture_previous(ctx), sort_keys=True))
        return 0
    if args.command == 'report':
        from . import report
        if args.action in ('send', 'save', 'check', 'work'):
            report.lower_priority()
        try:
            if args.action == 'status':
                result = report.status(ctx)
            elif args.action in ('enable', 'disable'):
                result = report.set_enabled(ctx, args.action == 'enable')
            elif args.action == 'new-id':
                result = report.new_install_id(ctx)
            elif args.action == 'send':
                progress = report.Progress(ctx)
                try:
                    result = report.queue(ctx, 'manual', topic=args.topic, progress=progress)
                    if result.get('queued'):
                        # Sent at once when it can be; otherwise it waits in the queue.
                        result['upload'] = report.upload_now(ctx, progress=progress, first=result['name'])
                finally:
                    progress.close()
            elif args.action == 'save':
                if args.to is None:
                    raise ValueError('save_needs_to')
                progress = report.Progress(ctx, args.to)
                try:
                    result = report.save_to(ctx, args.to, topic=args.topic, progress=progress)
                finally:
                    progress.close()
            elif args.action == 'upload':
                result = report.upload(ctx)
            elif args.action == 'work':
                # The platform service's periodic run: new triggers, then the queue.
                result = {'reports': report.run_checks(ctx), 'upload': report.upload_now(ctx)}
            else:
                result = {'reports': report.run_checks(ctx)}
            print(json.dumps(result, sort_keys=True))
            return 0
        except (OSError, ValueError) as error:
            print(json.dumps({'failure': 'report_failed', 'reason': type(error).__name__}))
            return 1
    if args.command == 'export-diagnostics':
        from .diagnostics import export
        try:
            result = export(ctx)
            print(json.dumps(result, sort_keys=True))
            return 0
        except (OSError, ValueError, TimeoutError):
            print(json.dumps({'state': 'Failed', 'failure': 'diagnostic_export_failed'}))
            return 1
    if args.command in ('reset', 'export-state'):
        from . import maintenance
        try:
            if args.command == 'export-state':
                result = maintenance.export(ctx, args.include_database, args.include_network)
            elif args.action == 'plan':
                result = maintenance.plan(ctx, args.scope)
            elif args.action == 'purge':
                result = maintenance.purge(ctx, args.id or '', args.confirm)
            else:
                result = maintenance.execute(ctx, args.confirm, args.erase_user_music, args.action == 'resume')
            print(json.dumps(result, sort_keys=True))
            return 0
        except (OSError, ValueError, TimeoutError) as error:
            print(json.dumps({'state':'Failed','failure':str(error)}))
            return 1
    if args.command == 'firmware':
        from . import radiofw
        try:
            if args.action == 'install' and not args.path:
                raise ValueError('install needs a system.img or a directory')
            print(json.dumps(radiofw.main(ctx, args.action, args.path), sort_keys=True))
            return 0
        except (OSError, ValueError, TimeoutError) as error:
            print(json.dumps({'state': 'Failed', 'failure': str(error)}))
            return 1
    if args.command == 'update':
        from . import update
        try:
            if args.action in ('check', 'stage'):
                url = args.url or (update.channel(ctx).get('manifest_url') if not args.package else None)
                result = update.stage(ctx, args.package, url, args.action == 'check')
            elif args.action == 'auto-check':
                result = update.automatic_check(ctx)
            elif args.action == 'probe-recovery':
                result = update.probe(ctx)
            elif args.action == 'recovery-status':
                result = update.core(ctx, 'misc')
            elif args.action in ('apply', 'rollback'):
                result = update.apply(ctx, args.action == 'rollback')
            elif args.action == 'cancel':
                result = update.cancel(ctx)
            else:
                result = update.health_ack(ctx) if args.action == 'health-ack' else update.status(ctx)
            print(json.dumps(result, sort_keys=True))
            return 0
        except (OSError, ValueError, TimeoutError) as error:
            print(json.dumps({'state': 'Failed', 'failure': str(error)}))
            return 1
    if args.command == 'transfer':
        from .transfer import begin, commit, inspect, discard
        if args.action == 'begin':
            result = begin(ctx, args.name, args.bytes, args.sha256 or '')
        else:
            result = {'commit': commit, 'status': inspect, 'discard': discard}[args.action](ctx, args.id or '')
        print(json.dumps(result, sort_keys=True))
        return 0
    if args.command == 'ssh-daemon':
        from .ssh import serve
        serve(ctx)
        return 0
    if args.command == 'bluetooth-power':
        from .bt_control import power
        result = power(ctx, args.value)
        print(json.dumps(result))
        return 0 if result['ok'] else 1
    if args.command == 'network-check':
        from .network_check import check as network_check
        result = network_check(ctx, args.peer, args.seconds, args.throughput, args.interface)
        print(json.dumps(result, sort_keys=True))
        return 0 if result['record']['result'] == 'OK' else 1
    if args.command in ('time-set', 'time-remember'):
        from .timekeeping import set_manual, remember
        try:
            result = set_manual(ctx, args.epoch) if args.command == 'time-set' else remember(ctx, every=0)
        except (OSError, ValueError) as error:
            print(json.dumps({'state': 'Failed', 'failure': str(error)}))
            return 1
        print(json.dumps(result or {'state': 'Unchanged'}))
        return 0
    if args.command in ('time', 'time-bootstrap', 'ntp-event'):
        from .timekeeping import status, bootstrap, ntp_event
        import os
        result = status(ctx) if args.command == 'time' else bootstrap(ctx) if args.command == 'time-bootstrap' else ntp_event(ctx, args.event, os.environ)
        print(json.dumps(result))
        return 0
    if args.command in ('wifi-event', 'dhcp-event'):
        from .network import lease_event, dhcp_event
        import os
        result = lease_event(ctx, args.event) if args.command == 'wifi-event' else dhcp_event(ctx, args.event, os.environ)
        print(json.dumps(result))
        return 0
    if args.command == 'connectivity-daemon':
        from .connectivity import serve
        serve(ctx)
        return 0
    if args.command == 'service-daemon':
        from .service import serve
        serve(ctx)
        return 0
    if args.command == 'power-daemon':
        from .power import serve
        serve(ctx)
        return 0
    if args.command == 'shutdown':
        from .power import rpc
        print(json.dumps(rpc(ctx, {'action': args.action, 'reason': args.reason})))
        return 0
    if args.command == 'collect':
        from .collect import collect
        collect(ctx, sys.stdout, args.seconds, args.interval, args.pid, args.pss,
                args.workload, args.warmup, args.reborn)
        return 0
    if args.command == 'status':
        if not 0 <= args.interval <= 10 or 0 < args.interval < 0.1 or len(args.pid) > 32:
            parser.error('interval must be zero or 0.1–10 seconds; at most 32 PIDs')
        if args.reborn:
            from .power import process_identity
            current = process_identity(ctx, 'process', '/usr/bin/reborn')
            args.pid = list(dict.fromkeys([*args.pid, *([current['pid']] if current else [])]))[:32]
        result = snapshot(ctx, args.pid, args.pss, args.interval)
        if args.reborn:
            from .collect import metrics
            result['reborn_metrics'] = metrics(ctx)
        if args.section:
            result = {'schema': result['schema'], 'record': result['record'], args.section:
                      result['system']['usb'] if args.section == 'usb' else result[args.section]}
    elif args.command == 'health':
        result = check(ctx, args.full)
    elif args.command == 'bench-library':
        from .bench import library_benchmark
        result = library_benchmark(ctx, args.volume, args.tracks, args.scan)
    elif args.command == 'space':
        from .space import policy
        result = policy(ctx, args.cleanup)
    elif args.command == 'media':
        from .media import change_card, operation, reconcile
        if args.action in ('format', 'grow'):
            result = change_card(ctx, args.action, args.confirm_cid)
        else:
            result = reconcile(ctx) if args.action == 'reconcile' else operation(ctx, args.action)
    elif args.command == 'internal-media':
        from . import internal_media
        if args.action == 'status':
            result = internal_media.status(ctx)
        elif args.action == 'provision':
            result = internal_media.provision(ctx, args.confirm_serial, not args.no_migrate)
        elif args.action == 'setup':
            serial = args.confirm_serial
            if args.confirm_shown:
                shown = internal_media.setup_status(ctx)
                serial = shown.get('serial') if shown.get('state') == 'Waiting' else None
            result = internal_media.setup(ctx, args.automatic, serial)
        else:
            result = getattr(internal_media, args.action)(ctx)
    elif args.command == 'bench-storage':
        from .bench import storage_benchmark
        result = storage_benchmark(ctx, args.volume, args.size_mib, args.operations, args.seconds)
    elif args.command == 'boot-stage':
        from .boot import mark
        result = mark(ctx, args.stage)
    else:
        from .capabilities import status as capability_status
        result = capability_status(ctx)
        # A root overlay may retain an older capability template. Runtime
        # identity always comes from the installed release, never the template.
        result['platform_version'] = ctx.json('/etc/y2linux/versions.json', {}).get('release_version')
        for name, path in [('wifi', '/data/network/enabled'), ('bluetooth', '/data/bluetooth/enabled')]:
            if name in result.get('capabilities', {}):
                result['capabilities'][name]['enabled'] = ctx.read(path) == '1'
    print(json.dumps(result, sort_keys=True, allow_nan=False))
    failed = (args.command == 'health' and result['state'] == 'FAILED' or
              args.command == 'internal-media' and (
                  args.action in ('mount', 'provision') and result['state'] != 'Ready' or
                  args.action == 'setup' and result['state'] in ('Failed', 'Unavailable') or
                  args.action == 'unmount' and result['state'] == 'Failed') or
              args.command == 'media' and (result['state'] in ('Failed', 'Degraded') or
                                          args.action in ('mount', 'format', 'grow') and result['state'] != 'Ready') or
              args.command in ('bench-storage', 'bench-library') and result['record']['result'] == 'FAILED')
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
