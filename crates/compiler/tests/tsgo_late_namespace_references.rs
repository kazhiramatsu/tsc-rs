//! References generated after runtime syntax qualify at their producing transform.
use std::path::PathBuf;

use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_emitter::EmitArtifactKind;
use tsc_host::MemoryCompilerHost;
use tsc_program::{load_emitting_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

#[allow(dead_code)]
#[path = "support/witness_libraries.rs"]
mod witness_libraries;

#[test]
fn late_namespace_references_match_pinned_tsgo() {
    // Every expected byte is from pinned tsgo, including tsxReactEmit6's
    // namespace factories and both ordinary and custom fragment factories.
    macro_rules! case {
        ($stem:literal, $target:literal, $module:literal, $factory:expr, $fragment:expr, $metadata:literal) => {
            (
                $stem,
                $target,
                $module,
                $factory,
                $fragment,
                $metadata,
                include_str!(concat!(
                    "../../emitter/tests/fixtures/tsgo-late-namespace-references/",
                    $stem,
                    ".tsx"
                )),
                include_str!(concat!(
                    "../../emitter/tests/fixtures/tsgo-late-namespace-references/",
                    $stem,
                    ".js"
                )),
            )
        };
    }
    let cases: [(_, _, _, Option<&str>, Option<&str>, _, _, _); 11] = [
        case!("tsxReactEmit6", 2, 1, None, None, false),
        case!("fragment-namespace", 11, 99, None, None, false),
        case!("custom-factories", 11, 99, Some("h"), Some("frag"), false),
        case!("shadow-factory", 11, 99, None, None, false),
        case!("renamed-namespace", 11, 99, None, None, false),
        case!("later-helpers", 11, 99, None, None, false),
        case!("standard-context", 11, 99, None, None, false),
        case!("metadata-helper", 11, 99, None, None, true),
        case!("nested-namespace", 11, 99, None, None, false),
        case!(
            "qualified-custom-root",
            11,
            99,
            Some("UI.h"),
            Some("UI.Fragment"),
            false
        ),
        case!("export-import-factory", 11, 99, None, None, false),
    ];
    for (name, target, module, factory, fragment, metadata, source, expected) in cases {
        let mut builder = MemoryCompilerHost::builder("/project")
            .case_sensitive(true)
            .file("/project/input.tsx", source.as_bytes())
            .file(
                "/project/jsx.d.ts",
                include_bytes!(
                    "../../emitter/tests/fixtures/tsgo-late-namespace-references/jsx.d.ts"
                ),
            );
        for (path, bytes) in witness_libraries::files() {
            builder = builder.file(path.as_str(), bytes.as_slice());
        }
        let host = builder.build().expect("memory host");
        let options = CompilerOptions {
            target: Some(target),
            module: Some(module),
            jsx: Some(2),
            jsx_factory: factory.map(Into::into),
            jsx_fragment_factory: fragment.map(Into::into),
            experimental_decorators: metadata,
            emit_decorator_metadata: Some(metadata),
            strict: Some(true),
            always_strict: Some(true),
            out_dir: Some("/project/out".into()),
            ..CompilerOptions::default()
        };
        let prepared = load_emitting_program(
            &host,
            &[
                PathBuf::from("/project/input.tsx"),
                PathBuf::from("/project/jsx.d.ts"),
            ],
            options,
            ProgramOptions::default(),
            &LibraryCatalog::typescript_7_1("/lib"),
            ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
        )
        .expect("emitting program");
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared).emit(&mut sink).expect("emit");
        let artifact = sink
            .writes()
            .iter()
            .find(|artifact| artifact.kind() == EmitArtifactKind::JavaScript)
            .expect("javascript");
        assert_eq!(
            artifact.materialized_bytes().as_ref(),
            expected.as_bytes(),
            "{name}"
        );
    }
}
