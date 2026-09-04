//! W1-E's ignored byte-diff census for the 132 H2.7b writes-only rows.
//!
//! This is deliberately self-contained.  It replays the qualified-VFS
//! declaration-family entry used by the H2.7b m-2 controls, then writes the
//! first unified declaration hunk for each row under `target/w1-census`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Map, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";
const MANIFEST_RELATIVE_PATH: &str = "ratchets/h2-7b-known-divergences.v1.json";

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

/// Copied from the m-2 controls: the qualified-VFS row loader intentionally
/// follows the production declaration-family route, including its option
/// floor and virtual-config placement.
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

fn writes_only_rows<'artifact>(artifact: &'artifact Value) -> Vec<&'artifact Value> {
    artifact["cases"]
        .as_array()
        .expect("manifest cases")
        .iter()
        .filter(|case| {
            case["writes_diverging"].as_u64().unwrap_or(0) > 0
                && case["diagnostics_diverging"].as_bool() == Some(false)
                && case["emit_result_diverging"].as_bool() == Some(false)
        })
        .collect()
}

fn qualification_case<'artifact>(artifact: &'artifact Value, case_id: &str) -> &'artifact Value {
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"].as_str() == Some(case_id))
        .unwrap_or_else(|| panic!("{case_id} is absent from the qualification artifact"))
}

fn callback_text(write: &Value) -> String {
    String::from_utf8(decode(&write["callback_utf8_base64"]))
        .expect("frozen declaration callback is UTF-8")
}

fn declaration_writes<'write>(writes: &'write [Value]) -> Vec<&'write Value> {
    writes
        .iter()
        .filter(|write| {
            write["kind"].as_str() == Some("declaration")
                || write["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(".d.ts"))
        })
        .collect()
}

fn actual_declaration_writes<'write>(
    sink: &'write MemoryOutputSink,
) -> Vec<&'write tsc_emitter::EmitArtifact> {
    sink.writes()
        .iter()
        .filter(|write| is_declaration_path(&write.path().to_string_lossy()))
        .collect()
}

fn is_declaration_path(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}

fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').collect()
    }
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

fn changed_text<'line>(lines: &'line [&'line str], start: usize, end: usize) -> String {
    lines[start..end].concat()
}

fn classify_hunk(expected: &str, actual: &str, removed: &str, added: &str) -> String {
    let changed = format!("{removed}\n{added}");
    let trim = |line: &str| line.trim().to_owned();
    let removed_lines = removed.lines().map(trim).collect::<Vec<_>>();
    let added_lines = added.lines().map(trim).collect::<Vec<_>>();
    let mut removed_sorted = removed_lines.clone();
    let mut added_sorted = added_lines.clone();
    removed_sorted.sort();
    added_sorted.sort();

    if removed_lines.is_empty() || added_lines.is_empty() {
        return "missing-or-extra-members".to_owned();
    }
    if removed_sorted == added_sorted && removed_lines != added_lines {
        return "ordering".to_owned();
    }
    if changed.contains("@someDec")
        || changed.contains("static [x: string]: any;")
        || changed.contains("namespace m_private")
        || (removed_lines.len() >= 4 && removed_lines.len() >= added_lines.len() * 2)
    {
        return "missing-or-extra-members".to_owned();
    }
    if changed.contains("import(\"") || changed.contains("import('") {
        return "import()-qualification".to_owned();
    }
    if changed.contains("/*")
        || changed.contains("*/")
        || changed
            .lines()
            .any(|line| line.trim_start().starts_with('*'))
        || changed
            .lines()
            .any(|line| line.trim_start().starts_with("//"))
    {
        return "comment-reuse".to_owned();
    }
    if [
        "public ",
        "private ",
        "protected ",
        "readonly ",
        "abstract ",
        "static ",
        "override ",
    ]
    .iter()
    .any(|token| changed.contains(token))
        || changed.contains("!:")
    {
        return "modifiers".to_owned();
    }
    if expected.contains('\r') != actual.contains('\r')
        || (expected.ends_with('\n') != actual.ends_with('\n'))
    {
        return "line-ending-or-final-newline".to_owned();
    }
    "type-printing-or-member-shape".to_owned()
}

