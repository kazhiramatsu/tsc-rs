//! CFG1e: config graph reuse and exact host callback/fault observations.
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use tsc_diagnostics::{Diagnostic, DiagnosticCategory};
use tsc_program::{
    parse_config_root_plan, parse_config_root_plan_with_cache, ConfigExtendedCache,
    ConfigHostError, ConfigHostOperation, ConfigOptionBag, ConfigOptionValueState, ConfigParseHost,
    ConfigRootPlanRequest, ConfigTypedListElement,
};

fn diagnostic(d: &Diagnostic) -> Value {
    let category = match d.category() {
        DiagnosticCategory::Error => "error",
        DiagnosticCategory::Warning => "warning",
        DiagnosticCategory::Suggestion => "suggestion",
        DiagnosticCategory::Message => "message",
    };
    json!({"code": d.code(), "category": category, "file": d.file_name, "start": d.start, "length": d.length, "message": d.message_text(), "related_information": d.related.iter().map(|r| diagnostic(&Diagnostic::new(r.file_name.clone(),r.start,r.length,r.message.clone()))).collect::<Vec<_>>()})
}
fn options(bag: &ConfigOptionBag) -> Value {
    let mut object = serde_json::Map::new();
    for entry in bag.entries() {
        let value = match bag.typed_value_state(&entry.name) {
            ConfigOptionValueState::Absent | ConfigOptionValueState::Undefined => continue,
            ConfigOptionValueState::Value(v) => v.clone(),
            ConfigOptionValueState::Object(v) => v.json_projection(),
            ConfigOptionValueState::List(v) => Value::Array(
                v.iter()
                    .map(|v| match v {
                        ConfigTypedListElement::Undefined => Value::Null,
                        ConfigTypedListElement::Value(v) => v.clone(),
                    })
                    .collect(),
            ),
            ConfigOptionValueState::PositiveInfinity | ConfigOptionValueState::NegativeInfinity => {
                Value::Null
            }
        };
        object.insert(entry.name.clone(), value);
    }
    Value::Object(object)
}
struct Host {
    files: RefCell<BTreeMap<String, String>>,
    calls: RefCell<Vec<Value>>,
    fault: Value,
    sensitive: bool,
}
impl Host {
    fn key(&self, path: &str) -> String {
        if self.sensitive {
            path.to_owned()
        } else {
            path.to_lowercase()
        }
    }
    fn call(
        &self,
        op: ConfigHostOperation,
        path: &str,
        mut args: Value,
    ) -> Result<bool, ConfigHostError> {
        args["operation"] = json!(op.to_string());
        args["path"] = json!(path);
        self.calls.borrow_mut().push(args);
        if self.fault["operation"] == op.to_string()
            && self.fault["path"]
                .as_str()
                .is_some_and(|p| self.key(p) == self.key(path))
        {
            if self.fault["mode"] == "absent" {
                return Ok(true);
            }
            return Err(ConfigHostError::new(op, path, "synthetic fault"));
        }
        Ok(false)
    }
}
impl ConfigParseHost for Host {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.sensitive
    }
    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        Ok(
            !self.call(ConfigHostOperation::FileExists, path, json!({}))?
                && self.files.borrow().contains_key(&self.key(path)),
        )
    }
    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        if self.call(ConfigHostOperation::ReadFile, path, json!({}))? {
            return Ok(None);
        }
        Ok(self.files.borrow().get(&self.key(path)).cloned())
    }
    fn read_directory(
        &self,
        path: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        self.call(
            ConfigHostOperation::ReadDirectory,
            path,
            json!({"extensions":extensions,"excludes":excludes,"includes":includes,"depth":depth}),
        )?;
        Ok(Vec::new())
    }
}
fn observe(case: &Value) -> Value {
    let host = Host {
        files: RefCell::new(BTreeMap::new()),
        calls: RefCell::new(Vec::new()),
        fault: case["fault"].clone(),
        sensitive: case["use_case_sensitive_file_names"].as_bool().unwrap(),
    };
    for file in case["files"].as_array().unwrap() {
        host.files.borrow_mut().insert(
            host.key(file["path"].as_str().unwrap()),
            file["text"].as_str().unwrap().to_owned(),
        );
    }
    let config_path = case["config_path"].as_str().unwrap();
    let text = case["config"].as_str().unwrap();
    host.files
        .borrow_mut()
        .insert(host.key(config_path), text.to_owned());
    let mut results = Vec::new();
    let mut cache = ConfigExtendedCache::default();
    for step in case["steps"].as_array().unwrap() {
        for update in step["updates"].as_array().into_iter().flatten() {
            host.files.borrow_mut().insert(
                host.key(update["path"].as_str().unwrap()),
                update["text"].as_str().unwrap().to_owned(),
            );
        }
        host.calls.borrow_mut().clear();
        if step["clear_cache"] == true {
            cache.clear();
        }
        let request = ConfigRootPlanRequest {
            file_name: config_path.to_owned(),
            text: text.to_owned(),
            base_path: Path::new(config_path)
                .parent()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        };
        let plan = if case["cache"] == true {
            parse_config_root_plan_with_cache(&host, request, &mut cache)
        } else {
            parse_config_root_plan(&host, request)
        };
        let actual = match plan {
            Ok(plan) => {
                let mut converted = options(plan.options());
                converted["configFilePath"] = json!(config_path);
                json!({"calls":host.calls.borrow().clone(),"raw":plan.raw(),"options":converted,"watch_options":plan.watch_options(),"type_acquisition":plan.type_acquisition(),"compile_on_save":plan.compile_on_save_enabled(),"file_names":plan.file_names(),"extended_source_files":plan.extended_source_files(),"root_parse_diagnostics":plan.root_parse_diagnostics().iter().map(diagnostic).collect::<Vec<_>>(),"parsed_errors":plan.errors().iter().map(diagnostic).collect::<Vec<_>>(),"config_diagnostics":plan.diagnostics().map(diagnostic).collect::<Vec<_>>()})
            }
            Err(error) => match error.host_error() {
                Some(e) => {
                    json!({"calls":host.calls.borrow().clone(),"host_failure":{"operation":e.operation().to_string(),"path":e.path(),"detail":e.detail()}})
                }
                None => {
                    json!({"calls":host.calls.borrow().clone(),"unexpected_failure":error.to_string()})
                }
            },
        };
        results.push(actual);
    }
    json!(results)
}
#[test]
fn config_reuse_matches_typescript_host_and_cache_observations() {
    let inputs: Value =
        serde_json::from_slice(include_bytes!("../fixtures/h2-8b-config-reuse-inputs.json"))
            .unwrap();
    let oracle: Value =
        serde_json::from_slice(include_bytes!("../fixtures/h2-8b-config-reuse.json")).unwrap();
    assert_eq!(oracle["typescript"], "6.0.3");
    assert_eq!(oracle["config_parse_attempts"], 60);
    let cases = inputs["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 26);
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(oracle["cases"].as_array().unwrap()) {
        assert_eq!(case["case_id"], expected["case_id"]);
        for repetition in 1..=2 {
            let actual = observe(case);
            let exact = actual == expected["typescript_observation"];
            eprintln!(
                "H2.8b-CFG1e {}",
                json!({"case_id":case["case_id"],"repetition":repetition,"exact":exact,"actual":actual})
            );
            if !exact {
                failures.push(format!("{} repetition {repetition}", case["case_id"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
