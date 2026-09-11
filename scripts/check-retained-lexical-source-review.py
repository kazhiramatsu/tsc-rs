#!/usr/bin/env python3
"""Check A40 source-review integrity; this is explicitly not a readiness gate."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
sha = lambda p: digest((ROOT / p).read_bytes())
review = read('ratchets/h2-8a-retained-lexical-source-review.v1.json')
assert review['status'] == 'Source review checkpoint; NOT implementation-ready'
for key in ['source_inventory', 'candidate', 'before', 'runtime_preimage', 'context_research_input']:
    pin = review[key]
    assert sha(pin['path']) == pin['sha256'], pin['path']
subprocess.run(['node', 'scripts/inventory-retained-lexical-owners.mjs', '--check'], cwd=ROOT, check=True)
inventory = read(review['source_inventory']['path'])
owners = {r['id']: r for r in inventory['owners']}
before = read(review['before']['path'])
cases = set(before['exact_twice'] + before['failed_twice'])
assert len(cases) == before['eligible'] == 522
assert len(before['required_repair_ids']) == 76 and len(before['successor_controls']) == 57
assert len(before['exact_twice']) == 389 and before['minimum_exact_after'] == 465
candidate = (ROOT / review['candidate']['path']).read_bytes().splitlines(keepends=True)
source = (ROOT / inventory['source']['path']).read_bytes().splitlines(keepends=True)

def check_functions(rows):
    for row in rows:
        name = row['symbol'].split('::')[-1]
        body = b''.join(candidate[row['start'] - 1:row['end']])
        assert body.decode().startswith('    fn ' + name), name
        assert body.endswith(b'    }\n'), name
        assert digest(body) == row['sha256'], name

assert len({r['id'] for r in review['invariants']}) == len(review['invariants']) == 5
for row in review['invariants']:
    assert row['requirements'] and row['limits'] and row['implementation_orders']
    assert set(row['implementation_orders']) <= set(range(1, 10))
    assert row['witness_ids'] and set(row['witness_ids']) <= cases
    assert len(row['witness_ids']) == len(set(row['witness_ids']))
    for ref in row['source_owners']:
        assert owners[ref['id']]['sha256'] == ref['sha256']
    check_functions(row['candidate_functions'])
assert len(review['open_findings']) == 1
finding = review['open_findings'][0]
assert finding['id'] == 'A40-F-COMMA-FACTORY'
assert finding['state'] == 'unresolved-blocks-production'
assert finding['identity_obligation'] and len(finding['next_actions']) == 4
for owner in finding['source_owners']:
    body = b''.join(source[owner['start'] - 1:owner['end']])
    assert digest(body) == owner['line_span_sha256']
    assert body.decode() == owner['body']
check_functions(finding['candidate_functions'])
factory = finding['factory_observations']
assert factory['program_commands'] == factory['native_commands'] == 0
assert digest(finding['factory_observer_source'].encode()) == factory['observer_sha256']
assert sha('vendor/typescript-6.0.3/lib/typescript.js') == factory['compiler_sha256']
assert digest((json.dumps(factory, indent=2) + '\n').encode()) == finding['factory_observations_sha256']
assert len(factory['rows']) == 8
assert [len(r['result']['elements']) for r in factory['rows']] == [12, 12, 12, 11, 11, 11, 11, 11]
assert review['counts'] == {'source_owners': 220, 'predicates': 960, 'calls': 1436,
    'reviewed_invariants': 5, 'new_concrete_blocking_findings': 1}
print('A40 source review intact:5 reviewed invariants;220 inventoried owners;1 concrete factory gap remains;NOT implementation-ready')
