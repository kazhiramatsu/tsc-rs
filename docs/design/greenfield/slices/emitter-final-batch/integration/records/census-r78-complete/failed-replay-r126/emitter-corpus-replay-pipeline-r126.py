"""Continue the reviewed parser proof only after the unchanged census finishes."""
from pathlib import Path
import gzip, hashlib, json, os, subprocess, time

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
census = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census')
probe = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-corpus-replay')
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
relative = Path('docs/design/greenfield/slices/emitter-final-batch/integration')
root_head = 'd0edc1a74a6249318b19aa449ab2735983a8df46'
census_head = '67df86615a45f9595025064bcc12fde93063fedf'
sha = lambda data: hashlib.sha256(data).hexdigest()
def git(tree, *args):
    return subprocess.check_output(['git', '-C', str(tree), *args])
def clean(tree, head):
    assert git(tree, 'rev-parse', 'HEAD').decode().strip() == head
    assert not git(tree, 'diff', 'HEAD', '--name-only').strip()
def alive(pid):
    try: os.kill(pid, 0)
    except ProcessLookupError: return False
    return True

print('Waiting for original census and its receipt writer; no replacement census or rebuild.', flush=True)
while any(alive(pid) for pid in (56011, 56007, 53919)):
    time.sleep(45)
clean(census, census_head)
clean(root, root_head)
receipt_path = census / relative / 'records/local/recovery-full-census-r78.json'
receipt = json.loads(receipt_path.read_bytes())
assert receipt['head'] == census_head and receipt['tracked_clean'] and receipt['exit'] == 0, receipt
log = gzip.decompress(receipt_path.with_suffix('.log.gz').read_bytes())
assert receipt['log_sha256'] == sha(log) and receipt['log_bytes'] == len(log)
binary = target / 'debug/xtask'
assert sha(binary.read_bytes()) == '8bdfd58da50dd32576998c8604c14acca42ec64ae812ed4590aa3eba442ab47d'
assert binary.stat().st_ino == 120896494
inventory_path = root / relative / 'records/census-r78-claimed-id-inventory.json.gz'
inventory = json.loads(gzip.decompress(inventory_path.read_bytes()))
assert inventory['head'] == census_head
assert inventory['producer_source_sha256'] == sha((census / 'crates/xtask/src/utf16_literal_recovery_census.rs').read_bytes())
for path, expected in inventory['input_sha256'].items():
    assert sha((census / path).read_bytes()) == expected, path
snapshot_path = target / 'emitter-recovery-census-r78/parse-snapshot.json'
snapshot_bytes = snapshot_path.read_bytes()
snapshot = json.loads(snapshot_bytes)
assert snapshot['schema'] == 1 and snapshot['kind'] == 'emitter-recovery-parse-snapshot'
assert snapshot['head'] == census_head
loaded = [row['case_id'] for row in snapshot['rows']]
failed = [row['case_id'] for row in snapshot['load_failures']]
expected = inventory['case_ids']
assert len(expected) == len(set(expected)) == 14329
assert len(loaded) == len(set(loaded)) and len(failed) == len(set(failed))
assert not set(loaded) & set(failed)
assert set(loaded) | set(failed) == set(expected), 'census did not account for every claimed input'
assert set(snapshot['inputs']) == set(snapshot['digests'])
coverage = {'head': census_head, 'snapshot_sha256': sha(snapshot_bytes),
            'claimed_ids': 14329, 'loaded_rows': len(loaded), 'load_failures': len(failed),
            'parse_inputs': len(snapshot['inputs']), 'inventory_sha256': sha(inventory_path.read_bytes()),
            'execution_receipt_sha256': sha(receipt_path.read_bytes()),
            'complete_command_qualification': False}
with Path('/tmp/emitter-census-coverage-r126.json').open('x') as file:
    json.dump(coverage, file, indent=2); file.write('\n')
print(json.dumps(coverage), flush=True)
del snapshot, snapshot_bytes
subprocess.run(['python3', '/tmp/emitter-parser-replays-after-census.py'], check=True, cwd=probe)
clean(root, root_head)
out = target / 'emitter-recovery-parser-replays-r107'
subprocess.run(['python3', str(probe / 'scripts/select-recovery-parse-corpus.py'),
    '--snapshot', str(snapshot_path), '--current', str(out / 'current.json'),
    '--projection', str(out / 'projection.json'), '--merge-base', str(out / 'merge-base.json'),
    '--profiles-dir', str(target / 'emitter-recovery-census-r78/profiles'),
    '--successor', str(out / 'successor.json'), '--out', str(out / 'selection.json')], check=True, cwd=probe)
clean(root, root_head)
print('Parser proof and selection finished; review selection and all failures before native/TypeScript command qualification.', flush=True)
