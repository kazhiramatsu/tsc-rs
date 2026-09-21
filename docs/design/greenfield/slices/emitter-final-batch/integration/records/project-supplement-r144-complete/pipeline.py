"""Serialize the independent project experiment after the variable native run."""
from pathlib import Path
import hashlib, json, os, shutil, subprocess, time

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-project-projection-prep')
base = root / 'docs/design/greenfield/slices/emitter-final-batch/integration'
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert head.startswith('d345e83c4')

def alive(pid):
    try: os.kill(pid, 0)
    except ProcessLookupError: return False
    return True

print('Waiting for variable pipeline 93408; no competing Cargo builds.', flush=True)
while alive(93408): time.sleep(30)
variable_receipt = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-recovery-prep') / base.relative_to(root) / 'records/local/variable-type-pipeline-r143.json'
for _ in range(5):
    if variable_receipt.exists(): break
    time.sleep(1)
assert variable_receipt.exists(), 'previous pipeline must have a completed receipt'
out = target / 'emitter-project-supplement-r144'; out.mkdir(exist_ok=False)
manifest = {'head': head, 'qualified': False, 'artifacts': [], 'steps': [], 'prior_pipeline_exit': json.loads(variable_receipt.read_bytes())['exit']}
sha = lambda b: hashlib.sha256(b).hexdigest()

def save():
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')

def run(label, args):
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
    code = subprocess.run(['python3', str(base / 'run-local.py'), label, *map(str, args)], cwd=root).returncode
    manifest['steps'].append({'label': label, 'exit': code}); save()
    if code: raise SystemExit(code)

def collect(label, package, count, test):
    found = []
    for line in (base / f'records/local/{label}.log').read_text().splitlines():
        try: event = json.loads(line)
        except json.JSONDecodeError: continue
        if event.get('reason') != 'compiler-artifact' or not event.get('executable') or event.get('profile', {}).get('test') != test: continue
        assert package in event['package_id']
        original = Path(event['executable']); binary = out / original.name
        assert not binary.exists()
        shutil.copy2(original, binary); os.chmod(binary, 0o555)
        item = {'target': event['target']['name'], 'binary': str(binary), 'sha256': sha(binary.read_bytes()), 'test': test}
        if test:
            listing = subprocess.check_output([str(binary), '--list', '--format', 'terse'], text=True)
            item['tests'] = [s.removesuffix(': test') for s in listing.splitlines() if s.endswith(': test')]
        found.append(item); manifest['artifacts'].append(item)
    assert len(found) == count, found
    save(); return found

run('project-harness-build-r144', ['cargo', 'test', '--offline', '-p', 'tsc-rs-harness', '--lib', '--test', 'contracts', '--no-run', '--message-format=json'])
harness = collect('project-harness-build-r144', 'tsc-rs-harness', 2, True)
for a in harness:
    if a['target'] == 'contracts':
        assert sum(name.startswith('h2_5h_project_emit::') for name in a['tests']) == 4
        run('project-harness-contracts-r144', [a['binary'], 'h2_5h_project_emit::', '--nocapture', '--test-threads=1'])
    else:
        assert any('absent_descriptor_roots_override_config_roots' in name for name in a['tests'])
        run('project-harness-lib-r144', [a['binary'], '--test-threads=1'])
run('project-xtask-build-r144', ['cargo', 'build', '--offline', '-p', 'tsc-rs-xtask', '--bin', 'xtask', '--message-format=json'])
native, = collect('project-xtask-build-r144', 'tsc-rs-xtask', 1, False)
run('project-xtask-tests-build-r144', ['cargo', 'test', '--offline', '-p', 'tsc-rs-xtask', '--bin', 'xtask', '--no-run', '--message-format=json'])
tests, = collect('project-xtask-tests-build-r144', 'tsc-rs-xtask', 1, True)
assert any(name.startswith('recovery_corpus_native::') for name in tests['tests'])
run('project-native-guards-r144', [tests['binary'], 'recovery_corpus_native::', '--nocapture', '--test-threads=1'])
roster = base / 'records/project-projection-roster-r139.json'
run('project-native-108-r144', [native['binary'], 'project-command-supplement', roster, out / 'native.json', '/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('project-oracle-108-r144', ['node', 'scripts/observe-project-command-supplement.mjs', out / 'native.json', roster, out / 'oracle.json'])
run('project-comparison-108-r144', ['python3', 'scripts/compare-project-command-supplement.py', '--native', out / 'native.json', '--oracle', out / 'oracle.json', '--out', out / 'comparison.json'])
for a in manifest['artifacts']: assert sha(Path(a['binary']).read_bytes()) == a['sha256']
manifest['qualified'] = True; save()
print(json.dumps(manifest['steps']), flush=True)
