//! H2.7a ca controls: the declaration foundation remains dormant and typed.

use std::collections::BTreeSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use tsc_checker::CompilerOptions;
use tsc_emitter::{EmitFailure, EmitHost, H2ActivityCanary, H2RuntimeSlice, SourceFileId};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

struct ControlHost {
    options: CompilerOptions,
    sources: [SourceFileId; 1],
}

impl EmitHost for ControlHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/control")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/control")
    }

    fn config_file_path(&self) -> Option<&Path> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.sources
    }

    fn source_file(&self, id: SourceFileId) -> Option<tsc_emitter::EmitSource<'_>> {
        (id == self.sources[0]).then(|| {
            let path = Path::new("/control/input.ts");
            tsc_emitter::EmitSource::new(id, path, path, true, None, None)
        })
    }
}

fn control_host(options: CompilerOptions) -> ControlHost {
    ControlHost {
        options,
        sources: [SourceFileId::from_raw(0)],
    }
}

#[test]
fn h2_7a_activation_panics_on_the_production_profile() {
    let constructors: [fn() -> H2ActivityCanary; 27] = [
        H2ActivityCanary::h1_profile,
        H2ActivityCanary::h2_1a_profile,
        H2ActivityCanary::h2_1b_profile,
        H2ActivityCanary::h2_1c_profile,
        H2ActivityCanary::h2_1d_profile,
        H2ActivityCanary::h2_1e_profile,
        H2ActivityCanary::h2_2a_profile,
        H2ActivityCanary::h2_2b_profile,
        H2ActivityCanary::h2_2c_profile,
        H2ActivityCanary::h2_2d_profile,
        H2ActivityCanary::h2_3a_profile,
        H2ActivityCanary::h2_3b_profile,
        H2ActivityCanary::h2_3c_profile,
        H2ActivityCanary::h2_3d_profile,
        H2ActivityCanary::h2_4a_profile,
        H2ActivityCanary::h2_4b_profile,
        H2ActivityCanary::h2_5a_profile,
        H2ActivityCanary::h2_5b_profile,
        H2ActivityCanary::h2_5c_profile,
        H2ActivityCanary::h2_5d_profile,
        H2ActivityCanary::h2_5e_profile,
        H2ActivityCanary::h2_5f_profile,
        H2ActivityCanary::h2_5g_profile,
        H2ActivityCanary::h2_5h_profile,
        H2ActivityCanary::h2_6a_profile,
        H2ActivityCanary::h2_6b_profile,
        H2ActivityCanary::h2_6c_profile,
    ];

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for constructor in constructors {
        assert_eq!(
            constructor()
                .counters()
                .runtime_slice(H2RuntimeSlice::H2_7a),
            0
        );
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut canary = constructor();
            canary.observe_runtime_slice(H2RuntimeSlice::H2_7a);
        }));
        let payload = result.expect_err("H2.7a activation unexpectedly succeeded");
        let contains_message = payload
            .downcast_ref::<String>()
            .is_some_and(|message| message.contains("unadmitted H2 runtime activity: H2.7a"))
            || payload
                .downcast_ref::<&str>()
                .is_some_and(|message| message.contains("unadmitted H2 runtime activity: H2.7a"));
        assert!(contains_message, "H2.7a refusal panic payload changed");
    }
    std::panic::set_hook(original_hook);
}

#[test]
fn no_h2_7a_admission_exists_in_production() {
    let activity = include_str!("../../../emitter/src/activity.rs");
    assert!(!activity.contains("fn h2_7a_profile"));
    assert!(!activity.contains("H2RuntimeSlice::H2_7a.index()"));

    let workspace = workspace();
    let mut files = Vec::new();
    collect_rs_files(&workspace.join("crates"), &mut files);
    let symbol = "observe_runtime_slice(H2RuntimeSlice::H2_7a";
    let mut actual = BTreeSet::new();
    for path in files {
        let relative = path
            .strip_prefix(&workspace)
            .expect("inside workspace")
            .to_string_lossy()
            .replace('\\', "/");
        if relative.contains("/tests/") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("readable Rust source");
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            if line.contains(symbol) {
                actual.insert(relative.clone());
            }
        }
    }
    assert_eq!(actual, BTreeSet::new());
}

fn assert_unsupported_option(options: CompilerOptions, expected: &'static str) {
    let host = control_host(options);
    assert!(matches!(
        tsc_emitter::validate_bootstrap_emit_request(&host),
        Err(EmitFailure::UnsupportedCompilerOption { option }) if option == expected
    ));
}

#[test]
fn declaration_family_options_remain_typed_refusals() {
    assert_unsupported_option(
        CompilerOptions {
            declaration_map: Some(true),
            ..CompilerOptions::default()
        },
        "declarationMap",
    );
    assert_unsupported_option(
        CompilerOptions {
            emit_declaration_only: Some(true),
            ..CompilerOptions::default()
        },
        "emitDeclarationOnly",
    );
    let combined = control_host(CompilerOptions {
        declaration: Some(true),
        emit_declaration_only: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&combined),
        Ok(())
    );
    assert_unsupported_option(
        CompilerOptions {
            strip_internal: Some(true),
            ..CompilerOptions::default()
        },
        "stripInternal",
    );
    assert_unsupported_option(
        CompilerOptions {
            declaration_dir: Some("/control/declarations".to_owned()),
            ..CompilerOptions::default()
        },
        "declarationDir",
    );
    assert_unsupported_option(
        CompilerOptions {
            out_file: Some("/control/bundle.js".to_owned()),
            ..CompilerOptions::default()
        },
        "outFile",
    );

    let host = control_host(CompilerOptions {
        declaration: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(tsc_emitter::validate_bootstrap_emit_request(&host), Ok(()));
}

#[test]
fn h2_7b_has_one_profile_admission_and_five_production_constructors() {
    let workspace = workspace();
    let activity = fs::read_to_string(workspace.join("crates/emitter/src/activity.rs"))
        .expect("read activity source");
    assert_eq!(
        activity.matches("H2RuntimeSlice::H2_7b.index()").count(),
        1,
        "H2.7b must be admitted in exactly one profile function"
    );
    assert!(activity.contains(
        "pub const fn h2_7b_profile() -> Self {\n        let mut profile = Self::h2_6c_profile();"
    ));

    let expected = BTreeSet::from([
        "crates/compiler/src/lib.rs:1".to_owned(),
        "crates/emitter/src/builtins.rs:2".to_owned(),
        "crates/emitter/src/execute.rs:2".to_owned(),
    ]);
    let mut actual = BTreeSet::new();
    for relative in [
        "crates/compiler/src/lib.rs",
        "crates/emitter/src/builtins.rs",
        "crates/emitter/src/execute.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).expect("read production source");
        let count = source
            .lines()
            .filter(|line| line.contains("H2ActivityCanary::h2_7b_profile()"))
            .count();
        actual.insert(format!("{relative}:{count}"));
        assert!(
            !source.contains("H2ActivityCanary::h2_6c_profile()"),
            "legacy production profile call remains in {relative}"
        );
    }
    assert_eq!(actual, expected);
}
