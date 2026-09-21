from pathlib import Path
from unittest.mock import patch
import hashlib,importlib.util,json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path(__file__).parent
sha=lambda b:hashlib.sha256(b).hexdigest();git=lambda *a:subprocess.check_output(['git',*a],cwd=r)
spec=importlib.util.spec_from_file_location('test_replay',r/'.github/ci/test_replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
pairs=[]
with patch.object(m.WitnessTests,'assertEqual',side_effect=lambda a,b:pairs.append((a,b))):m.WitnessTests('test_frozen_input_catalog_counts').test_frozen_input_catalog_counts()
assert len(pairs)==1;actual,expected=pairs[0];assert actual==expected
before=json.loads((out/'catalog-before.json').read_bytes());catalog=[]
for row in before['files']:
 p=r/'crates/compiler/tests/fixtures'/(row['name']+'.json');data=p.read_bytes();assert sha(data)==row['sha256'];cases=json.loads(data)['cases'];ids=[x['case_id'] for x in cases];assert len(ids)==len(set(ids))
 catalog.append({**row,'declared_after':788 if row['name']=='emitter-context-recovery' else row['declared'],'unchanged_fixture_bytes':True})
assert len(catalog)==51 and all(x['declared_after']==x['actual'] for x in catalog)
context='crates/compiler/tests/fixtures/emitter-context-recovery.json';new=json.loads((r/context).read_bytes())['cases'];history=[]
for ref,count in [('4dfc0fb4d',432),('70696be10',748),('8859338f1',788)]:
 old=json.loads(git('show',ref+':'+context))['cases'];assert len(old)==count and new[:count]==old;history.append({'ref':ref,'cases':count,'unchanged_prefix':True})
certpath='docs/design/greenfield/slices/emitter-final-batch/integration/records/canonical-walk-r562/certificate.json';cert=json.loads((r/certpath).read_bytes());paths=sorted(str(p.relative_to(r)) for p in (r/'crates').rglob('*.rs'))
tree=sha(''.join(f'{sha((r/p).read_bytes())}  {p}\n' for p in paths).encode());assert tree==cert['converged_crates_sha256'] and len(paths)==cert['rust_file_count'];assert git('show','HEAD:'+certpath)==(r/certpath).read_bytes()
protected=['crates','ratchets','.github/ci/contracts','scripts/chain-walk.sh','README.md','STAGE','ratchet.toml','diag-families.json'];assert not git('diff','HEAD','--',*protected)
untracked=json.loads(Path('/tmp/emitter-final-untracked-before-ci-r569.json').read_bytes())['files']
for row in untracked:
 p=r/row['path'];assert sha(p.read_bytes())==row['sha256'] and p.stat().st_mtime_ns==row['mtime_ns']
result={'qualification_scope':'Static membership and unchanged-input proof, not final CI or hosted success','catalog_51':catalog,'all_suite_counts':actual,'all_suite_counts_match':True,'context_history':history,'unchanged_crates_sha256':tree,'rust_file_count':len(paths),'certificate_sha256':sha((r/certpath).read_bytes()),'protected_surfaces_unchanged':protected,'preserved_untracked_files':len(untracked)}
(out/'final-catalog-proof.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ['catalog_51','all_suite_counts','context_history']},indent=2));print('suite count',len(actual))
