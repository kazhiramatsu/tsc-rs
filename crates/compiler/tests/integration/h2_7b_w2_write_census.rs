//! h2-7b-w2 evidence instrument — the non-declaration write census of the 34
//! `declaration-text-exact` rows.
//!
//! The selection is fixed to the row names recorded by the W1 census.  The
//! comparison is deliberately path-keyed: declarations are retained in the
//! record as controls, while every divergent non-declaration write gets its
//! own callback hunk (and every observable callback facet is recorded).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Map, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";
const MANIFEST_RELATIVE_PATH: &str = "ratchets/h2-7b-known-divergences.v1.json";

/// The W1 declaration-text-exact population, in manifest order.  Keeping the
/// names explicit prevents a later classifier change from silently changing
/// the census population.
const DECLARATION_TEXT_EXACT_CASE_IDS: &[&str] = &[
    "typescript-6.0.3/compiler/accessorInAmbientContextES5.ts#target%3Des5",
    "typescript-6.0.3/compiler/commentsClass.ts#target%3Des5",
    "typescript-6.0.3/compiler/commentsClassMembers.ts#target%3Des5",
    "typescript-6.0.3/compiler/commentsModules.ts#target%3Des2015",
    "typescript-6.0.3/compiler/commentsModules.ts#target%3Des5",
    "typescript-6.0.3/compiler/declarationEmitAmdModuleNameDirective.ts#default",
    "typescript-6.0.3/compiler/declarationEmitClassMemberNameConflict.ts#target%3Des5",
    "typescript-6.0.3/compiler/declarationEmitNestedBindingPattern.ts#default",
    "typescript-6.0.3/compiler/defaultDeclarationEmitShadowedNamedCorrectly.ts#default",
    "typescript-6.0.3/compiler/shorthandOfExportedEntity02_targetES5_CommonJS.ts#target%3Des5",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace1.ts#module%3Damd",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace1.ts#module%3Des2015",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace1.ts#module%3Dsystem",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace2.ts#module%3Damd",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace2.ts#module%3Des2015",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace2.ts#module%3Dsystem",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace3.ts#module%3Damd",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace3.ts#module%3Des2015",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace3.ts#module%3Dsystem",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace4.ts#module%3Des2015",
    "typescript-6.0.3/conformance/es2020/modules/exportAsNamespace4.ts#module%3Dsystem",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Damd",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dcommonjs",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Des6",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode16",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode18",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode20",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnodenext",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnone",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dsystem",
    "typescript-6.0.3/conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dumd",
    "typescript-6.0.3/conformance/importAssertion/importAssertion2.ts#module%3Des2015",
    "typescript-6.0.3/conformance/importAttributes/importAttributes2.ts#module%3Des2015",
    "typescript-6.0.3/conformance/types/typeParameters/typeArgumentLists/instantiationExpressions.ts#default",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn load_json(root: &Path, relative_path: &str) -> Value {
    serde_json::from_slice(
        &fs::read(root.join(relative_path)).expect("frozen H2.7b artifact exists"),
    )
    .unwrap_or_else(|error| panic!("{relative_path} is valid JSON: {error}"))
}

fn decode(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().expect("base64 text"))
        .expect("valid base64")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

/// Copied from the m-2 controls and the W1 instrument.  The declaration-family
/// floor is part of the observation being replayed; this helper does not
/// reconstruct fixture settings by hand.
fn prepared_band_row(root: &Path, case: &Value) -> tsc_program::PreparedProgram {
    let input = &case["input"];
    let mut files = input["files"]
        .as_array()
        .expect("case input files")
        .iter()
        .map(|file| {
            (
                PathBuf::from(file["path"].as_str().expect("file path")),
                decode(&file["utf8_base64"]),
            )
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            decode(&config["utf8_base64"]),
        ));
    }
    let roots = input["roots"]
        .as_array()
        .expect("case roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .expect("case settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("setting name").to_owned(),
                setting["value"].as_str().expect("setting value").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_option_floor(
        root,
        input["current_directory"]
            .as_str()
            .expect("case current directory"),
        &files,
        &roots,
        &settings,
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .unwrap_or_else(|error| panic!("{}: row loads: {error}", case["case_id"]))
}

fn qualification_case<'artifact>(artifact: &'artifact Value, case_id: &str) -> &'artifact Value {
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"].as_str() == Some(case_id))
        .unwrap_or_else(|| panic!("{case_id} is absent from the qualification artifact"))
}

