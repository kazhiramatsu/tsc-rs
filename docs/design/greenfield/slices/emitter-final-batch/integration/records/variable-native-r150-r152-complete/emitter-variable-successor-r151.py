"""Replay only the successor against the immutable original census/probes."""
from pathlib import Path
import hashlib, json, os, subprocess, time, tomllib

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-comma-prep')
probe = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-corpus-replay')
census = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census')
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert head.startswith('d5ceab6e6')

def alive(pid):
    try: os.kill(pid, 0)
    except ProcessLookupError: return False
    return True

def check():
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
    assert subprocess.check_output(['git', '-C', str(probe), 'rev-parse', 'HEAD'], text=True).strip() == '33fddc898e1086dad95830e3a9ef99fe816dadef'
    assert not subprocess.check_output(['git', '-C', str(probe), 'diff', 'HEAD', '--name-only']).strip()

print('Waiting for native pipeline 96094; preserve source HEAD through the successor proof.', flush=True)
while alive(96094): time.sleep(20)
check()
out = target / 'emitter-variable-successor-r151'; out.mkdir(exist_ok=False)
snapshot = target / 'emitter-recovery-census-r78/parse-snapshot.json'
assert hashlib.sha256(snapshot.read_bytes()).hexdigest() == '1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75'
subprocess.run(['python3', str(probe / 'scripts/replay-recovery-parse.py'), '--parser-tree', str(root), '--input', str(snapshot), '--out', str(out / 'successor.json'), '--build-dir', str(out / 'build-successor'), '--label', 'r151-successor', '--baseline-kind', 'successor', '--with-recovery-profiles'], cwd=probe, check=True)
check()
original = target / 'emitter-recovery-parser-replays-r131'
subprocess.run(['python3', str(probe / 'scripts/select-recovery-parse-corpus.py'), '--snapshot', str(snapshot), '--current', str(original / 'current.json'), '--projection', str(original / 'projection.json'), '--merge-base', str(original / 'merge-base.json'), '--profiles-dir', str(target / 'emitter-recovery-census-r78/profiles'), '--successor', str(out / 'successor.json'), '--out', str(out / 'selection.json')], cwd=probe, check=True)
old = json.loads((original / 'selection.json').read_bytes()); selected = json.loads((out / 'selection.json').read_bytes())
assert {c['case_id'] for c in old['cases']} <= {c['case_id'] for c in selected['cases']}
reference = tomllib.loads((census / 'Cargo.lock').read_text())['package']
expected = {(p['name'], p['version'], p.get('source'), p.get('checksum')) for p in reference}
lock = tomllib.loads((out / 'build-successor/Cargo.lock').read_text())['package']
for p in lock:
    if p.get('source'): assert (p['name'], p['version'], p.get('source'), p.get('checksum')) in expected
with (out / 'resolved-dependencies.json').open('x') as f:
    json.dump([p for p in lock if p.get('source')], f, indent=2); f.write('\n')
check()
print(json.dumps({'head': head, 'summary': selected['summary'], 'original43_retained': True, 'complete_command_qualification': False}), flush=True)
