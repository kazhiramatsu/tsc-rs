"""First native syntax and printer probes for the reviewed variable successor."""
from pathlib import Path
import hashlib, json, os, shutil, subprocess, time

root = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-comma-prep')
base = root / 'docs/design/greenfield/slices/emitter-final-batch/integration'
target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert head.startswith('cfb0137c2')

def alive(pid):
    try: os.kill(pid, 0)
    except ProcessLookupError: return False
    return True

print('Waiting for independent project pipeline 94026; source HEAD remains fixed.', flush=True)
while alive(94026): time.sleep(20)
out = target / 'emitter-variable-probe-r148'; out.mkdir(exist_ok=False)
manifest = {'head': head, 'qualified': False, 'artifacts': [], 'steps': []}
sha = lambda data: hashlib.sha256(data).hexdigest()

def save():
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')

def run(label, args):
    assert subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() == head
    assert not subprocess.check_output(['git', '-C', str(root), 'diff', 'HEAD', '--name-only']).strip()
    code = subprocess.run(['python3', str(base / 'run-local.py'), label, *map(str, args)], cwd=root).returncode
    manifest['steps'].append({'label': label, 'exit': code}); save()
    return code

def collect(label, package, tests):
    found = []
    for line in (base / f'records/local/{label}.log').read_text().splitlines():
        try: event = json.loads(line)
        except json.JSONDecodeError: continue
        if event.get('reason') != 'compiler-artifact' or not event.get('executable') or not event.get('profile', {}).get('test'): continue
        assert package in event['package_id']
        original = Path(event['executable']); binary = out / original.name
        shutil.copy2(original, binary); os.chmod(binary, 0o555)
        listing = subprocess.check_output([str(binary), '--list', '--format', 'terse'], text=True)
        names = [s.removesuffix(': test') for s in listing.splitlines() if s.endswith(': test')]
        assert len(names) == tests, names
        item = {'binary': str(binary), 'sha256': sha(binary.read_bytes()), 'tests': names}
        found.append(item); manifest['artifacts'].append(item)
    assert len(found) == 1
    save(); return found[0]

code = run('variable-syntax-build-r148', ['cargo', 'test', '--offline', '-p', 'tsc-rs-syntax', '--lib', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
syntax = collect('variable-syntax-build-r148', 'tsc-rs-syntax', 203)
syntax_code = run('variable-syntax-native-r148', [syntax['binary'], '--test-threads=1'])
# Collect the independent emitter observation even if a syntax negative
# control disproves a prior assumption. Preserve each raw failed exit.
code = run('variable-flags-build-r148', ['cargo', 'test', '--offline', '-p', 'tsc-rs-emitter', '--test', 'list_comment_flags_contract', '--no-run', '--message-format=json'])
if code: raise SystemExit(code)
flags = collect('variable-flags-build-r148', 'tsc-rs-emitter', 1)
flags_code = run('variable-flags-native-r148', [flags['binary'], '--nocapture', '--test-threads=1'])
for a in manifest['artifacts']: assert sha(Path(a['binary']).read_bytes()) == a['sha256']
print(json.dumps(manifest['steps']), flush=True)
raise SystemExit(syntax_code or flags_code)
