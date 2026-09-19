use tsc_syntax::{parse_source_file, ParseOptions, for_each_child, NodeData, SyntaxKind};
use tsc_types::{ScriptTarget, NodeFlags};
fn main() {
 let args=std::env::args().collect::<Vec<_>>();
 let text=std::fs::read_to_string(&args[1]).unwrap();
 let filename=args.get(2).map(String::as_str).unwrap_or("main.ts");
 for target in [ScriptTarget::ES5,ScriptTarget::ES2015] {
  let s=parse_source_file(filename,text.clone(),ParseOptions{script_target:target,..Default::default()},None);
  println!("TARGET {target:?} DECL {} DIAGNOSTICS {} PROFILES {:?} FINAL {}",s.is_declaration_file,s.parse_diagnostics.len(),[s.has_only_literal_recovery(),s.has_only_literal_or_missing_await_recovery(),s.has_only_missing_node_emit_recovery(),s.has_only_parameter_gap_emit_recovery(),s.has_only_statement_gap_emit_recovery()],s.has_supported_emit_recovery());
  println!("FACTS {:?}",s.parse_recovery());
  let mut stack=vec![s.root];let mut seen=std::collections::BTreeSet::new();
  while let Some(id)=stack.pop(){if !seen.insert(id){continue;}let n=s.arena.node(id);
   if n.kind==SyntaxKind::VariableStatement {println!("VARIABLE {}..{} UTF16_START {:?} AMBIENT {}",n.pos,n.end,s.positions().byte_to_utf16(n.pos),NodeFlags::from_bits(n.flags).contains(NodeFlags::AMBIENT));}
   if let NodeData::VariableDeclarationList(d)=&n.data{let a=s.arena.node_array(d.declarations.unwrap());println!("LIST {}..{} ARRAY {}..{} CHILDREN {:?}",n.pos,n.end,a.pos,a.end,a.nodes);}
   for_each_child(&s.arena,n,|child|{stack.push(child);false});
  }
 }
}
