#!/usr/bin/env python3
"""Record-and-run wrapper for A6-41-SUPER native replays.

Writes the pre-run manifest (production/fixture/binary SHA-256, argv, env,
start time) into a fresh run directory, executes the command demoted, and
records the actual exit code and elapsed time immediately afterwards.
"""
from pathlib import Path
import hashlib, json, os, subprocess, sys, time

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def main():
    run_dir = Path(sys.argv[1]).resolve()
    label = sys.argv[2]
    command = sys.argv[3:]
    assert command
    assert not run_dir.exists(), f'{run_dir} must be new'
    run_dir.mkdir(parents=True)
    root = Path(__file__).resolve().parent.parent
    tracked = ['crates/emitter/src/builtins/standard_decorators.rs',
               'crates/emitter/src/builtins/class_fields.rs',
               'crates/emitter/src/builtins/class_fields/downlevel.rs',
               'crates/emitter/src/builtins/target_bindings.rs',
               'crates/emitter/src/factory.rs', 'crates/emitter/src/metadata.rs',
               'crates/emitter/src/builtins/es2018.rs', 'crates/emitter/src/printer.rs',
               'crates/emitter/src/builtins/es2021.rs', 'crates/emitter/src/transform.rs',
               'crates/emitter/src/builtins/helpers.rs']
    fixtures = sorted(str(p.relative_to(root)) for p in (root / 'crates/compiler/tests/fixtures').glob('decorator-*.json'))
    fixtures += sorted(str(p.relative_to(root)) for p in (root / 'crates/compiler/tests/fixtures').glob('decorator-*.json.zst'))
    fixtures += sorted(str(p.relative_to(root)) for p in (root / 'crates/compiler/tests/fixtures').glob('retained-*.json'))
    fixtures += ['crates/compiler/tests/fixtures/class-helper-accessor-producers.json',
                 'crates/compiler/tests/fixtures/class-field-alias-map-positions.json']
    binaries = [c for c in command if c.startswith('/') and Path(c).is_file() and os.access(c, os.X_OK)]
    head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
    status = subprocess.run(['git', 'status', '--short'], cwd=root, capture_output=True, text=True, check=True).stdout
    manifest = {
        'label': label, 'head': head, 'git_status_short': status,
        'cwd': str(root), 'argv': command,
        'env': {k: v for k, v in os.environ.items() if k.startswith(('CARGO', 'TSC_RS', 'DEC_SUPER', 'RUST'))},
        'start_time': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        'production': {p: sha(root / p) for p in tracked if (root / p).is_file()},
        'fixtures': {p: sha(root / p) for p in fixtures if (root / p).is_file()},
        'binaries': {b: sha(b) for b in binaries},
        'vendor_tsc': sha(root / 'vendor/typescript-6.0.3/lib/_tsc.js'),
    }
    (run_dir / 'prelaunch.json').write_text(json.dumps(manifest, indent=2) + '\n')
    log = run_dir / 'run.log'
    started = time.monotonic()
    with log.open('wb') as stream:
        proc = subprocess.run(command, cwd=root, stdout=stream, stderr=subprocess.STDOUT)
    elapsed = time.monotonic() - started
    terminal = {'actual_exit': proc.returncode, 'elapsed_seconds': elapsed,
                'end_time': time.strftime('%Y-%m-%dT%H:%M:%S%z'), 'log_sha256': sha(log),
                'production_after': {p: sha(root / p) for p in tracked if (root / p).is_file()}}
    terminal['production_unchanged'] = terminal['production_after'] == manifest['production']
    (run_dir / 'terminal.json').write_text(json.dumps(terminal, indent=2) + '\n')
    print(json.dumps({'run_dir': str(run_dir), 'actual_exit': proc.returncode, 'elapsed_seconds': round(elapsed, 1)}))
    sys.exit(proc.returncode)

if __name__ == '__main__':
    main()
