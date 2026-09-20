from pathlib import Path
import json,subprocess,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/deps');O=Path('/tmp/emitter-function-body-flags-probe-r358');O.mkdir(exist_ok=False)
source='function f() {\n// head\n\nx();\n// tail\n}\n'
js='''import ts from "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js";
const source=SOURCE;
for(const [name,flag] of [["none",0],["no-leading",ts.EmitFlags.NoLeadingComments],["no-trailing",ts.EmitFlags.NoTrailingComments],["no-nested",ts.EmitFlags.NoNestedComments],["no-own",ts.EmitFlags.NoComments]]) {
 const file=ts.createSourceFile("/a.ts",source,ts.ScriptTarget.Latest,true);ts.setEmitFlags(file.statements[0].body,flag);
 console.log(JSON.stringify({name,text:ts.createPrinter({newLine:ts.NewLineKind.LineFeed}).printFile(file)}));
}
'''.replace('SOURCE',json.dumps(source));(O/'probe.mjs').write_text(js)
rust='''use tsc_emitter::*;
use tsc_syntax::{parse_source_file,NodeData};
fn main(){
 for (name,flag) in [("none",EmitFlags::NONE),("no-leading",EmitFlags::NO_LEADING_COMMENTS),("no-trailing",EmitFlags::NO_TRAILING_COMMENTS),("no-nested",EmitFlags::NO_NESTED_COMMENTS),("no-own",EmitFlags::NO_COMMENTS)] {
 let parsed=parse_source_file("/a.ts",SOURCE,Default::default(),None);
 let mut arena=TransformArena::new();let source=arena.add_source(&parsed,None);let root=arena.root(source).unwrap();
 let NodeData::SourceFile(data)=&arena.node(root).unwrap().data else{panic!()};let array=arena.node_array_ref(source,data.statements.unwrap()).unwrap();let id=arena.node_array(array).unwrap().nodes[0];let function=arena.node_ref(source,id).unwrap();
 let NodeData::FunctionDeclaration(data)=&arena.node(function).unwrap().data else{panic!()};let body=arena.node_ref(source,data.body.unwrap()).unwrap();arena.metadata_mut(body).add_flags(flag);
 let mut result=transform_nodes(arena,vec![TransformRoot::SourceFile(source)],Vec::new(),false).unwrap();
 let out=create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_source_file_text_mode(SourceFileTextMode::Canonical)).print(&mut result,PrintRequest::SourceFile(source),Some(SourceMapRecordingInputs{file:"a.js".into(),source_root:"".into(),sources_directory_path:"/".into(),current_directory:"/".into(),use_case_sensitive_source_keys:true,inline_sources:false})).unwrap();
 println!("{}",serde_json::json!({"name":name,"text":out.text()}));
 }
}
'''.replace('SOURCE',json.dumps(source));(O/'main.rs').write_text(rust)
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D),'-o',str(O/'probe')];libs={}
for name in ['tsc_emitter','tsc_syntax','serde_json']:
 p=max(D.glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime_ns);libs[name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()};args+=['--extern',name+'='+str(p)]
report={'scope':'Internal standalone printer body flags probe using compiler-pinned Canonical text mode and map recording; JS text only compared to TS printFile, no complete map/command/diagnostic/admission qualification. Supersedes356 raw-source fastpath and357 typed map refusal on identity arm; neither was a valid flag comparison. Read-only frozen342 candidate library. TypeScript local canonical6.0.3 printFile baseline comparison.','libraries':libs}
with (O/'ts.log').open('w') as f:report['ts_exit']=subprocess.run(['node',str(O/'probe.mjs')],cwd=C,stdout=f,stderr=subprocess.STDOUT).returncode
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(args,cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
if report['build_exit']==0:
 with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run([str(O/'probe')],cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
 if report['native_exit']==0 and report['ts_exit']==0:
  ts=[json.loads(x) for x in (O/'ts.log').read_text().splitlines()];native=[json.loads(x) for x in (O/'native.log').read_text().splitlines()];report['rows']=[{'name':a['name'],'exact_text':a==b,'typescript':a['text'],'native':b['text']} for a,b in zip(ts,native,strict=True)]
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report.get('rows',report),ensure_ascii=False,indent=2))
