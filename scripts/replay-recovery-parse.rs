//! Standalone probe compiled with the identical digest source against the
//! candidate, the restored pre-interface projection, and the main merge base.
#[path = "../crates/xtask/src/recovery_parse_snapshot.rs"]
mod snapshot;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};

fn main() {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "emitter-recovery-parse-snapshot");
    let digest_hash = snapshot::sha256(include_bytes!(
        "../crates/xtask/src/recovery_parse_snapshot.rs"
    ));
    assert_eq!(artifact["digest_code_sha256"], digest_hash);
    let mut digests = BTreeMap::new();
    let mut recovery_digests = BTreeMap::new();
    let mut legacy_digests = BTreeMap::new();
    let mut keyword_actions = BTreeMap::new();
    let mut terminator_reports = BTreeMap::new();
    for (id, input) in artifact["inputs"].as_object().unwrap() {
        assert_eq!(id, input["input_id"].as_str().unwrap());
        let source = snapshot::replay(input);
        recovery_digests.insert(id.clone(), recovery_digest(&source));
        let (legacy, actions) = keyword_extension(&source);
        legacy_digests.insert(id.clone(), legacy);
        keyword_actions.insert(id.clone(), actions);
        terminator_reports.insert(id.clone(), retained_statement_terminators(&source));
        assert!(digests
            .insert(id.clone(), json!({"core":snapshot::digest(&source),"profiles":profiles(&source)}))
            .is_none());
    }
    let result = json!({"schema":1,"kind":"emitter-recovery-parse-replay",
        "input_artifact_sha256":snapshot::sha256(bytes),"digest_code_sha256":digest_hash,
        "digests":digests, "recovery_facts_format":"rust-debug-ParseRecovery-v1",
        "recovery_facts_sha256":recovery_digests,
        "recovery_extension_format":"escaped-keyword-consumed-v1",
        "legacy_recovery_facts_sha256":legacy_digests,
        "escaped_keyword_actions":keyword_actions, "retained_statement_terminator_reports":terminator_reports});
    serde_json::to_writer(std::io::stdout().lock(), &result).unwrap();
    std::io::stdout().write_all(b"\n").unwrap();
}

// The original digest schema and original observations remain untouched.
// Reconstruct its exact Debug value after removing only the additive action.
#[cfg(feature = "current-recovery-profiles")]
fn keyword_extension(source: &tsc_syntax::SourceFile) -> (Value, Value) {
    use tsc_syntax::{ParseRecoveryAction, ParseRecoveryKind, ParseDiagnosticOrigin};
    let recovery = source.parse_recovery();
    let mut structural = Vec::new();
    let mut facts = Vec::new();
    for action in recovery.actions() {
        match *action {
            ParseRecoveryAction::EscapedKeywordConsumed { token, start, length, statement_start } => {
                assert!(length > 0 && statement_start <= start);
                let end = start.checked_add(length).unwrap();
                assert!(source.positions().utf16_to_byte(start).is_some());
                assert!(source.positions().utf16_to_byte(end).is_some());
                let matching = recovery.events().iter().filter(|event|
                    event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                        && event.start == start && event.length == length
                        && event.diagnostic_index.is_some_and(|index| source.parse_diagnostics[index].code() == 1260)).count();
                assert_eq!(matching, 1, "consumed keyword must retain its own reporting event");
                assert_eq!(recovery.events().iter().filter(|event|
                    event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                        && event.diagnostic_index.is_some() && event.start == start && event.length == length).count(),
                    1, "keyword span must not justify another retained report");
                facts.push(json!({"token":token as u16,"start":start,"length":length,
                    "statement_start":statement_start,"matching_report_events":matching}));
            }
            ParseRecoveryAction::TokenSkipped { .. } | ParseRecoveryAction::Reparsed { .. } => structural.push(action),
        }
    }
    assert_eq!(facts.len(), source.parse_diagnostics.iter().filter(|diagnostic| diagnostic.code() == 1260).count(),
        "every retained escaped-keyword report has exactly one consumption fact");
    let legacy = format!("ParseRecovery {{ diagnostic_origins: {:?}, events: {:?}, actions: {:?} }}",
        recovery.diagnostic_origins(), recovery.events(), structural);
    if facts.is_empty() { assert_eq!(legacy, format!("{recovery:?}")); }
    (json!(snapshot::sha256(legacy)), json!(facts))
}

#[cfg(not(feature = "current-recovery-profiles"))]
fn keyword_extension(_source: &tsc_syntax::SourceFile) -> (Value, Value) { (Value::Null, Value::Null) }

