"""Check the separate transpile/noCheck KNOWN guards at the measured source."""
from pathlib import Path
import datetime, hashlib, json, os, re, shutil, signal, subprocess

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base = root / 'docs/design/greenfield/slices/emitter-final-batch/integration'
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = 'd0edc1a74a6249318b19aa449ab2735983a8df46'
pid = 56011
manifest = json.loads((target / 'emitter-census-r78-immutable/manifest.json').read_bytes())
out = target / 'emitter-transpile-r127'
out.mkdir(exist_ok=False)
sha = lambda data: hashlib.sha256(data).hexdigest()
now = lambda: datetime.datetime.now(datetime.timezone.utc).isoformat()
def check_source():
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
def check_census():
    binary = Path(manifest['original_executable'])
    assert sha(binary.read_bytes()) == manifest['sha256']
    mappings = subprocess.check_output(['lsof', '-a', '-p', str(pid), '-d', 'txt', '-Fni'], text=True)
    assert f"i{manifest['original_inode']}\nn{binary}\n" in mappings
def run(label, command):
    check_source()
    result = subprocess.run(['python3', str(base / 'run-local.py'), label, *map(str, command)], cwd=root)
    check_source()
    return result.returncode

check_source(); check_census()
state = {'head': head, 'census_binary': manifest, 'purpose': 'Check independent live KNOWN tables affected by parse admission; not performance qualification'}
os.kill(pid, signal.SIGSTOP)
state['paused_at'] = now()
try:
    code = run('transpile-route-build-r127', ['cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'transpile_routes_contract', '--no-run'])
finally:
    check_census(); os.kill(pid, signal.SIGCONT)
    state['resumed_at'] = now()
    state['census_executable_preserved'] = True
    (base / 'records/local/census-transpile-build-pause-r127.json').write_text(json.dumps(state, indent=2) + '\n')
if code: raise SystemExit(code)
log = (base / 'records/local/transpile-route-build-r127.log').read_text()
paths = re.findall(r'Executable tests/transpile_routes_contract.rs \(([^)]+)\)', log)
assert len(paths) == 1, paths
binary = out / 'transpile_routes_contract'
shutil.copy2(paths[0], binary); os.chmod(binary, 0o555)
binary_sha = sha(binary.read_bytes())
listing = subprocess.check_output([str(binary), '--list', '--format', 'terse'], text=True)
names = [line.removesuffix(': test') for line in listing.splitlines() if line.endswith(': test')]
assert len(names) == len(set(names)) == 9
evidence = root / 'target/h2-8c'
if evidence.exists(): shutil.copytree(evidence, out / 'prior-evidence')
(out / 'manifest.json').write_text(json.dumps({'head': head, 'binary': str(binary), 'sha256': binary_sha, 'test_names': names}, indent=2) + '\n')
code = run('transpile-routes-native-r127', [binary, '--nocapture', '--test-threads=1'])
assert sha(binary.read_bytes()) == binary_sha
if evidence.exists(): shutil.copytree(evidence, out / 'evidence')
print(json.dumps({'exit': code, 'head': head, 'retained_evidence': str(out / 'evidence')}), flush=True)
raise SystemExit(code)
