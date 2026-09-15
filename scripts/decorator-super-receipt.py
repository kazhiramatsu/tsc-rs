#!/usr/bin/env python3
"""Assemble the A6-41-SUPER candidate receipt from the recorded runs.

usage: decorator-super-receipt.py <evidence-dir> <out.json> <before-new> <before-530> <after-new> <after-extra> <after-530> [<before-extra> [<before-followup> <after-followup> [<before-followup2> <after-followup2> [<before-followup3> <after-followup3>]]]]

Every run directory carries prelaunch.json (inputs/binaries SHA-256, argv, env)
and terminal.json (actual exit, elapsed, log SHA-256) written by
decorator-super-run.py, plus captures/. The receipt pins those files, the
fixtures, the production sources, the candidate patch, and the per-case
classification of each run (decorator-super-analyze.py rows), and never
credits an upstream exception as a complete command.
"""
from pathlib import Path
import hashlib, json, subprocess, sys

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def run_summary(root, run_dir, fixture_key=None):
    run_dir = Path(run_dir)
    pre = json.loads((run_dir / 'prelaunch.json').read_text())
    term = json.loads((run_dir / 'terminal.json').read_text())
    log = run_dir / 'run.log'
    text = log.read_text(errors='replace')
    exact = text.count('EXACT x2 ')
    failed = text.count('REPEATED FAILURE ')
    exceptions = text.count('UPSTREAM EXCEPTION ')
    analysis = run_dir / 'analysis.json'
    rows = json.loads(analysis.read_text())['rows'] if analysis.exists() else None
    captures = sorted((run_dir / 'captures').glob('*.json')) if (run_dir / 'captures').is_dir() else []
    return {
        'run_dir': str(run_dir.relative_to(root)) if str(run_dir).startswith(str(root)) else str(run_dir),
        'label': pre['label'], 'head': pre['head'], 'start_time': pre['start_time'], 'end_time': term['end_time'],
        'argv': pre['argv'], 'binaries': pre['binaries'], 'production': pre['production'], 'fixtures': pre['fixtures'],
        'actual_exit': term['actual_exit'], 'elapsed_seconds': term['elapsed_seconds'],
        'log_sha256': term['log_sha256'], 'production_unchanged_during_run': term['production_unchanged'],
        'exact_twice': exact, 'repeated_failures': failed, 'upstream_exceptions_excluded': exceptions,
        'captures': len(captures), 'captures_sha256': hashlib.sha256(b''.join(sha(c).encode() for c in captures)).hexdigest(),
        'analysis_rows': rows,
    }

