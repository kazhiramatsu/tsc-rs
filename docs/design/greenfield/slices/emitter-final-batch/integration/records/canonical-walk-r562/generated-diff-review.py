"""Read-only review of completed walk differences; run only after real success."""
from pathlib import Path
import gzip
import hashlib
import json
import re
import subprocess

ROOT = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
BASE = ROOT / 'docs/design/greenfield/slices/emitter-final-batch/integration'
OUT = Path('/tmp/emitter-final-walk-review-r562.json')
git = lambda *args: subprocess.check_output(['git', *args], cwd=ROOT)
head = git('rev-parse', 'HEAD').decode().strip()
assert head == '11d1971dd31a68e05fe18bedb26b6fdef4e9d170'
receipt = json.loads((BASE / 'records/local/final-canonical-walk-r561.json').read_text())
assert receipt['head'] == head and receipt['exit'] == 0 and receipt['tracked_clean']
raw = gzip.decompress((BASE / 'records/local/final-canonical-walk-r561.log.gz').read_bytes())
assert hashlib.sha256(raw).hexdigest() == receipt['log_sha256']
assert b'chain walk: converged and green' in raw
changes = git('diff', '--name-only').decode().splitlines()
assert all(name.startswith(('crates/oracle/', 'ratchets/', '.github/ci/contracts/')) for name in changes)
assert not any(name.endswith('.rs') for name in changes)
review = []
for name in changes:
    before = git('show', 'HEAD:' + name)
    after = (ROOT / name).read_bytes()
    row = {'path': name, 'before_sha256': hashlib.sha256(before).hexdigest(),
           'after_sha256': hashlib.sha256(after).hexdigest()}
    if name.endswith('.mjs'):
        normalize = lambda data: re.sub(rb'[a-f0-9]{64}', b'<HASH>', data)
        row['hash_literals_only'] = normalize(before) == normalize(after)
        assert row['hash_literals_only'], name
    if name.endswith('.json'):
        old, new = json.loads(before), json.loads(after)
        if isinstance(old, dict) and isinstance(new, dict):
            row['changed_top_level'] = sorted(key for key in old.keys() | new.keys() if old.get(key) != new.get(key))
            row['content_checks'] = {key: old[key] == new.get(key) for key in
                ['cases', 'observations', 'case_manifest', 'summary', 'stratum',
                 'm2_supplement', 'm3_supplement', 'execution_contract', 'selection_contract'] if key in old}
    review.append(row)
output = {'walk_source_head': head, 'walk_receipt_success': True,
          'scope': 'Read-only generated diff classification; manual review and final local/hosted gates remain required',
          'changes': review}
assert not OUT.exists()
OUT.write_text(json.dumps(output, indent=2) + '\n')
print(json.dumps(output, indent=2))
