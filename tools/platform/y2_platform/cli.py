"""Command line entry point for the versioned platform contract."""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
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
    media = sub.add_parser('media')
    media.add_argument('action', choices=['mount', 'unmount', 'check', 'status'])
    media.add_argument('--json', action='store_true')
    bench = sub.add_parser('bench-storage')
    bench.add_argument('--volume', choices=['/data', '/media/sd'], default='/data')
    bench.add_argument('--size-mib', type=int, default=16)
    bench.add_argument('--operations', type=int, default=64)
    bench.add_argument('--seconds', type=int, default=300)
    space = sub.add_parser('space')
    space.add_argument('--cleanup', action='store_true')
    args = parser.parse_args()
    ctx = Context()
    if args.command == 'status':
        if not 0 <= args.interval <= 10 or 0 < args.interval < 0.1 or len(args.pid) > 32:
            parser.error('interval must be zero or 0.1–10 seconds; at most 32 PIDs')
        result = snapshot(ctx, args.pid, args.pss, args.interval)
        if args.section:
            result = {'schema': result['schema'], 'record': result['record'], args.section:
                      result['system']['usb'] if args.section == 'usb' else result[args.section]}
    elif args.command == 'health':
        result = check(ctx, args.full)
    elif args.command == 'space':
        from .space import policy
        result = policy(ctx, args.cleanup)
    elif args.command == 'media':
        from .media import operation
        result = operation(ctx, args.action)
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
              args.command == 'bench-storage' and result['record']['result'] == 'FAILED')
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
