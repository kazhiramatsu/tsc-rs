from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=T/'emitter-leading-binding-controls-mint-r320';O.mkdir(exist_ok=False);p=R/'crates/compiler/tests/fixtures/export-name-syntax-maps.json';raw=p.read_bytes();old=json.loads(raw);assert len(old['cases'])==40;(O/'original40.json.gz').write_bytes(gzip.compress(raw,mtime=0));code=subprocess.run(['python3',str(B/'run-local.py'),'leading-binding-controls-oracle-r320','node','scripts/observe-export-name-syntax-maps.mjs','--write'],cwd=R).returncode;assert code==0
new=json.loads(p.read_bytes());byid={c['case_id']:c for c in new['cases']};assert len(new['cases'])==len(byid)==80
for c in old['cases']:assert byid[c['case_id']]==c,c['case_id']
for key,value in old.items():
 if key not in ['cases','observer_sha256']:assert new[key]==value,key
fresh=[c for c in new['cases'] if c['case_id'] not in {x['case_id'] for x in old['cases']}];assert len(fresh)==40 and all(c['options']['declarationMap'] is True for c in fresh)
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'oracle_exit':0,'old40_unchanged':True,'new40_case_ids':[c['case_id'] for c in fresh],'new40_all_have_declarationMap':True,'fixture_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'observer_sha256':new['observer_sha256'],'qualified_native_new40':False,'scope':'ordinary40 oldcommands passed2df beforeextension; fresh40adjacentcommands adddeclarationmaps aswellasJS/maps/declarations. Nativecomparison pending.'};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report),flush=True)
