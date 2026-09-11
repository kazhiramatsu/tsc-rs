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
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
production_paths = subprocess.check_output(
    ['git', 'diff', '--name-only', 'cb4e5f3e8', head, '--', 'crates'],
    cwd=ROOT, text=True).splitlines()
production_paths = [p for p in production_paths if '/src/' in p]
production_inputs = {p: sha(ROOT / p) for p in production_paths}
provenance = {}
prelaunch_path = run_dir / 'prelaunch.json'
if prelaunch_path.exists():
    prelaunch = json.loads(prelaunch_path.read_text())
    execution_path = run_dir / 'execution.json'
    execution = json.loads(execution_path.read_text())
    assert sha(prelaunch_path) == execution['prelaunch_sha256']
    assert sha(run_dir / 'run.log') == execution['log_sha256']
    assert int(exit_match[-1]) == execution['actual_exit']
    assert not execution['inputs_changed_during_run']
    assert sha(pathlib.Path(prelaunch['archive']['path'])) == prelaunch['archive']['sha256']
    # A measured working-tree candidate may be committed after the run.
    # Preserve its actual launch head and verify the candidate head's source
    # bytes against the immutable prelaunch manifest, rather than relabeling
    # the run as having launched from that later commit.
    for p, digest in production_inputs.items():
        assert prelaunch['inputs'][p] == digest, f'production changed since run: {p}'
        committed = subprocess.check_output(['git', 'show', f'{head}:{p}'], cwd=ROOT)
        assert hashlib.sha256(committed).hexdigest() == digest, f'uncommitted production: {p}'
    for p in fixtures:
        assert prelaunch['inputs'][p] == sha(ROOT / p), f'fixture changed since run: {p}'
    for p, entry in execution.get('archived_binaries', {}).items():
        assert sha(ROOT / p) == entry['sha256']
    provenance = {
        'launch_head': prelaunch['head'],
        'prelaunch': {'path': str(prelaunch_path), 'sha256': sha(prelaunch_path)},
        'execution': {'path': str(execution_path), 'sha256': sha(execution_path)},
        'archive': prelaunch['archive'],
        'archived_binaries': execution.get('archived_binaries', {}),
    }
captures = {}
observations = {}
for path in sorted(glob.glob(str(run_dir / 'captures' / '*.json'))):
    data = json.load(open(path))
    observations.setdefault(data['case_id'], []).append(
        {k: v for k, v in data.items() if k != 'capture_index'})
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
inconsistent = [case_id for case_id, caps in observations.items()
                if any(cap != caps[0] for cap in caps[1:])]
receipt = {
    'version': 1,
    'slice': 'H2.8a-A6-41 decorator-next',
    'status': 'isolated draft evidence; not production admission',
    'worktree': str(ROOT),
    'head': head,
    'branch': subprocess.check_output(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], cwd=ROOT, text=True).strip(),
    'production_inputs': production_inputs,
    **provenance,
    'fixtures': {p: sha(ROOT / p) for p in fixtures},
    'run_dir': str(run_dir),
    'log_sha256': sha(run_dir / 'run.log'),
    'actual_exit': int(exit_match[-1]),
    'executed_binary': binary[-1] if binary else None,
    'executed_binary_sha256': sha(ROOT / binary[-1]) if binary and (ROOT / binary[-1]).exists() else None,
    'summary': {'cases': len(rows), 'exact_twice': len(exact_twice), 'failed': len(failed),
                'errors': sum(1 for r in rows if r['error']), 'inconsistent': len(inconsistent)},
    'exact_twice': exact_twice,
    'failed': failed,
    'rows': rows,
    'captures': captures,
}
receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps({'receipt': str(receipt_path), 'sha256': sha(receipt_path), **receipt['summary']}))
