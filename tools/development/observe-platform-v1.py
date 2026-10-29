#!/usr/bin/env python3
"""Read-only Platform v1 worktree inventory; write receipts only below --output.
RETIRED for the player: its device side runs Python, and the player has none since test build 169
(docs/planning/PLATFORM-RUST-REFACTOR-PLAN.md §2.6). Kept for its host-side logic and history.

Never infer completion, acceptance, physical success or candidate currency from
Git activity. Each snapshot is a bounded observation of independently live trees.
No device access, ref/index mutations, build, merge, checkout or network requests.
"""
import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess


BASES = {
    'Y2Linux': 'b5aea937a3b8e3e8ec398f8cb2e04656b8a53f8c',
    'Y2Reborn': 'b71b468860233faa0a42b8448ec5777fa952b8e3',
}
PROTECTED = {
    'Y2Linux': ['kernel/platform/idle.c', 'kernel/platform/spm.c',
                'kernel/platform/spm-idle-policy.h', 'kernel/platform/spm-uart-policy.h',
                'kernel/platform/uart-idle-policy.h', 'kernel/platform/spm-idle-pcm.h',
                'kernel/platform/local-timer.c', 'kernel/platform/local-timer.h',
                'kernel/platform/cirq.c', 'kernel/platform/idle-clock-policy.h',
                'kernel/platform/system-idle.c', 'kernel/platform/idle-completion-policy.h',
                'tools/platform/y2_platform/cpu_idle.py',
                'buildroot/board/y2/production-overlay/etc/init.d/S05y2-cpu-idle'],
    'Y2Reborn': ['crates/reborn-graphics/native/graphics.c',
                 'crates/reborn-graphics/src/native.rs', 'app/reborn/src/main.rs'],
}


def git(path, *args):
    result = subprocess.run(['git', '--no-optional-locks', '-C', str(path), *args],
                            env=dict(os.environ, GIT_OPTIONAL_LOCKS='0'),
                            capture_output=True, timeout=90)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace').strip())
    return result.stdout.decode(errors='replace')


def status(path):
    fields = iter(git(path, 'status', '--porcelain=v1', '-z',
                      '--untracked-files=all').split('\0'))
    changes = []
    for field in fields:
        if not field:
            continue
        value = {'status': field[:2], 'path': field[3:]}
        if 'R' in field[:2] or 'C' in field[:2]:
            value['old_path'] = next(fields)
        changes.append(value)
    return changes


