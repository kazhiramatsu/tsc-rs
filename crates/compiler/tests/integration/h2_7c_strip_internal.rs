//! Complete tsc observations for the first H2.7c option, without a harness floor.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

use super::h2_7b_w4a_controls::assert_observation;

#[test]
fn strip_internal_matches_complete_typescript_observations() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let artifact: Value = serde_json::from_slice(include_bytes!("../fixtures/strip-internal.json"))
        .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 15);
    for case in cases {
        let case_id = case["case_id"].as_str().expect("case id");
        let filename = Path::new(case["input"]["source"].as_str().expect("source"))
            .file_name()
            .expect("filename")
            .to_str()
            .expect("UTF-8");
        let root = PathBuf::from(format!("/project/{filename}"));
        let text = case["input"]["text"].as_str().expect("source text");
        let mut builder =
            MemoryCompilerHost::builder("/project").file(root.clone(), text.as_bytes().to_vec());
        for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib"))
            .expect("library directory")
        {
            let entry = entry.expect("library file");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                builder =
                    builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
            }
        }
        let host = builder.build().expect("memory host");
        let mut options = CompilerOptions::default();
        for (key, value) in case["options"].as_object().expect("compiler options") {
            match key.as_str() {
                "target" => options.target = Some(value.as_i64().unwrap() as i32),
                "module" => options.module = Some(value.as_i64().unwrap() as i32),
                "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                "declaration" => options.declaration = value.as_bool(),
                "stripInternal" => options.strip_internal = value.as_bool(),
                "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
                "removeComments" => options.remove_comments = value.as_bool(),
                "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                other => panic!("unexpected option {other}"),
            }
        }
        // Same emitted-file listing projection as the frozen H2.7b comparator.
        options.list_emitted_files = Some(true);
        for _ in 0..2 {
            let prepared = load_emitting_program(
                &host,
                std::slice::from_ref(&root),
                options.clone(),
                ProgramOptions::default(),
                &LibraryCatalog::typescript_6_0_3("/lib"),
                ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
            )
            .expect("load official case without dropping stripInternal");
            assert_eq!(
                prepared.compiler_options().strip_internal,
                options.strip_internal
            );
            assert_observation(case_id, prepared, &case["typescript_observation"]);
        }
    }
}
