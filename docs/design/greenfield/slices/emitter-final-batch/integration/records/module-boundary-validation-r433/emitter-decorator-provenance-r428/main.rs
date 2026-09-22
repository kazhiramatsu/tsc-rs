use tsc_syntax::{SourceFile,NodeId,for_each_child,ParseOptions};
fn walk(s:&SourceFile,id:NodeId,parent:Option<u16>){let n=s.arena.node(id);println!("{} {} {} {}",n.kind as u16,n.pos,n.end,parent.map_or(-1,|p|p as i32));for_each_child(&s.arena,n,|c|{walk(s,c,Some(n.kind as u16));false});}
fn main(){let s=tsc_syntax::parse_source_file("decoratorOnUsing.ts",include_str!("input.ts"),ParseOptions{script_target:tsc_types::ScriptTarget::ES_NEXT,..ParseOptions::default()},None);walk(&s,s.root,None);}
