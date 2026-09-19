"""Archive the completed full comparisons without converting stale-guard failures to success."""
from pathlib import Path
import gzip, hashlib, json, os, re, subprocess

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
prep = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-declaration-prep')
relative = Path('docs/design/greenfield/slices/emitter-final-batch/integration')
base, dest = root / relative, prep / relative
head = 'b451489e4a18abbff42651d8eb814537f4c5f800'
for pid in (83365, 83628):
    try: os.kill(pid, 0)
    except ProcessLookupError: pass
    else: raise SystemExit(f'full comparison still active: {pid}')
assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
driver = json.loads((base / 'records/local/universe-full-driver-r124.json').read_bytes())
assert driver['head'] == head and len(driver['steps']) == 2 and 'finished' in driver
sha = lambda data: hashlib.sha256(data).hexdigest()
assert sha(Path(driver['binary']['immutable']).read_bytes()) == driver['binary']['sha256']
result = {'source_head': head, 'binary': driver['binary'], 'repetitions': 2,
          'corpus_impact_qualification': 'pending', 'known_retirement': 'pending',
          'performance_qualification': False, 'sets': []}
copies = [Path('records/local/universe-full-driver-r124.json')]
known = json.loads((base / 'records/local/census-known-build-pause-r122.json').read_bytes())
fixture_root = root / 'crates/compiler/tests/fixtures'
old_rows = json.loads((fixture_root / 'emitter-final-known-native.json').read_bytes())['cases']
for label, fixture, count, stale_count in [
    ('universe-217-r124', 'emitter-final-universe.json', 217, 1),
    ('plan-base-1798-r124', 'emitter-final-universe-plan-base.json', 1798, 35),
]:
    path = fixture_root / fixture
    fixture_bytes = path.read_bytes() if path.exists() else subprocess.check_output(['zstd', '-q', '-d', '-c', str(path) + '.zst'])
    rows = json.loads(fixture_bytes)['cases']
    expected_ids = [row['case_id'] for row in rows]
    receipt = json.loads((base / f'records/local/{label}.json').read_bytes())
    log_bytes = gzip.decompress((base / f'records/local/{label}.log.gz').read_bytes())
    log = log_bytes.decode()
    ids = re.findall(r'emitter-final universe EXACT x2 (\S+)', log)
    stale = re.findall(r'KNOWN rows replay exact now; retire them: (\[.*\])', log)
    assert len(ids) == len(set(ids)) == len(expected_ids) == count and set(ids) == set(expected_ids)
    assert receipt['head'] == head and receipt['tracked_clean'] and receipt['exit'] == 101
    assert receipt['log_sha256'] == sha(log_bytes) and receipt['log_bytes'] == len(log_bytes)
    assert log.count('panicked at') == 1 and log.count('DIVERGING') == 0 and len(stale) == 1
    stale_ids = json.loads(stale[0])
    assert len(stale_ids) == stale_count and set(stale_ids) == {row['case_id'] for row in old_rows if row['fixture'] == fixture}
    assert not Path(receipt['env']['TSC_RS_EMITTER_FINAL_FAILURE_DIR']).exists()
    result['sets'].append({'label': label, 'exact_cases': count, 'case_ids': ids,
                          'stale_known_ids': stale_ids, 'raw_test_exit': receipt['exit'],
                          'failure_reason': 'stale-KNOWN retirement assertion only',
                          'seconds': receipt['seconds'], 'log_sha256': sha(log_bytes),
                          'reference_json_sha256': sha(fixture_bytes), 'new_divergences': 0})
    copies += [Path(f'records/local/{label}{suffix}') for suffix in ('.json', '.log.gz')]
guard = json.loads((base / 'records/local/universe-comparison-guards-r124.json').read_bytes())
guard_log = gzip.decompress((base / 'records/local/universe-comparison-guards-r124.log.gz').read_bytes())
assert guard['exit'] == 0 and guard['head'] == head and guard['log_sha256'] == sha(guard_log)
assert b'3 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out' in guard_log
result['comparison_guards'] = {'passed': 3, 'ignored': 0, 'filtered': 2, 'exit': 0, 'log_sha256': sha(guard_log)}
copies += [Path(f'records/local/universe-comparison-guards-r124{suffix}') for suffix in ('.json', '.log.gz')]
for path in copies:
    with (dest / path).open('xb') as file: file.write((base / path).read_bytes())
with (dest / 'records/measure-universe-full-r124.py').open('xb') as file:
    file.write(Path('/tmp/emitter-universe-r124.py').read_bytes())
with (dest / 'records/archive-universe-full-r124.py').open('xb') as file:
    file.write(Path(__file__).read_bytes())
with (dest / 'cross-review/r124-full-universe-results.json').open('x') as file:
    json.dump(result, file, indent=2); file.write('\n')
text = f'''# Full EF7 and PLAN-BASE comparison at the r120 source

Source `{head}` and the immutable universe binary remained unchanged throughout both full runs. All **217 EF7 rows and 1,798 PLAN-BASE rows compare exact twice**, with no new differences or failure captures. The original reference membership is unchanged; `r124-full-universe-results.json` records every executed ID, fixture content hash, binary hash and log hash.

Both Rust tests return **101**, solely because their stale-KNOWN assertions correctly detect the old one-row and 35-row refusal registrations. This is not a green test-suite claim. The corpus impact proof remains pending, so these registrations have not yet been retired. The three independent comparator/shard guards pass, with no ignored tests.

The raw receipts and compressed logs are under `../records/local/`. EF7 took {result['sets'][0]['seconds']} seconds and PLAN-BASE took {result['sets'][1]['seconds']} seconds. These are demoted functional measurements concurrent with the census, not performance qualification. The archived driver preserves both raw failing exits; the archiver verifies exact fixture ID coverage and the sole stale-guard failure before recording this result.
'''
with (dest / 'cross-review/r124-full-universe-results.md').open('x') as file: file.write(text)
print(json.dumps({'exact_x2': [s['exact_cases'] for s in result['sets']], 'stale_known': [len(s['stale_known_ids']) for s in result['sets']], 'guards': 3}))