fn selected_manifest_rows<'artifact>(artifact: &'artifact Value) -> Vec<&'artifact Value> {
    let expected = DECLARATION_TEXT_EXACT_CASE_IDS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let rows = artifact["cases"]
        .as_array()
        .expect("manifest cases")
        .iter()
        .filter(|case| {
            case["case_id"]
                .as_str()
                .is_some_and(|case_id| expected.contains(case_id))
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), DECLARATION_TEXT_EXACT_CASE_IDS.len());
    let actual = rows
        .iter()
        .map(|case| case["case_id"].as_str().expect("manifest case id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "the manifest selection is exactly the W1 34"
    );
    for row in &rows {
        assert!(row["writes_diverging"].as_u64().unwrap_or(0) > 0);
        assert_eq!(row["diagnostics_diverging"].as_bool(), Some(false));
        assert_eq!(row["emit_result_diverging"].as_bool(), Some(false));
        let vector = row["mismatch_vector"].as_array().expect("mismatch vector");
        assert!(vector.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|value| value.starts_with("H2_7b:write:"))
        }));
        assert!(vector.iter().any(|value| {
            value.as_str().is_some_and(|value| {
                value.starts_with("H2_7b:write:") && value.contains(":callback_bytes=")
            })
        }));
    }
    rows
}

fn is_declaration_path(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}

fn expected_write<'write>(writes: &'write [Value], path: &str) -> Option<&'write Value> {
    writes
        .iter()
        .find(|write| write["path"].as_str() == Some(path))
}

fn actual_write<'write>(
    writes: &'write [EmitArtifact],
    path: &str,
) -> Option<&'write EmitArtifact> {
    writes
        .iter()
        .find(|write| write.path().to_string_lossy() == path)
}

fn assert_unique_expected_paths(writes: &[Value], case_id: &str) {
    let mut paths = BTreeSet::new();
    for write in writes {
        let path = write["path"].as_str().expect("frozen write path");
        assert!(
            paths.insert(path),
            "{case_id}: duplicate frozen output path {path}"
        );
    }
}

fn assert_unique_actual_paths(writes: &[EmitArtifact], case_id: &str) {
    let mut paths = BTreeSet::new();
    for write in writes {
        let path = write.path().to_string_lossy().into_owned();
        assert!(
            paths.insert(path.clone()),
            "{case_id}: duplicate Rust output path {path}"
        );
    }
}

fn actual_source_files(write: &EmitArtifact) -> Value {
    write.source_files().map_or(Value::Null, |files| {
        Value::Array(
            files
                .iter()
                .map(|file| Value::String(file.to_string_lossy().into_owned()))
                .collect(),
        )
    })
}

fn actual_metadata_url(write: &EmitArtifact) -> Value {
    match write.metadata() {
        Some(EmitWriteMetadata::Text(metadata)) => metadata
            .source_map_url_position()
            .map(|position| Value::from(u64::from(position.value())))
            .unwrap_or(Value::Null),
        Some(EmitWriteMetadata::BuildInfo(_)) | None => Value::Null,
    }
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn diagnostic_category(category: DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::Warning => "Warning",
        DiagnosticCategory::Error => "Error",
        DiagnosticCategory::Suggestion => "Suggestion",
        DiagnosticCategory::Message => "Message",
    }
}

