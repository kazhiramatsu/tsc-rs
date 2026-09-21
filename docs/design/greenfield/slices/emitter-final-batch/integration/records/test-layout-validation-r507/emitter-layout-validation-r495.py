from pathlib import Path
import hashlib,json,os,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';D=Path('/tmp/emitter-layout-validation-r495');D.mkdir(exist_ok=False)
h=lambda b:hashlib.sha256(b).hexdigest()
receipt=B/'records/local/488-layout-affected-libraries.json'
while not receipt.exists():time.sleep(2)
old=json.loads(receipt.read_bytes());assert old['exit']==0,old
assert old['diff_sha256']==h(subprocess.check_output(['git','diff','HEAD'],cwd=R)),'Candidate changed during initial affected libraries'
report=dict(qualification_claim=False,scope='Focused test-layout, frozen identity and gate boundary validation; not final CI or delivery.',steps=[],source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip())
def save():(D/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def pre(label,cmd):
 with (D/(label+'.log')).open('xb') as f:code=subprocess.run(cmd,cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
 report['steps'].append(dict(label=label,argv=cmd,exit=code));save();print(label,code,flush=True);assert code==0,label
pre('apply-followup',['python3','/tmp/emitter-apply-layout-followup-r493.py'])
pre('format',['cargo','fmt','--all'])
def inputs():
 names=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z','--','crates','scripts','.github/ci'],cwd=R).decode().strip('\0').split('\0')
 return {n:h((R/n).read_bytes()) for n in sorted(set(names)) if (R/n).is_file()}
report['source_inputs']=inputs();save()
steps=[
 ('495-layout-fmt',['cargo','fmt','--all','--','--check']),
 ('495-layout-python-controls',['python3','scripts/inline-tests-scan.py','--self-test']),
 ('495-layout-python-scan',['python3','scripts/inline-tests-scan.py']),
 ('495-layout-receipt-controls',['python3','/tmp/emitter-test-preflight-fingerprint-r494.py']),
 ('495-layout-command-observer',['node','--test','scripts/recovery-command-input.test.mjs']),
 ('495-layout-qualification',['node','--test','.github/ci/qualification.test.mjs']),
 ('495-layout-workspace-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','workspace_maintenance::','--','--test-threads=1']),
 ('495-layout-de-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','h2_7de_acceptance::','--','--test-threads=1']),
 ('495-layout-native-units',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','recovery_corpus_native::','--','--test-threads=1']),
 ('495-layout-workspace-audit',['cargo','xtask','workspace','audit']),
]
for label,cmd in steps:
 assert inputs()==report['source_inputs'],'Source changed before '+label
 code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode
 r=json.loads((B/'records/local'/f'{label}.json').read_bytes());assert r['exit']==code
 report['steps'].append(dict(label=label,argv=cmd,exit=code,receipt_sha256=h((B/'records/local'/f'{label}.json').read_bytes())));save()
 assert inputs()==report['source_inputs'],'Source changed after '+label
 assert code==0,label
report['focused_qualified']=True;save();print('LAYOUT FOCUSED VALIDATION GREEN',flush=True)
