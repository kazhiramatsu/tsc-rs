use std::path::PathBuf;

use tsc_host::FsCompilerHost;
use tsc_program::{
    load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn generous_library_limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

#[test]
fn vendored_typescript_6_0_3_library_closures_match_the_pinned_catalog() {
    let host = FsCompilerHost::from_process().expect("construct filesystem compiler host");
    let catalog = LibraryCatalog::typescript_6_0_3(workspace_path("vendor/typescript-6.0.3/lib"));
    let root = workspace_path(
        "ts-tests/tests/cases/conformance/interfaces/declarationMerging/mergeTwoInterfaces.ts",
    );

    let profiles = [
        (
            CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            82,
            "ES2025 default",
        ),
        (
            CompilerOptions {
                no_emit: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            },
            19,
            "ES2015 default",
        ),
        (
            CompilerOptions {
                no_emit: Some(true),
                lib: Some(vec!["es5".to_owned(), "dom".to_owned()]),
                ..CompilerOptions::default()
            },
            15,
            "explicit es5+dom",
        ),
    ];

    for (options, expected_library_count, profile) in profiles {
        let program = load_program(
            &host,
            std::slice::from_ref(&root),
            options,
            ProgramOptions::default().with_types(Vec::new()),
            &catalog,
            generous_library_limits(),
        )
        .unwrap_or_else(|error| panic!("load {profile} library closure: {error}"));

        assert_eq!(
            program.library_files().len(),
            expected_library_count,
            "{profile}"
        );
        assert_eq!(
            program.source_files().len(),
            expected_library_count + 1,
            "{profile}"
        );
        assert!(program.diagnostics().program().is_empty(), "{profile}");
        assert_eq!(program.roots().len(), 1, "{profile}");
        assert!(program.roots()[0].source().is_some(), "{profile}");
        assert!(program
            .library_files()
            .iter()
            .enumerate()
            .all(|(position, source)| source.index() == position));
    }
}

/// The TypeScript 7.1 catalog: its ES2026 default library closes over the
/// ES2025 closure plus `lib.es2026.d.ts` and the seven ES2026 feature files,
/// and the moved `esnext.*` aliases resolve to the ES2026 files.
#[test]
fn vendored_typescript_7_1_library_closures_match_the_pinned_catalog() {
    let host = FsCompilerHost::from_process().expect("construct filesystem compiler host");
    let catalog = LibraryCatalog::typescript_7_1(workspace_path(
        "vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs",
    ));
    let root = workspace_path(
        "ts-tests/tests/cases/conformance/interfaces/declarationMerging/mergeTwoInterfaces.ts",
    );
    let profiles = [
        (
            CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            90,
            "ES2026 default",
        ),
        (
            CompilerOptions {
                no_emit: Some(true),
                target: Some(13),
                ..CompilerOptions::default()
            },
            90,
            "explicit es2026",
        ),
        (
            CompilerOptions {
                no_emit: Some(true),
                target: Some(12),
                ..CompilerOptions::default()
            },
            82,
            "explicit es2025",
        ),
        (
            CompilerOptions {
                no_emit: Some(true),
                lib: Some(vec!["es5".to_owned(), "esnext.array".to_owned()]),
                ..CompilerOptions::default()
            },
            4,
            "explicit es5+esnext.array (lib.es2026.array.d.ts and its references)",
        ),
    ];
    for (options, expected_library_count, profile) in profiles {
        let program = load_program(
            &host,
            std::slice::from_ref(&root),
            options,
            ProgramOptions::default().with_types(Vec::new()),
            &catalog,
            generous_library_limits(),
        )
        .unwrap_or_else(|error| panic!("load {profile} library closure: {error}"));
        assert_eq!(
            program.library_files().len(),
            expected_library_count,
            "{profile}"
        );
        assert!(program.diagnostics().program().is_empty(), "{profile}");
        assert_eq!(
            program.compiler_options().reference_profile,
            tsc_program::ReferenceProfile::TypeScript71,
            "{profile}"
        );
        let names: Vec<String> = program
            .library_files()
            .iter()
            .map(|source| {
                program.source_files()[source.index()]
                    .path()
                    .display()
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        let expected_name = if profile.starts_with("explicit es5+") {
            "lib.es2026.array.d.ts"
        } else if profile == "explicit es2025" {
            "lib.es2025.full.d.ts"
        } else {
            "lib.es2026.full.d.ts"
        };
        assert!(
            names.iter().any(|name| name.ends_with(expected_name)),
            "{profile}: {names:?}"
        );
    }
}
