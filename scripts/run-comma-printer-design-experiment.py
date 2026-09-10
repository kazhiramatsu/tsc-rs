#!/usr/bin/env python3
"""Compare the staged comma printer in the isolated A40 design workspace.

This produces research evidence, never production admission or qualification.
Completed attempts and the production implementation are immutable.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
CANDIDATE = Path('docs/design/greenfield/slices/h2-8a-retained-lexical-owners.candidate-v2.rs.txt')
PRODUCTION = Path('crates/emitter/src/builtins/class_fields.rs')
BASELINE = Path('ratchets/h2-8a-retained-lexical-owners-before.v3.json')
PRODUCTION_SHA = '26de511b53ea232b073235a4507f703ae78cd4881c12bfea720f71f932dc03e7'
CANDIDATE_SHA = 'ab3363962f79229d4a10433f0c88126912efea062f61347e3a94eb38ccc39573'
BASELINE_SHA = 'b16b64964c0e8cd78d652d38efd9b303906a35147df73e32a67fc35c1ed0bed8'
PRINTER = Path('crates/emitter/src/printer.rs')
PATCH = Path('docs/design/greenfield/slices/h2-8a-comma-printer.candidate.patch')
PRINTER_SHA = '8951dc94e07df2cfdca2f41a7b82e4c9ceeae75d6d5ff60ae5dabb752db53398'
PATCH_SHA = 'f8a30aca24079b013b1396af6ffb769eabe8509af700e4f63f1fdf4f2ce8b90c'


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def main():
    attempt = int(sys.argv[1])
    assert attempt > 0
    selection = sys.argv[2]
    assert selection in ['direct', 'all', 'edges', 'comma-factory']
    prefix = ROOT / f'target/h2-8a-comma-printer-design-experiment-{attempt}'
    pre_path = prefix.with_suffix('.pre.json')
    log_path = prefix.with_suffix('.log')
    exit_path = prefix.with_suffix('.exit.json')
    assert not any(p.exists() for p in [pre_path, log_path, exit_path]), 'poll the existing attempt; never restart it'
    assert sha(ROOT / PRODUCTION) == PRODUCTION_SHA
    assert sha(ROOT / CANDIDATE) == CANDIDATE_SHA
    assert sha(ROOT / BASELINE) == BASELINE_SHA
    assert sha(ROOT / PRINTER) == PRINTER_SHA
    assert sha(ROOT / PATCH) == PATCH_SHA
    workspace = ROOT / 'target/h2-8a-retained-lexical-design-workspace'
    assert workspace.is_dir(), 'use the existing isolated typecheck workspace'
    source_paths = sorted(p.relative_to(ROOT) for p in (ROOT / 'crates').rglob('*') if p.is_file())
    source_paths += [Path(p) for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']]
    source_paths += sorted(p.relative_to(ROOT) for p in (ROOT / '.cargo').rglob('*') if p.is_file())
    # The contracts target compiles sibling tests even when only one test is
    # selected. Copy their root-relative include_bytes!/include_str! inputs as
    # well; merely copying crates is sufficient only for the library check.
    include_pattern = re.compile(
        r'include_(?:str|bytes)!\(\s*concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),'
        r'\s*"/\.\./\.\./([^"\n]+)"\s*,?\s*\)\s*\)', re.MULTILINE)
    includes = {}
    for source in source_paths:
        if source.suffix == '.rs':
            for spelling in include_pattern.findall((ROOT / source).read_text()):
                included = Path(spelling)
                assert not included.is_absolute() and '..' not in included.parts
                assert (ROOT / included).is_file(), spelling
                includes.setdefault(spelling, []).append(str(source))
    source_paths = sorted(set(source_paths) | {Path(p) for p in includes})
    for relative in source_paths:
        original = ROOT / (CANDIDATE if relative == PRODUCTION else relative)
        destination = workspace / relative
        if not destination.exists() or sha(destination) != sha(original):
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(original, destination)
    subprocess.run(['git', 'apply', '--check', str(ROOT / PATCH)], cwd=workspace, check=True)
    subprocess.run(['git', 'apply', str(ROOT / PATCH)], cwd=workspace, check=True)
    inputs = [{'path': str(p), 'sha256': sha(workspace / p)} for p in source_paths]
    production_inputs = [{'path': str(p), 'sha256': sha(ROOT / p)} for p in source_paths]
    previous_pre = json.loads((ROOT / 'target/h2-8a-retained-comma-native-before-1-prelaunch.json').read_text())
    vendor_inputs = [r for r in previous_pre['inputs'] if r['path'].startswith('vendor/')]
    for row in vendor_inputs:
        assert sha(ROOT / row['path']) == row['sha256'], row['path']
    archive = Path(tempfile.mkdtemp(prefix='tsc-rs-comma-printer-design-experiment-'))
    with tarfile.open(archive / 'source-and-inputs.tar.gz', 'w:gz') as tar:
        for row in inputs:
            tar.add(workspace / row['path'], arcname=row['path'])
        for row in vendor_inputs:
            tar.add(ROOT / row['path'], arcname=row['path'])
        for path in [CANDIDATE, BASELINE, PATCH, Path(__file__).relative_to(ROOT),
                     Path('docs/design/greenfield/slices/h2-8a-retained-lexical-owners.md')]:
            tar.add(ROOT / path, arcname=str(path))
    target = ROOT / 'target/h2-8a-retained-lexical-design-artifacts'
    command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
               'cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', '--',
               'retained_accessor_owners_match_complete_typescript_observations',
               '--nocapture', '--test-threads=1']
    if selection == 'direct':
        command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
                   'cargo', 'test', '--offline', '-p', 'tsc-rs-emitter', '--test', 'comma_list_printer_contract',
                   '--', '--nocapture', '--test-threads=1']
    environment = {'CARGO_BUILD_JOBS': '2', 'CARGO_TARGET_DIR': str(target),
                   'TSC_RS_RETAINED_ACCESSOR_CASE_SET': selection,
                   'TSC_RS_H2_8A_CAPTURE_WRITES_DIR': str(archive / 'captures'),
                   'TSC_RS_H2_8A_FAILURE_DIR': str(archive / 'failures')}
    pre = {'version': 1, 'attempt': attempt, 'status': 'isolated design experiment; no production edit, admission or qualification',
           'base': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
           'workspace': str(workspace), 'archive': str(archive), 'argv': command,
           'environment': environment, 'inputs': inputs, 'vendor_inputs': vendor_inputs,
           'root_relative_include_consumers': includes,
           'production_inputs': production_inputs, 'candidate_sha256': CANDIDATE_SHA,
           'selection': selection, 'printer_patch_sha256': PATCH_SHA,
           'printer_base_sha256': PRINTER_SHA,
           'baseline': {'path': str(BASELINE), 'sha256': BASELINE_SHA},
           'source_archive_sha256': sha(archive / 'source-and-inputs.tar.gz')}
    write_json(pre_path, pre)
    shutil.copy2(pre_path, archive / 'prelaunch.json')
    print(json.dumps({'event': 'launch', 'pre': str(pre_path), 'log': str(log_path), 'archive': str(archive)}), flush=True)
    started = time.monotonic()
    with log_path.open('x') as log:
        result = subprocess.run(command, cwd=workspace, env=dict(os.environ, **environment), stdout=log, stderr=subprocess.STDOUT)
    record = {'actual_exit': result.returncode, 'elapsed_seconds': time.monotonic() - started,
              'manifest_sha256': sha(pre_path), 'log_sha256': sha(log_path),
              'production_unchanged': all(sha(ROOT / row['path']) == row['sha256'] for row in production_inputs),
              'copied_inputs_unchanged': all(sha(workspace / row['path']) == row['sha256'] for row in inputs)}
    # Preserve the executed binary before allowing another Cargo job.
    binaries = re.findall(r'Running tests/[^ ]+ \(([^\)]+)\)', log_path.read_text())
    record['binaries'] = []
    for index, spelling in enumerate(binaries):
        binary = Path(spelling)
        if not binary.is_absolute():
            binary = workspace / binary
        retained = archive / f'test-{index}.bin'
        shutil.copy2(binary, retained)
        record['binaries'].append({'path': str(binary), 'retained_path': str(retained),
                                   'sha256': sha(retained), 'size': retained.stat().st_size})
    write_json(exit_path, record)
    shutil.copy2(exit_path, archive / 'actual-exit.json')
    shutil.copy2(log_path, archive / 'run.log')
    print(json.dumps(record), flush=True)
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
