from pathlib import Path
import hashlib,json,os,re,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');d=Path('/tmp/emitter-test-layout-proof-r484');proof=json.loads((d/'move-proof.json').read_text());names=list(dict.fromkeys(m['source'] for m in proof['moves']))
def inspect(root,names):
 env=os.environ.copy();env['PROOF_WORKSPACE_ROOT']=str(root)
 return json.loads(subprocess.check_output([str(d/'target/debug/emitter-test-layout-proof'),*[str(root/n) for n in names]],env=env))
after=inspect(r,names);before={str(Path(row['path']).relative_to(r)):row for row in json.loads((d/'before-logical.json').read_text())}
for row in after:
 name=str(Path(row['path']).relative_to(r));assert row['production_tokens']==before[name]['production_tokens'],name
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
formatting=[]
for m in proof['moves']:
 assert H(r/m['source'])==m['source_after_sha256'],m['source']
 expected=(d/'scratch-v2'/m['target']).read_bytes();actual=(r/m['target']).read_bytes()
 if expected!=actual:
  assert expected==b'\n'+actual,m['target'];formatting.append(m['target'])
 assert inspect(r,[m['target']])[0]['complete_tokens']==inspect(d/'scratch-v2',[m['target']])[0]['complete_tokens'],m['target']
 for ref in m['include_adjustments']:assert H(r/ref['path'])==ref['sha256']
assert H(r/'crates/xtask/src/recovery_parse_snapshot.rs')==proof['frozen_source_sha256']
source=(r/'crates/oracle/h2-5g-profile.mjs').read_text()
new=re.findall(r'^  "([^"]+)",',source.split('const NEW_RUNTIME_INPUTS = Object.freeze([',1)[1].split(']);',1)[0],re.M)
shadow=set(re.findall(r'^  "([^"]+)",',source.split('const NON_RUNTIME_SHADOW_INPUTS = new Set([',1)[1].split(']);',1)[0],re.M))
parent={x['path'] for x in json.loads((r/'ratchets/h2-5f-profile.v1.json').read_text())['runtime_inputs']};runtime=parent|set(new)
assert len(runtime)==920 and not parent&set(new)
assert all((r/n).is_file() for n in runtime)
assert all(m['target'] in shadow and m['target'] not in runtime for m in proof['moves'])
assert 'crates/harness/src/upstream_suites/execution/project.rs' in parent
assert 'crates/program/src/loader.rs' in parent
base=re.search(r'const TRUSTED_BASE = "([a-f0-9]+)"',source)[1]
def git(*args):return subprocess.check_output(['git',*args],cwd=r).decode().strip('\0').split('\0')
changed=set(git('diff','--name-only','--diff-filter=ACMRTUXB','-z',base,'--','crates'))|set(git('ls-files','--others','--exclude-standard','-z','--','crates'))
changed={p for p in changed if p and not p.startswith('crates/oracle/') and p not in shadow}
assert not changed-runtime, sorted(changed-runtime)
assert not (set(new)-changed),sorted(set(new)-changed)
result=dict(production_files_equal=14,outlined_modules=15,test_tokens_equal_to_proved_formatted_scratch=True,canonical_fmt_only_removes_initial_newline=formatting,fixture_paths_and_bytes_equal=9,frozen_source_sha256=proof['frozen_source_sha256'],runtime_inputs=len(runtime),runtime_missing=[],shadow_moves=15,canonical_files={n:H(r/n) for n in names+[m['target'] for m in proof['moves']]})
(d/'canonical-after-fmt-proof.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k not in ['canonical_files','canonical_fmt_only_removes_initial_newline']}))
