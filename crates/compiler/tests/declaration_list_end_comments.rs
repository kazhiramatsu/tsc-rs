//! Declaration emit of a semicolon-less source whose union types precede a
//! JSDoc comment. `emitNodeListItems` emits the comments at the last list
//! element's end only when the list's parent ends elsewhere: a union type
//! ends with its last constituent, so the comment after `type A = B | C` is
//! the next statement's and the `;` follows the union (hono's
//! prettier-formatted sources).
use std::path::PathBuf;

use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_emitter::EmitArtifactKind;
use tsc_host::MemoryCompilerHost;
use tsc_program::{load_emitting_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

#[allow(dead_code)]
#[path = "support/witness_libraries.rs"]
mod witness_libraries;

fn javascript_output(source_text: &str, target: i32) -> String {
    let mut builder = MemoryCompilerHost::builder("/project")
        .case_sensitive(true)
        .file(PathBuf::from("/project/a.ts"), source_text.as_bytes());
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let options = CompilerOptions {
        target: Some(target),
        module: Some(6),
        out_dir: Some("/project/out".into()),
        ..CompilerOptions::default()
    };
    let prepared = load_emitting_program(
        &host,
        &[PathBuf::from("/project/a.ts")],
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("emitting program");
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("javascript emit");
    let artifact = sink
        .writes()
        .iter()
        .find(|artifact| artifact.kind() == EmitArtifactKind::JavaScript)
        .expect("one javascript file");
    String::from_utf8(artifact.materialized_bytes().into_owned()).expect("utf-8 javascript")
}

#[test]
fn a_lowered_optional_chain_operand_keeps_its_line_break() {
    // emitBinaryExpression reads getLinesBetweenNodes over the operands with
    // synthesized parentheses skipped: the ES2020 lowering's conditional
    // keeps the chain's range, so `a ||\n  b?.d` breaks after `||` under
    // target ES2018 exactly as tsc 6.0.3 prints it.
    assert_eq!(
        javascript_output(
            "declare const a: boolean;\n\
             declare const b: { c?: () => number; d?: number } | undefined;\n\
             export const x1 = a ||\n\
             \x20 b?.d;\n\
             export const x2 = a ||\n\
             \x20 b?.c?.();\n",
            /* ES2018 */ 5,
        ),
        "var _a;\n\
         export const x1 = a ||\n\
         \x20   (b === null || b === void 0 ? void 0 : b.d);\n\
         export const x2 = a ||\n\
         \x20   ((_a = b === null || b === void 0 ? void 0 : b.c) === null || _a === void 0 ? void 0 : _a.call(b));\n"
    );
}

fn declaration_output(source_text: &str) -> String {
    let mut builder = MemoryCompilerHost::builder("/project")
        .case_sensitive(true)
        .file(PathBuf::from("/project/a.ts"), source_text.as_bytes());
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let options = CompilerOptions {
        declaration: Some(true),
        target: Some(9),
        module: Some(6),
        out_dir: Some("/project/out".into()),
        ..CompilerOptions::default()
    };
    let prepared = load_emitting_program(
        &host,
        &[PathBuf::from("/project/a.ts")],
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("emitting program");
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("declaration emit");
    let artifact = sink
        .writes()
        .iter()
        .find(|artifact| artifact.kind() == EmitArtifactKind::Declaration)
        .expect("one declaration file");
    String::from_utf8(artifact.materialized_bytes().into_owned()).expect("utf-8 declaration")
}

#[test]
fn a_typescript_script_with_a_require_call_keeps_its_global_declarations() {
    // bindWorker binds a `require()` call as a CommonJS module indicator only
    // in a JavaScript file; a TypeScript script stays a global script, so
    // its top-level declarations are visible in the declaration output
    // (playwright-core's bootstrap.ts).
    assert_eq!(
        declaration_output(
            "const minimumMajorNodeVersion = 20\n\
             const currentNodeVersion: string = \"1\"\n\
             if (currentNodeVersion) {\n\
             \x20 const Module = require(\"module\")\n\
             \x20 console.log(Module, minimumMajorNodeVersion)\n\
             }\n",
        ),
        "declare const minimumMajorNodeVersion = 20;\n\
         declare const currentNodeVersion: string;\n"
    );
}

#[test]
fn union_alias_and_member_before_jsdoc_close_with_a_semicolon() {
    // TypeScript 6.0.3 `tsc --declaration` output, byte for byte.
    assert_eq!(
        declaration_output(
            "type A =\n\
             \x20 | string\n\
             \x20 | number\n\
             \n\
             /**\n\
             \x20* Doc for B.\n\
             \x20*/\n\
             export type B = string | A\n\
             \n\
             /**\n\
             \x20* Doc for C.\n\
             \x20*/\n\
             export interface C {\n\
             \x20 a?: string | undefined\n\
             \x20 /**\n\
             \x20  * Handler.\n\
             \x20  */\n\
             \x20 b: number\n\
             }\n\
             export const v = 1\n",
        ),
        "type A = string | number;\n\
         /**\n\
         \x20* Doc for B.\n\
         \x20*/\n\
         export type B = string | A;\n\
         /**\n\
         \x20* Doc for C.\n\
         \x20*/\n\
         export interface C {\n\
         \x20   a?: string | undefined;\n\
         \x20   /**\n\
         \x20    * Handler.\n\
         \x20    */\n\
         \x20   b: number;\n\
         }\n\
         export declare const v = 1;\n\
         export {};\n"
    );
}

#[test]
fn union_return_types_before_jsdoc_close_with_a_semicolon() {
    assert_eq!(
        declaration_output(
            "/**\n\
             \x20* Doc for B.\n\
             \x20*/\n\
             export type B = [string, number]\n\
             \n\
             /**\n\
             \x20* Doc for C.\n\
             \x20*/\n\
             export type C = (a: string) => string | number\n\
             \n\
             /**\n\
             \x20* Doc for D.\n\
             \x20*/\n\
             export declare function d(): string | number\n\
             \n\
             /**\n\
             \x20* Doc for E.\n\
             \x20*/\n\
             export const e = 1\n",
        ),
        "/**\n\
         \x20* Doc for B.\n\
         \x20*/\n\
         export type B = [string, number];\n\
         /**\n\
         \x20* Doc for C.\n\
         \x20*/\n\
         export type C = (a: string) => string | number;\n\
         /**\n\
         \x20* Doc for D.\n\
         \x20*/\n\
         export declare function d(): string | number;\n\
         /**\n\
         \x20* Doc for E.\n\
         \x20*/\n\
         export declare const e = 1;\n"
    );
}
