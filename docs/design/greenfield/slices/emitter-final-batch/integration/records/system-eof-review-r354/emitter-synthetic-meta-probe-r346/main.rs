use tsc_emitter::*;
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
