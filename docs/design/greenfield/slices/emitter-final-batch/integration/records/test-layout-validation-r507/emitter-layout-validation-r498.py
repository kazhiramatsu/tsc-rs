from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';D=Path('/tmp/emitter-layout-validation-r498');D.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest()
def inputs():
 names=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z','--','crates','scripts','.github/ci'],cwd=R).decode().strip('\0').split('\0')
 return {n:h((R/n).read_bytes()) for n in sorted(set(names)) if (R/n).is_file()}
previous=json.loads(Path('/tmp/emitter-layout-validation-r495/manifest.json').read_bytes());actual=inputs();assert actual.keys()==previous['source_inputs'].keys();changed=[n for n in actual if actual[n]!=previous['source_inputs'][n]];assert changed==['.github/ci/qualification-policy.v2.json'],changed
pin=json.loads(Path('/tmp/emitter-layout-policy-pin-r497/manifest.json').read_bytes());assert actual[changed[0]]==pin['policy_after_sha256'] and previous['source_inputs'][changed[0]]==pin['policy_before_sha256']
report=dict(scope='Successor of focused495; same source except one verified current-source hash in hosted policy. Re-run all50qualification tests; then previously unexecuted Rust unit bands and actual workspaceaudit. Not final walk/CI/delivery.',source_head=previous['source_head'],source_inputs=actual,prior_manifest_sha256=h(Path('/tmp/emitter-layout-validation-r495/manifest.json').read_bytes()),retained_successful_steps=[s for s in previous['steps'] if s['exit']==0],retained_failure=previous['steps'][-1],steps=[],focused_qualified=False)
def save():(D/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
save()
steps=[
 ('498-layout-qualification',['node','--test','.github/ci/qualification.test.mjs']),
 ('498-layout-workspace-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','workspace_maintenance::','--','--test-threads=1']),
 ('498-layout-de-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','h2_7de_acceptance::','--','--test-threads=1']),
 ('498-layout-native-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','recovery_corpus_native::','--','--test-threads=1']),
 ('498-layout-workspace-audit',['cargo','xtask','workspace','audit']),
 ('498-layout-pin-preflight',['python3','scripts/walk-preflight.py']),
]
for label,cmd in steps:
 assert inputs()==actual,'Source changed before '+label
 code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode
 receipt=B/'records/local'/f'{label}.json';v=json.loads(receipt.read_bytes());assert v['exit']==code
 report['steps'].append(dict(label=label,argv=cmd,exit=code,receipt_sha256=h(receipt.read_bytes())));save();assert inputs()==actual,'Source changed after '+label
 assert code==0,label
report['focused_qualified']=True;save();print('LAYOUT SUCCESSOR FOCUSED VALIDATION GREEN',flush=True)
