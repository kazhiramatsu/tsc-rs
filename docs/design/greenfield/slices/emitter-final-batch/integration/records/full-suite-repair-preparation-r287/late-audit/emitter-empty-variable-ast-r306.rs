use tsc_syntax::{parse_source_file,ParseOptions,NodeData,SyntaxKind,for_each_child};
use tsc_types::{ScriptTarget,NodeFlags};
fn main(){
 let text=std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
 for target in [ScriptTarget::ES5,ScriptTarget::ES2015]{
 let s=parse_source_file("/project/input.ts",text.clone(),ParseOptions{script_target:target,..Default::default()},None);
 println!("TARGET {target:?}");let NodeData::SourceFile(d)=&s.arena.node(s.root).data else{panic!()};
 for (index,id) in s.arena.node_array(d.statements.unwrap()).nodes.iter().enumerate(){let n=s.arena.node(*id);println!("STATEMENT {index} {:?} {}..{} flags={} data={:?}",n.kind,n.pos,n.end,n.flags,n.data);if let NodeData::ExpressionStatement(d)=&n.data{let n=s.arena.node(d.expression.unwrap());println!("EXPRESSION {:?} {}..{} data={:?}",n.kind,n.pos,n.end,n.data);}}
 let mut stack=vec![s.root];let mut seen=std::collections::BTreeSet::new();let mut missing=0;
 while let Some(id)=stack.pop(){if !seen.insert(id){continue;}let n=s.arena.node(id);
 if n.kind==SyntaxKind::Identifier && n.pos==n.end{missing+=1;println!("ZERO_WIDTH_IDENTIFIER {:?} {}..{}",id,n.pos,n.end);}
 if let NodeData::VariableDeclarationList(d)=&n.data{let a=s.arena.node_array(d.declarations.unwrap());println!("DECLARATION_LIST {}..{} flags={} CONST={} ARRAY {}..{} LEN={}",n.pos,n.end,n.flags,NodeFlags::from_bits(n.flags).contains(NodeFlags::CONST),a.pos,a.end,a.nodes.len());}
 for_each_child(&s.arena,n,|child|{stack.push(child);false});
 }
 println!("ZERO_WIDTH_IDENTIFIER_COUNT {missing}");
 }
}