// Independently identify the retained expression plus its own terminator.
// This is evidence classification only; production admission remains in syntax.
#[cfg(feature = "current-recovery-profiles")]
fn retained_statement_terminators(source: &tsc_syntax::SourceFile) -> Value {
    use tsc_syntax::{NodeData, ParseRecoveryKind, ParseDiagnosticOrigin};
    use std::collections::BTreeSet;
    if source.parse_recovery().events().is_empty() { return json!([]); }
    let mut pending = vec![source.root];
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if reachable.insert(id) {
            tsc_syntax::for_each_child(&source.arena, source.arena.node(id), |child| { pending.push(child); false });
        }
    }
    let mut reports = Vec::new();
    for event in source.parse_recovery().events() {
        if event.kind != ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
            || event.diagnostic_index.is_none() || event.missing_node.is_some() || event.length == 0 { continue; }
        let Some(start) = source.positions().utf16_to_byte(event.start) else { continue; };
        let Some(end) = event.start.checked_add(event.length).and_then(|end| source.positions().utf16_to_byte(end)) else { continue; };
        let owners: Vec<_> = reachable.iter().filter_map(|id| {
            let node = source.arena.node(*id);
            let NodeData::ExpressionStatement(data) = &node.data else { return None; };
            let expression = source.arena.node(data.expression?);
            let semicolon = node.end.checked_sub(1)?;
            (expression.end == end && node.end != end
                && tsc_syntax::skip_trivia(source.text(), node.pos as usize) == start as usize
                && tsc_syntax::skip_trivia(source.text(), end as usize) == semicolon as usize
                && source.text().as_bytes().get(semicolon as usize) == Some(&b';'))
                .then(|| json!({"statement_pos":node.pos,"statement_end":node.end,"expression_end":expression.end}))
        }).collect();
        if owners.len() == 1 { reports.push(json!({"start":event.start,"length":event.length,"owner":owners[0]})); }
    }
    json!(reports)
}

#[cfg(not(feature = "current-recovery-profiles"))]
fn retained_statement_terminators(_source: &tsc_syntax::SourceFile) -> Value { Value::Null }

// This independent digest includes committed events, suppressed reports and
// skip/reparse actions. It leaves the census's parse-graph digest unchanged.
#[cfg(feature = "current-recovery-profiles")]
fn recovery_digest(source: &tsc_syntax::SourceFile) -> Value {
    json!(snapshot::sha256(format!("{:?}", source.parse_recovery())))
}

#[cfg(not(feature = "current-recovery-profiles"))]
fn recovery_digest(_source: &tsc_syntax::SourceFile) -> Value { Value::Null }

#[cfg(feature = "current-recovery-profiles")]
fn profiles(source: &tsc_syntax::SourceFile) -> Value {
    json!({"literal":source.has_only_literal_recovery(),
        "missing_await":source.has_only_literal_or_missing_await_recovery(),
        "missing_declaration":source.has_only_missing_node_emit_recovery(),
        "parameter_gaps":source.has_only_parameter_gap_emit_recovery(),
        "statement_gaps":source.has_only_statement_gap_emit_recovery(),
        "context_recovery":source.has_supported_emit_recovery()})
}

#[cfg(not(feature = "current-recovery-profiles"))]
fn profiles(_source: &tsc_syntax::SourceFile) -> Value { Value::Null }

#[cfg(test)]
mod tests {
    #[test]
    fn original_census_input_identity_survives_standalone_build() {
        // Verbatim first input from the completed r78 snapshot (SHA256
        // 1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75).
        // A round trip generated by this binary alone would miss a differing
        // serde_json map representation, which changes input and graph hashes.
        let input = serde_json::from_str(r#"{
            "input_id":"0000b94d3e3d22ccf57b0164a10a575183f316f79f9f0f039e0260941a85dfda",
            "file_name":"/.src/1.ts",
            "text_utf8_base64":"ZXhwb3J0ICogYXMgbnMgZnJvbSAnLi8wJzsKbnMuYTsKbnMuYjsKbGV0IG5zID0ge2E6IDEsIGI6IDJ9Cm5zLmE7Cm5zLmI7Cg==",
            "text_sha256":"47c399190d545283d78fdf230adca597e6ac9797ade5fbc9bf3c65b5440bf73f",
            "text_utf8_bytes":73,"utf16_len":73,
            "options":{"script_target":2,"language_variant":0,"javascript_file":false,
                "js_doc_parsing_mode":0,"force_external_module":false,
                "detect_external_module_from_jsx":false,"node_id_base":0,"node_array_id_base":0}
        }"#).unwrap();
        super::snapshot::replay(&input);
    }
}
