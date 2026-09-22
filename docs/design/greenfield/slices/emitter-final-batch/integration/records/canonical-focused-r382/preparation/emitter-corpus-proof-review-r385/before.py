"""Fresh canonical replay + unchanged selected48 commands +108 project supplement."""
from pathlib import Path
import argparse,copy,gzip,hashlib,importlib.util,json,os,shutil,subprocess,time,tomllib
p=argparse.ArgumentParser();p.add_argument('--head',required=True);a=p.parse_args()
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=T/'emitter-canonical-corpus-r375';old=T/'emitter-combined-recovery-successor-r330';P=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-escaped-keyword-proof');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census')
h=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
def load(p):return json.loads(Path(p).read_bytes())
def module(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
# Bind the reused comparator/proof to the retained immutable archive before import.
archive=B/'records/empty-variable-proof-r344';index={r['path']:r for r in load(archive/'manifest.json')['files']}
for rel,live in [
 ('emitter-combined-recovery-proof-r330/replay.py',Path('/tmp/emitter-combined-recovery-proof-r330/replay.py')),
 ('emitter-combined-recovery-proof-r330/pins.json',Path('/tmp/emitter-combined-recovery-proof-r330/pins.json')),
 ('emitter-combined-recovery-successor-r330/manifest.json',old/'manifest.json'),
 ('emitter-combined-recovery-successor-r330/successor.json.gz',old/'successor.json'),
 ('emitter-combined-recovery-successor-r330/selection.json.gz',old/'selection.json')]:
 data=(archive/rel).read_bytes();assert h(data)==index[rel]['sha256'];assert (gzip.decompress(data) if rel.endswith('.gz') else data)==live.read_bytes()
m=module('original_r330','/tmp/emitter-combined-recovery-proof-r330/replay.py');replay=module('frozen_replay',P/'scripts/replay-recovery-parse.py');previous=load(old/'successor.json');selection=load(old/'selection.json');snapraw=m.SNAPSHOT.read_bytes();assert h(snapraw)==m.PINS['snapshot_sha256'];snapshot=json.loads(snapraw)
assert len(selection['cases'])==48 and len(selection['load_failures'])==110
# All runtime parser dependencies remain byte-identical to the prior full proof.
# Only the source test's fixture cardinality changes; repeat the full replay so
# the native command validator can bind the actual final syntax tree/catalog.
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=P,text=True).strip()==m.PINS['proof_head']
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=P).strip()
 assert git('rev-parse','HEAD').decode().strip()==a.head
 assert not git('diff','HEAD','--name-only').strip()
 assert not git('ls-files','--others','--exclude-standard','--','crates','scripts').strip()
 ids=replay.source_identity(R);prior=previous['build']['source_files_sha256'];assert ids.keys()==prior.keys()
 changed={p for p in ids if ids[p]!=prior[p]};assert changed=={'crates/syntax/tests/emitter_recovery.rs'},changed
 for p in ids:
  if p not in changed:assert ids[p]==prior[p]
 before=subprocess.check_output(['git','show','4aeb1640156f291f4b5189b8ad9ed370cf17cf9d:crates/syntax/tests/emitter_recovery.rs'],cwd=R)
 after=(R/'crates/syntax/tests/emitter_recovery.rs').read_bytes();assert before.replace(b'("cases", true, 432)',b'("cases", true, 748)')==after
 for p,v in previous['build']['probe_files_sha256'].items():assert h(Path(p).read_bytes())==v,p
 assert git('rev-parse','HEAD:vendor/typescript-6.0.3').decode().strip()==selection['vendor_tree_hash']
 return ids
ids=check();O.mkdir(exist_ok=False);report={'head':a.head,'parser_qualified':False,'commands_qualified':False,'projects_qualified':False,'scope':'Fresh original16994 replay oncanonical; allruntimeparserdependenciesidentical330, onlyconsumer count432->748 changed. Original48selectionand110loadfails retained. Freshcompletecommandx2 selected48 plus108projectsupplement; noEmit2notemitproof.','steps':[]};env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T));prefix=['taskpolicy','-b','nice','-n','15']
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def run(label,cmd,e=None):
 check();start=time.monotonic();log=O/(label+'.log')
 with log.open('xb') as f:code=subprocess.run(prefix+list(map(str,cmd)),cwd=R,env=e or env,stdout=f,stderr=subprocess.STDOUT).returncode
 data=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(data,mtime=0));report['steps'].append({'label':label,'argv':prefix+list(map(str,cmd)),'exit':code,'seconds':time.monotonic()-start,'log_sha256':h(data)});save();check();print(json.dumps(report['steps'][-1]),flush=True);assert code==0,(label,code)
