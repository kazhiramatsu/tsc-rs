//! Complete planning comparisons; production emission has separate full-tuple controls.
use serde_json::{json, Value};
use tsc_syntax::{parse_source_file, SourceFile};
use tsc_types::CompilerOptions;

use super::*;

struct Source {
    path: PathBuf,
    canonical: PathBuf,
    syntax: SourceFile,
    may_be_emitted: bool,
    may_emit_forced_declaration: bool,
}

struct Host {
    options: CompilerOptions,
    cwd: PathBuf,
    common: PathBuf,
    case_sensitive: bool,
    syntax_available: bool,
    module_facts_available: bool,
    sources: Vec<Source>,
    ids: Vec<SourceFileId>,
}

impl Host {
    fn from_case(case: &Value) -> Self {
        let mut options = CompilerOptions::default();
        for (key, value) in case["options"].as_object().unwrap() {
            match key.as_str() {
                "target" => options.target = Some(value.as_i64().unwrap() as i32),
                "module" => options.module = Some(value.as_i64().unwrap() as i32),
                "moduleResolution" => {
                    options.module_resolution = Some(value.as_i64().unwrap() as i32)
                }
                "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                "outFile" => options.out_file = value.as_str().map(str::to_owned),
                "outDir" => options.out_dir = value.as_str().map(str::to_owned),
                "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
                "declaration" => options.declaration = value.as_bool(),
                "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
                "sourceMap" => options.source_map = value.as_bool(),
                "inlineSourceMap" => options.inline_source_map = value.as_bool(),
                "declarationMap" => options.declaration_map = value.as_bool(),
                "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
                "noEmit" => options.no_emit = value.as_bool(),
                "noResolve" => options.no_resolve = value.as_bool(),
                "strict" => options.strict = value.as_bool(),
                "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                "allowJs" => options.allow_js = value.as_bool().unwrap(),
                "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
                "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
                "emitBOM" => options.emit_bom = value.as_bool(),
                "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
                other => panic!("unprojected compiler option {other}"),
            }
        }
        let case_sensitive = case["use_case_sensitive_file_names"].as_bool().unwrap();
        let sources = case["program_sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|source| {
                let path = source["path"].as_str().unwrap();
                let text = case["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|file| file["path"] == path)
                    .map_or_else(
                        || {
                            assert!(path.starts_with("/lib/lib") && path.ends_with(".d.ts"));
                            ""
                        },
                        |file| file["text"].as_str().unwrap(),
                    );
                let syntax = parse_source_file(path, text, Default::default(), None);
                assert_eq!(
                    syntax.external_module_indicator.is_some(),
                    source["is_external_module"].as_bool().unwrap()
                );
                Source {
                    path: PathBuf::from(path),
                    canonical: PathBuf::from(if case_sensitive {
                        path.to_owned()
                    } else {
                        path.to_lowercase()
                    }),
                    syntax,
                    may_be_emitted: source["may_be_emitted"].as_bool().unwrap(),
                    may_emit_forced_declaration: source["may_emit_forced_declaration"]
                        .as_bool()
                        .unwrap(),
                }
            })
            .collect::<Vec<_>>();
        Self {
            options,
            cwd: PathBuf::from(case["current_directory"].as_str().unwrap()),
            common: PathBuf::from(case["common_source_directory"].as_str().unwrap()),
            case_sensitive,
            syntax_available: true,
            module_facts_available: false,
            ids: (0..sources.len())
                .map(|index| SourceFileId::from_raw(index as u32))
                .collect(),
            sources,
        }
    }

    fn selection(&self, case: &Value) -> EmitSelection {
        case["target_source"]
            .as_str()
            .map_or(EmitSelection::WholeProgram, |path| {
                let index = self
                    .sources
                    .iter()
                    .position(|source| source.path == Path::new(path))
                    .unwrap();
                EmitSelection::TargetSourceFile(self.ids[index])
            })
    }
}

impl EmitHost for Host {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }
    fn current_directory(&self) -> &Path {
        &self.cwd
    }
    fn common_source_directory(&self) -> &Path {
        &self.common
    }
    fn config_file_path(&self) -> Option<&Path> {
        None
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }
    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }
    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let source = self.sources.get(id.index())?;
        Some(
            EmitSource::new(
                id,
                &source.path,
                &source.canonical,
                source.may_be_emitted,
                None,
                self.syntax_available.then_some(&source.syntax),
            )
            .with_may_emit_forced_declaration(source.may_emit_forced_declaration)
            .with_is_external_module(
                self.module_facts_available
                    .then_some(source.syntax.external_module_indicator.is_some()),
            ),
        )
    }
}

