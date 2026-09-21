from pathlib import Path
import hashlib,json,subprocess,tempfile
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');d=Path('/tmp/emitter-layout-followup-r493')
s=(r/'scripts/chain-walk.sh').read_text();body='crate_tree_sha() {'+s.split('crate_tree_sha() {',1)[1].split('PREFLIGHT_RECEIPT=',1)[0]
assert 'scripts/inline-tests-scan.py scripts/frozen-test-layout.json' in body
with tempfile.TemporaryDirectory(prefix='emitter-receipt-inputs-') as tmp:
 root=Path(tmp)
 files={'Cargo.toml':'workspace','Cargo.lock':'lock','crates/fixture/Cargo.toml':'package','crates/fixture/src/lib.rs':'pub fn marker() {}','scripts/inline-tests-scan.py':'scanner-v1','scripts/frozen-test-layout.json':'{"schema":1}'}
 for p,text in files.items():
  path=root/p;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
 def fingerprint():
  return subprocess.check_output(['bash'],input=(body+'\npreflight_tree_sha\n').encode(),cwd=root).decode().strip()
 original=fingerprint();assert len(original)==64
 controls=[]
 for p in ['scripts/inline-tests-scan.py','scripts/frozen-test-layout.json','crates/fixture/src/lib.rs','Cargo.toml','Cargo.lock']:
  path=root/p;before=path.read_bytes();path.write_bytes(before+b'\n');changed=fingerprint();assert changed!=original;path.write_bytes(before);assert fingerprint()==original
  controls.append(dict(changed_input=p,before=original,after=changed,restored=True))
 result=dict(qualified=True,scope='Actual extracted preflight_tree_sha function refuses reuse after scanner, shared layout descriptor, Rust or Cargo input changes; no canonical walk executed.',chain_walk_sha256=hashlib.sha256(s.encode()).hexdigest(),controls=controls)
 (d/'preflight-fingerprint-controls.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps(dict(qualified=True,controls=len(controls))))
