//! Declaration emit with the import attributes of TypeScript 7.1: an import
//! type naming an ambient pattern module with import attributes writes them,
//! and import and export declarations keep theirs as written. The expected
//! outputs are tsgo's for the same inputs.
use std::path::PathBuf;

use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_emitter::EmitArtifactKind;
use tsc_host::MemoryCompilerHost;
use tsc_program::{load_emitting_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

#[allow(dead_code)]
#[path = "../support/witness_libraries.rs"]
mod witness_libraries;

fn declaration_output(files: &[(&str, &str)], output: &str) -> String {
    let mut builder = MemoryCompilerHost::builder("/project").case_sensitive(true);
    for (name, text) in files {
        builder = builder.file(PathBuf::from(*name), text.as_bytes());
    }
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let options = CompilerOptions {
        strict: Some(true),
        target: Some(99),
        module: Some(200),
        module_resolution: Some(100),
        declaration: Some(true),
        emit_declaration_only: Some(true),
        out_dir: Some("/project/out".into()),
        ..CompilerOptions::default()
    };
    let roots: Vec<PathBuf> = files.iter().map(|(name, _)| PathBuf::from(*name)).collect();
    let prepared = load_emitting_program(
        &host,
        &roots,
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
        .find(|artifact| {
            artifact.kind() == EmitArtifactKind::Declaration
                && artifact.path().to_string_lossy().ends_with(output)
        })
        .expect("the declaration file");
    String::from_utf8(artifact.materialized_bytes().into_owned()).expect("utf-8 declaration")
}

#[test]
fn import_types_and_declarations_write_import_attributes_like_tsgo() {
    let files = [
        (
            "/project/types.d.ts",
            "declare module \"*.style\" with { type: \"css\", \"x-format\": \"module\" } {\n\
             \x20   export interface Style { kind: \"css\"; }\n\
             }\n\
             declare module \"*.style\" {\n\
             \x20   export interface Style { kind: \"plain\"; }\n\
             }\n",
        ),
        (
            "/project/dep.d.ts",
            "import type { Style as CssStyle } from \"a.style\" with { type: \"css\", \"x-format\": \"module\" };\n\
             import type { Style as PlainStyle } from \"a.style\";\n\
             export declare const css: CssStyle;\n\
             export declare const plain: PlainStyle;\n",
        ),
        (
            "/project/index.ts",
            "import { css, plain } from \"./dep\";\n\
             export type { Style } from \"b.style\" with { type: \"css\", \"x-format\": \"module\" };\n\
             import type { Style as S2 } from \"c.style\" with { \"x-format\": \"module\", type: \"css\" };\n\
             export const inferredCss = css;\n\
             export const inferredPlain = plain;\n\
             export const loaded = import(\"d.style\", { with: { type: \"css\", \"x-format\": \"module\" } });\n\
             export declare const s2: S2;\n",
        ),
    ];
    // The attributes are sorted by name; `x-format` is not an identifier.
    assert_eq!(
        declaration_output(&files, "index.d.ts"),
        "export type { Style } from \"b.style\" with { type: \"css\", \"x-format\": \"module\" };\n\
         import type { Style as S2 } from \"c.style\" with { \"x-format\": \"module\", type: \"css\" };\n\
         export declare const inferredCss: S2;\n\
         export declare const inferredPlain: import(\"*.style\").Style;\n\
         export declare const loaded: Promise<{\n\
         \x20   default: typeof import(\"*.style\", { with: { type: \"css\", \"x-format\": \"module\" } });\n\
         }>;\n\
         export declare const s2: S2;\n"
    );
}

#[test]
fn ambient_module_declarations_keep_their_import_attributes_like_tsgo() {
    let files = [(
        "/project/index.ts",
        "declare module \"*.style\" with { type: \"css\", \"x-format\": \"module\" } {\n\
         \x20   export interface Style { kind: \"css\"; }\n\
         }\n\
         declare module \"*.text\" with { type: \"text\" };\n",
    )];
    assert_eq!(
        declaration_output(&files, "index.d.ts"),
        "declare module \"*.style\" with {\n\
         \x20   type: \"css\";\n\
         \x20   \"x-format\": \"module\";\n\
         } {\n\
         \x20   interface Style {\n\
         \x20       kind: \"css\";\n\
         \x20   }\n\
         }\n\
         declare module \"*.text\" with {\n\
         \x20   type: \"text\";\n\
         };\n"
    );
}
