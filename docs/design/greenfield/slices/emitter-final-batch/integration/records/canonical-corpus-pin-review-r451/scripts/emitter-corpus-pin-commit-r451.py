from pathlib import Path
import gzip, hashlib, json, subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration'
S=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-canonical-corpus-r375')
O=B/'records/canonical-corpus-pin-review-r451'
P=Path('/tmp/emitter-corpus-pin-review-r450')
h=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
m=json.loads((S/'manifest.json').read_bytes());review=json.loads((P/'manifest.json').read_bytes())
assert git('rev-parse','HEAD').decode().strip()==m['head']==review['head']
assert not git('status','--porcelain').strip()
assert m['parser_qualified'] and not m['commands_qualified'] and not m['projects_qualified']
assert len(m['steps'])==4 and m['steps'][-1]['label']=='selected-ts' and m['steps'][-1]['exit']==1
assert all(x['exit']==0 for x in m['steps'][:-1])
assert b'review input mirror after loader changes' in (S/'selected-ts.log').read_bytes()
pin=R/review['pin_path'];assert h(pin.read_bytes())==review['pin_before_sha256']
proposed=(P/'recovery-command-pins.json').read_bytes();assert h(proposed)==review['pin_after_sha256']
old=json.loads(pin.read_bytes());new=json.loads(proposed)
assert old.keys()==new.keys() and {p for p in old if old[p]!=new[p]}=={x['path'] for x in review['changes']}
for path,digest in new.items():assert h((R/path).read_bytes())==digest,path
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 files.append({'path':name,'bytes':len(data),'sha256':h(data)})
for p in sorted(S.iterdir()):
 if not p.is_file() or p.name=='xtask' or p.suffix=='.log':continue
 data=p.read_bytes();name='failed-run/'+p.name
 if p.suffix=='.json' and p.name not in ['manifest.json','resolved-dependencies.json']:
  name+='.gz';data=gzip.compress(data,mtime=0)
 put(name,data)
for step in m['steps']:
 assert h(gzip.decompress((S/(step['label']+'.log.gz')).read_bytes()))==step['log_sha256']
for p in sorted(P.iterdir()):put('review/'+p.name,p.read_bytes())
for name in ['Cargo.toml','Cargo.lock']:put('failed-run/build/'+name,(S/'build'/name).read_bytes())
for name in ['emitter-canonical-corpus-r375.py','emitter-canonical-corpus-r452.py','archive-emitter-canonical-corpus-r453.py']:
 put('scripts/'+name,(Path('/tmp')/name).read_bytes())
put('scripts/'+Path(__file__).name,Path(__file__).read_bytes())
(O/'manifest.json').write_text(json.dumps({'head':m['head'],'qualified_parser_scope':True,'qualified_commands_scope':False,'qualified_final':False,'scope':'Original 16994 replay passed. Command observer refused before observations because two source pins retained pre-Clippy file hashes. Exact source replacements reviewed and only two mirror hashes updated; next final-head full command replay remains required. No runtime, comparator, input roster, observer schema, or disposition changes.','files':files},indent=2)+'\n')
pin.write_bytes(proposed)
p=B/'README.md'
with p.open('a') as f:f.write('\n[元corpus再検証の途中記録](records/canonical-corpus-pin-review-r451/manifest.json)：\n16,994入力のparser再実行は一致した。実出力比較は、過去のClippy修正に伴う検証用ソースhash\n2件の更新漏れで採取前に停止した。配列コピー4箇所と不要な参照1箇所の差分だけであることを\n元ファイルからの完全一致で照合し、2件のhashを更新した。比較器・入力集合・期待値は維持し、\n確定HEADで全体再実行する。\n')
subprocess.run(['git','diff','--check'],cwd=R,check=True)
assert set(git('diff','HEAD','--name-only').decode().splitlines())=={review['pin_path'],p.relative_to(R).as_posix()}
untracked=git('ls-files','--others','--exclude-standard').decode().splitlines()
assert all(x.startswith(O.relative_to(R).as_posix()+'/') for x in untracked)
paths=[review['pin_path'],p.relative_to(R).as_posix(),*untracked]
subprocess.run(['git','add','--pathspec-from-file=-','--pathspec-file-nul'],cwd=R,input=b''.join(x.encode()+b'\0' for x in paths),check=True)
assert set(git('diff','--cached','--name-only').decode().splitlines())==set(paths)
body=Path('/tmp/emitter-corpus-pin-commit-r451.txt');body.write_text('emitter final: refresh reviewed command mirror source pins\n\nThe selected-corpus observer stopped before output capture because two pins\nretained the pre-Clippy source hashes. Prove that the differences are exactly\nfour ordered slice copies changed to to_vec and one removed needless borrow.\nUpdate only these source identities, preserving runtime, input and comparator\nbytes. Retain the successful original 16994 parser replay and failed observer\nreceipt; complete final-head command replay remains required.\n')
subprocess.run(['git','commit','--quiet','--file',str(body)],cwd=R,check=True)
assert not git('status','--porcelain').strip()
print(json.dumps({'head':git('rev-parse','HEAD').decode().strip(),'archive_files':len(files),'updated_pins':2,'final_qualified':False}),flush=True)
