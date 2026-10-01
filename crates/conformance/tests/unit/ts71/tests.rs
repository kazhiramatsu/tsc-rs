use super::*;

const PROFILE: &str = "7.1.0-dev-19dadef8";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A corpus sample on which tsc-rs reproduces the 7.1 error baselines byte for
/// byte, except one elaboration (7.1 relates the tuples through `pop()`); a
/// regression here is a checker change or a runner change.
#[test]
fn the_tuple_sample_reproduces_its_error_baselines() {
    let options = RunOptions {
        profile: PROFILE.to_owned(),
        filter: Some("types/tuple/".to_owned()),
        case: None,
        threads: 2,
        checkers: 1,
        dump: None,
    };
    let results = run(&workspace(), &options).unwrap();
    assert_eq!(results.len(), 34);
    let below_full: Vec<_> = results
        .iter()
        .filter(|result| {
            !matches!(
                result.outcome,
                Outcome::Compared {
                    agreement: Agreement::Full,
                    ..
                }
            )
        })
        .map(|result| (&result.stem, &result.outcome))
        .collect();
    assert!(
        below_full.iter().all(|(stem, outcome)| {
            stem.as_str() == "arityAndOrderCompatibility01"
                && matches!(
                    outcome,
                    Outcome::Compared {
                        agreement: Agreement::Text,
                        ..
                    }
                )
        }),
        "{below_full:?}"
    );
}

/// A restarted worker receives the stems of its case that it must not run
/// again, and announces each lane-A configuration before running it.
#[test]
fn a_case_runs_every_configuration_except_those_left_out() {
    let workspace = workspace();
    let profile = NativeProfile::load(&workspace, PROFILE).unwrap();
    let case = parse_case_key("compiler/functionAssignabilityWithArrayLike01.ts").unwrap();
    let mut started = Vec::new();
    let mut finished = Vec::new();
    run_case(
        &workspace,
        &profile,
        &case,
        &["functionAssignabilityWithArrayLike01(strict=false)"],
        None,
        1,
        &mut |configuration, stem| started.push((configuration.name.clone(), stem.to_owned())),
        &mut |result| finished.push(result),
    );
    assert_eq!(
        started,
        [(
            "strict=true".to_owned(),
            "functionAssignabilityWithArrayLike01(strict=true)".to_owned()
        )]
    );
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].stem, started[0].1);
    assert!(matches!(finished[0].outcome, Outcome::Compared { .. }));
}

#[test]
fn case_keys_name_a_suite_and_a_path() {
    let case = parse_case_key("conformance/types/tuple/castingTuple.ts").unwrap();
    assert_eq!(case.suite, NativeSuite::Conformance);
    assert_eq!(case_key(&case), "conformance/types/tuple/castingTuple.ts");
    assert!(parse_case_key("castingTuple.ts").is_err());
    assert!(parse_case_key("fourslash/castingTuple.ts").is_err());
}

#[test]
fn the_summary_block_parses_located_masked_and_global_diagnostics() {
    let text = "error TS5102: Option 'downlevelIteration' has been removed.\r\n\
a.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.\r\n\
  The types differ.\r\n\
lib.es5.d.ts(--,--): error TS2300: Duplicate identifier 'x'.\r\n\
/foo/b(1).ts(1,1): warning TS6133: 'y' is declared but its value is never read.\r\n\
\r\n\
\r\n\
!!! error TS5102: Option 'downlevelIteration' has been removed.\r\n\
==== a.ts (1 errors) ====";
    let parsed = parse_errors_baseline(text);
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].file, None);
    assert_eq!(parsed[0].code, 5102);
    assert_eq!(parsed[1].file.as_deref(), Some("a.ts"));
    assert_eq!((parsed[1].line, parsed[1].column), (Some(3), Some(7)));
    assert_eq!(
        parsed[1].text,
        "Type 'string' is not assignable to type 'number'."
    );
    assert_eq!(parsed[2].file.as_deref(), Some("lib.es5.d.ts"));
    assert_eq!((parsed[2].line, parsed[2].column), (None, None));
    assert_eq!(parsed[3].file.as_deref(), Some("/foo/b(1).ts"));
    assert_eq!(parsed[3].category, "warning");
}

#[test]
fn agreement_reports_the_deepest_matching_tier() {
    let diagnostic = |category: &str, text: &str| BaselineDiagnostic {
        file: Some("a.ts".to_owned()),
        line: Some(1),
        column: Some(1),
        code: 2322,
        category: category.to_owned(),
        text: text.to_owned(),
    };
    let expected = vec![diagnostic("error", "x")];
    assert_eq!(
        agreement(&expected, &[diagnostic("error", "x")]),
        Agreement::Text
    );
    assert_eq!(
        agreement(&expected, &[diagnostic("error", "y")]),
        Agreement::Category
    );
    assert_eq!(
        agreement(&expected, &[diagnostic("warning", "x")]),
        Agreement::Location
    );
    assert_eq!(agreement(&expected, &[]), Agreement::None);
}

