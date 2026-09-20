"""Preserve failed walk and bounded repair evidence; does not qualify or mint."""
import datetime
import gzip
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
PREP = Path('/tmp/emitter-w5-stratum-repair-r520')
BASE = ROOT / 'docs/design/greenfield/slices/emitter-final-batch/integration'
OUT = BASE / 'records/w5-stratum-validation-r524'
HEAD = '9da1f8bf5a5fac111b2d27e287c3285e8448bfdc'
sha = lambda data: hashlib.sha256(data).hexdigest()
git = lambda *args: subprocess.check_output(['git', *args], cwd=ROOT)

assert git('rev-parse', 'HEAD').decode().strip() == HEAD
assert not git('diff', '--name-only', '--', '*.rs').strip()
failed = json.loads((PREP / 'failed-walk/final-canonical-walk-r519.json').read_text())
assert failed['head'] == HEAD and failed['exit'] == 1
for row in json.loads((PREP / 'initial-manifest.json').read_text())['files']:
    data = (PREP / row['path']).read_bytes()
    assert sha(data) == row['sha256'] and len(data) == row['bytes'], row['path']
for number in (212, 213, 214):
    review = json.loads((PREP / f'round{number}-response.json').read_text())
    assert review['is_error'] is False and review['result'].strip()
proof = json.loads((PREP / 'fresh-native-proof.json').read_text())
assert proof['native_census_passed'] and proof['complete_roster_rows'] == 67
assert proof['m1_projection_preserved'] and proof['s2_projection_preserved']
assert proof['generator_sha256'] == sha((ROOT / 'crates/oracle/h2-7a-witnesses.mjs').read_bytes())
labels = ['w5-stratum-qualification-r521', 'w5-stratum-qualification-final-r522',
          'w5-stratum-fresh-native-r522', 'w5-declaration-replay-r523']
receipts = []
for label in labels:
    receipt = json.loads((BASE / f'records/local/{label}.json').read_text())
    raw = gzip.decompress((BASE / f'records/local/{label}.log.gz').read_bytes())
    assert receipt['exit'] == 0 and receipt['head'] == HEAD
    assert receipt['log_sha256'] == sha(raw) and receipt['log_bytes'] == len(raw)
    receipts.append({'label': label, 'exit': receipt['exit'], 'seconds': receipt['seconds']})
assert b'tests 55' in gzip.decompress((BASE / 'records/local/w5-stratum-qualification-final-r522.log.gz').read_bytes())
assert b'3 passed; 0 failed' in gzip.decompress((BASE / 'records/local/w5-declaration-replay-r523.log.gz').read_bytes())
OUT.mkdir(exist_ok=False)
files = []
def put(name, data):
    destination = OUT / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    files.append({'path': name, 'sha256': sha(data), 'bytes': len(data)})
for path in sorted(PREP.rglob('*')):
    if path.is_file():
        assert not path.is_symlink()
        name = 'preparation/' + path.relative_to(PREP).as_posix()
        data = path.read_bytes()
        if path.suffix == '.log':
            name += '.gz'
            data = gzip.compress(data, mtime=0)
        put(name, data)
for label in labels:
    for suffix in ('.json', '.log.gz'):
        put('validation/' + label + suffix, (BASE / ('records/local/' + label + suffix)).read_bytes())
for name in ['crates/oracle/h2-7a-witnesses.mjs', 'scripts/h2-7a-stratum.test.mjs', '.github/ci/qualification.test.mjs']:
    put('sources/' + name, (ROOT / name).read_bytes())
put('bounded-repair.patch.gz', gzip.compress(git('diff', 'HEAD', '--', 'crates/oracle/h2-7a-witnesses.mjs', '.github/ci/qualification.test.mjs'), mtime=0))
manifest = {
    'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'source_head': HEAD,
    'qualified': False,
    'failed_official_walk': {'label': 'final-canonical-walk-r519', 'exit': 1, 'run_id': '20260920-200719-83219'},
    'scope': 'Preserved failed second official walk, legitimate partial mints, bounded current W5 census repair, actual Opus 212-214, focused controls and native checks. A new official walk and exact-head local/hosted gates remain required.',
    'historical_m1_sha256': '44b0cca40a9ae8869ee219e6bbb6e449ce87346556084dfd622f167bd3f55b72',
    'historical_s2_sha256': 'd9cb88a8100cf481c1221dfe986bfc6036857cb875cc7dd667eb246cea90a4d3',
    'current_census': proof,
    'validation': receipts,
    'files': files,
}
(OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
for row in files:
    assert sha((OUT / row['path']).read_bytes()) == row['sha256']
print(json.dumps({'archive': str(OUT), 'files': len(files), 'bytes': sum(row['bytes'] for row in files), 'qualified': False}))
