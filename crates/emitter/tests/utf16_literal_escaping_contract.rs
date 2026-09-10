use serde_json::{json, Value};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, StandaloneWriter, TransformArena, TransformRoot,
};
use tsc_syntax::{parse_source_file, SyntaxKind};

#[test]
fn utf16_literal_escaping_matches_typescript() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/utf16-literal-escaping.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 288);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let parsed = parse_source_file("main.ts", "", Default::default(), None);
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let units = case["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| u16::try_from(v.as_u64().unwrap()).unwrap())
                    .collect::<Vec<_>>();
                let spelling = case["kind"].as_str().unwrap();
                let node = match spelling {
                    "double" | "single" => arena
                        .factory()
                        .create_string_literal_from_code_units(source, &units, spelling == "single")
                        .unwrap(),
                    _ => {
                        let kind = match spelling {
                            "NoSubstitutionTemplateLiteral" => {
                                SyntaxKind::NoSubstitutionTemplateLiteral
                            }
                            "TemplateHead" => SyntaxKind::TemplateHead,
                            "TemplateMiddle" => SyntaxKind::TemplateMiddle,
                            "TemplateTail" => SyntaxKind::TemplateTail,
                            _ => unreachable!(),
                        };
                        arena
                            .factory()
                            .create_template_literal_like_from_code_units(
                                source, kind, &units, None,
                            )
                            .unwrap()
                    }
                };
                arena.metadata_mut(node).set_flags(EmitFlags::from_bits(
                    u32::try_from(case["emit_flags"].as_u64().unwrap()).unwrap(),
                ));
                let record = arena.node(node).unwrap();
                let metadata = arena.metadata(node).unwrap();
                assert_eq!((record.pos, record.end), (u32::MAX, u32::MAX));
                let tree_state = json!({"kind":record.kind as u16,"pos":-1,"end":-1,"flags":record.flags,
                    "emit_flags":metadata.flags().bits(),"value_utf16":metadata.javascript_string_value().unwrap().code_units()});
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_never_ascii_escape(case["never_ascii_escape"].as_bool().unwrap()),
                )
                .print(
                    &mut transformation,
                    PrintRequest::StandaloneNode {
                        node,
                        writer: StandaloneWriter::MultiLine,
                    },
                    None,
                )
                .unwrap();
                let actual = json!({"tree_state":tree_state,"text_utf16":printed.text().encode_utf16().collect::<Vec<_>>(),
                    "utf8_base64":base64_encode(printed.text().as_bytes()),"utf8_bytes":printed.text().len(),
                    "end_utf16":{"position":printed.end().position().value(),"line":printed.end().line(),"column":printed.end().column()}});
                assert_eq!(
                    actual, case["typescript_observation"],
                    "{id} repetition {repetition}"
                );
            });
            if result.is_err() {
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "utf16 literal escaping failures: {failures:?}"
    );
}