/// Emit the first differing hunk with three lines of context.  This mirrors
/// the repository's existing replay-test convention: the census is about the
/// first causal-looking difference, while the complete frozen and Rust byte
/// counts remain in summary.json.
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
    let start = first.saturating_sub(3);
    let expected_context_end = (expected_end + 3).min(expected_lines.len());
    let actual_context_end = (actual_end + 3).min(actual_lines.len());
    let removed = changed_text(&expected_lines, first.min(expected_end), expected_end);
    let added = changed_text(&actual_lines, first.min(actual_end), actual_end);
    let bucket = classify_hunk(expected, actual, &removed, &added);
    let mut diff = format!(
        "--- {expected_name}\n+++ {actual_name}\n@@ -{},{} +{},{} @@\n",
        start + 1,
        expected_context_end.saturating_sub(start),
        start + 1,
        actual_context_end.saturating_sub(start),
    );
    for line in &expected_lines[start..first.min(expected_context_end)] {
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
        expected_start: start + 1,
        actual_start: start + 1,
        expected_count: expected_context_end.saturating_sub(start),
        actual_count: actual_context_end.saturating_sub(start),
        removed_lines: expected_end.saturating_sub(first),
        added_lines: actual_end.saturating_sub(first),
        bucket,
    })
}

fn safe_case_file(case_id: &str) -> String {
    case_id.replace('/', "__").replace('#', "__") + ".diff"
}

fn output_write_path(write: &tsc_emitter::EmitArtifact) -> String {
    write.path().to_string_lossy().into_owned()
}

