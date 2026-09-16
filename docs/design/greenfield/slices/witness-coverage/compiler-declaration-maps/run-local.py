#!/usr/bin/env python3
"""Capture this batch's local baseline/final execution without rewriting evidence."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parents[6]
HERE = Path(__file__).resolve().parent
BASE = '3e8f582cb'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('phase', choices=['baseline', 'final'])
args = parser.parse_args()
receipt = HERE / (args.phase + '.v1.json')
if receipt.exists():
    raise SystemExit('refusing to overwrite ' + str(receipt))
env = os.environ.copy()
env.update(CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0', CARGO_PROFILE_TEST_DEBUG='0', RUSTC_WRAPPER='')
commands = ([['cargo', 'test', '--manifest-path', 'crates/compiler/Cargo.toml', '--test',
              target, '--', '--nocapture', '--test-threads=1']
             for target in ('h2_7e_declaration_map_apis', 'h2_7e_declaration_maps')]
            if args.phase == 'baseline' else [['python3', '.github/ci/replay.py', 'witnesses']])
if args.phase == 'final':
    env['WITNESS_SUITES'] = '["declaration-map-apis", "declaration-maps"]'
paths = subprocess.check_output(['git', 'ls-files', 'crates', 'scripts', '.github', 'Cargo.toml', 'Cargo.lock', '.node-version', 'vendor', 'ratchets/h2-7de-candidate-inputs.v1.json', 'ratchets/h2-7de-observations.v1.json'], cwd=ROOT, text=True).splitlines()
manifest = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in paths if (ROOT / name).is_file()}
source = HERE / (args.phase + '-source.v1.json')
source.write_text(json.dumps(manifest, indent=2) + '\n')
record = {'base': BASE, 'phase': args.phase, 'platform': platform.platform(),
          'source_manifest': source.name, 'source_manifest_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
          'node': subprocess.check_output(['node', '--version'], text=True).strip(),
          'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
          'env': {key: env[key] for key in ['CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_TEST_DEBUG', 'RUSTC_WRAPPER']},
          'runs': []}
if args.phase == 'final':
    record['env']['WITNESS_SUITES'] = env['WITNESS_SUITES']
for i, command in enumerate(commands):
    actual = ['taskpolicy', '-b', 'nice', '-n', '15', *command] if platform.system() == 'Darwin' else command
    log = HERE / f'{args.phase}-{i}.log.gz'
    print('START', actual, flush=True)
    started = time.monotonic()
    result = subprocess.run(actual, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    log.write_bytes(gzip.compress(result.stdout, mtime=0))
    row = {'command': actual, 'exit': result.returncode, 'seconds': round(time.monotonic()-started, 3),
           'log': log.name, 'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(),
           'summaries': [line for line in result.stdout.decode(errors='replace').splitlines() if 'test result:' in line or line.startswith('{"compiler_direct"')]}
    row['binaries_sha256'] = {str(file.relative_to(ROOT)): hashlib.sha256(file.read_bytes()).hexdigest()
                              for target in ('h2_7e_declaration_map_apis', 'h2_7e_declaration_maps')
                              for file in (ROOT / 'target/debug/deps').glob(target + '-*')
                              if file.is_file() and file.suffix == '' and os.access(file, os.X_OK)}
    record['runs'].append(row)
    receipt.write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(row), flush=True)
print('DONE', receipt, flush=True)
raise SystemExit(int(any(row['exit'] for row in record['runs'])))
