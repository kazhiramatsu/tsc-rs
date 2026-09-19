"""Test the isolated candidate after the original census/parser proof releases Cargo."""
from pathlib import Path
import hashlib, json, os, re, shutil, subprocess, time

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-recovery-prep')
main = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
relative = Path('docs/design/greenfield/slices/emitter-final-batch/integration')
base = root / relative
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert head.startswith('84ed2b9e4')
sha = lambda data: hashlib.sha256(data).hexdigest()
def alive(pid):
    try: os.kill(pid, 0)
    except ProcessLookupError: return False
    return True
def check():
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
def run(label, argv, env=None):
    check()
    code = subprocess.run(['python3', str(base / 'run-local.py'), label, *map(str, argv)], cwd=root, env=env).returncode
    check()
    return code

print('Waiting for the original census and four-parser proof; candidate sources remain fixed.', flush=True)
while any(alive(pid) for pid in (56011, 56007, 53919, 89429)):
    time.sleep(45)
proof_path = main / relative / 'records/local/corpus-replay-pipeline-r131.json'
for _ in range(5):
    if proof_path.exists(): break
    time.sleep(1)
proof = json.loads(proof_path.read_bytes())
assert proof['exit'] == 0, 'review the original parser proof failure before building a later candidate'
out = target / 'emitter-variable-type-r132-native'
out.mkdir(exist_ok=False)
manifest = {'head': head, 'qualified': False, 'artifacts': [], 'steps': []}
def save():
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
def collect(label, package, count):
    found = []
    for line in (base / f'records/local/{label}.log').read_text().splitlines():
        try: event = json.loads(line)
        except json.JSONDecodeError: continue
        if event.get('reason') == 'compiler-artifact' and event.get('executable') and event.get('profile', {}).get('test'):
            assert package in event['package_id']
            original = Path(event['executable'])
            binary = out / original.name
            shutil.copy2(original, binary); os.chmod(binary, 0o555)
            listing = subprocess.check_output([str(binary), '--list', '--format', 'terse'], text=True)
            names = [line.removesuffix(': test') for line in listing.splitlines() if line.endswith(': test')]
            artifact = {'target': event['target']['name'], 'binary': str(binary), 'sha256': sha(binary.read_bytes()), 'tests': names}
            found.append(artifact); manifest['artifacts'].append(artifact)
    assert len(found) == count, found
    save(); return found

code = run('variable-type-syntax-build-r132', ['cargo', 'test', '--offline', '-p', 'tsc-rs-syntax', '--lib', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
syntax, = collect('variable-type-syntax-build-r132', 'tsc-rs-syntax', 1)
assert sum('missing_variable_type_' in name for name in syntax['tests']) == 2
code = run('variable-type-syntax-native-r132', [syntax['binary'], '--test-threads=1'])
manifest['steps'].append({'label': 'variable-type-syntax-native-r132', 'exit': code}); save()
if code: raise SystemExit(code)
code = run('variable-type-compiler-build-r132', ['cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', '--test', 'transpile_routes_contract', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
artifacts = {a['target']: a for a in collect('variable-type-compiler-build-r132', 'tsc-rs-compiler', 2)}
contract = artifacts['contracts']; transpile = artifacts['transpile_routes_contract']
test = 'emitter_residual_audit::variable_type_recovery_matches_complete_typescript_commands'
assert test in contract['tests'] and len(contract['tests']) == 484 and len(transpile['tests']) == 9
env = os.environ.copy()
capture = target / 'emitter-variable-type-r132-captures'
assert not capture.exists()
env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR'] = str(capture)
code = run('variable-type-native-r132', [contract['binary'], '--exact', test, '--nocapture', '--test-threads=1'], env)
manifest['steps'].append({'label': 'variable-type-native-r132', 'exit': code, 'expected_cases': 92}); save()
# The old transpile route is an independent impact check. Retain its raw stale
# KNOWN exit even if the new command controls exposed a different failure.
evidence = root / 'target/h2-8c'
if evidence.exists(): shutil.copytree(evidence, out / 'prior-transpile-evidence')
transpile_code = run('variable-type-transpile-r132', [transpile['binary'], '--test-threads=1'])
if evidence.exists(): shutil.copytree(evidence, out / 'transpile-evidence')
manifest['steps'].append({'label': 'variable-type-transpile-r132', 'exit': transpile_code}); save()
for artifact in manifest['artifacts']:
    assert sha(Path(artifact['binary']).read_bytes()) == artifact['sha256']
print(json.dumps(manifest['steps']), flush=True)
raise SystemExit(code or transpile_code)
