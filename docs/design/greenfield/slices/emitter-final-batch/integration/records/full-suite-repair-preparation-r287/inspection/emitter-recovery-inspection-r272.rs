use tsc_syntax::{parse_source_file, ParseOptions, for_each_child};
use tsc_types::ScriptTarget;
fn main() {
 let text = std::fs::read_to_string(std::env::args().nth(1).expect("source path")).unwrap();
 for target in [ScriptTarget::ES5, ScriptTarget::ES2015] {
  let s = parse_source_file("main.ts", text.clone(), ParseOptions { script_target: target, ..ParseOptions::default() }, None);
  println!("TARGET {target:?}\nRECOVERY {:?}\nSUPPORTED {}", s.parse_recovery(), s.has_supported_emit_recovery());
  for d in &s.parse_diagnostics { println!("DIAGNOSTIC {} {:?} {:?} {}", d.code(), d.start, d.length, d.message.text.to_string_lossy()); }
  let mut pending=vec![s.root]; let mut seen=std::collections::BTreeSet::new();
  while let Some(id)=pending.pop(){if !seen.insert(id){continue;} let n=s.arena.node(id);
   println!("NODE {} {:?} {}..{} {:?}", id.0,n.kind,n.pos,n.end,n.data);
   for_each_child(&s.arena,n,|child|{pending.push(child);false});
  }
 }
}
