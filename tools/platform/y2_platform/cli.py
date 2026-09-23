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
    status.add_argument('--interval', type=float, default=0)
    health = sub.add_parser('health')
    health.add_argument('--json', action='store_true')
    health.add_argument('--full', action='store_true')
    caps = sub.add_parser('capabilities')
    caps.add_argument('--json', action='store_true')
    boot = sub.add_parser('boot-stage')
    boot.add_argument('stage')
    bt_power = sub.add_parser('bluetooth-power')
    bt_power.add_argument('value', choices=['on', 'off'])
    sub.add_parser('power-daemon')
    sub.add_parser('service-daemon')
    sub.add_parser('time-bootstrap')
    sub.add_parser('time')
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
    media.add_argument('action', choices=['mount', 'unmount', 'check', 'status', 'reconcile'])
    media.add_argument('--json', action='store_true')
    bench = sub.add_parser('bench-storage')
    bench.add_argument('--volume', choices=['/data', '/media/sd'], default='/data')
    bench.add_argument('--size-mib', type=int, default=16)
    bench.add_argument('--operations', type=int, default=64)
    bench.add_argument('--seconds', type=int, default=300)
    space = sub.add_parser('space')
    space.add_argument('--cleanup', action='store_true')
    library = sub.add_parser('bench-library')
    library.add_argument('--volume', choices=['/data', '/media/sd'], default='/data')
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
    if args.command == 'bluetooth-power':
        from .bt_control import power
        result = power(ctx, args.value)
        print(json.dumps(result))
        return 0 if result['ok'] else 1
    if args.command == 'network-check':
        from .network_check import check
        result = check(ctx, args.peer, args.seconds, args.throughput, args.interface)
        print(json.dumps(result, sort_keys=True))
        return 0 if result['record']['result'] == 'OK' else 1
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
        result = snapshot(ctx, args.pid, args.pss, args.interval)
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
        from .media import operation, reconcile
        result = reconcile(ctx) if args.action == 'reconcile' else operation(ctx, args.action)
    elif args.command == 'bench-storage':
        from .bench import storage_benchmark
        result = storage_benchmark(ctx, args.volume, args.size_mib, args.operations, args.seconds)
    elif args.command == 'boot-stage':
        from .boot import mark
        result = mark(ctx, args.stage)
    else:
        result = ctx.json('/etc/y2linux/capabilities.json', {
            'schema': 'org.y2linux.capabilities/v1', 'state': 'Unavailable'})
        for name, path in [('wifi', '/data/network/enabled'), ('bluetooth', '/data/bluetooth/enabled')]:
            if name in result.get('capabilities', {}):
                result['capabilities'][name]['enabled'] = ctx.read(path) == '1'
    print(json.dumps(result, sort_keys=True, allow_nan=False))
    failed = (args.command == 'health' and result['state'] == 'FAILED' or
              args.command == 'media' and (result['state'] in ('Failed', 'Degraded') or
                                          args.action == 'mount' and result['state'] != 'Ready') or
              args.command in ('bench-storage', 'bench-library') and result['record']['result'] == 'FAILED')
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
