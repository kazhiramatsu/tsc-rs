from pathlib import Path
import base64,hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug');O=Path('/tmp/emitter-decorator-provenance-r428');O.mkdir(exist_ok=False)
h=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
c=next(c for c in json.loads((R/'ratchets/h2-1a-qualification.v1.json').read_text())['cases'] if 'decoratorOnUsing' in c['case_id']);assert len(c['input']['files'])==1
source=base64.b64decode(c['input']['files'][0]['utf8_base64']);(O/'input.ts').write_bytes(source)
previous=json.loads(Path('/tmp/emitter-historical-source-promotion-r426/manifest.json').read_text());libs={}
def resolve(name,expected):
 found=[]
 for p in (D/'.fingerprint').glob('*/lib-'+name):
  if int.from_bytes(bytes.fromhex(p.read_text().strip()),'little')==expected:
   lib=D/'deps'/('lib'+name+'-'+p.parent.name.rsplit('-',1)[1]+'.rlib')
   if lib.is_file():found.append((p,lib))
 assert len(found)==1,(name,found)
 p,lib=found[0];libs[name]={'path':str(lib),'sha256':h(lib),'fingerprint':expected};return json.loads(p.with_suffix('.json').read_text())
program=resolve('tsc_program',previous['libraries']['tsc_program']['expected_fingerprint']);syntax=resolve('tsc_syntax',next(x[3] for x in program['deps'] if x[1]=='tsc_syntax'));resolve('tsc_types',next(x[3] for x in syntax['deps'] if x[1]=='tsc_types'))
(O/'main.rs').write_text('''use tsc_syntax::{SourceFile,NodeId,for_each_child,ParseOptions};
fn walk(s:&SourceFile,id:NodeId,parent:Option<u16>){let n=s.arena.node(id);println!("{} {} {} {}",n.kind as u16,n.pos,n.end,parent.map_or(-1,|p|p as i32));for_each_child(&s.arena,n,|c|{walk(s,c,Some(n.kind as u16));false});}
fn main(){let s=tsc_syntax::parse_source_file("decoratorOnUsing.ts",include_str!("input.ts"),ParseOptions{script_target:tsc_types::ScriptTarget::ES_NEXT,..ParseOptions::default()},None);walk(&s,s.root,None);}
''')
(O/'oracle.mjs').write_text('''import fs from 'node:fs';
import ts from '''+json.dumps(str(R/'vendor/typescript-6.0.3/lib/typescript.js'))+''';
const text=fs.readFileSync(new URL('./input.ts',import.meta.url),'utf8');
const s=ts.createSourceFile('decoratorOnUsing.ts',text,ts.ScriptTarget.ESNext,true);const rows=[];
function walk(n,parent=null){rows.push({kind:n.kind,pos:n.pos,end:n.end,parent:parent?.kind??-1,name:ts.SyntaxKind[n.kind],parentName:parent?ts.SyntaxKind[parent.kind]:null});ts.forEachChild(n,c=>{walk(c,n);});}walk(s);fs.writeFileSync(new URL('./oracle.json',import.meta.url),JSON.stringify(rows,null,2)+'\\n');
''')
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D/'deps'),'-o',str(O/'probe')]
for name,v in libs.items():args+=['--extern',name+'='+v['path']]
report={'scope':'Read-only syntax provenance; exact complete preorder kind/range/parent comparison, not emit qualification','head':previous['head'],'input_sha256':h(O/'input.ts'),'libraries':libs,'source_sha256':h(O/'main.rs'),'oracle_sha256':h(O/'oracle.mjs')}
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(['taskpolicy','-b','nice','-n','15']+args,stdout=f,stderr=subprocess.STDOUT).returncode
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');assert report['build_exit']==0
with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run([str(O/'probe')],stdout=f,stderr=subprocess.STDOUT).returncode
report['oracle_exit']=subprocess.run(['node',str(O/'oracle.mjs')]).returncode
assert report['oracle_exit']==report['native_exit']==0
native=[dict(zip(['kind','pos','end','parent'],map(int,l.split()))) for l in (O/'native.log').read_text().splitlines()];oracle=json.loads((O/'oracle.json').read_text());expected=[{k:r[k] for k in ['kind','pos','end','parent']} for r in oracle];report.update(nodes=len(native),full_tree_equal=native==expected,decorators=[r for r in oracle if r['name']=='Decorator'],native_sha256=h(O/'native.log'),observations_sha256=h(O/'oracle.json'));(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
