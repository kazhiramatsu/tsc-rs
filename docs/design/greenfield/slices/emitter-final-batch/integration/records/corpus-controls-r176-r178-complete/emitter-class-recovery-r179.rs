use tsc_syntax::{parse_source_file,ParseOptions,for_each_child};
fn main(){
 for text in ["class C { m(x: number) => x; }", "class C { constructor(x: number) => 1; }", "class C { get g(): number => 1; }", "function f(x: number) => x;", "var o = { m(x: number) => x };"] {
 let s=parse_source_file("main.ts",text.to_owned(),ParseOptions::default(),None);
 println!("SOURCE {text}\nRECOVERY {:?}\nSUPPORTED {}",s.parse_recovery(),s.has_supported_emit_recovery());
 let mut pending=vec![s.root];let mut seen=std::collections::BTreeSet::new();
 while let Some(id)=pending.pop(){if !seen.insert(id){continue;}let n=s.arena.node(id);println!("NODE {} {:?} {}..{} {:?}",id.0,n.kind,n.pos,n.end,n.data);for_each_child(&s.arena,n,|child|{pending.push(child);false});}
 }
}
