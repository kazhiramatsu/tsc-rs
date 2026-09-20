from pathlib import Path
import hashlib,json,os,shutil,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path(__file__).parent;P=D/'scratch-v2';P.mkdir(exist_ok=False)
B=D/'target/debug/emitter-test-layout-proof';frozen='crates/xtask/src/recovery_parse_snapshot.rs';H=lambda b:hashlib.sha256(b).hexdigest()
paths=list(dict.fromkeys(row['path'] for row in json.loads((D/'layout-inputs.json').read_text()) if row['path']!=frozen))
def inspect(root,names):
 env=os.environ.copy();env['PROOF_WORKSPACE_ROOT']=str(root)
 return json.loads(subprocess.check_output([str(B),*[str(root/name) for name in names]],env=env))
before=inspect(R,paths);(D/'before-logical.json').write_text(json.dumps(before)+'\n')
for name in paths+['crates/diagnostics/src/js_string/tests.rs',frozen]:
 target=P/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(R/name,target)
for row in before:
 for ref in row['include_references']:
  name=str(Path(ref['target']).relative_to(R));target=P/name
  if not target.exists():target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(R/name,target)
def offset(text,position):
 line,column=position;return sum(len(v) for v in text.splitlines(keepends=True)[:line-1])+column
moves=[]
for row in before:
 name=str(Path(row['path']).relative_to(R));source=(R/name).read_text();patched=source
 for module in reversed(row['modules']):
  assert module['attrs']==['# [cfg (test)]'] and module['visibility']=='',module
  crate=Path(*Path(name).parts[:2]);sub=Path(name).relative_to(crate/'src').with_suffix('')
  destination=crate/'tests/unit'/sub/(module['name']+'.rs');assert not (R/destination).exists()
  target=P/destination;target.parent.mkdir(parents=True,exist_ok=True)
  refs=[];old_file=None
  if 'open_end' in module:
   lo=offset(source,module['open_end']);hi=offset(source,module['close_start']);body=source[lo:hi]
   # Source coordinates come from syn, never brace matching in comments/strings.
   for ref in reversed(row['include_references']):
    start=offset(source,ref['start']);end=offset(source,ref['end'])
    if not lo<=start<end<=hi:continue
    old=json.dumps(ref['literal']);assert source[start:end]==old
    fixture=Path(ref['target']).relative_to(R);new=os.path.relpath(P/fixture,target.parent)
    assert (target.parent/new).resolve()==(P/fixture).resolve()
    assert H((P/fixture).read_bytes())==H((R/fixture).read_bytes())
    body=body[:start-lo]+json.dumps(new)+body[end-lo:]
    refs.append(dict(before=ref['literal'],after=new,path=str(fixture),sha256=H((R/fixture).read_bytes())))
  else:
   assert name=='crates/diagnostics/src/js_string.rs' and module['name']=='tests'
   old_file='crates/diagnostics/src/js_string/tests.rs';body=(R/old_file).read_text();(P/old_file).unlink()
  target.write_text(body)
  start=offset(source,module['start']);end=offset(source,module['end'])
  relative=os.path.relpath(target,(P/name).parent)
  patched=patched[:start]+f'#[cfg(test)]\n#[path = "{relative}"]\nmod {module["name"]};'+patched[end:]
  moves.append(dict(source=name,target=str(destination),module=module['name'],old_file=old_file,include_adjustments=refs,
   source_before_sha256=H((R/name).read_bytes()),body_expected_tokens=module.get('body_tokens')))
 (P/name).write_text(patched)
assert len(paths)==14 and len(moves)==15
source_after=inspect(P,paths);by_before={str(Path(r['path']).relative_to(R)):r for r in before}
for row in source_after:
 name=str(Path(row['path']).relative_to(P));assert row['production_tokens']==by_before[name]['production_tokens'],name
 assert [m['name'] for m in row['modules']]==[m['name'] for m in by_before[name]['modules']],name
 assert all('open_start' not in m for m in row['modules']),name
for move in moves:
 observed=inspect(P,[move['target']])[0]
 expected=move.pop('body_expected_tokens')
 if expected is None:expected=inspect(R,[move['old_file']])[0]['complete_tokens']
 assert observed['complete_tokens']==expected,move['target']
 move['test_tokens_sha256']=H(expected.encode());move['target_sha256']=H((P/move['target']).read_bytes());move['source_after_sha256']=H((P/move['source']).read_bytes())
for move in moves:move['target_before_fmt_sha256']=move['target_sha256']
with (D/'scratch-v2-fmt.log').open('wb') as log:
 result=subprocess.run(['rustfmt','--edition','2021',*[str(P/m['target']) for m in moves]],stdout=log,stderr=subprocess.STDOUT)
assert result.returncode==0
for move in moves:move['target_sha256']=H((P/move['target']).read_bytes())
assert H((R/frozen).read_bytes())=='3ed02323517f463624014157ad811ff7ed2103b3bc18e4b23547ec270fbc1254'
(D/'after-logical.json').write_text(json.dumps(source_after)+'\n')
result=dict(scope='Scratch-only extraction:14 production token streams identical;15 module bodies token-identical before standard rustfmt, after resolving9 fixture include paths; final formatter output hashes bound; no canonical changes. Native observer self-include still denotes the same source path, whose test-only bytes change and must rebuild for its existing self-staleness guard.',root=str(R),head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),moves=moves,frozen_source_sha256=H((R/frozen).read_bytes()),proof_source_sha256=H((D/'src/main.rs').read_bytes()),proof_binary_sha256=H(B.read_bytes()))
(D/'move-proof.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(files=len(paths),modules=len(moves),fixture_adjustments=sum(len(m['include_adjustments']) for m in moves),production_tokens_equal=True,test_tokens_equal_before_fmt=True)))
