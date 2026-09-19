//! Replay the exact loaders captured by the parser census, then observe two
//! complete production commands. Input drift is fatal; emit refusals remain
//! observations and cannot be counted as qualification.
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, JsStr, JsString, MessageChain};
use tsc_emitter::{EmitArtifact, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_compiler_no_emit, load_project_emit, load_project_no_emit,
    load_recorded_execution_plans, observable_input, UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_program::PreparedProgram;

use crate::codegen_common::find_workspace_root;
use crate::recovery_parse_snapshot::sha256;
use crate::utf16_literal_recovery_census::{
    candidate_input, qualified_input, CANDIDATE_INPUT_ARTIFACTS, QUALIFIED_INPUT_ARTIFACTS,
    TYPESCRIPT_FACT_ARTIFACTS,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
mod options;

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("missing string {field}").into())
}

fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into().into())
    }
}

fn canonical_input(value: &Value) -> Result<Value> {
    Ok(match value {
        Value::Array(values) => {
            Value::Array(values.iter().map(canonical_input).collect::<Result<_>>()?)
        }
        Value::Object(values) => {
            // preserve_order features unify; never rely on the map's order.
            let sorted: BTreeMap<_, _> = values.iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| Ok((key.clone(), canonical_input(value)?)))
                    .collect::<Result<_>>()?,
            )
        }
        Value::Number(number) => {
            require(
                number
                    .as_i64()
                    .is_some_and(|n| (-9007199254740991..=9007199254740991).contains(&n))
                    || number.as_u64().is_some_and(|n| n <= 9007199254740991),
                "command input numbers must be safe integers",
            )?;
            value.clone()
        }
        value => value.clone(),
    })
}

fn git(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()?;
    require(output.status.success(), format!("git {args:?} failed"))?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn clean(workspace: &Path) -> Result<String> {
    require(
        git(workspace, &["diff", "HEAD", "--name-only"])?.is_empty(),
        "native corpus replay requires committed, unchanged sources",
    )?;
    require(
        git(
            workspace,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--",
                "crates",
                "scripts",
            ],
        )?
        .is_empty(),
        "native corpus replay requires tracked code and fixtures",
    )?;
    git(workspace, &["rev-parse", "HEAD"])
}

// Keys never use case_id. A case_id is checked only after finding the exact
// pinned fixture and matrix configuration.
fn plan_key(input: &UpstreamExecutionInput) -> String {
    match input {
        UpstreamExecutionInput::Compiler(plan) => json!([
            "recorded-compiler",
            plan.fixture.source.relative_path.as_ref(),
            plan.fixture.source.git_blob_sha1.as_ref(),
            plan.variant.key.as_ref(),
            plan.variant.configuration_index
        ]),
        UpstreamExecutionInput::Project(plan) => json!([
            "recorded-project",
            plan.fixture.source.relative_path.as_ref(),
            plan.fixture.source.git_blob_sha1.as_ref(),
            format!("{:?}", plan.module_variant),
            plan.fixture.scenario.as_ref()
        ]),
    }
    .to_string()
}

fn input_key(input: &Value) -> Result<String> {
    Ok(match string(input, "route")? {
        "recorded-compiler" => json!([
            "recorded-compiler",
            string(input, "fixture_path")?,
            string(input, "fixture_blob_sha1")?,
            string(&input["variant"], "key")?,
            input["variant"]["configuration_index"]
                .as_u64()
                .ok_or("missing configuration_index")?
        ]),
        "recorded-project" => json!([
            "recorded-project",
            string(input, "descriptor_path")?,
            string(input, "descriptor_blob_sha1")?,
            string(input, "module_variant")?,
            string(input, "scenario")?
        ]),
        route => return Err(format!("not a recorded plan route: {route}").into()),
    }
    .to_string())
}

fn index_plans(
    plans: &[UpstreamExecutionPlan],
) -> Result<BTreeMap<String, &UpstreamExecutionPlan>> {
    let mut index = BTreeMap::new();
    for plan in plans {
        let key = plan_key(&plan.input);
        require(
            index.insert(key.clone(), plan).is_none(),
            format!("duplicate execution plan key {key}"),
        )?;
    }
    Ok(index)
}