fn actual_diagnostic_value(diagnostic: &Diagnostic) -> Value {
    let mut message = String::new();
    flatten_message_chain(&diagnostic.message, 0, &mut message);
    json!({
        "code": diagnostic.code(),
        "category": diagnostic_category(diagnostic.category()),
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "message": message,
    })
}

fn actual_metadata_diagnostics(write: &EmitArtifact) -> Value {
    match write.metadata() {
        Some(EmitWriteMetadata::Text(metadata)) => Value::Array(
            metadata
                .diagnostics()
                .iter()
                .map(actual_diagnostic_value)
                .collect(),
        ),
        Some(EmitWriteMetadata::BuildInfo(_)) | None => Value::Null,
    }
}

fn actual_kind(kind: EmitArtifactKind) -> &'static str {
    match kind {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::JavaScriptMap => "source-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::BuildInfo => "build-info",
    }
}

fn expected_kind(write: &Value) -> &'static str {
    match write["kind"].as_str().expect("frozen write kind") {
        "javascript" | "mjs" | "cjs" | "jsx" => "javascript",
        "source-map" => "source-map",
        "declaration" => "declaration",
        "declaration-map" => "declaration-map",
        other => panic!("unsupported frozen write kind {other:?}"),
    }
}

fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').collect()
    }
}

fn changed_text<'line>(lines: &'line [&'line str], start: usize, end: usize) -> String {
    lines[start..end].concat()
}

fn push_prefixed(output: &mut String, prefix: char, line: &str) {
    output.push(prefix);
    output.push_str(line);
    if !line.ends_with('\n') {
        output.push('\n');
    }
}

#[derive(Clone, Debug)]
struct Hunk {
    diff: String,
    expected_start: usize,
    actual_start: usize,
    expected_count: usize,
    actual_count: usize,
    removed_lines: usize,
    added_lines: usize,
    bucket: String,
}

/// First-hunk triage only.  These labels are evidence buckets, not ownership
/// decisions; the packet documents the upstream and Rust sites as hypotheses.
fn classify_hunk(changed: &str) -> String {
    let lower = changed.to_ascii_lowercase();
    if lower.contains("fx<>") || lower.contains("array<>") {
        return "instantiation-expression-transform".to_owned();
    }
    if lower.contains("constructor([[")
        || lower.contains("constructor([{ ")
        || lower.contains("constructor({")
    {
        return "binding-pattern-transform".to_owned();
    }
    if lower.contains("exports.test") {
        return "shorthand-property-transform".to_owned();
    }
    if lower.contains("name_of_foo") || (lower.contains("define(") && lower.contains("require(")) {
        return "amd-module-specifier".to_owned();
    }
    if lower.contains("exports_1") {
        return "system-module-wrapper".to_owned();
    }
    if lower.contains("assert") || lower.contains("with {") || lower.contains("with{") {
        return "import-assertion-attribute-transform".to_owned();
    }
    if lower.contains("export * as") || lower.contains("import * as") {
        return "module-reexport-shape".to_owned();
    }
    if lower.contains("define([") {
        return "amd-module-wrapper".to_owned();
    }
    if changed.contains("/*")
        || changed.contains("*/")
        || changed
            .lines()
            .any(|line| line.trim_start().starts_with('*'))
        || changed.lines().any(|line| line.contains("//"))
    {
        return "comment-emission".to_owned();
    }
    if lower.contains("object.defineproperty")
        || lower.contains("exports.")
        || lower.contains("export=")
    {
        return "module-namespace-wrapper".to_owned();
    }
    if lower.contains("function") || lower.contains("require(") || lower.contains("import(") {
        return "javascript-transform-text".to_owned();
    }
    "javascript-printer-text".to_owned()
}

