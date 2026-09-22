use tsc_compiler::ProgramSession;
use tsc_program::{CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramPath};
const LIB:&str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;
fn path(name:&str)->ProgramPath {ProgramPath::from_trusted_parts(name,name).unwrap()}
fn main(){for module in [100,101,102,199]{
 let options=CompilerOptions{no_emit:Some(true),module:Some(module),module_resolution:Some(1),ignore_deprecations:Some("6.0".into()),..Default::default()};
 let context=PathContext::new(ProgramPath::from_trusted_parts("/Display/Project","/canonical/project").unwrap(),true);
 let mut builder=PreparedProgram::builder(context,options);
 let lib=builder.add_source_file(PreparedSourceFile::new(path("lib.d.ts"),LIB)).unwrap();builder.add_library_file(lib).unwrap();
 let main=builder.add_source_file(PreparedSourceFile::new(path("main.ts"),"export {};\n")).unwrap();builder.add_root_file(main).unwrap();
 let outcome=ProgramSession::new(builder.build().unwrap()).run().unwrap();
 println!("MODULE {module} OPTION_DIAGNOSTICS {}",outcome.options_diagnostics().len());
 for d in outcome.options_diagnostics(){println!("{} {:?} file={:?} start={:?} length={:?}",d.code(),d.message_text(),d.file_name,d.start,d.length);}
}}