fn write_case_diff(
    output_dir: &Path,
    case_id: &str,
    expected_writes: &[&Value],
    actual_writes: &[&tsc_emitter::EmitArtifact],
) -> Value {
    let expected_declarations = expected_writes
        .iter()
        .filter(|write| {
            write["kind"].as_str() == Some("declaration")
                || write["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(".d.ts"))
        })
        .copied()
        .collect::<Vec<_>>();
    let actual_declarations = actual_writes.to_vec();
    let mut path_order = expected_declarations
        .iter()
        .filter_map(|write| write["path"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    for write in &actual_declarations {
        let path = output_write_path(write);
        if !path_order.contains(&path) {
            path_order.push(path);
        }
    }

    let mut diff = format!("# case_id: {case_id}\n");
    let mut first_hunk: Option<Value> = None;
    let mut first_bucket = None;
    let mut hunk_count = 0;
    let mut declaration_bytes_frozen = 0usize;
    let mut declaration_bytes_rust = 0usize;
    let mut declaration_records = Vec::new();

    for path in path_order {
        let frozen = expected_declarations
            .iter()
            .find(|write| write["path"].as_str() == Some(path.as_str()))
            .map(|write| callback_text(write));
        let rust = actual_declarations
            .iter()
            .find(|write| output_write_path(write) == path)
            .map(|write| write.callback_text().to_owned());
        declaration_bytes_frozen += frozen.as_ref().map_or(0, String::len);
        declaration_bytes_rust += rust.as_ref().map_or(0, String::len);
        let hunk = match (&frozen, &rust) {
            (Some(frozen), Some(rust)) => first_unified_hunk(
                &format!("frozen:{path}"),
                &format!("rust:{path}"),
                frozen,
                rust,
            ),
            (Some(frozen), None) => first_unified_hunk(
                &format!("frozen:{path}"),
                &format!("rust:{path} (missing)"),
                frozen,
                "",
            ),
            (None, Some(rust)) => first_unified_hunk(
                &format!("frozen:{path} (missing)"),
                &format!("rust:{path}"),
                "",
                rust,
            ),
            (None, None) => None,
        };
        if let Some(hunk) = hunk {
            hunk_count += 1;
            if first_hunk.is_none() {
                first_bucket = Some(hunk.bucket.clone());
                first_hunk = Some(json!({
                    "path": path.clone(),
                    "expected_start": hunk.expected_start,
                    "actual_start": hunk.actual_start,
                    "expected_count": hunk.expected_count,
                    "actual_count": hunk.actual_count,
                    "removed_lines": hunk.removed_lines,
                    "added_lines": hunk.added_lines,
                    "bucket": hunk.bucket,
                }));
            }
            diff.push('\n');
            diff.push_str(&hunk.diff);
        }
        declaration_records.push(json!({
            "path": path,
            "frozen_callback_bytes": frozen.as_ref().map_or(0, String::len),
            "rust_callback_bytes": rust.as_ref().map_or(0, String::len),
            "frozen_present": frozen.is_some(),
            "rust_present": rust.is_some(),
            "callback_bytes_equal": frozen == rust,
        }));
    }

    if hunk_count == 0 {
        if expected_declarations.is_empty() && actual_declarations.is_empty() {
            diff.push_str("# no frozen or Rust .d.ts write; the writes-only facet is non-declaration output\n");
            first_bucket = Some("no-declaration-write".to_owned());
        } else {
            diff.push_str("# frozen and Rust declaration callback texts are byte-identical\n");
            first_bucket = Some("declaration-text-exact".to_owned());
        }
    }
    let diff_file = safe_case_file(case_id);
    fs::write(output_dir.join(&diff_file), diff).expect("write per-case census diff");

    json!({
        "diff_file": diff_file,
        "bucket": first_bucket.expect("census bucket"),
        "first_differing_hunk": first_hunk,
        "declaration_hunk_count": hunk_count,
        "declaration_writes": declaration_records,
        "byte_counts": {
            "frozen_declaration_callback": declaration_bytes_frozen,
            "rust_declaration_callback": declaration_bytes_rust,
        },
    })
}

#[test]
#[ignore = "W1-E evidence instrument; run explicitly to refresh target/w1-census"]
fn h2_7b_w1_diff_census() {
    let root = workspace_root();
    let manifest = load_json(&root, MANIFEST_RELATIVE_PATH);
    let qualification = load_json(&root, QUALIFICATION_RELATIVE_PATH);
    let rows = writes_only_rows(&manifest);
    assert_eq!(rows.len(), 132, "the frozen writes-only population");

    let output_dir = root.join("target/w1-census");
    fs::create_dir_all(&output_dir).expect("create W1 census output directory");
    let mut cases = Map::new();
    let mut bucket_counts = BTreeMap::<String, usize>::new();

    let row_count = rows.len();
    for row in &rows {
        let case_id = row["case_id"].as_str().expect("manifest case id");
        let case = qualification_case(&qualification, case_id);
        assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
        let prepared = prepared_band_row(&root, case);
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .unwrap_or_else(|error| panic!("{case_id}: production emit completes: {error}"));

        let expected_writes = case["typescript_observation"]["writes"]
            .as_array()
            .expect("frozen writes")
            .clone();
        let actual_declarations = actual_declaration_writes(&sink);
        let expected_all_callback_bytes = expected_writes
            .iter()
            .map(|write| decode(&write["callback_utf8_base64"]).len())
            .sum::<usize>();
        let rust_all_callback_bytes = sink
            .writes()
            .iter()
            .map(|write| write.callback_text().len())
            .sum::<usize>();
        let mut record = write_case_diff(
            &output_dir,
            case_id,
            &declaration_writes(&expected_writes),
            &actual_declarations,
        );
        record["byte_counts"]["frozen_all_callback"] = json!(expected_all_callback_bytes);
        record["byte_counts"]["rust_all_callback"] = json!(rust_all_callback_bytes);
        record["byte_counts"]["frozen_write_count"] = json!(expected_writes.len());
        record["byte_counts"]["rust_write_count"] = json!(sink.writes().len());
        record["mismatch_vector"] = row["mismatch_vector"].clone();
        record["writes_diverging"] = row["writes_diverging"].clone();
        let bucket = record["bucket"].as_str().expect("bucket").to_owned();
        *bucket_counts.entry(bucket).or_default() += 1;
        cases.insert(case_id.to_owned(), record);
    }

    let summary = json!({
        "schema": "h2-7b-w1-diff-census.v1",
        "source": {
            "manifest": MANIFEST_RELATIVE_PATH,
            "qualification": QUALIFICATION_RELATIVE_PATH,
            "entry": "load_qualified_compiler_emit_with_option_floor(..., EmitOptionFloor::DeclarationFamily) + ProgramSession::new(prepared).emit_with_reported_diagnostics_for_harness",
        },
        "writes_only_rows": row_count,
        "bucket_counts": bucket_counts,
        "cases": cases,
    });
    fs::write(
        output_dir.join("summary.json"),
        serde_json::to_vec_pretty(&summary).expect("serialize W1 census summary"),
    )
    .expect("write W1 census summary");
}
