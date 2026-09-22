from pathlib import Path
import json,shutil,subprocess,time
ROOT=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');BASE=ROOT/'docs/design/greenfield/slices/emitter-final-batch/integration';TARGET=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');OUT=TARGET/'emitter-postprewalk-r304';OUT.mkdir(exist_ok=False);HEAD='628562c31a5102348a7e8769f3a2aa88deef70f1';report={'head':HEAD,'status':'waiting for all7r244steps','steps':[],'qualified':False}
def save():(OUT/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==HEAD
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=ROOT).strip()
 assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=ROOT).strip()
def run(label,args):
 check();start=time.monotonic();code=subprocess.run(args,cwd=ROOT).returncode;check();report['steps'].append({'label':label,'exit':code,'seconds':round(time.monotonic()-start,3)});save();return code
check();save();print('Waiting for r244 completion before ignored control and frozen-source archives.',flush=True)
receipt=BASE/'records/local/final-prewalk-validation-pipeline-r244.json'
while not receipt.exists():time.sleep(10)
m=json.loads((TARGET/'emitter-prewalk-r244/manifest.json').read_text());outer=json.loads(receipt.read_text());assert len(m['steps'])==7 and m['head']==HEAD==outer['head'];check();report['status']='archiving completed frozen-source validation';save()
label='ignored-reused-type-reference-r265'
for suffix in ['.json','.log','.log.gz']:assert not (BASE/'records/local'/(label+suffix)).exists()
code=run(label,['python3',str(BASE/'run-local.py'),label,'cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','h2_7b_w3a_controls::w4_a0_reused_type_references_apply_factory_parenthesization','--','--exact','--ignored','--nocapture','--test-threads=1'])
report['ignored_control_passed']=code==0;save()
assert run('archive244',['python3','/tmp/archive-emitter-prewalk-r244.py'])==0
source=Path('/tmp/emitter-delivery-preparation-r251');destination=BASE/'records/delivery-preparation-r251';assert not destination.exists();shutil.copytree(source,destination);report['steps'].append({'label':'copy251','exit':0});save()
assert run('archive258',['python3','/tmp/archive-emitter-delivery-preparation-r258.py'])==0
report['status']='completed; archive287 and repairs remain pending';save();print(json.dumps(report),flush=True)