fn source_names(host: &Host, ids: &[SourceFileId]) -> Value {
    json!(ids
        .iter()
        .map(|id| host.sources[id.index()].path.to_string_lossy().into_owned())
        .collect::<Vec<_>>())
}

fn unit_value(host: &Host, paths: &EmitOutputPaths, root: &EmitRoot) -> Value {
    let (kind, ids) = match root {
        EmitRoot::SourceFile(id) => ("source-file", vec![*id]),
        EmitRoot::Bundle(bundle) => ("bundle", bundle.source_files().to_vec()),
    };
    let path = |path: Option<&Path>| path.map(|path| path.to_string_lossy().into_owned());
    json!({ "root_kind": kind, "source_files": source_names(host, &ids), "paths": {
        "javascript": path(paths.javascript_path()), "javascript_map": path(paths.javascript_map_path()),
        "declaration": path(paths.declaration_path()), "declaration_map": path(paths.declaration_map_path()),
        "build_info": path(paths.build_info_path()) } })
}

fn diagnostic_value(diagnostic: &Diagnostic) -> Value {
    fn flatten(chain: &MessageChain, indent: usize, text: &mut String) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&"  ".repeat(indent));
        text.push_str(&chain.text);
        for child in &chain.next {
            flatten(child, indent + 1, text);
        }
    }
    let mut message = String::new();
    flatten(&diagnostic.message, 0, &mut message);
    assert!(!diagnostic.related_information_present && diagnostic.related.is_empty());
    json!({ "code": diagnostic.code(), "category": format!("{:?}", diagnostic.category()),
        "file": diagnostic.file_name, "start": diagnostic.start, "length": diagnostic.length,
        "message": message, "related_information": null })
}

fn cases() -> Vec<Value> {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../../fixtures/bundle-plan.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["focused_cases"], 52);
    assert_eq!(artifact["corpus_cases"], 3);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 55);
    artifact["cases"].as_array().unwrap().clone()
}

#[test]
fn h2_7d_bundle_plans_match_typescript_selection_paths_and_collisions() {
    for case in cases() {
        let host = Host::from_case(&case);
        let selection = host.selection(&case);
        let expected = &case["planning_observation"];
        for _ in 0..2 {
            let selected = get_source_files_to_emit(&host, selection).unwrap();
            assert_eq!(
                source_names(&host, &selected),
                expected["source_files"],
                "{} selection",
                case["case_id"]
            );
            let mut units = Vec::new();
            for_each_emitted_file(&host, selection, |paths, root| {
                units.push(unit_value(&host, paths, root))
            })
            .unwrap();
            assert_eq!(
                json!(units),
                expected["units"],
                "{} ordinary paths",
                case["case_id"]
            );
            let forced = get_source_files_for_forced_declaration_emit(&host, selection).unwrap();
            assert_eq!(
                source_names(&host, &forced),
                expected["forced_source_files"],
                "{} forced selection",
                case["case_id"]
            );
            let mut forced_units = Vec::new();
            for_each_emitted_file_with_force(&host, selection, true, |paths, root| {
                forced_units.push(unit_value(&host, paths, root))
            })
            .unwrap();
            assert_eq!(
                json!(forced_units),
                expected["forced_units"],
                "{} forced paths",
                case["case_id"]
            );
            let preflight = preflight_emit(&host, selection).unwrap();
            assert_eq!(
                json!(preflight
                    .diagnostics()
                    .iter()
                    .map(diagnostic_value)
                    .collect::<Vec<_>>()),
                expected["preflight_diagnostics"],
                "{} collisions",
                case["case_id"]
            );
            let forced_preflight = preflight_forced_declarations(&host, selection).unwrap();
            assert_eq!(forced_preflight.diagnostics(), preflight.diagnostics());
            for unit in forced_preflight
                .plan()
                .units()
                .iter()
                .chain(preflight.plan().units())
            {
                for path in [
                    unit.paths().javascript_path(),
                    unit.paths().declaration_path(),
                ]
                .into_iter()
                .flatten()
                {
                    let prefix = format!("Cannot write file '{}' because ", path.to_string_lossy());
                    let blocked = expected["preflight_diagnostics"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|diagnostic| {
                            diagnostic["message"].as_str().unwrap().starts_with(&prefix)
                        });
                    assert_eq!(
                        preflight.is_emit_blocked(&host, path),
                        blocked,
                        "{} ordinary blocking for {}",
                        case["case_id"],
                        path.display()
                    );
                    assert_eq!(
                        forced_preflight.is_emit_blocked(&host, path),
                        blocked,
                        "{} forced blocking for {}",
                        case["case_id"],
                        path.display()
                    );
                }
            }
            for (unit, expected) in forced_preflight
                .plan()
                .units()
                .iter()
                .zip(expected["forced_units"].as_array().unwrap())
            {
                let mut expected = expected.clone();
                expected["paths"]["javascript"] = Value::Null;
                expected["paths"]["javascript_map"] = Value::Null;
                assert_eq!(
                    unit_value(&host, unit.paths(), unit.root()),
                    expected,
                    "{} forced declaration plan",
                    case["case_id"]
                );
                assert_eq!(unit.mode(), EmitMode::DeclarationOnly);
            }
            assert_eq!(
                forced_preflight.plan().units().len(),
                expected["forced_units"].as_array().unwrap().len()
            );
        }
    }
}