fn first_unified_hunk(
    expected_name: &str,
    actual_name: &str,
    expected: &str,
    actual: &str,
) -> Option<Hunk> {
    if expected == actual {
        return None;
    }
    let expected_lines = lines(expected);
    let actual_lines = lines(actual);
    let first = expected_lines
        .iter()
        .zip(&actual_lines)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected_lines.len().min(actual_lines.len()));
    let mut expected_end = expected_lines.len();
    let mut actual_end = actual_lines.len();
    let mut suffix = 0;
    while expected_end > first + suffix
        && actual_end > first + suffix
        && expected_lines[expected_end - suffix - 1] == actual_lines[actual_end - suffix - 1]
    {
        suffix += 1;
    }
    expected_end -= suffix;
    actual_end -= suffix;
    let expected_start = first.saturating_sub(3);
    let actual_start = first.min(actual_lines.len()).saturating_sub(3);
    let expected_context_end = (expected_end + 3).min(expected_lines.len());
    let actual_context_end = (actual_end + 3).min(actual_lines.len());
    let removed = changed_text(&expected_lines, first.min(expected_end), expected_end);
    let added = changed_text(&actual_lines, first.min(actual_end), actual_end);
    let bucket = classify_hunk(&format!("{removed}\n{added}"));
    let mut diff = format!(
        "--- {expected_name}\n+++ {actual_name}\n@@ -{},{} +{},{} @@\n",
        expected_start + 1,
        expected_context_end.saturating_sub(expected_start),
        actual_start + 1,
        actual_context_end.saturating_sub(actual_start),
    );
    for line in &expected_lines[expected_start..first.min(expected_context_end)] {
        push_prefixed(&mut diff, ' ', line);
    }
    for line in &expected_lines[first.min(expected_end)..expected_end] {
        push_prefixed(&mut diff, '-', line);
    }
    for line in &actual_lines[first.min(actual_end)..actual_end] {
        push_prefixed(&mut diff, '+', line);
    }
    for line in &expected_lines[expected_end..expected_context_end] {
        push_prefixed(&mut diff, ' ', line);
    }
    Some(Hunk {
        diff,
        expected_start: expected_start + 1,
        actual_start: actual_start + 1,
        expected_count: expected_context_end.saturating_sub(expected_start),
        actual_count: actual_context_end.saturating_sub(actual_start),
        removed_lines: expected_end.saturating_sub(first),
        added_lines: actual_end.saturating_sub(first),
        bucket,
    })
}

fn hunk_value(path: &str, hunk: &Hunk) -> Value {
    json!({
        "path": path,
        "expected_start": hunk.expected_start,
        "actual_start": hunk.actual_start,
        "expected_count": hunk.expected_count,
        "actual_count": hunk.actual_count,
        "removed_lines": hunk.removed_lines,
        "added_lines": hunk.added_lines,
        "bucket": hunk.bucket,
    })
}