def main():
    evidence, out, before_new, before_530, after_new, after_extra, after_530 = sys.argv[1:8]
    before_extra = sys.argv[8] if len(sys.argv) > 8 else None
    before_followup = sys.argv[9] if len(sys.argv) > 9 else None
    after_followup = sys.argv[10] if len(sys.argv) > 10 else None
    before_followup2 = sys.argv[11] if len(sys.argv) > 11 else None
    after_followup2 = sys.argv[12] if len(sys.argv) > 12 else None
    before_followup3 = sys.argv[13] if len(sys.argv) > 13 else None
    after_followup3 = sys.argv[14] if len(sys.argv) > 14 else None
    root = Path(subprocess.run(['git', 'rev-parse', '--show-toplevel'], capture_output=True, text=True, check=True).stdout.strip())
    head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
    fixtures = ['crates/compiler/tests/fixtures/decorator-super-inputs.json', 'crates/compiler/tests/fixtures/decorator-super.json.zst',
                'crates/compiler/tests/fixtures/decorator-super-extra-inputs.json', 'crates/compiler/tests/fixtures/decorator-super-extra.json.zst',
                'crates/compiler/tests/fixtures/decorator-super-followup-inputs.json', 'crates/compiler/tests/fixtures/decorator-super-followup.json.zst',
                'crates/compiler/tests/fixtures/decorator-super-followup2-inputs.json', 'crates/compiler/tests/fixtures/decorator-super-followup2.json.zst',
                'crates/compiler/tests/fixtures/decorator-super-followup3-inputs.json', 'crates/compiler/tests/fixtures/decorator-super-followup3.json.zst']
    production = ['crates/emitter/src/builtins/standard_decorators.rs', 'crates/emitter/src/builtins/class_fields.rs', 'crates/emitter/src/builtins/class_fields/downlevel.rs',
                  'crates/emitter/src/builtins/es2018.rs', 'crates/emitter/src/printer.rs', 'crates/emitter/src/builtins/es2021.rs',
                  'crates/emitter/src/transform.rs', 'crates/emitter/src/builtins/helpers.rs']
    scripts = ['scripts/generate-decorator-super-inputs.mjs', 'scripts/observe-decorator-super.mjs', 'scripts/decorator-super-run.py',
               'scripts/decorator-super-analyze.py', 'scripts/decorator-super-mapdiff.py', 'scripts/decorator-super-runtime-control.mjs', 'scripts/decorator-super-receipt.py',
               'scripts/decorator-super-compare.py', 'scripts/observe-decorator-super-direct.mjs']
    tests = ['crates/compiler/tests/decorator_super_contract.rs', 'crates/compiler/tests/integration/h2_8a_decorator_super.rs',
             'crates/emitter/tests/decorator_super_direct_contract.rs', 'crates/emitter/tests/fixtures/decorator-super-direct.json']
    patches = sorted(str(p.relative_to(root)) for p in (root / 'docs/design/greenfield/slices').glob('h2-8a-decorator-super*.patch'))
    runtime = Path(evidence) / 'runtime-control.json'
    receipt = {
        'version': 1, 'slice': 'H2.8a-A6-41-SUPER',
        'status': 'isolated decorator static-super candidate; no production admission or whole-goal completion',
        'head': head,
        'restored_base_commit': '13f5767ece720108f41e46fb50a713283ec429be',
        'evidence_commit': '6e298cda8dbf362f61ce6a8e690ec145270681df',
        'vendor_tsc_sha256': sha(root / 'vendor/typescript-6.0.3/lib/_tsc.js'),
        'production': {p: sha(root / p) for p in production},
        'fixtures': {p: sha(root / p) for p in fixtures},
        'fixtures_decoded_sha256': {p: hashlib.sha256(subprocess.run(['zstd', '-dc', str(root / p)], capture_output=True, check=True).stdout).hexdigest()
                                    for p in fixtures if p.endswith('.zst')},
        'scripts': {p: sha(root / p) for p in scripts if (root / p).is_file()},
        'tests': {p: sha(root / p) for p in tests},
        'patches': {p: sha(root / p) for p in patches},
        'runs': {
            'before_new': run_summary(root, before_new), 'before_530': run_summary(root, before_530),
            'after_new': run_summary(root, after_new), 'after_extra': run_summary(root, after_extra), 'after_530': run_summary(root, after_530),
        },
        'runtime_control': json.loads(runtime.read_text())['summary'] if runtime.exists() else None,
        'runtime_control_extra': json.loads((Path(evidence) / 'runtime-control-extra.json').read_text())['summary'] if (Path(evidence) / 'runtime-control-extra.json').exists() else None,
        'emitter_suite': (lambda log: {'run_log': str(log.resolve().relative_to(root)), 'log_sha256': sha(log),
                                       'results': [line.strip() for line in log.read_text(errors='replace').splitlines() if line.startswith('test result')],
                                       'actual_exit': next((int(line.split(':')[1]) for line in log.read_text(errors='replace').splitlines() if line.startswith('emitter suite exit:')), None)}
                          if log.exists() else None)(Path(evidence) / 'runs' / 'after-18-emitter-suite' / 'run.log'),
        'runtime_control_followup': json.loads((Path(evidence) / 'runtime-control-followup.json').read_text())['summary'] if (Path(evidence) / 'runtime-control-followup.json').exists() else None,
        'runtime_control_followup2': json.loads((Path(evidence) / 'runtime-control-followup2.json').read_text())['summary'] if (Path(evidence) / 'runtime-control-followup2.json').exists() else None,
        'runtime_control_followup3': json.loads((Path(evidence) / 'runtime-control-followup3.json').read_text())['summary'] if (Path(evidence) / 'runtime-control-followup3.json').exists() else None,
        'evidence_dir': str(Path(evidence)),
        'scope_limit': 'Counts describe distinct witness populations (672 primary complete commands, 42 extra controls, 156 follow-up witnesses, 162 second follow-up witnesses, 48 third follow-up witnesses, 530 retained commands, runtime event-log controls, 32 direct controls of which 4 are recorded divergences) and must not be summed or projected onto the whole emitter.',
    }
    if before_extra:
        receipt['runs']['before_extra'] = run_summary(root, before_extra)
    if before_followup:
        receipt['runs']['before_followup'] = run_summary(root, before_followup)
    if after_followup:
        receipt['runs']['after_followup'] = run_summary(root, after_followup)
    if before_followup2:
        receipt['runs']['before_followup2'] = run_summary(root, before_followup2)
    if after_followup2:
        receipt['runs']['after_followup2'] = run_summary(root, after_followup2)
    if before_followup3:
        receipt['runs']['before_followup3'] = run_summary(root, before_followup3)
    if after_followup3:
        receipt['runs']['after_followup3'] = run_summary(root, after_followup3)
    Path(out).write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({k: (v['exact_twice'], v['repeated_failures'], v['actual_exit']) for k, v in receipt['runs'].items()}))

if __name__ == '__main__':
    main()
