//! The native transpile runner's cases (`testrunner/transpile_runner.go`):
//! the units, configurations and options of the vendored profile's cases
//! reproduce the names of its reference baselines.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tsc_harness::upstream_suites::native::NativeProfile;

const PROFILE: &str = "7.1.0-dev-19dadef8";

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn transpile_cases_reproduce_the_reference_baseline_names() {
    let profile = NativeProfile::load(&workspace(), PROFILE).expect("native profile");
    let paths = profile.transpile_case_paths().expect("transpile cases");
    assert_eq!(paths.len(), 25);
    let mut predicted = BTreeSet::new();
    for path in &paths {
        let case = profile.transpile_case(path).expect("transpile case");
        for configuration in &case.configurations {
            let options = &configuration.compiler_options;
            // runTest: the JavaScript baseline unless emitDeclarationOnly,
            // the declaration baseline when declaration (every case is a
            // .ts or .tsx file emitting .js without jsx preserve).
            if options.emit_declaration_only != Some(true) {
                predicted.insert(format!("{}.js", configuration.configured_name));
            }
            if options.declaration == Some(true) {
                predicted.insert(format!("{}.d.ts", configuration.configured_name));
            }
        }
    }
    let references = std::fs::read_dir(profile.transpile_baselines_root())
        .expect("transpile baselines")
        .map(|entry| {
            entry
                .expect("baseline entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(predicted, references);
}

#[test]
fn transpile_configurations_vary_by_the_map_options_only() {
    let profile = NativeProfile::load(&workspace(), PROFILE).expect("native profile");
    let case = profile
        .transpile_case("declarationBasicSyntax.ts")
        .expect("transpile case");
    assert_eq!(case.extension, ".ts");
    let units = case
        .units
        .iter()
        .map(|unit| unit.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        units,
        [
            "variables.ts",
            "interface.ts",
            "class.ts",
            "namespace.ts",
            "alias.ts"
        ]
    );
    assert_eq!(
        case.units[4].content, "export type A<T> = { x: T };",
        "the last unit keeps no line break the file does not have"
    );
    let names = case
        .configurations
        .iter()
        .map(|configuration| configuration.configured_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "declarationBasicSyntax(declarationMap=true)",
            "declarationBasicSyntax(declarationMap=false)"
        ]
    );
    let options = &case.configurations[0].compiler_options;
    assert_eq!(options.declaration, Some(true));
    assert_eq!(options.declaration_map, Some(true));
    assert_eq!(options.target, Some(2));
    assert_eq!(
        case.configurations[1].compiler_options.declaration_map,
        Some(false)
    );

    // `@reportDiagnostics` is a harness option; the options start empty.
    let reported = profile
        .transpile_case("declarationSingleFileHasErrorsReported.ts")
        .expect("transpile case");
    assert!(reported.configurations[0].report_diagnostics);
    assert_eq!(
        reported.configurations[0]
            .compiler_options
            .no_error_truncation,
        None
    );
    let tsx = profile
        .transpile_case("syntheticImports.tsx")
        .expect("transpile case");
    assert_eq!(tsx.extension, ".tsx");
    assert_eq!(tsx.configurations[0].compiler_options.jsx, Some(4));
    assert!(!tsx.configurations[0].report_diagnostics);
}