#[test]
fn h2_7d_bundle_module_filter_requires_authoritative_source_syntax() {
    let mut host = Host::from_case(&cases()[0]);
    let expected = get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap();
    host.syntax_available = false;
    let eligible = host
        .ids
        .iter()
        .copied()
        .filter(|&id| source_file_may_be_emitted_for_host(host.source_file(id).unwrap(), &host))
        .collect::<Vec<_>>();
    assert_eq!(eligible, expected);
    assert_eq!(
        get_source_files_to_emit(&host, EmitSelection::WholeProgram),
        Err(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(expected[0])
        ))
    );
    host.options.module = Some(2);
    assert_eq!(
        get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap(),
        expected
    );
    host.options.module = Some(0);
    host.options.emit_declaration_only = Some(true);
    assert_eq!(
        get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap(),
        expected
    );
    host.options.emit_declaration_only = None;
    host.options.out_file = None;
    assert_eq!(
        get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap(),
        expected
    );
}

#[test]
fn h2_7d_common_directory_eligibility_includes_excluded_modules_without_syntax() {
    let cases = cases();
    let case = cases
        .iter()
        .find(|case| case["case_id"] == "adjacent/module-selection#none")
        .unwrap();
    let mut host = Host::from_case(case);
    let bundled = get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(bundled.len(), 2);
    host.syntax_available = false;
    let eligible = host
        .ids
        .iter()
        .copied()
        .filter(|&id| source_file_may_be_emitted_for_host(host.source_file(id).unwrap(), &host))
        .collect::<Vec<_>>();
    assert_eq!(eligible.len(), 3);
    assert!(eligible
        .iter()
        .any(|&id| host.source_file(id).unwrap().path() == Path::new("/project/mod.ts")));
    assert!(bundled.iter().all(|id| eligible.contains(id)));
}

#[test]
fn h2_7d_retained_module_facts_plan_without_borrowing_checked_syntax() {
    for case in cases() {
        let mut host = Host::from_case(&case);
        host.syntax_available = false;
        host.module_facts_available = true;
        let selection = host.selection(&case);
        assert_eq!(
            source_names(&host, &get_source_files_to_emit(&host, selection).unwrap()),
            case["planning_observation"]["source_files"],
            "{} prepared selection",
            case["case_id"]
        );
        let plan = preflight_emit(&host, selection).unwrap();
        assert_eq!(
            json!(plan
                .plan()
                .units()
                .iter()
                .map(|unit| unit_value(&host, unit.paths(), unit.root()))
                .collect::<Vec<_>>()),
            case["planning_observation"]["units"],
            "{} prepared plan",
            case["case_id"]
        );
    }
}

#[test]
fn bundle_shape_is_valid_but_older_profiles_still_reject_emit_requests() {
    let cases = cases();
    let host = Host::from_case(&cases[0]);
    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(preflight.plan().validate_bootstrap_shape(), Ok(()));
    let mut sink = crate::MemoryOutputSink::new();
    let mut activity = crate::H2ActivityCanary::h2_7c_profile();
    let before = activity.counters();
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::emit_files_with_activity(
            &crate::UnavailableEmitResolver,
            &host,
            preflight,
            EmitSelection::WholeProgram,
            &crate::EmitDiagnosticGate::default(),
            &mut sink,
            &mut activity,
        )
    }))
    .expect_err("ordinary Bundle request needs H2.7d admission");
    assert!(denied
        .downcast_ref::<String>()
        .unwrap()
        .contains("unadmitted H2 runtime activity: H2.7d"));
    assert!(sink.writes().is_empty());
    assert_eq!(activity.counters(), before);
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::emit_forced_declarations_with_activity(
            &crate::UnavailableEmitResolver,
            &host,
            EmitSelection::WholeProgram,
            &mut sink,
            &mut activity,
        )
    }))
    .expect_err("forced Bundle request needs H2.7d admission");
    assert!(denied
        .downcast_ref::<String>()
        .unwrap()
        .contains("unadmitted H2 runtime activity: H2.7d"));
    assert!(sink.writes().is_empty());
    assert_eq!(activity.counters(), before);
}