fn write_facet_record(
    path: &str,
    expected: Option<&Value>,
    actual: Option<&EmitArtifact>,
) -> (bool, Value, Option<Hunk>) {
    let Some(expected) = expected else {
        let actual = actual.expect("actual write for path union");
        let text = actual.callback_text().to_owned();
        let hunk = first_unified_hunk(
            &format!("frozen:{path} (missing)"),
            &format!("rust:{path}"),
            "",
            &text,
        );
        return (
            true,
            json!({
                "frozen_present": false,
                "rust_present": true,
                "callback_bytes_equal": false,
                "materialized_bytes_equal": false,
                "write_byte_order_mark_equal": false,
                "kind_equal": false,
                "source_files_equal": false,
                "data_present_equal": false,
                "source_map_url_pos_equal": false,
                "data_diagnostics_equal": false,
                "rust_callback_bytes": actual.callback_bytes().len(),
                "rust_materialized_bytes": actual.materialized_bytes().len(),
            }),
            hunk,
        );
    };
    let Some(actual) = actual else {
        let expected_callback = decode(&expected["callback_utf8_base64"]);
        let expected_text =
            String::from_utf8(expected_callback).expect("frozen UTF-8 callback text");
        let hunk = first_unified_hunk(
            &format!("frozen:{path}"),
            &format!("rust:{path} (missing)"),
            &expected_text,
            "",
        );
        return (
            true,
            json!({
                "frozen_present": true,
                "rust_present": false,
                "callback_bytes_equal": false,
                "materialized_bytes_equal": false,
                "write_byte_order_mark_equal": false,
                "kind_equal": false,
                "source_files_equal": false,
                "data_present_equal": false,
                "source_map_url_pos_equal": false,
                "data_diagnostics_equal": false,
                "frozen_callback_bytes": expected_text.len(),
                "frozen_materialized_bytes": decode(&expected["materialized_utf8_base64"]).len(),
            }),
            hunk,
        );
    };

    let expected_callback = decode(&expected["callback_utf8_base64"]);
    let expected_materialized = decode(&expected["materialized_utf8_base64"]);
    let actual_materialized = actual.materialized_bytes();
    let expected_sources = expected["source_files"].clone();
    let actual_sources = actual_source_files(actual);
    let expected_url = expected["data_source_map_url_pos"].clone();
    let actual_url = actual_metadata_url(actual);
    let expected_data_diagnostics = expected["data_diagnostics"].clone();
    let actual_data_diagnostics = actual_metadata_diagnostics(actual);
    let callback_equal = expected_callback == actual.callback_bytes();
    let materialized_equal = expected_materialized == actual_materialized.as_ref();
    let bom_equal =
        expected["write_byte_order_mark"].as_bool() == Some(actual.write_byte_order_mark());
    let kind_equal = expected_kind(expected) == actual_kind(actual.kind());
    let sources_equal = expected_sources == actual_sources;
    let data_present_equal =
        expected["data_present"].as_bool() == Some(actual.metadata().is_some());
    let url_equal = expected_url == actual_url;
    let diagnostics_equal = expected_data_diagnostics == actual_data_diagnostics;
    let diverging = [
        callback_equal,
        materialized_equal,
        bom_equal,
        kind_equal,
        sources_equal,
        data_present_equal,
        url_equal,
        diagnostics_equal,
    ]
    .iter()
    .any(|equal| !equal);
    let expected_text = String::from_utf8(expected_callback).expect("frozen UTF-8 callback text");
    let hunk = first_unified_hunk(
        &format!("frozen:{path}"),
        &format!("rust:{path}"),
        &expected_text,
        actual.callback_text(),
    );
    (
        diverging,
        json!({
            "frozen_present": true,
            "rust_present": true,
            "callback_bytes_equal": callback_equal,
            "materialized_bytes_equal": materialized_equal,
            "write_byte_order_mark_equal": bom_equal,
            "kind_equal": kind_equal,
            "source_files_equal": sources_equal,
            "data_present_equal": data_present_equal,
            "source_map_url_pos_equal": url_equal,
            "data_diagnostics_equal": diagnostics_equal,
            "frozen_callback_bytes": expected_text.len(),
            "rust_callback_bytes": actual.callback_bytes().len(),
            "frozen_materialized_bytes": expected_materialized.len(),
            "rust_materialized_bytes": actual_materialized.len(),
            "frozen_kind": expected_kind(expected),
            "rust_kind": actual_kind(actual.kind()),
            "frozen_source_files": expected_sources,
            "rust_source_files": actual_sources,
            "frozen_data_present": expected["data_present"],
            "rust_data_present": actual.metadata().is_some(),
            "frozen_data_source_map_url_pos": expected_url,
            "rust_data_source_map_url_pos": actual_url,
            "frozen_data_diagnostics": expected_data_diagnostics,
            "rust_data_diagnostics": actual_data_diagnostics,
        }),
        hunk,
    )
}

fn safe_case_file(case_id: &str) -> String {
    case_id.replace(['/', '#'], "__") + ".diff"
}

