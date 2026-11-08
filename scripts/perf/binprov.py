#!/usr/bin/env python3
"""Provenance of measured binaries (integrator rule 2026-11-06, after the main-906 mix-up: a "main" binary built from a
card branch that lacked #905 confounded a pilot window).

Every binary in $TSRS_PERF_ROOT/bin (default target/perf/bin) that a window measures needs a line in its MANIFEST.tsv:
  name  sha256  commit  patch_sha256  worktree  recorded_at  note
`commit` is the source tree it was built from; `patch_sha256` is "-" or the sha of a patch applied on that commit (lane builds).

  binprov.py record NAME COMMIT [--patch FILE] [--worktree DIR] [--note TEXT]
      Record $TSRS_PERF_ROOT/bin/NAME. Refuses a dirty worktree unless --patch names the change.
  binprov.py check NAME [NAME ...]
      Exit 1 unless each binary's sha matches its record, and each main-<sha> binary was built from code equal to that main
      commit (the build-defining paths below are identical).
  binprov.py pair BASE CARD
      `check` both, then require BASE's code to equal the code CARD started from: CARD's commit when it carries a patch,
      otherwise the merge base of CARD's commit with origin/main (or CARD's commit's parent if that merge base is CARD itself).
Windows call `pair` (or `check`) before measuring and stop on a non-zero exit.
"""
import hashlib, os, subprocess, sys, time

REPO = os.environ.get('TSRS_REPO') or os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PERF_ROOT = os.environ.get('TSRS_PERF_ROOT') or os.path.join(REPO, 'target', 'perf')
BIN = os.path.join(PERF_ROOT, 'bin')
MANIFEST = os.path.join(BIN, 'MANIFEST.tsv')
CODE = ['crates', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo', 'vendor']   # what a release build reads
FIELDS = ['name', 'sha256', 'commit', 'patch_sha256', 'worktree', 'recorded_at', 'note']

def git(*args, cwd=REPO):
    return subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True)

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()

def load():
    rows = {}
    if os.path.exists(MANIFEST):
        for line in open(MANIFEST):
            if line.strip() and not line.startswith('#'):
                parts = line.rstrip('\n').split('\t')
                rows[parts[0]] = dict(zip(FIELDS, parts))
    return rows

def full(commit):
    r = git('rev-parse', '--verify', commit + '^{commit}')
    if r.returncode:
        sys.exit(f'binprov: unknown commit {commit}')
    return r.stdout.strip()

def same_code(a, b):
    return git('diff', '--quiet', a, b, '--', *CODE).returncode == 0

def fail(msg):
    print(f'binprov: FAIL {msg}')
    sys.exit(1)

def check(names, rows):
    for name in names:
        row = rows.get(name)
        if row is None:
            fail(f'{name}: no MANIFEST record (record it from its exact source, or rebuild)')
        path = os.path.join(BIN, name)
        if not os.path.exists(path) or sha256(path) != row['sha256']:
            fail(f'{name}: file missing or sha differs from its record')
        if name.startswith('main-'):
            claimed = name[len('main-'):]
            if row['patch_sha256'] != '-':
                fail(f'{name}: a main binary must not carry a patch')
            if not same_code(row['commit'], full(claimed)):
                fail(f'{name}: built from {row["commit"][:9]}, whose code differs from main {claimed}')
        print(f'binprov: ok {name} = {row["commit"][:9]}{" + patch " + row["patch_sha256"][:12] if row["patch_sha256"] != "-" else ""}')

def start_of(row):
    commit = row['commit']
    if row['patch_sha256'] != '-':
        return commit
    base = git('merge-base', commit, 'origin/main').stdout.strip()
    if base == commit:
        base = full(commit + '^')
    return base

def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    cmd, rows = sys.argv[1], load()
    if cmd == 'record':
        name, commit = sys.argv[2], full(sys.argv[3])
        opt = lambda k: sys.argv[sys.argv.index(k) + 1] if k in sys.argv else None
        patch, worktree, note = opt('--patch'), opt('--worktree') or '-', opt('--note') or '-'
        if worktree != '-':
            head = git('rev-parse', 'HEAD', cwd=worktree).stdout.strip()
            dirty = git('status', '--porcelain', '--untracked-files=no', '--', *CODE, cwd=worktree).stdout.strip()
            if head != commit:
                fail(f'{worktree} is at {head[:9]}, not {commit[:9]}')
            if dirty and not patch:
                fail(f'{worktree} has uncommitted code changes; pass --patch with the applied diff')
        row = dict(name=name, sha256=sha256(os.path.join(BIN, name)), commit=commit,
                   patch_sha256=sha256(patch) if patch else '-', worktree=worktree,
                   recorded_at=time.strftime('%Y-%m-%dT%H:%M:%S'), note=note.replace('\t', ' '))
        rows[name] = row
        with open(MANIFEST, 'w') as f:
            f.write('#' + '\t'.join(FIELDS) + '\n')
            for r in rows.values():
                f.write('\t'.join(r[k] for k in FIELDS) + '\n')
        check([name], rows)
    elif cmd == 'check':
        check(sys.argv[2:], rows)
    elif cmd == 'pair':
        base, card = sys.argv[2], sys.argv[3]
        check([base, card], rows)
        start = start_of(rows[card])
        if not same_code(rows[base]['commit'], start):
            fail(f'{card} started from {start[:9]}, but {base} was built from {rows[base]["commit"][:9]} (code differs)')
        print(f'binprov: ok pair {base} -> {card} (same starting code {start[:9]})')
    else:
        sys.exit(__doc__)

main()
