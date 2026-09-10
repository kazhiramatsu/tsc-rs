#!/usr/bin/env python3
"""Freezes a decorator-next witness/regression run into a receipt.

usage: freeze-decorator-next-evidence.py <run-dir> <receipt.json> [--fixtures a,b,c]

The run directory holds run.log (with a trailing `exit: N` line) and captures/
(the supplemental complete-command captures). The receipt records the worktree
HEAD, the production file hashes, the fixture hashes, the executed binary, the
log and every capture with its outcome, so the result is bound to exact bytes.
"""
import hashlib, json, pathlib, subprocess, sys, glob, re

ROOT = pathlib.Path(__file__).resolve().parent.parent
def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

run_dir = pathlib.Path(sys.argv[1]).resolve()
receipt_path = pathlib.Path(sys.argv[2]).resolve()
fixtures = []
if len(sys.argv) > 3 and sys.argv[3] == '--fixtures':
    fixtures = sys.argv[4].split(',')
log = (run_dir / 'run.log').read_text(errors='replace')
exit_match = re.findall(r'^exit: (\d+)$', log, re.M)
assert exit_match, 'run.log has no terminal exit line'
binary = re.findall(r'Running .*?\((.*?)\)', log)
captures = {}
for path in sorted(glob.glob(str(run_dir / 'captures' / '*.json'))):
    data = json.load(open(path))
    exact = data['actual'] is not None and data['error'] is None and data['actual'] == data['expected']
    captures.setdefault(data['case_id'], []).append({
        'file': pathlib.Path(path).name, 'capture_index': data['capture_index'],
        'sha256': sha(path), 'exact': exact, 'error': data['error']})
rows = []
for case_id, caps in sorted(captures.items()):
    caps.sort(key=lambda c: c['capture_index'])
    rows.append({'case_id': case_id, 'attempts': len(caps),
                 'exact_twice': len(caps) >= 2 and all(c['exact'] for c in caps),
                 'error': next((c['error'] for c in caps if c['error']), None)})
exact_twice = [r['case_id'] for r in rows if r['exact_twice']]
failed = [r['case_id'] for r in rows if not r['exact_twice']]
receipt = {
    'version': 1,
    'slice': 'H2.8a-A6-41 decorator-next',
    'status': 'isolated draft evidence; not production admission',
    'worktree': str(ROOT),
    'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
    'branch': subprocess.check_output(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], cwd=ROOT, text=True).strip(),
    'production_inputs': {p: sha(ROOT / p) for p in [
        'crates/emitter/src/builtins/standard_decorators.rs',
        'crates/emitter/src/builtins/class_fields.rs',
        'crates/emitter/src/builtins/class_fields/downlevel.rs',
        'crates/emitter/src/builtins/target_bindings.rs',
        'crates/emitter/src/builtins/generated_bindings.rs']},
    'fixtures': {p: sha(ROOT / p) for p in fixtures},
    'run_dir': str(run_dir),
    'log_sha256': sha(run_dir / 'run.log'),
    'actual_exit': int(exit_match[-1]),
    'executed_binary': binary[-1] if binary else None,
    'executed_binary_sha256': sha(binary[-1]) if binary and pathlib.Path(binary[-1]).exists() else None,
    'summary': {'cases': len(rows), 'exact_twice': len(exact_twice), 'failed': len(failed),
                'errors': sum(1 for r in rows if r['error'])},
    'exact_twice': exact_twice,
    'failed': failed,
    'rows': rows,
    'captures': captures,
}
receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps({'receipt': str(receipt_path), 'sha256': sha(receipt_path), **receipt['summary']}))
