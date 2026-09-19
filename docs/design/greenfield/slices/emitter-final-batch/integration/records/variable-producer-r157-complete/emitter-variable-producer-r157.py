"""Qualify the repaired bounded candidate, keeping original failures immutable."""
from pathlib import Path
import hashlib, json, os, re, shutil, subprocess

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
base = root / 'docs/design/greenfield/slices/emitter-final-batch/integration'
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert head == Path('/tmp/emitter-variable-producer-r157-head').read_text().strip()
out = target / 'emitter-variable-type-r157-native'
out.mkdir(exist_ok=False)
manifest = {'head': head, 'qualified': False, 'artifacts': [], 'steps': []}
sha = lambda data: hashlib.sha256(data).hexdigest()

def save():
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')

def check():
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()

def run(label, args, env=None):
    check()
    code = subprocess.run(['python3', str(base / 'run-local.py'), label, *map(str, args)], cwd=root, env=env).returncode
    check()
    manifest['steps'].append({'label': label, 'exit': code}); save()
    return code

def collect(label, package, count):
    artifacts = []
    for line in (base / f'records/local/{label}.log').read_text().splitlines():
        try: event = json.loads(line)
        except json.JSONDecodeError: continue
        if event.get('reason') != 'compiler-artifact' or not event.get('executable') or not event.get('profile', {}).get('test'): continue
        assert package in event['package_id']
        source = Path(event['executable']); binary = out / source.name
        assert not binary.exists()
        shutil.copy2(source, binary); os.chmod(binary, 0o555)
        listing = subprocess.check_output([str(binary), '--list', '--format', 'terse'], text=True)
        names = [s.removesuffix(': test') for s in listing.splitlines() if s.endswith(': test')]
        item = {'target': event['target']['name'], 'binary': str(binary), 'sha256': sha(binary.read_bytes()), 'tests': names}
        artifacts.append(item); manifest['artifacts'].append(item)
    assert len(artifacts) == count
    save(); return artifacts

code = run('variable-type-compiler-build-r157', ['cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', '--test', 'transpile_routes_contract', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
artifacts = {a['target']: a for a in collect('variable-type-compiler-build-r157', 'tsc-rs-compiler', 2)}
contract = artifacts['contracts']; transpile = artifacts['transpile_routes_contract']
assert len(contract['tests']) == 485 and len(transpile['tests']) == 9
groups = [('variable-type-native-r157', ['variable_type_recovery_matches_complete_typescript_commands'], 404),
          ('variable-comma-native-r157', ['variable_comma_recovery_matches_complete_typescript_commands'], 145),
          ('variable-type-neighbours-r157', [n + '_match_complete_typescript_commands' for n in ['r104_object_rest_controls', 'r107_declaration_comment_controls', 'r113_type_comment_controls', 'r117_type_comment_controls', 'r109_token_neighbours', 'r111_do_body_controls', 'r119_type_comment_controls']], None)]
for label, names, cases in groups:
    tests = ['emitter_residual_audit::' + n for n in names]
    assert set(tests) <= set(contract['tests'])
    capture = target / (label + '-captures'); assert not capture.exists()
    env = os.environ.copy(); env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR'] = str(capture)
    code = run(label, [contract['binary'], '--exact', *tests, '--nocapture', '--test-threads=1'], env)
    manifest['steps'][-1]['expected_cases'] = cases; save()
    if code: raise SystemExit(code)

code = run('variable-type-emitter-build-r157', ['cargo', 'test', '--offline', '-p', 'tsc-rs-emitter', '--tests', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
emitter = collect('variable-type-emitter-build-r157', 'tsc-rs-emitter', 23)
assert sum(len(a['tests']) for a in emitter) == 1015
for artifact in sorted(emitter, key=lambda item: item['target'] != 'list_comment_flags_contract'):
    label = 'variable-emitter-' + artifact['target'].replace('_', '-') + '-r157'
    code = run(label, [artifact['binary'], '--test-threads=1'])
    if code: raise SystemExit(code)
    log = (base / f'records/local/{label}.log').read_text()
    results = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;', log)
    assert results == [('ok', str(len(artifact['tests'])), '0', '0', '0')], results



evidence = root / 'target/h2-8c'
if evidence.exists(): shutil.copytree(evidence, out / 'prior-transpile-evidence')
code = run('variable-type-transpile-r157', [transpile['binary'], '--test-threads=1'])
if evidence.exists(): shutil.copytree(evidence, out / 'transpile-evidence')
for a in manifest['artifacts']: assert sha(Path(a['binary']).read_bytes()) == a['sha256']
print(json.dumps(manifest['steps']), flush=True)
# Even a stale-KNOWN exit remains a raw failure; retirement needs separate proof.
raise SystemExit(code)