fn path_order(expected: &[Value], actual: &[EmitArtifact]) -> Vec<String> {
    let mut paths = Vec::new();
    for write in expected {
        let path = write["path"]
            .as_str()
            .expect("frozen write path")
            .to_owned();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    for write in actual {
        let path = write.path().to_string_lossy().into_owned();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn write_case_diff(
    output_dir: &Path,
    case_id: &str,
    expected_writes: &[Value],
    actual_writes: &[EmitArtifact],
) -> Value {
    assert_unique_expected_paths(expected_writes, case_id);
    assert_unique_actual_paths(actual_writes, case_id);
    let paths = path_order(expected_writes, actual_writes);
    let expected_order = expected_writes
        .iter()
        .map(|write| write["path"].as_str().expect("frozen path"))
        .collect::<Vec<_>>();
    let actual_order = actual_writes
        .iter()
        .map(|write| write.path().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut diff = format!(
        "# case_id: {case_id}\n# frozen_write_count: {}\n# rust_write_count: {}\n",
        expected_writes.len(),
        actual_writes.len()
    );
    let mut records = Vec::new();
    let mut diverging_paths = Vec::new();
    let mut first_hunk = None;
    let mut first_bucket = None;
    let mut frozen_all_callback = 0usize;
    let mut rust_all_callback = 0usize;
    let mut frozen_all_materialized = 0usize;
    let mut rust_all_materialized = 0usize;
    let mut frozen_non_declaration_callback = 0usize;
    let mut rust_non_declaration_callback = 0usize;

    for path in &paths {
        let expected = expected_write(expected_writes, path);
        let actual = actual_write(actual_writes, path);
        if let Some(write) = expected {
            frozen_all_callback += decode(&write["callback_utf8_base64"]).len();
            frozen_all_materialized += decode(&write["materialized_utf8_base64"]).len();
            if !is_declaration_path(path) {
                frozen_non_declaration_callback += decode(&write["callback_utf8_base64"]).len();
            }
        }
        if let Some(write) = actual {
            rust_all_callback += write.callback_bytes().len();
            rust_all_materialized += write.materialized_bytes().len();
            if !is_declaration_path(path) {
                rust_non_declaration_callback += write.callback_bytes().len();
            }
        }
        let (diverging, facets, hunk) = write_facet_record(path, expected, actual);
        if diverging {
            diverging_paths.push(path.clone());
            diff.push_str(&format!("\n## diverging write: {path}\n"));
            if let Some(hunk) = &hunk {
                diff.push('\n');
                diff.push_str(&hunk.diff);
                if first_hunk.is_none() {
                    first_bucket = Some(hunk.bucket.clone());
                    first_hunk = Some(hunk_value(path, hunk));
                }
            } else {
                diff.push_str("# callback text equal; divergence is an observable write facet\n");
            }
        }
        records.push(json!({
            "path": path,
            "frozen_index": expected.and_then(|write| write["index"].as_u64()),
            "rust_index": actual.and_then(|_write| actual_writes.iter().position(|candidate| candidate.path() == Path::new(path)).map(|index| index as u64)),
            "diverging": diverging,
            "facets": facets,
            "callback_hunk": hunk.as_ref().map(|hunk| hunk_value(path, hunk)),
        }));
    }

    assert!(
        diverging_paths
            .iter()
            .all(|path| !is_declaration_path(path)),
        "{case_id}: a declaration write diverges in the W1 declaration-text-exact population"
    );
    assert!(
        !diverging_paths.is_empty(),
        "{case_id}: selected W1 row has no observable write divergence"
    );
    diff.push_str(&format!(
        "\n# write_order_equal: {}\n",
        expected_order
            .iter()
            .map(|path| (*path).to_owned())
            .eq(actual_order.iter().cloned())
    ));
    let diff_file = safe_case_file(case_id);
    fs::write(output_dir.join(&diff_file), diff).expect("write per-case W2 census diff");
    json!({
        "w1_bucket": "declaration-text-exact",
        "diff_file": diff_file,
        "diverging_paths": diverging_paths,
        "diverging_write_count": records.iter().filter(|record| record["diverging"] == true).count(),
        "first_hunk": first_hunk,
        "bucket": first_bucket.unwrap_or_else(|| "write-facet-only".to_owned()),
        "write_order_equal": expected_order.iter().map(|path| (*path).to_owned()).eq(actual_order.iter().cloned()),
        "frozen_write_count": expected_writes.len(),
        "rust_write_count": actual_writes.len(),
        "byte_counts": {
            "frozen_all_callback": frozen_all_callback,
            "rust_all_callback": rust_all_callback,
            "frozen_all_materialized": frozen_all_materialized,
            "rust_all_materialized": rust_all_materialized,
            "frozen_non_declaration_callback": frozen_non_declaration_callback,
            "rust_non_declaration_callback": rust_non_declaration_callback,
        },
        "writes": records,
    })
}

#[test]
#[ignore = "W2-E evidence instrument; run explicitly to refresh target/w2-census"]
fn h2_7b_w2_write_census() {
    let root = workspace_root();
    let manifest = load_json(&root, MANIFEST_RELATIVE_PATH);
    let qualification = load_json(&root, QUALIFICATION_RELATIVE_PATH);
    let rows = selected_manifest_rows(&manifest);
    assert_eq!(
        rows.len(),
        34,
        "the frozen W1 declaration-text-exact population"
    );

    let output_dir = root.join("target/w2-census");
    fs::create_dir_all(&output_dir).expect("create W2 census output directory");
    let mut cases = Map::new();
    let mut bucket_counts = BTreeMap::<String, usize>::new();
    for row in rows {
        let case_id = row["case_id"].as_str().expect("manifest case id");
        let case = qualification_case(&qualification, case_id);
        assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
        let prepared = prepared_band_row(&root, case);
        let mut sink = MemoryOutputSink::new();
        let (outcome, reported) = ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .unwrap_or_else(|error| panic!("{case_id}: production emit completes: {error}"));
        let expected_writes = case["typescript_observation"]["writes"]
            .as_array()
            .expect("frozen writes")
            .clone();
        let mut record = write_case_diff(&output_dir, case_id, &expected_writes, sink.writes());
        record["mismatch_vector"] = row["mismatch_vector"].clone();
        record["manifest_writes_diverging"] = row["writes_diverging"].clone();
        record["frozen_reported_diagnostic_count"] = json!(case["typescript_observation"]
            ["reported_diagnostics"]
            .as_array()
            .expect("frozen reported diagnostics")
            .len());
        record["rust_reported_diagnostic_count"] = json!(reported.len());
        record["frozen_emit_skipped"] =
            case["typescript_observation"]["emit_result"]["emit_skipped"].clone();
        record["rust_emit_skipped"] = json!(outcome.emit_skipped());
        let bucket = record["bucket"].as_str().expect("census bucket").to_owned();
        *bucket_counts.entry(bucket).or_default() += 1;
        cases.insert(case_id.to_owned(), record);
    }

    let summary = json!({
        "schema": "h2-7b-w2-write-census.v1",
        "source": {
            "manifest": MANIFEST_RELATIVE_PATH,
            "qualification": QUALIFICATION_RELATIVE_PATH,
            "w1_reference": "target/session-notes/7b/w1/census.md:53-61 (canonical read-only checkout)",
            "entry": "load_qualified_compiler_emit_with_option_floor(..., EmitOptionFloor::DeclarationFamily) + ProgramSession::new(prepared).emit_with_reported_diagnostics_for_harness",
        },
        "selection": {
            "rows": DECLARATION_TEXT_EXACT_CASE_IDS.len(),
            "case_ids": DECLARATION_TEXT_EXACT_CASE_IDS,
            "predicate": "fixed W1 declaration-text-exact IDs; manifest writes_diverging > 0, diagnostics_diverging = false, emit_result_diverging = false",
        },
        "rows": cases.len(),
        "bucket_counts": bucket_counts,
        "cases": cases,
    });
    fs::write(
        output_dir.join("summary.json"),
        serde_json::to_vec_pretty(&summary).expect("serialize W2 census summary"),
    )
    .expect("write W2 census summary");
}
