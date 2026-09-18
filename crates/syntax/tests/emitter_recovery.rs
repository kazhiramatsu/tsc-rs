use serde_json::{json, Value};
use tsc_syntax::{
    for_each_child, JSDocParsingMode, LanguageVariant, NodeId, ParseOptions, ParseRecoveryAction,
    SourceFile, SyntaxKind,
};
use tsc_types::{NodeFlags, ScriptTarget};

fn missing_nodes(
    source: &SourceFile,
    id: NodeId,
    parent: Option<SyntaxKind>,
    rows: &mut Vec<Value>,
) {
    let node = source.arena.node(id);
    if NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC) {
        return;
    }
    if node.kind == SyntaxKind::MissingDeclaration
        || node.pos == node.end
            && !matches!(
                node.kind,
                SyntaxKind::SourceFile | SyntaxKind::EndOfFileToken
            )
    {
        rows.push(json!({
            "kind": node.kind as u16,
            "pos": source.positions().byte_to_utf16(node.pos).unwrap(),
            "end": source.positions().byte_to_utf16(node.end).unwrap(),
            "parent": parent.map(|kind| kind as u16),
        }));
    }
    for_each_child(&source.arena, node, |child| {
        missing_nodes(source, child, Some(node.kind), rows);
        false
    });
}

#[test]
fn original_emit_recovery_rows_preserve_typescript_syntax_and_committed_facts() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/emitter-recovery.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 36);
    let mut facts = Vec::new();
    for case in cases {
        let mut file_facts = Vec::new();
        for file in case["files"].as_array().unwrap() {
            let path = file["path"].as_str().unwrap();
            let source = tsc_syntax::parse_source_file(
                path,
                file["text"].as_str().unwrap(),
                ParseOptions {
                    script_target: ScriptTarget::from_bits(case["target"].as_i64().unwrap() as i32),
                    javascript_file: path.ends_with(".js") || path.ends_with(".jsx"),
                    language_variant: if path.ends_with(".tsx") || path.ends_with(".jsx") {
                        LanguageVariant::Jsx
                    } else {
                        LanguageVariant::Standard
                    },
                    js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                    ..ParseOptions::default()
                },
                None,
            );
            let mut missing = Vec::new();
            missing_nodes(&source, source.root, None, &mut missing);
            let diagnostics: Vec<_> = source.parse_diagnostics.iter().map(|diagnostic| {
                assert!(diagnostic.message.next.is_empty());
                json!({
                    "code": diagnostic.code(), "start": diagnostic.start, "length": diagnostic.length,
                    "message_utf16": diagnostic.message.text.to_utf16(),
                })
            }).collect();
            assert_eq!(
                json!({"missing": missing, "diagnostics": diagnostics}),
                file["expected"],
                "{}: {path}",
                case["case_id"]
            );
            // Data-only provenance must not weaken the existing literal boundary.
            assert!(!source.has_only_literal_recovery());
            let recovery = source.parse_recovery();
            let events: Vec<_> = recovery.events().iter().map(|event| json!({
                "kind": format!("{:?}", event.kind), "start": event.start, "length": event.length,
                "diagnostic_index": event.diagnostic_index, "full_start": event.full_start,
                "missing_node": event.missing_node.map(|node| json!({"kind": node.kind as u16, "position": node.position})),
            })).collect();
            let actions: Vec<_> = recovery.actions().iter().map(|action| match *action {
                ParseRecoveryAction::TokenSkipped { token, start, length, statement_start, site } =>
                    json!({"kind": "token-skipped", "token": token as u16, "start": start,
                        "length": length, "statement_start": statement_start, "site": format!("{site:?}")}),
                ParseRecoveryAction::Reparsed { start, end } => json!({"kind": "reparsed", "start": start, "end": end}),
            }).collect();
            file_facts.push(json!({"path": path, "events": events, "actions": actions, "missing_nodes": missing}));
        }
        facts.push(json!({"case_id": case["case_id"], "files": file_facts}));
    }
    if let Some(path) = std::env::var_os("TSC_RS_RECOVERY_FACTS_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({
            "schema": 1, "purpose": "syntax-provenance-only-not-emit-compatibility", "cases": facts,
        })).unwrap()).unwrap();
    }
    eprintln!("emitter recovery syntax SUMMARY exact=36 failed=0 selected=36");
    assert_context_control_admission();
}

fn assert_context_control_admission() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../compiler/tests/fixtures/emitter-context-recovery.json"
    ))
    .unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    for (key, admitted, count) in [("cases", true, 432), ("refused_cases", false, 72)] {
        let cases = fixture[key].as_array().unwrap();
        assert_eq!(cases.len(), count);
        for case in cases {
            let config: Value = serde_json::from_str(case["config"].as_str().unwrap()).unwrap();
            let target = match config["compilerOptions"]["target"].as_str().unwrap() {
                "es5" => ScriptTarget::ES5,
                "es2015" => ScriptTarget::ES2015,
                "esnext" => ScriptTarget::ESNext,
                other => panic!("unexpected context-control target: {other}"),
            };
            assert_eq!(case["typescript_observation"]["emit_refused"], false);
            for file in case["files"].as_array().unwrap() {
                let source = tsc_syntax::parse_source_file(
                    file["path"].as_str().unwrap(),
                    file["text"].as_str().unwrap(),
                    ParseOptions {
                        script_target: target,
                        js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                        ..ParseOptions::default()
                    },
                    None,
                );
                assert_eq!(
                    source.has_supported_emit_recovery(),
                    admitted,
                    "{}: recovery={:?}",
                    case["case_id"],
                    source.parse_recovery()
                );
            }
        }
        eprintln!("context recovery syntax {key}: {count} expected admission={admitted}");
    }
}
