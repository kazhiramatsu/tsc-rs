//! CFG1d: fresh config inheritance, converted options, roots, and diagnostics.

use std::path::Path;

use serde_json::{json, Value};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    parse_config_root_plan, CompilerConfigHost, ConfigOptionBag, ConfigOptionValueState,
    ConfigRootPlanRequest, ConfigTypedListElement,
};

fn diagnostic_record(diagnostic: &Diagnostic) -> Value {
    let category = match diagnostic.category() {
        DiagnosticCategory::Warning => "warning",
        DiagnosticCategory::Error => "error",
        DiagnosticCategory::Suggestion => "suggestion",
        DiagnosticCategory::Message => "message",
    };
    json!({
        "code": diagnostic.code(),
        "category": category,
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "message": diagnostic.message_text(),
        "related_information": diagnostic.related.iter().map(|related| diagnostic_record(
            &Diagnostic::new(related.file_name.clone(), related.start, related.length, related.message.clone())
        )).collect::<Vec<_>>(),
    })
}

fn option_record(options: &ConfigOptionBag, name: &str) -> Value {
    match options.typed_value_state(name) {
        ConfigOptionValueState::Absent => json!({"name": name, "state": "absent"}),
        ConfigOptionValueState::Undefined => json!({"name": name, "state": "undefined"}),
        ConfigOptionValueState::Value(value) => {
            json!({"name": name, "state": "value", "value": value})
        }
        ConfigOptionValueState::Object(value) => {
            json!({"name": name, "state": "value", "value": value.json_projection()})
        }
        ConfigOptionValueState::List(elements) => json!({
            "name": name, "state": "list",
            "elements": elements.iter().map(|element| match element {
                ConfigTypedListElement::Undefined => json!({"state": "undefined"}),
                ConfigTypedListElement::Value(value) => json!({"state": "value", "value": value}),
            }).collect::<Vec<_>>(),
        }),
        ConfigOptionValueState::PositiveInfinity => {
            json!({"name": name, "state": "positive-infinity"})
        }
        ConfigOptionValueState::NegativeInfinity => {
            json!({"name": name, "state": "negative-infinity"})
        }
    }
}

// JSONC values follow JavaScript Number semantics. serde retains whether a
// finite number was parsed as an integer or a float (for example 42 / 42.0).
fn json_values_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_values_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_values_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

fn observe(case: &Value, option_keys: &[Value]) -> Value {
    let config_path = case["config_path"].as_str().expect("config path");
    let config = case["config"].as_str().expect("config source");
    let base = Path::new(config_path).parent().expect("config directory");
    let mut builder = MemoryCompilerHost::builder(base);
    for file in case["files"].as_array().expect("virtual files") {
        builder = builder.file(
            file["path"].as_str().expect("file path"),
            file["text"].as_str().expect("file source").as_bytes(),
        );
    }
    let host = builder
        .file(config_path, config.as_bytes())
        .build()
        .expect("fresh memory host");
    let plan = match parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: config_path.to_owned(),
            text: config.to_owned(),
            base_path: base.to_str().expect("Unicode directory").to_owned(),
        },
    ) {
        Ok(plan) => plan,
        Err(error) => return json!({"unexpected_config_failure": error.to_string()}),
    };
    json!({
        "watch_options_present": plan.watch_option_bag().is_some(),
        "watch_option_probes": (["watchFile", "watchDirectory", "fallbackPolling", "synchronousWatchDirectory", "excludeDirectories", "excludeFiles"].iter()
            .map(|name| plan.watch_option_bag().map_or_else(|| json!({"name": name, "state": "absent"}), |bag| option_record(bag, name))).collect::<Vec<_>>()),
        "acquisition_option_probes": (["enable", "include", "exclude", "disableFilenameBasedTypeAcquisition"].iter()
            .map(|name| option_record(plan.type_acquisition_option_bag(), name)).collect::<Vec<_>>()),
        "compile_on_save": plan.compile_on_save_enabled(),
        "raw": plan.raw(),
        "file_names": plan.file_names(),
        "wildcard_directories": plan.wildcard_directories().iter()
            .map(|directory| json!({"path": directory.path, "recursive": directory.recursive}))
            .collect::<Vec<_>>(),
        "extended_source_files": plan.extended_source_files(),
        "extended_sources": plan.extended_sources().iter()
            .map(|source| json!({"file_name": source.file_name, "text": source.text()}))
            .collect::<Vec<_>>(),
        "option_probes": option_keys.iter()
            .map(|name| option_record(plan.options(), name.as_str().expect("option key")))
            .collect::<Vec<_>>(),
        "root_parse_diagnostics": plan.root_parse_diagnostics().iter().map(diagnostic_record).collect::<Vec<_>>(),
        "parsed_errors": plan.errors().iter().map(diagnostic_record).collect::<Vec<_>>(),
        "config_diagnostics": plan.diagnostics().map(diagnostic_record).collect::<Vec<_>>(),
    })
}

#[test]
fn config_root_boundaries_matches_fresh_typescript_observations() {
    let inputs: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/h2-8b-config-root-boundaries-inputs.json"
    ))
    .expect("config inputs");
    let oracle: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/h2-8b-config-root-boundaries.json"
    ))
    .expect("frozen config observations");
    assert_eq!(oracle["typescript"], "6.0.3");
    assert_eq!(oracle["repetitions"], 2);
    assert_eq!(oracle["program_executions"], 0);
    let cases = inputs["cases"].as_array().expect("input cases");
    let expected = oracle["cases"].as_array().expect("observed cases");
    assert_eq!(cases.len(), 32);
    assert_eq!(expected.len(), cases.len());
    let option_keys = inputs["option_probe_keys"].as_array().expect("option keys");
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["case_id"], expected["case_id"]);
        let case_id = case["case_id"].as_str().expect("case id");
        let expected = &expected["typescript_observation"];
        for repetition in 1..=2 {
            let actual = observe(case, option_keys);
            let exact = json_values_equivalent(&actual, expected);
            eprintln!(
                "H2.8b-CFG1d-boundary {}",
                json!({"case_id": case_id, "repetition": repetition, "exact": exact, "actual": actual})
            );
            if !exact {
                let fields = expected
                    .as_object()
                    .expect("observed config record")
                    .iter()
                    .filter(|(name, value)| !json_values_equivalent(&actual[*name], value))
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                failures.push(format!("{case_id} repetition {repetition}: {fields:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