# Reuse the pinned probe code, with dependencies resolved from canonical paths.
build=O/'build';build.mkdir();manifest=(old/'build/Cargo.toml').read_text().replace('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype',str(R));assert str(R/'crates/syntax') in manifest;(build/'Cargo.toml').write_text(manifest);(build/'Cargo.lock').write_bytes((C/'Cargo.lock').read_bytes());penv=env.copy();penv['CARGO_TARGET_DIR']=str(build/'target');save()
for action in ['build','test']:run('parser-'+action,['cargo',action,'--offline','--manifest-path',build/'Cargo.toml','--features','current-recovery-profiles'],penv)
packages=tomllib.loads((build/'Cargo.lock').read_text())['package'];registry=[p for p in packages if p.get('source')];tuples=lambda rows:{(p['name'],p['version'],p.get('source'),p.get('checksum')) for p in rows};assert tuples(registry)==tuples(load(old/'resolved-dependencies.json'));(O/'resolved-dependencies.json').write_text(json.dumps(registry,indent=2)+'\n')
binary=build/'target/debug/leading-binding-parse-replay';start=time.monotonic()
with (O/'raw-replay.json').open('xb') as out,(O/'replay.stderr.log').open('xb') as err:subprocess.run(prefix+[str(binary)],input=snapraw,stdout=out,stderr=err,check=True)
fresh=load(O/'raw-replay.json');assert m.compare_inputs(previous,fresh,snapshot)==[];assert check()==ids
fresh['build']={**previous['build'],'label':'canonical-final-r375','parser_head':a.head,'syntax_tree_hash':git('rev-parse','HEAD:crates/syntax').decode().strip(),'source_files_sha256':ids,'binary_sha256':h(binary.read_bytes()),'manifest_sha256':h((build/'Cargo.toml').read_bytes()),'lock_sha256':h((build/'Cargo.lock').read_bytes()),'wrapper_sha256':h(Path(__file__).read_bytes()),'seconds':time.monotonic()-start,'prior_proof_sha256':h((old/'successor.json').read_bytes())}
(O/'successor.json').write_text(json.dumps(fresh,separators=(',',':'))+'\n');chosen=copy.deepcopy(selection);chosen['successor']={k:fresh['build'][k] for k in ['parser_head','syntax_tree_hash','source_files_sha256','binary_sha256','predicate_diff_sha256']};chosen['evidence']['canonical_final_successor_sha256']=h((O/'successor.json').read_bytes());assert chosen['cases']==selection['cases'] and chosen['load_failures']==selection['load_failures'];(O/'selection.json').write_text(json.dumps(chosen,ensure_ascii=False,separators=(',',':'))+'\n');report['parser_qualified']=True;report['selection_sha256']=h((O/'selection.json').read_bytes());report['source_files_sha256']=ids;save()
# Build/copy one canonical command runner with the same normal dev profile as prior proof.
xenv=env.copy();xenv['CARGO_PROFILE_DEV_DEBUG']='0';run('xtask-build',['cargo','build','--offline','-p','tsc-rs-xtask','--bin','xtask','--message-format=json'],xenv)
found=[]
for line in (O/'xtask-build.log').read_text().splitlines():
 try:event=json.loads(line)
 except json.JSONDecodeError:continue
 if event.get('reason')=='compiler-artifact' and event.get('executable') and 'tsc-rs-xtask' in event['package_id'] and not event.get('profile',{}).get('test'):found.append(event['executable'])
assert len(found)==1;native=O/'xtask';shutil.copy2(found[0],native);os.chmod(native,0o555);report['native_sha256']=h(native.read_bytes());save()
run('selected-ts',['node','scripts/observe-recovery-selected-corpus.mjs',O/'selection.json',O/'oracle.json',C]);run('selected-native',[native,'recovery-corpus-native',O/'selection.json',O/'native.json',C]);run('selected-compare',['python3','scripts/compare-recovery-selected-corpus.py','--selection',O/'selection.json','--native',O/'native.json','--oracle',O/'oracle.json','--out',O/'comparison.json']);report['commands_qualified']=True;save()
roster=B/'records/project-projection-roster-r139.json';run('projects-native',[native,'project-command-supplement',roster,O/'projects-native.json',C]);run('projects-ts',['node','scripts/observe-project-command-supplement.mjs',O/'projects-native.json',roster,O/'projects-oracle.json']);run('projects-compare',['python3','scripts/compare-project-command-supplement.py','--native',O/'projects-native.json','--oracle',O/'projects-oracle.json','--out',O/'projects-comparison.json']);assert h(native.read_bytes())==report['native_sha256'];report['projects_qualified']=True;save()