#[test]
fn deprecated_options_route_to_lane_b() {
    let configuration = |pairs: &[(&str, &str)]| NativeConfiguration {
        name: String::new(),
        settings: pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    };
    assert_eq!(
        deprecated_option(&configuration(&[("target", "ES5")])),
        Some("target=es5")
    );
    assert_eq!(
        deprecated_option(&configuration(&[("downleveliteration", "true")])),
        Some("downlevelIteration")
    );
    assert_eq!(
        deprecated_option(&configuration(&[("esmoduleinterop", "false")])),
        Some("esModuleInterop=false")
    );
    assert_eq!(
        deprecated_option(&configuration(&[("target", "es2015"), ("strict", "true")])),
        None
    );
}

#[test]
fn the_js_baseline_lists_sources_then_javascript_then_declarations() {
    use super::emit_baseline::{render_js, Emission, EmittedFile};
    let sources = [
        errors_baseline::InputFile {
            name: "/.src/other.ts",
            content: "export const o = 1;",
        },
        errors_baseline::InputFile {
            name: "/.src/main.ts",
            content: "import { o } from \"./other\";\nexport const m = o;",
        },
    ];
    let emission = Emission {
        js: vec![
            EmittedFile {
                path: "/.src/other.js".to_owned(),
                content: "export const o = 1;\r\n".to_owned(),
            },
            EmittedFile {
                path: "/.src/main.js".to_owned(),
                content: "import { o } from \"./other\";\r\nexport const m = o;\r\n".to_owned(),
            },
        ],
        dts: vec![EmittedFile {
            path: "/.src/main.d.ts".to_owned(),
            content: "export declare const m = 1;\r\n".to_owned(),
        }],
        maps: Vec::new(),
    };
    let rendered = render_js("tests/cases/compiler/main.ts", &sources, &emission, false).unwrap();
    assert_eq!(
        rendered,
        "//// [tests/cases/compiler/main.ts] ////\r\n\r\n\
         //// [other.ts]\r\nexport const o = 1;\r\n\
         //// [main.ts]\r\nimport { o } from \"./other\";\nexport const m = o;\r\n\r\n\
         //// [other.js]\r\nexport const o = 1;\r\n\
         //// [main.js]\r\nimport { o } from \"./other\";\r\nexport const m = o;\r\n\r\n\r\n\
         //// [main.d.ts]\r\nexport declare const m = 1;\r\n"
    );
    // Nothing emitted: no baseline (the runner writes no file).
    assert!(render_js(
        "tests/cases/compiler/main.ts",
        &sources,
        &Emission::default(),
        false
    )
    .is_none());
    // `@fullEmitPaths` keeps the output path without the harness prefix.
    let full = render_js("tests/cases/compiler/main.ts", &sources, &emission, true).unwrap();
    assert!(full.contains("//// [other.js]\r\n"), "{full}");
}

#[test]
fn the_js_map_baseline_follows_the_map_options() {
    use super::emit_baseline::{render_js_map, Emission, EmittedFile, MapOptions};
    let inputs = [errors_baseline::InputFile {
        name: "/.src/a.ts",
        content: "const a = 1;",
    }];
    let map = "{\"version\":3,\"file\":\"a.js\",\"sources\":[\"a.ts\"],\"mappings\":\"AAAA\"}";
    let emission = Emission {
        js: vec![EmittedFile {
            path: "/.src/a.js".to_owned(),
            content: "const a = 1;\r\n".to_owned(),
        }],
        dts: Vec::new(),
        maps: vec![EmittedFile {
            path: "/.src/a.js.map".to_owned(),
            content: map.to_owned(),
        }],
    };
    let on = MapOptions {
        source_map: true,
        ..MapOptions::default()
    };
    let rendered = render_js_map(on, false, &emission, &inputs, false).unwrap();
    assert_eq!(
        rendered,
        format!(
            "//// [a.js.map]\r\n{map}\n//// https://sokra.github.io/source-map-visualization#base64,\
             Y29uc3QgYSA9IDE7DQo=,{},Y29uc3QgYSA9IDE7\n",
            // The map itself, base64-encoded.
            "eyJ2ZXJzaW9uIjozLCJmaWxlIjoiYS5qcyIsInNvdXJjZXMiOlsiYS50cyJdLCJtYXBwaW5ncyI6IkFBQUEifQ=="
        )
    );
    // Inline maps, no map option, a noEmitOnError run with diagnostics and
    // no map at all write no baseline.
    let inline = MapOptions {
        inline_source_map: true,
        ..on
    };
    assert!(render_js_map(inline, false, &emission, &inputs, false).is_none());
    assert!(render_js_map(MapOptions::default(), false, &emission, &inputs, false).is_none());
    let on_error = MapOptions {
        no_emit_on_error: true,
        ..on
    };
    assert!(render_js_map(on_error, true, &emission, &inputs, false).is_none());
    assert!(render_js_map(on, false, &Emission::default(), &inputs, false).is_none());
}