def fingerprint(path):
    try:
        stat = path.lstat()
        value = {'size': stat.st_size, 'mtime_ns': stat.st_mtime_ns}
        if path.is_symlink():
            value['symlink'] = os.readlink(path)
        elif path.is_file() and stat.st_size <= 4 * 1024 * 1024:
            value['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
        return value
    except OSError as error:
        return {'error': str(error)}


def worktrees(repo):
    values = []
    for block in git(repo, 'worktree', 'list', '--porcelain').strip().split('\n\n'):
        record = {}
        for line in block.splitlines():
            key, _, value = line.partition(' ')
            record[key] = value or True
        values.append(record)
    return values


def nearby_checkouts(code_root):
    """Bounded layout discovery, including independently cloned owner sources."""
    parents = [code_root]
    directory = code_root / 'worktrees'
    if directory.is_dir():
        parents.append(directory)
        parents.extend(path for path in directory.iterdir() if path.is_dir())
    return sorted({parent / name for parent in parents
                   for name in ('Y2Linux', 'Y2Reborn', 'Y2Player')
                   if (parent / name / '.git').exists()})


def auxiliary_checkout(path, snapshot, index):
    """Inventory supplementary sources; never treat them as integration targets."""
    values = worktrees(path)
    result = {'path': str(path), 'integration_target': False,
              'registered_worktrees': values,
              'refs': git(path, 'for-each-ref', '--format=%(refname:short) %(objectname)').splitlines()}
    for value in values:
        root = Path(value['worktree'])
        value['exists'] = root.is_dir()
        if value['exists']:
            value['head'] = git(root, 'rev-parse', 'HEAD').strip()
            value['changes'] = status(root)
            value['dirty'] = bool(value['changes'])
            value['commits_not_in_local_main'] = git(root, 'log', '--format=%H %s',
                                                     'main..HEAD').splitlines()
            value['head_after'] = git(root, 'rev-parse', 'HEAD').strip()
            value['stable'] = (value['head'] == value['head_after'] and
                               value['changes'] == status(root))
    (snapshot / f'auxiliary-{index}-graph.txt').write_text(
        git(path, 'log', '--all', '--graph', '--decorate', '--oneline', '-120'))
    return result


def inspect(record, name, snapshot):
    value = dict(record)
    path = Path(record['worktree'])
    value['exists'] = path.is_dir()
    if not value['exists']:
        return value
    try:
        head = git(path, 'rev-parse', 'HEAD').strip()
        changes = status(path)
        files = git(path, 'diff', '--name-only', BASES[name], head).splitlines()
        value.update(head=head, changes=changes, dirty=bool(changes),
                     committed_files=files,
                     commits_since_foundation=git(path, 'log', '--format=%H %s',
                                                 BASES[name] + '..' + head).splitlines(),
                     commits_not_in_main=git(path, 'log', '--format=%H %s',
                                            'main..' + head).splitlines(),
                     main_relation=git(path, 'rev-list', '--left-right', '--count',
                                       'main...' + head).strip(),
                     retains_foundation=git(path, 'merge-base', BASES[name], head).strip() == BASES[name])
        branch = str(record.get('branch', ''))
        session = (path.parent.name if
                   (branch.startswith('refs/heads/codex/platform-v1-') or
                    (path.parent.name.startswith('platform-v1-') and
                     not path.parent.name.endswith('-worktrees'))) and
                   branch != 'refs/heads/codex/platform-v1-integration' else None)
        value['session'] = session
        if session:
            paths = sorted(set(files) | {item['path'] for item in changes})
            value['files'] = {file: fingerprint(path / file) for file in paths}
            destination = snapshot / (session + '-' + name)
            destination.mkdir()
            diff = git(path, 'diff', '--no-ext-diff', '--no-textconv', BASES[name])
            (destination / 'tracked.diff').write_text(diff)
            value['diff_sha256'] = hashlib.sha256(diff.encode()).hexdigest()
            value['reports'] = []
            value['copied_text_files'] = []
            for file in paths:
                source = path / file
                suffixes = {'.md', '.json', '.rs', '.py', '.c', '.h', '.sh', '.patch',
                            '.config', '.mk', '.hash', '.toml', '.asm'}
                if source.suffix in suffixes or file.startswith('buildroot/'):
                    if (not source.is_symlink() and source.is_file() and
                            source.stat().st_size < 2 * 1024 * 1024):
                        contents = source.read_bytes()
                        if b'\0' in contents:
                            continue
                        target = destination / file
                        target.parent.mkdir(parents=True, exist_ok=True)
                        target.write_bytes(contents)
                        value['copied_text_files'].append(file)
                        if file.startswith(('docs/validation/', 'docs/release/')):
                            value['reports'].append(file)
            value['protected_changes'] = sorted(set(paths) & set(PROTECTED[name]))
            after = {file: fingerprint(path / file) for file in paths}
            value['changed_during_read'] = [file for file in paths if after[file] != value['files'][file]]
        value['head_after'] = git(path, 'rev-parse', 'HEAD').strip()
        value['status_after'] = status(path)
        value['stable'] = (value['head_after'] == head and value['status_after'] == changes
                           and not value.get('changed_during_read'))
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        value['error'] = str(error)
    return value


def candidates(roots, snapshot):
    found = []
    seen = set()
    for root in roots:
        if not root.is_dir():
            continue
        folders = []
        for parent, dirs, _ in os.walk(root):
            path = Path(parent)
            depth = len(path.relative_to(root).parts)
            dirs[:] = [name for name in dirs if depth < 2 and name not in
                       {'sources', 'target', 'build', 'host', 'staging', 'cache', 'vendor',
                        'platform-v1-integration-analysis'} and not name.endswith('worktrees')]
            if path != root and 'candidate' in path.name:
                folders.append(path)
                dirs[:] = []
        for folder in sorted(folders):
            manifest = folder / 'manifest.json'
            if not manifest.is_file():
                found.append({'path': str(folder), 'recorded_status': 'UNSEALED_NO_MANIFEST',
                              'image_hashes_recomputed': False})
                continue
            if str(manifest.resolve()) in seen:
                continue
            seen.add(str(manifest.resolve()))
            try:
                before = manifest.stat()
                if before.st_size > 2 * 1024 * 1024:
                    raise ValueError('manifest exceeds bounded observation size')
                raw = manifest.read_bytes()
                data = json.loads(raw)
                after = manifest.stat()
                digest = hashlib.sha256(raw).hexdigest()
                saved = snapshot / 'candidate-manifests' / str(len(found)) / 'manifest.json'
                saved.parent.mkdir(parents=True, mode=0o700)
                saved.write_bytes(raw)
                found.append({'path': str(folder), 'manifest': {
                                  'size': len(raw), 'mtime_ns': after.st_mtime_ns,
                                  'sha256': digest,
                                  'stable': (before.st_size, before.st_mtime_ns) ==
                                            (after.st_size, after.st_mtime_ns),
                                  'snapshot': str(saved)},
                              'build_id': data.get('build_id'),
                              'linux_commit': data.get('build_git_commit', data.get('linux_source')),
                              'rootfs_commit': data.get('rootfs_build_git_commit'),
                              'reborn_commit': data.get('reborn_source_commit', data.get('reborn_source')),
                              'kind': data.get('kind'),
                              'physical_acceptance': data.get('physical_acceptance'),
                              'flashed': data.get('flashed'),
                              'recorded_files': data.get('files', {}),
                              'recorded_status': data.get('status'),
                              'distribution_intent': data.get('distribution_intent'),
                              'images': [item.get('raw') for item in data.get('payloads', [])],
                              'image_hashes_recomputed': False})
            except (OSError, ValueError, AttributeError) as error:
                found.append({'path': str(folder), 'error': str(error)})
    return found


def evidence_files(roots, output):
    found = []
    for root in roots:
        if not root.is_dir():
            continue
        for folder, dirs, files in os.walk(root):
            path = Path(folder)
            depth = len(path.relative_to(root).parts)
            dirs[:] = [d for d in dirs if depth < 3 and
                       (path / d).resolve() != output.resolve() and
                       d not in {'sources', 'target', 'build', 'host', 'staging', 'images', 'cache', 'vendor'} and
                       not d.endswith('worktrees')]
            for file in files:
                if file.endswith(('.json', '.log', '.txt', '.md', '.tsv')) and any(
                        key in str(path / file).lower() for key in
                        ('platform-v1', 'precision', 'metadata', 'benchmark', 'qualification', 'validation')):
                    try:
                        found.append({'path': str(path / file), 'size': (path / file).stat().st_size})
                    except OSError as error:
                        found.append({'path': str(path / file), 'error': str(error)})
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--code-root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    output = args.output or args.code_root / 'Y2Linux/out/platform-v1-integration-analysis'
    trees = {name: worktrees(args.code_root / name) for name in BASES}
    known = {Path(value['worktree']).resolve() for values in trees.values() for value in values}
    nearby = nearby_checkouts(args.code_root)
    auxiliary = sorted({path.resolve() for path in nearby if path.resolve() not in known})
    protected_trees = list(trees.values()) + [worktrees(path) for path in auxiliary]
    for values in protected_trees:
        for value in values:
            owner = Path(value['worktree']).resolve()
            if owner != (args.code_root / 'Y2Linux').resolve() and output.resolve().is_relative_to(owner):
                parser.error('output must not be inside an independently owned worktree')
    now = datetime.datetime.now(datetime.timezone.utc)
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    output.chmod(0o700)
    snapshot = output / now.strftime('%Y%m%dT%H%M%S.%fZ')
    snapshot.mkdir(mode=0o700)
    result = {'observed_utc': now.isoformat(), 'bases': BASES, 'snapshot': str(snapshot), 'repositories': {}}
    result['layout_aliases'] = [{'path': str(path), 'target': str(path.resolve())}
                                for path in nearby if path != path.resolve()]
    result['auxiliary_checkouts'] = [auxiliary_checkout(path, snapshot, index)
                                   for index, path in enumerate(auxiliary)]
    jobs = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        for name, values in trees.items():
            repo = args.code_root / name
            result['repositories'][name] = {'main': git(repo, 'rev-parse', 'main').strip(),
                'current_branch': git(repo, 'symbolic-ref', '--short', 'HEAD').strip(),
                'refs': git(repo, 'for-each-ref', '--format=%(refname:short) %(objectname) %(upstream:short) %(upstream:track)').splitlines(),
                'worktrees': []}
            (snapshot / (name + '-graph.txt')).write_text(git(repo, 'log', '--all', '--graph', '--decorate', '--oneline', '-120'))
            for value in values:
                jobs.append((name, pool.submit(inspect, value, name, snapshot)))
        for name, job in jobs:
            result['repositories'][name]['worktrees'].append(job.result())
    sessions = [w for r in result['repositories'].values() for w in r['worktrees'] if w.get('session')]
    roots = [args.code_root / name / 'out' for name in BASES] + [Path(w['worktree']) / 'out' for w in sessions]
    result['candidates'] = candidates(roots, snapshot)
    result['evidence_files'] = evidence_files(roots, output)
    result['overlaps'] = {}
    for name, repo in result['repositories'].items():
        owners = {}
        for tree in repo['worktrees']:
            if tree.get('session'):
                for file in tree.get('files', {}):
                    owners.setdefault(file, []).append(tree['session'])
        result['overlaps'][name] = {file: values for file, values in sorted(owners.items()) if len(values) > 1}
        repo['main_after'] = git(args.code_root / name, 'rev-parse', 'main').strip()
    text = json.dumps(result, indent=2) + '\n'
    (snapshot / 'inventory.json').write_text(text)
    temporary = output / 'latest.tmp'
    temporary.write_text(text)
    temporary.replace(output / 'latest.json')
    print(snapshot)
    for tree in sessions:
        print(Path(tree['worktree']).name, tree['session'], tree.get('head', '?')[:12],
              'dirty' if tree.get('dirty') else 'clean',
              'stable' if tree.get('stable') else 'CHANGED_DURING_READ',
              'new_commits=' + str(len(tree.get('commits_since_foundation', []))),
              'files=' + str(len(tree.get('files', {}))))
    print('Completion and physical qualification require reviewed owner handoffs.')


if __name__ == '__main__':
    main()
