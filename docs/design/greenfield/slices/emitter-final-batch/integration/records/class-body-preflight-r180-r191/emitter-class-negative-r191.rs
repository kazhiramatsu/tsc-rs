use tsc_syntax::{parse_source_file,ParseOptions};
fn main() { for text in ["class C { m(x: number) = x; }", "class C { m(x: number) => => x; }", "class C { x = 1 => 2 }", "class C { m() {} => x; }", "interface I { m(): void => x }"] {
let source=parse_source_file("main.ts",text.to_owned(),ParseOptions::default(),None);
println!("{text}\n{:?}\n",source.parse_recovery()); } }
