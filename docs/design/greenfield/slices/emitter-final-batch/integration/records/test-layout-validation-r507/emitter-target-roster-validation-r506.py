from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';D=Path('/tmp/emitter-target-roster-validation-r506');D.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest()
def inputs():
 paths=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z','--','crates','scripts','.github/ci'],cwd=R).decode().strip('\0').split('\0')
 return {n:h((R/n).read_bytes()) for n in sorted(set(paths)) if (R/n).is_file()}
actual=inputs();prior=json.loads(Path('/tmp/emitter-layout-validation-r498/manifest.json').read_bytes());changed={n for n in actual if actual[n]!=prior['source_inputs'].get(n)};assert changed=={'crates/xtask/src/workspace_maintenance.rs','crates/xtask/tests/unit/workspace_maintenance/tests.rs','scripts/chain-walk.sh','scripts/workspace-test-targets.json'},changed
report=dict(source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),source_inputs=actual,scope='Final target-roster and test-layout gate repair; all70dedicatedtestfiles and14outlinedproductiontokenstreams preserved. Includes actualworkspaceaudit that failed priorcap2; not final walk/CI/delivery.',prior_manifest_sha256=h(Path('/tmp/emitter-layout-validation-r498/manifest.json').read_bytes()),steps=[],focused_qualified=False)
def save():(D/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
save()
steps=[
 ('506-target-roster-fmt',['cargo','fmt','--all','--','--check']),
 ('506-target-roster-workspace-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','workspace_maintenance::','--','--test-threads=1']),
 ('506-target-roster-workspace-audit',['cargo','xtask','workspace-audit']),
 ('506-target-roster-qualification',['node','--test','.github/ci/qualification.test.mjs']),
 ('506-target-roster-pin-preflight',['python3','scripts/walk-preflight.py']),
 ('506-target-roster-receipt-controls',['python3','/tmp/emitter-test-preflight-fingerprint-r503.py']),
]
for label,cmd in steps:
 assert inputs()==actual,'source changed before '+label
 code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode
 p=B/'records/local'/f'{label}.json';receipt=json.loads(p.read_bytes());assert receipt['exit']==code
 report['steps'].append(dict(label=label,exit=code,argv=cmd,receipt_sha256=h(p.read_bytes())));save();assert inputs()==actual,'source changed after '+label
 assert code==0,label
report['focused_qualified']=True;save();print('FINAL TOPOLOGY AND LAYOUT CHECKS GREEN',flush=True)
