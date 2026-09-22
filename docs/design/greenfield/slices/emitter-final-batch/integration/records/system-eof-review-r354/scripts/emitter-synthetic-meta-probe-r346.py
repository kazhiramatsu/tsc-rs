from pathlib import Path
import json,subprocess,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/deps');O=Path('/tmp/emitter-synthetic-meta-probe-r346');O.mkdir(exist_ok=False)
s='''use tsc_emitter::*;
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};
fn main() {
 let parsed=parse_source_file("/a.ts", "", Default::default(), None);
 let mut arena=TransformArena::new();let source=arena.add_source(&parsed,None);
 let name=arena.factory().create_identifier(source,"meta").unwrap();
 let meta=arena.factory().create_node(source,NodeData::MetaProperty(tsc_syntax::nodes::MetaPropertyData{keyword_token:SyntaxKind::ImportKeyword,name:Some(name.node())}),TransformFlags::NONE).unwrap();
 let statement=arena.factory().create_node(source,NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData{expression:Some(meta.node())}),TransformFlags::NONE).unwrap();
 let statements=arena.factory().create_node_array(source,vec![statement]).unwrap();
 let root=arena.root(source).unwrap();let NodeData::SourceFile(mut data)=arena.node(root).unwrap().data.clone() else {panic!()};data.statements=Some(statements.array());
 let root=arena.factory().update_node(root,NodeData::SourceFile(data),TransformFlags::NONE).unwrap();arena.replace_root(source,root).unwrap();
 let mut result=transform_nodes(arena,vec![TransformRoot::SourceFile(source)],Vec::new(),false).unwrap();
 let recording=Some(SourceMapRecordingInputs{file:"a.js".into(),source_root:"".into(),sources_directory_path:"/".into(),current_directory:"/".into(),use_case_sensitive_source_keys:true,inline_sources:false});
 let printed=create_printer(PrinterOptions::new(NewLineKind::LineFeed)).print(&mut result,PrintRequest::SourceFile(source),recording).unwrap();
 println!("{}",printed.text()); println!("{}",printed.source_map().unwrap().clone().to_json_string());
}
''';(O/'main.rs').write_text(s)
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D),'-o',str(O/'probe')];libs={}
for name in ['tsc_emitter','tsc_syntax']:
 p=max(D.glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime_ns);libs[name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()};args+=['--extern',name+'='+str(p)]
report={'scope':'Internal synthetic-node printer panic probe only; not a source-language admission or full command. No tree mutations.','libraries':libs,'build_argv':args}
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(args,cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
if report['build_exit']==0:
 with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run([str(O/'probe')],cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
