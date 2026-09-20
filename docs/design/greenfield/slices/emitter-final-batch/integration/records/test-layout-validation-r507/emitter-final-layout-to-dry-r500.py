from pathlib import Path
import json,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';P=Path('/tmp/emitter-layout-validation-r498/manifest.json');D=Path('/tmp/emitter-layout-final-preflight-r500');D.mkdir(exist_ok=False)
# Proceed only after the entire focused predecessor succeeds, never on a partial log.
while True:
 result=json.loads(P.read_bytes())
 if result.get('focused_qualified'):break
 if any(row['exit']!=0 for row in result['steps']):raise SystemExit('Focused predecessor failed; preserve and inspect')
 time.sleep(2)
with (D/'commit.log').open('xb') as log:
 code=subprocess.run(['python3','/tmp/commit-emitter-layout-r499.py'],cwd=R,stdout=log,stderr=subprocess.STDOUT).returncode
print('commit',code,flush=True);assert code==0
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip();out=D/'pins'
with (D/'pin-prepare.log').open('xb') as log:
 code=subprocess.run(['python3','/tmp/prepare-emitter-final-pins.py','--tree',str(R),'--head',head,'--out',str(out)],cwd=R,stdout=log,stderr=subprocess.STDOUT).returncode
assert code==0;proposal=json.loads((out/'proposal.json').read_bytes());assert proposal['changes']==[],proposal['changes'];assert all(row['before']==row['after'] for row in proposal['files'])
label='final-walk-dry-r500';code=subprocess.run(['python3',str(B/'run-local.py'),label,'env','WALK_DRY=1','bash','scripts/chain-walk.sh'],cwd=R).returncode
(D/'manifest.json').write_text(json.dumps(dict(source_head=head,qualified_preflight=code==0,qualification_claim=False,label=label,exit=code,source_pin_changes=0),indent=2)+'\n')
raise SystemExit(code)