fn documents(artifact: &Value) -> Result<BTreeMap<String, Vec<u8>>> {
    artifact["documents"]
        .as_object()
        .ok_or("documents is not an object")?
        .iter()
        .map(|(hash, encoded)| {
            let bytes = STANDARD.decode(encoded.as_str().ok_or("document is not base64 text")?)?;
            require(
                sha256(&bytes) == *hash,
                format!("document pool SHA-256 differs: {hash}"),
            )?;
            Ok((hash.clone(), bytes))
        })
        .collect()
}

fn verify_document_references(value: &Value, pool: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "content_sha256" && !child.is_null() {
                    require(
                        pool.contains_key(child.as_str().ok_or("invalid document reference")?),
                        format!("missing document pool entry {child}"),
                    )?;
                } else {
                    verify_document_references(child, pool)?;
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                verify_document_references(child, pool)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn same_input(expected: &Value, actual: &Value) -> Result<()> {
    for field in [
        "compiler_options_debug_sha256",
        "program_options_debug_sha256",
    ] {
        require(
            expected["prepared"][field] == actual["prepared"][field],
            format!("{field} differs; rebuild the census with the matching loader binary"),
        )?;
    }
    require(serde_json::to_vec(expected)? == serde_json::to_vec(actual)?,
        "reloaded command input differs from the census (including roots, source hashes and limits)")
}

fn prepare(
    workspace: &Path,
    row: &Value,
    plans: &BTreeMap<String, &UpstreamExecutionPlan>,
    pool: &BTreeMap<String, Vec<u8>>,
) -> Result<PreparedProgram> {
    let input = &row["command_input"];
    let route = string(input, "route")?;
    let loader = string(row, "loader")?;
    require(
        input["floor"] == "established",
        "unexpected emit option floor",
    )?;
    verify_document_references(input, pool)?;
    let (program, actual) = match route {
        "recorded-compiler" | "recorded-project" => {
            require(
                row["universe"] == "recorded-execution-plans",
                "recorded plan universe differs",
            )?;
            let plan = plans
                .get(&input_key(input)?)
                .ok_or("no exact recorded plan identity")?;
            require(
                plan.provenance.case_id.as_ref() == string(row, "case_id")?,
                "recorded plan case_id check failed",
            )?;
            let program = match (&plan.input, loader) {
                (UpstreamExecutionInput::Compiler(plan), "load_compiler_emit") => {
                    load_compiler_emit(workspace, plan, observable_input::limits())?
                }
                (UpstreamExecutionInput::Compiler(plan), "load_compiler_no_emit") => {
                    load_compiler_no_emit(workspace, plan, observable_input::limits())?
                }
                (UpstreamExecutionInput::Project(plan), "load_project_emit") => {
                    load_project_emit(workspace, plan, observable_input::limits())?.prepared_program
                }
                (UpstreamExecutionInput::Project(plan), "load_project_no_emit") => {
                    load_project_no_emit(workspace, plan, observable_input::limits())?
                        .prepared_program
                }
                _ => return Err(format!("loader {loader} does not match {route}").into()),
            };
            let mut observed = BTreeMap::new();
            let actual = observable_input::plan_input(&plan.input, &program, &mut |bytes| {
                let hash = sha256(bytes);
                observed.insert(hash.clone(), bytes.to_vec());
                hash
            });
            for (hash, bytes) in observed {
                require(
                    pool.get(&hash) == Some(&bytes),
                    format!("reloaded document bytes differ: {hash}"),
                )?;
            }
            (program, actual)
        }
        "qualified" | "candidate" => {
            require(
                loader == "load_qualified_compiler_emit_with_symlinks",
                "artifact loader differs",
            )?;
            let allowed = if route == "qualified" {
                QUALIFIED_INPUT_ARTIFACTS
            } else {
                CANDIDATE_INPUT_ARTIFACTS
            };
            require(
                allowed.contains(&string(row, "universe")?),
                "artifact route/universe differs",
            )?;
            require(
                input["use_case_sensitive_file_names"] == true,
                "qualified loader case sensitivity differs",
            )?;
            let program = if route == "qualified" {
                require(
                    input["settings"].is_null(),
                    "qualified settings must remain in embedded input",
                )?;
                qualified_input(workspace, &input["input"])?
            } else {
                require(
                    input["input"]["route"] == "whole-program"
                        && input["input"]["shared_mount"].is_null(),
                    "candidate route differs",
                )?;
                candidate_input(
                    workspace,
                    &json!({"input":input["input"],"settings":input["settings"]}),
                )?
            };
            let actual = observable_input::artifact_input(
                route,
                &input["input"],
                &input["settings"],
                &program,
            );
            (program, actual)
        }
        _ => return Err(format!("unknown command route {route}").into()),
    };
    same_input(input, &actual)?;
    Ok(program)
}

fn string_value<'a>(text: impl Into<JsStr<'a>>) -> Value {
    let text = text.into();
    json!({"utf16":text.to_utf16(),"utf8_base64":STANDARD.encode(text.to_string_lossy().as_bytes())})
}

fn message(chain: &MessageChain, indent: usize, text: &mut JsString) {
    if indent != 0 {
        text.push_str("\n");
        text.push_str(&"  ".repeat(indent));
    }
    text.push_js(chain.text.as_js());
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(values: &[Diagnostic]) -> Value {
    json!(values
        .iter()
        .map(|d| {
            let mut text = JsString::new();
            message(&d.message, 0, &mut text);
            let related = (d.related_information_present || !d.related.is_empty()).then(|| {
                d.related
                    .iter()
                    .map(|r| {
                        let mut text = JsString::new();
                        message(&r.message, 0, &mut text);
                        json!({"code":r.message.code,"category":r.message.category as u8,
                "file":r.file_name.as_ref().map(string_value),"start":r.start,"length":r.length,
                "message":string_value(&text),"related_information":null})
                    })
                    .collect::<Vec<_>>()
            });
            json!({"code":d.code(),"category":d.category() as u8,
            "file":d.file_name.as_ref().map(string_value),"start":d.start,"length":d.length,
            "message":string_value(&text),"related_information":related})
        })
        .collect::<Vec<_>>())
}

fn captured_write(index: usize, artifact: &EmitArtifact) -> Result<Value> {
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        Some(EmitWriteMetadata::BuildInfo(_)) => return Err(
            "build-info callback metadata requires an explicit observer; cannot qualify this row"
                .into(),
        ),
    };
    Ok(json!({"index":index,"path":string_value(artifact.path()),
        "callback":{"utf16":artifact.callback_units(),"utf8_base64":STANDARD.encode(artifact.callback_bytes())},
        "write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":STANDARD.encode(artifact.materialized_bytes()),
        "on_error_callback_present":true,
        "source_files":artifact.source_files().map(|files|files.iter().map(string_value).collect::<Vec<_>>()),
        "data_present":artifact.metadata().is_some(),"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics}))
}

fn complete(program: PreparedProgram) -> Result<Value> {
    let mut sink = MemoryOutputSink::new();
    let result = ProgramSession::new(program).emit_command_for_harness(&mut sink);
    let writes = sink
        .writes()
        .iter()
        .enumerate()
        .map(|(index, artifact)| captured_write(index, artifact))
        .collect::<Result<Vec<_>>>()?;
    Ok(match result {
        Ok(command) => {
            let emit = command.emit();
            let maps = emit.source_maps().map(|maps|maps.iter().map(|map|json!({
                "input_source_file_names":map.input_source_files().iter().map(string_value).collect::<Vec<_>>(),
                "source_map_json":string_value(map.canonical_json())
            })).collect::<Vec<_>>());
            json!({"writes":writes,"reported_diagnostics":diagnostics(command.diagnostics()),
                "status_writes":command.status_writes().iter().map(string_value).collect::<Vec<_>>(),
                "exit_code":command.exit_code(),"emit_result":{"emit_skipped":emit.emit_skipped(),
                    "diagnostics":diagnostics(emit.diagnostics()),
                    "emitted_files":emit.emitted_files().map(|files|files.iter().map(string_value).collect::<Vec<_>>()),
                    "source_maps":maps}})
        }
        Err(error) => json!({"production_error":error.to_string(),"partial_writes":writes}),
    })
}

fn observe(program: PreparedProgram) -> Value {
    match complete(program) {
        Ok(observation) => observation,
        Err(error) => json!({"observer_unsupported":error.to_string()}),
    }
}

fn validate_data_workspace(workspace: &Path, selection: &Value) -> Result<()> {
    let census_head = string(selection, "head")?;
    git(
        workspace,
        &["merge-base", "--is-ancestor", census_head, "HEAD"],
    )?;
    require(
        selection["vendor_tree_hash"]
            == git(workspace, &["rev-parse", "HEAD:vendor/typescript-6.0.3"])?,
        "input vendor tree differs",
    )?;
    let mut paths = vec![
        "vendor/typescript-6.0.3",
        "ts-tests",
        tsc_harness::upstream_suites::MANIFEST_RELATIVE_PATH,
    ];
    for path in selection["input_manifest"]
        .as_object()
        .ok_or("missing input manifest")?
        .keys()
    {
        if !matches!(
            path.as_str(),
            "recorded_execution_plans" | "typescript_parity"
        ) {
            paths.push(path);
        }
    }
    for prefix in [
        vec!["diff", "--name-only", census_head, "HEAD", "--"],
        vec!["diff", "--name-only", "HEAD", "--"],
        vec!["ls-files", "--others", "--exclude-standard", "--"],
    ] {
        let mut args = prefix;
        args.extend_from_slice(&paths);
        require(
            git(workspace, &args)?.is_empty(),
            "census input workspace data changed",
        )?;
    }
    Ok(())
}

fn selected_syntax_tree(selection: &Value) -> Result<&str> {
    let authority = selection.get("successor").unwrap_or(selection);
    let pin = string(authority, "syntax_tree_hash")?;
    require(
        pin.len() == 40 && pin.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid selected syntax tree pin",
    )?;
    Ok(pin)
}

fn validate_selection(workspace: &Path, input_workspace: &Path, selection: &Value) -> Result<()> {
    require(
        selection["schema"] == 1 && selection["kind"] == "emitter-recovery-corpus-selection",
        "not a parser corpus selection",
    )?;
    require(
        selected_syntax_tree(selection)? == git(workspace, &["rev-parse", "HEAD:crates/syntax"])?,
        "syntax tree differs from selected parser proof",
    )?;
    require(
        selection["vendor_tree_hash"]
            == git(workspace, &["rev-parse", "HEAD:vendor/typescript-6.0.3"])?,
        "vendor tree differs from census",
    )?;
    // The captured parser stays an input authority even when a separately
    // proven successor is the code being qualified.
    let census_syntax = format!("{}:crates/syntax", string(selection, "head")?);
    require(
        selection["syntax_tree_hash"] == git(input_workspace, &["rev-parse", &census_syntax])?,
        "captured syntax tree differs from census authority",
    )?;
    if let Some(successor) = selection.get("successor") {
        let pins = successor["source_files_sha256"]
            .as_object()
            .ok_or("missing successor source identities")?;
        let tracked = git(
            workspace,
            &[
                "ls-files",
                "-z",
                "--",
                "crates/syntax",
                "crates/types",
                "crates/diagnostics",
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
            ],
        )?;
        let paths = tracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        require(
            paths == pins.keys().map(String::as_str).collect(),
            "successor source catalog differs from compiled workspace",
        )?;
        for path in paths {
            require(
                pins[path] == sha256(fs::read(workspace.join(path))?),
                format!("successor source pin differs: {path}"),
            )?;
        }
    }
    require(
        selection["digest_code_sha256"] == sha256(include_bytes!("recovery_parse_snapshot.rs")),
        "parse graph digest code differs from census",
    )?;
    let manifest = tsc_harness::upstream_suites::MANIFEST_RELATIVE_PATH;
    require(
        selection["plan_manifest_sha256"] == sha256(fs::read(input_workspace.join(manifest))?),
        "recorded execution manifest differs",
    )?;
    for (path, pin) in selection["input_manifest"]
        .as_object()
        .ok_or("missing input manifest")?
    {
        if matches!(
            path.as_str(),
            "recorded_execution_plans" | "typescript_parity"
        ) {
            continue;
        }
        require(
            QUALIFIED_INPUT_ARTIFACTS.contains(&path.as_str())
                || CANDIDATE_INPUT_ARTIFACTS.contains(&path.as_str())
                || TYPESCRIPT_FACT_ARTIFACTS.contains(&path.as_str()),
            "unexpected artifact path in input manifest",
        )?;
        require(
            pin["sha256"] == sha256(fs::read(input_workspace.join(path))?),
            format!("census artifact pin differs: {path}"),
        )?;
    }
    Ok(())
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let input_path = PathBuf::from(
        args.next()
            .ok_or("usage: recovery-corpus-native SELECTION OUTPUT INPUT_WORKSPACE")?,
    );
    let output_path = PathBuf::from(args.next().ok_or("missing output path")?);
    let input_workspace =
        fs::canonicalize(PathBuf::from(args.next().ok_or("missing input workspace")?))?;
    require(args.next().is_none(), "unexpected command argument")?;
    require(
        !output_path.exists(),
        "refusing to overwrite native observation",
    )?;
    let workspace = find_workspace_root()?;
    let head = clean(&workspace)?;
    require(
        fs::read(workspace.join("crates/xtask/src/recovery_corpus_native.rs"))?
            == include_bytes!("recovery_corpus_native.rs"),
        "native observation binary is stale",
    )?;
    let bytes = fs::read(&input_path)?;
    let selection: Value = serde_json::from_slice(&bytes)?;
    validate_selection(&workspace, &input_workspace, &selection)?;
    validate_data_workspace(&input_workspace, &selection)?;
    let pool = documents(&selection)?;
    let corpus = load_recorded_execution_plans(&input_workspace)?;
    let plans = index_plans(&corpus.plans)?;
    let rows = selection["cases"]
        .as_array()
        .ok_or("selection cases is not an array")?;
    require(
        selection["summary"]["selected_rows"] == json!(rows.len()),
        "selected count differs",
    )?;
    let mut seen = BTreeSet::new();
    let mut observations = Vec::new();
    let (mut completed, mut refused, mut fallback, mut unsupported) =
        (0usize, 0usize, 0usize, 0usize);
    for (index, row) in rows.iter().enumerate() {
        let id = string(row, "case_id")?;
        require(seen.insert(id), format!("duplicate selected row {id}"))?;
        if matches!(
            string(&row["command_input"], "route")?,
            "qualified" | "candidate"
        ) {
            require(
                selection["input_manifest"]
                    .get(string(row, "universe")?)
                    .is_some(),
                format!("{id}: unpinned artifact universe"),
            )?;
        }
        let program = prepare(&input_workspace, row, &plans, &pool)
            .map_err(|error| format!("{id}: input reconstruction failed: {error}"))?;
        let input_sha256 = sha256(serde_json::to_vec(&canonical_input(
            &row["command_input"],
        )?)?);
        let no_emit = matches!(
            string(row, "loader")?,
            "load_compiler_no_emit" | "load_project_no_emit"
        );
        let option_snapshot = options::snapshot(&program);
        let first = observe(program);
        let repeat_program = prepare(&input_workspace, row, &plans, &pool)?;
        require(
            option_snapshot == options::snapshot(&repeat_program),
            format!("{id}: native option repetition differs"),
        )?;
        let second = observe(repeat_program);
        require(
            first == second,
            format!("{id}: native command repetition differs"),
        )?;
        if first.get("observer_unsupported").is_some() {
            unsupported += 1;
        }
        let observation = if no_emit {
            require(
                row["emit_load_error"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
                    && row["emit_disposition"] == "parse-admission-only; emit-not-qualified",
                format!("{id}: fallback disposition lost"),
            )?;
            fallback += 1;
            json!({"case_id":id,"input_sha256":input_sha256,"disposition":"parse-admission-only; emit-not-qualified","emit_load_error":row["emit_load_error"],"options":option_snapshot,"complete_command_runs":[first,second]})
        } else {
            require(
                row["emit_load_error"].is_null()
                    && row["emit_disposition"] == "pending-complete-command-comparison",
                format!("{id}: emit disposition differs"),
            )?;
            let disposition = if first.get("observer_unsupported").is_some() {
                "observer-unsupported; emit-not-qualified"
            } else if first.get("production_error").is_some() {
                refused += 1;
                "production-refusal; emit-not-qualified"
            } else {
                completed += 1;
                "observed-twice; pending-typescript-comparison"
            };
            json!({"case_id":id,"input_sha256":input_sha256,"disposition":disposition,"options":option_snapshot,"complete_command_runs":[first,second]})
        };
        eprintln!(
            "native corpus {}/{} {id}: {}",
            index + 1,
            rows.len(),
            observation["disposition"]
        );
        observations.push(observation);
    }
    require(
        clean(&workspace)? == head,
        "source HEAD changed during native observation",
    )?;
    validate_data_workspace(&input_workspace, &selection)?;
    let result = json!({"schema":1,"kind":"emitter-recovery-native-observations","head":head,
        "syntax_tree_hash":git(&workspace, &["rev-parse", "HEAD:crates/syntax"] )?,
        "successor_source_files_sha256":selection["successor"]["source_files_sha256"],
        "input_workspace":input_workspace,"library_root":fs::canonicalize(input_workspace.join("vendor/typescript-6.0.3/lib"))?,
        "census_head":selection["head"],"selection_sha256":sha256(&bytes),
        "observer_sha256":sha256(include_bytes!("recovery_corpus_native.rs")),
        "binary_sha256":sha256(fs::read(std::env::current_exe()?)?),"repetitions":2,
        "summary":{"selected":rows.len(),"complete_commands":completed,"production_refusals":refused,
            "no_emit_fallbacks":fallback,"observer_unsupported":unsupported,"unloaded_rows":selection["load_failures"].as_array().ok_or("missing load failures")?.len()},
        "load_failures":selection["load_failures"],"cases":observations});
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)?;
    serde_json::to_writer(&mut file, &result)?;
    file.write_all(b"\n")?;
    println!("{}", result["summary"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successor_syntax_pin_is_required_without_changing_census_authority() {
        let original = "a".repeat(40);
        let successor = "b".repeat(40);
        let mut selection = json!({"syntax_tree_hash":original});
        assert_eq!(selected_syntax_tree(&selection).unwrap(), original);
        selection["successor"] = json!({"syntax_tree_hash":successor});
        assert_eq!(selected_syntax_tree(&selection).unwrap(), successor);
        assert_eq!(selection["syntax_tree_hash"], original);
        for invalid in [Value::Null, json!({}), json!({"syntax_tree_hash":"wrong"})] {
            selection["successor"] = invalid;
            assert!(selected_syntax_tree(&selection).is_err());
        }
    }

    #[test]
    fn canonical_input_hash_ignores_nested_object_insertion_order() {
        let first: Value = serde_json::from_str(r#"{"z":[{"y":2,"a":1}],"a":0}"#).unwrap();
        let second: Value = serde_json::from_str(r#"{"a":0,"z":[{"a":1,"y":2}]}"#).unwrap();
        let actual = serde_json::to_vec(&canonical_input(&first).unwrap()).unwrap();
        assert_eq!(actual, br#"{"a":0,"z":[{"a":1,"y":2}]}"#);
        assert_eq!(
            actual,
            serde_json::to_vec(&canonical_input(&second).unwrap()).unwrap()
        );
    }

    #[test]
    fn canonical_input_rejects_float_and_unsafe_integer_encodings() {
        for text in ["1.0", "0.25", "9007199254740992", "-9007199254740992"] {
            assert!(canonical_input(&serde_json::from_str(text).unwrap()).is_err());
        }
    }

    #[test]
    fn matrix_identity_ignores_case_id_and_retains_configuration() {
        let mut input = json!({"route":"recorded-compiler","fixture_path":"a.ts","fixture_blob_sha1":"b","variant":{"key":"target=es5","configuration_index":2},"case_id":"display"});
        let key = input_key(&input).unwrap();
        input["case_id"] = json!("other-display");
        assert_eq!(input_key(&input).unwrap(), key);
        input["variant"]["configuration_index"] = json!(3);
        assert_ne!(input_key(&input).unwrap(), key);
    }

    #[test]
    fn document_identity_and_absent_content_are_distinct() {
        let hash = sha256(b"");
        let pool = documents(&json!({"documents":{hash.clone():""}})).unwrap();
        verify_document_references(
            &json!([{"content_sha256":null},{"content_sha256":hash}]),
            &pool,
        )
        .unwrap();
        assert!(verify_document_references(&json!({"content_sha256":"missing"}), &pool).is_err());
        assert!(documents(&json!({"documents":{"bad-hash":""}})).is_err());
    }

    #[test]
    fn full_loader_verification_rejects_write_order_and_source_drift() {
        let expected = json!({"prepared":{"roots":["a.ts"],"source_files":[{"path":"a.ts","sha256":"x"}],"compiler_options_debug_sha256":"c","program_options_debug_sha256":"p"},"vfs_write_order":[0,1]});
        same_input(&expected, &expected).unwrap();
        let mut actual = expected.clone();
        actual["vfs_write_order"] = json!([1, 0]);
        assert!(same_input(&expected, &actual).is_err());
        actual = expected.clone();
        actual["prepared"]["source_files"][0]["sha256"] = json!("y");
        assert!(same_input(&expected, &actual).is_err());
    }
}
