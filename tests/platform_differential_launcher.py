"""Run the Python platform CLI on a fixture root (the Python side of tests/platform_differential.py).

The installed entry (tools/platform/main.py, with its error wrapper) runs unchanged; only `cli.Context` is rooted
at --root and its subprocess runner answers from --commands and records every request in --calls.
"""
import argparse
import json
import os
from pathlib import Path
import runpy
import sys

PLATFORM = Path(__file__).resolve().parents[1] / 'tools/platform'


def audit(log, own):
    """Record what the code under test reaches outside the fixture root: a path opened or listed without
    `ctx.path`, or a process started without `ctx.command`. The interpreter's own files are not counted."""
    allowed = tuple(str(Path(p).resolve()) for p in [*own, PLATFORM, sys.prefix, sys.base_prefix,
                                                     sys.exec_prefix, Path(__file__).parent])
    stream = open(log, 'a', buffering=1)
    seen = set()

    def hook(event, arguments):
        if event in ('open', 'os.listdir', 'os.scandir', 'os.chmod', 'os.remove', 'os.rename', 'os.mkdir',
                     'os.symlink', 'os.truncate', 'os.utime'):
            path = arguments[0] if arguments else None
            if isinstance(path, int) or path is None:
                return
            path = os.fsdecode(path)
            if not os.path.isabs(path):
                path = os.path.join(os.getcwd(), path)
            if os.path.normpath(path).startswith(allowed) or path in ('/dev/null', '/dev/urandom'):
                return
            entry = (event, os.path.normpath(path))
        elif event in ('subprocess.Popen', 'os.exec', 'os.posix_spawn', 'os.system'):
            entry = (event, json.dumps([os.fsdecode(a) for a in arguments[1]]
                                       if event == 'subprocess.Popen' else str(arguments[0])))
        else:
            return
        if entry not in seen:
            seen.add(entry)
            stream.write(json.dumps({'event': entry[0], 'target': entry[1]}) + '\n')
    sys.addaudithook(hook)


def main():
    if sys.argv[1:2] != ['--root'] and 'Y2_ROOT' in os.environ:
        # Started by the Rust dispatcher's fallback (Y2_PLATFORM_MAIN): the fixture comes from the same
        # environment the Rust side got, every argument is the platform's.
        args = argparse.Namespace(root=os.environ['Y2_ROOT'], commands=os.environ['Y2_FIXTURE_COMMANDS'],
                                  calls=os.environ['Y2_FIXTURE_CALLS'], host_access=None)
        argv = sys.argv[1:]
        if os.environ.get('Y2_FIXTURE_FALLBACK'):
            Path(os.environ['Y2_FIXTURE_FALLBACK']).touch()  # the harness tells ported from fallen-back
    else:
        parser = argparse.ArgumentParser()
        parser.add_argument('--root', required=True)
        parser.add_argument('--commands', required=True)
        parser.add_argument('--calls', required=True)
        parser.add_argument('--host-access', help='append each file/process access outside --root as a JSON line')
        parser.add_argument('argv', nargs=argparse.REMAINDER)
        args = parser.parse_args()
        argv = args.argv[1:] if args.argv[:1] == ['--'] else args.argv
    answers = json.loads(Path(args.commands).read_text())

    def runner(command, timeout=1.0, **_):
        with open(args.calls, 'a') as calls:
            calls.write(json.dumps({'argv': list(command), 'timeout': timeout}) + '\n')
        for answer in answers:
            if answer['argv'] == list(command):
                return {'ok': answer['ok'], 'reason': answer.get('reason'), 'output': answer.get('output')}
        return {'ok': False, 'reason': 'fixture', 'output': None}

    if args.host_access:
        audit(args.host_access, [args.root, args.commands, args.calls, args.host_access])
    sys.path.insert(0, str(PLATFORM))
    from y2_platform import cli, common
    cli.Context = lambda: common.Context(args.root, runner)
    sys.argv = ['y2-platform', *argv]
    runpy.run_path(str(PLATFORM / 'main.py'), run_name='__main__')


if __name__ == '__main__':
    main()
