//! Direct printer metadata controls, separate from ordinary Program commands.
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, TransformArena, TransformRoot,
};
use tsc_syntax::{parse_source_file, NodeId, SyntaxKind};

#[test]
fn class_header_printer_metadata_matches_typescript() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "fixtures/class-header-token-printer-metadata.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-printer-metadata");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 32);
    let mut failures = Vec::new();
    for (case_index, case) in cases.iter().enumerate() {
        let id = case["case_id"].as_str().unwrap();
        let mut exact = true;
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let parsed = parse_source_file(
                    "main.ts",
                    case["text"].as_str().unwrap(),
                    Default::default(),
                    None,
                );
                let classes = parsed
                    .arena
                    .nodes()
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| {
                        matches!(
                            node.kind,
                            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                        )
                    })
                    .map(|(index, _)| NodeId(u32::try_from(index).unwrap()))
                    .collect::<Vec<_>>();
                assert_eq!(classes.len(), 1);
                let bits = u32::try_from(case["emit_flags"].as_u64().unwrap()).unwrap();
                assert!([0, 3072, 4096, 7168].contains(&bits));
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let class = arena.node_ref(source, classes[0]).unwrap();
                arena
                    .metadata_mut(class)
                    .set_flags(EmitFlags::from_bits(bits));
                let mut result = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_source_file_text_mode(SourceFileTextMode::Canonical)
                        .with_remove_comments(case["remove_comments"].as_bool().unwrap()),
                )
                .print(&mut result, PrintRequest::SourceFile(source), None)
                .unwrap();
                let actual = serde_json::json!({
                    "text": printed.text(),
                    "utf8_base64": base64_encode(printed.text().as_bytes()),
                    "utf8_bytes": printed.text().len(),
                    "end_utf16": {
                        "position": printed.end().position().value(),
                        "line": printed.end().line(),
                        "column": printed.end().column(),
                    },
                });
                if actual != case["typescript_observation"] {
                    if let Some(directory) =
                        std::env::var_os("TSC_RS_H2_8A_CLASS_HEADER_PRINTER_FAILURE_DIR")
                    {
                        let directory = std::path::PathBuf::from(directory);
                        assert!(directory.is_absolute());
                        std::fs::create_dir_all(&directory).unwrap();
                        let path = directory.join(format!("{case_index}-{repetition}.json"));
                        assert!(!path.exists());
                        let captured = serde_json::json!({
                            "case_id": id,
                            "repetition": repetition,
                            "actual": actual,
                            "expected": case["typescript_observation"],
                        });
                        std::fs::write(path, serde_json::to_vec_pretty(&captured).unwrap())
                            .unwrap();
                    }
                }
                assert_eq!(
                    actual, case["typescript_observation"],
                    "{id} repetition {repetition}"
                );
            });
            if result.is_err() {
                exact = false;
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
        if exact {
            eprintln!("Class header printer metadata EXACT x2 {id}");
        }
    }
    assert!(
        failures.is_empty(),
        "class header printer metadata failures: {failures:?}"
    );
}
