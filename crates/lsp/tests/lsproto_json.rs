//! tsgo `lsproto/lsp_json_test.go` and `lsp_test.go`
//! (`TestUnmarshalCompletionItem`): the protocol types' JSON, with tsgo's
//! inputs and error messages. `TestUnmarshalParamsRequiresParams` belongs to
//! the base protocol's messages and `TestTryDynamicFileNameToDocumentUri` to
//! the URIs, which come with the server.

use std::borrow::Cow;

use serde::Serialize;
use serde_json::json;
use tsc_lsp::lsproto::*;

fn decode<T: Decode>(text: &str) -> Result<T, DecodeError> {
    from_str(text)
}

fn encode<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

fn error_of<T: Decode + std::fmt::Debug>(text: &str) -> String {
    decode::<T>(text).expect_err(text).0
}

#[test]
fn unmarshal_rejects_null_for_optional_non_nullable_fields() {
    let cases: [(&str, String, &str); 16] = [
        (
            "InlayHint kind null",
            error_of::<InlayHint>(
                r#"{"position": {"line": 0, "character": 0}, "label": "foo", "kind": null}"#,
            ),
            r#"null value is not allowed for field "kind""#,
        ),
        (
            "InlayHint textEdits null",
            error_of::<InlayHint>(
                r#"{"position": {"line": 0, "character": 0}, "label": "foo", "textEdits": null}"#,
            ),
            r#"null value is not allowed for field "textEdits""#,
        ),
        (
            "InlayHint paddingLeft null",
            error_of::<InlayHint>(
                r#"{"position": {"line": 0, "character": 0}, "label": "foo", "paddingLeft": null}"#,
            ),
            r#"null value is not allowed for field "paddingLeft""#,
        ),
        (
            "FoldingRange kind null",
            error_of::<FoldingRange>(r#"{"startLine": 0, "endLine": 10, "kind": null}"#),
            r#"null value is not allowed for field "kind""#,
        ),
        (
            "FoldingRange startCharacter null",
            error_of::<FoldingRange>(r#"{"startLine": 0, "endLine": 10, "startCharacter": null}"#),
            r#"null value is not allowed for field "startCharacter""#,
        ),
        (
            "CompletionItem insertTextFormat null",
            error_of::<CompletionItem>(r#"{"label": "test", "insertTextFormat": null}"#),
            r#"null value is not allowed for field "insertTextFormat""#,
        ),
        (
            "Hover range null",
            error_of::<Hover>(
                r#"{"contents": {"kind": "plaintext", "value": "hi"}, "range": null}"#,
            ),
            r#"null value is not allowed for field "range""#,
        ),
        (
            "WorkDoneProgressOptions workDoneProgress null",
            error_of::<WorkDoneProgressOptions>(r#"{"workDoneProgress": null}"#),
            r#"null value is not allowed for field "workDoneProgress""#,
        ),
        (
            "CallHierarchyIncomingCallsParams item null",
            error_of::<CallHierarchyIncomingCallsParams>(r#"{"item": null}"#),
            r#"null value is not allowed for field "item""#,
        ),
        (
            "CallHierarchyIncomingCall from null",
            error_of::<CallHierarchyIncomingCall>(r#"{"from": null, "fromRanges": []}"#),
            r#"null value is not allowed for field "from""#,
        ),
        (
            "InitializeParams capabilities null",
            error_of::<InitializeParams>(
                r#"{"processId": null, "rootUri": null, "capabilities": null}"#,
            ),
            r#"null value is not allowed for field "capabilities""#,
        ),
        (
            "InitializeResult capabilities null",
            error_of::<InitializeResult>(r#"{"capabilities": null}"#),
            r#"null value is not allowed for field "capabilities""#,
        ),
        (
            "SemanticTokens data null (required slice)",
            error_of::<SemanticTokens>(r#"{"data": null}"#),
            r#"null value is not allowed for field "data""#,
        ),
        (
            "TextDocumentEdit edits null (required slice)",
            error_of::<TextDocumentEdit>(
                r#"{"textDocument": {"uri": "file:///a.ts", "version": 1}, "edits": null}"#,
            ),
            r#"null value is not allowed for field "edits""#,
        ),
        (
            "Location receives null (struct)",
            error_of::<Location>("null"),
            "expected object start, but encountered null",
        ),
        (
            "IntegerOrString rejects null",
            error_of::<IntegerOrString>("null"),
            "invalid IntegerOrString: got null",
        ),
    ];
    for (name, error, expected) in cases {
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn unmarshal_accepts_null_for_nullable_fields() {
    for (name, result) in [
        (
            "InitializeParams rootUri null",
            decode::<InitializeParams>(r#"{"processId": null, "rootUri": null, "capabilities": {}}"#).map(drop),
        ),
        (
            "InitializeParams workspaceFolders null",
            decode::<InitializeParams>(
                r#"{"processId": null, "rootUri": null, "capabilities": {}, "workspaceFolders": null}"#,
            )
            .map(drop),
        ),
        (
            "InitializationOptions userPreferences null",
            decode::<InitializationOptions>(r#"{"userPreferences": null}"#).map(drop),
        ),
        (
            "InitializeParams initializationOptions null",
            decode::<InitializeParams>(
                r#"{"processId": null, "rootUri": null, "capabilities": {}, "initializationOptions": null}"#,
            )
            .map(drop),
        ),
    ] {
        assert_eq!(result, Ok(()), "{name}");
    }
}

#[test]
fn unmarshal_accepts_omitted_optional_fields() {
    let hint = decode::<InlayHint>(r#"{"position": {"line": 1, "character": 5}, "label": "test"}"#)
        .unwrap();
    assert_eq!(hint.kind, None);
    assert_eq!(hint.text_edits, None);
    assert_eq!(hint.tooltip, None);
    assert_eq!(hint.padding_left, None);
    assert_eq!(hint.padding_right, None);
    assert_eq!(hint.data, None);
    assert_eq!(hint.position.line, 1);
    assert_eq!(hint.position.character, 5);

    let range = decode::<FoldingRange>(r#"{"startLine": 5, "endLine": 10}"#).unwrap();
    assert_eq!(range.kind, None);
    assert_eq!(range.start_character, None);
    assert_eq!(range.end_character, None);
    assert_eq!(range.collapsed_text, None);
    assert_eq!(range.start_line, 5);
    assert_eq!(range.end_line, 10);
}

#[test]
fn unmarshal_rejects_incomplete_objects() {
    assert!(error_of::<InlayHint>(r#"{"label": "test"}"#)
        .contains("missing required properties: position"));
    assert!(
        error_of::<InlayHint>(r#"{"position": {"line": 0, "character": 0}}"#)
            .contains("missing required properties: label")
    );
    assert!(error_of::<Location>(
        r#"{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}}"#
    )
    .contains("missing required properties: uri"));
    assert!(error_of::<Location>("{}").contains("missing required properties: uri, range"));
}

fn round_trip<T: Decode + Serialize + PartialEq + std::fmt::Debug>(value: T) {
    let text = encode(&value);
    let decoded = decode::<T>(&text).unwrap();
    assert_eq!(decoded, value, "{text}");
}

#[test]
fn marshal_unmarshal_round_trip() {
    round_trip(InlayHint {
        position: Position {
            line: 1,
            character: 5,
        },
        label: StringOrInlayHintLabelParts::String("param".to_owned()),
        kind: Some(InlayHintKind::PARAMETER),
        ..InlayHint::default()
    });
    round_trip(InlayHint {
        position: Position::default(),
        label: StringOrInlayHintLabelParts::String("x".to_owned()),
        ..InlayHint::default()
    });
    round_trip(FoldingRange {
        start_line: 1,
        start_character: Some(0),
        end_line: 10,
        end_character: Some(5),
        kind: Some(FoldingRangeKind::REGION),
        collapsed_text: Some("...".to_owned()),
    });
    round_trip(Location {
        uri: DocumentUri::from("file:///test.ts"),
        range: Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 3,
                character: 4,
            },
        },
    });
    round_trip(InitializeParams {
        process_id: IntegerOrNull::Null,
        root_uri: DocumentUriOrNull::DocumentUri(DocumentUri::from("file:///workspace")),
        capabilities: ClientCapabilities::default(),
        ..InitializeParams::default()
    });
}

#[test]
fn unmarshal_union_types() {
    assert_eq!(
        decode::<IntegerOrString>("42"),
        Ok(IntegerOrString::Integer(42))
    );
    assert_eq!(
        decode::<IntegerOrString>(r#""hello""#),
        Ok(IntegerOrString::String("hello".to_owned()))
    );
    assert_eq!(
        decode::<IntegerOrNull>("42"),
        Ok(IntegerOrNull::Integer(42))
    );
    assert_eq!(decode::<IntegerOrNull>("null"), Ok(IntegerOrNull::Null));
    assert_eq!(
        decode::<DocumentUriOrNull>(r#""file:///test.ts""#),
        Ok(DocumentUriOrNull::DocumentUri(DocumentUri::from(
            "file:///test.ts"
        )))
    );
    assert_eq!(
        decode::<DocumentUriOrNull>("null"),
        Ok(DocumentUriOrNull::Null)
    );
}

#[test]
fn marshal_union_types() {
    assert_eq!(encode(&IntegerOrNull::Integer(42)), "42");
    assert_eq!(encode(&IntegerOrNull::Null), "null");
    assert_eq!(encode(&IntegerOrString::Integer(7)), "7");
    assert_eq!(
        encode(&IntegerOrString::String("tok".to_owned())),
        r#""tok""#
    );
}

#[test]
fn unmarshal_ignores_unknown_fields() {
    let location = decode::<Location>(
        r#"{
            "uri": "file:///test.ts",
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
            "someUnknownField": 42,
            "anotherUnknown": {"nested": true}
        }"#,
    )
    .unwrap();
    assert_eq!(location.uri, DocumentUri::from("file:///test.ts"));
    decode::<InlayHint>(
        r#"{
            "position": {"line": 0, "character": 0},
            "label": "x",
            "futureField": [1, 2, 3]
        }"#,
    )
    .unwrap();
}

#[test]
fn unmarshal_rejects_wrong_types() {
    for input in ["[]", r#""not an object""#, "42", "null"] {
        assert!(decode::<Location>(input).is_err(), "Location {input}");
    }
    assert!(decode::<FoldingRange>("true").is_err());
}

#[test]
fn unmarshal_union_type_wrong_kind() {
    for input in ["true", "null", "{}", "[]"] {
        assert!(
            decode::<IntegerOrString>(input).is_err(),
            "IntegerOrString {input}"
        );
    }
    for input in ["42", "true"] {
        assert!(
            decode::<StringOrInlayHintLabelParts>(input).is_err(),
            "StringOrInlayHintLabelParts {input}"
        );
    }
}

#[test]
fn unmarshal_boolean_union_types() {
    assert_eq!(
        decode::<BooleanOrHoverOptions>("true"),
        Ok(BooleanOrHoverOptions::Boolean(true))
    );
    assert_eq!(
        decode::<BooleanOrHoverOptions>("false"),
        Ok(BooleanOrHoverOptions::Boolean(false))
    );
    assert!(matches!(
        decode::<BooleanOrHoverOptions>("{}"),
        Ok(BooleanOrHoverOptions::HoverOptions(_))
    ));
    assert!(decode::<BooleanOrHoverOptions>(r#""nope""#).is_err());
}

#[test]
fn unmarshal_discriminator_union() {
    let Ok(WorkDoneProgressBeginOrReportOrEnd::Begin(begin)) =
        decode::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": "begin", "title": "Indexing"}"#)
    else {
        panic!("begin");
    };
    assert_eq!(begin.title, "Indexing");
    let Ok(WorkDoneProgressBeginOrReportOrEnd::Report(report)) =
        decode::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": "report", "message": "50%"}"#)
    else {
        panic!("report");
    };
    assert_eq!(report.message.as_deref(), Some("50%"));
    assert!(matches!(
        decode::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": "end"}"#),
        Ok(WorkDoneProgressBeginOrReportOrEnd::End(_))
    ));
    // The discriminator after the variant's fields.
    let Ok(WorkDoneProgressBeginOrReportOrEnd::Begin(begin)) =
        decode::<WorkDoneProgressBeginOrReportOrEnd>(
            r#"{"title": "Indexing", "percentage": 25, "kind": "begin"}"#,
        )
    else {
        panic!("begin after fields");
    };
    assert_eq!(begin.title, "Indexing");
    assert_eq!(begin.percentage, Some(25));
    assert!(decode::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": "invalid"}"#).is_err());
    assert!(decode::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": null}"#).is_err());
    assert!(
        error_of::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"message": "missing kind"}"#)
            .contains(r#"missing discriminator "kind""#)
    );
}

#[test]
fn unmarshal_presence_discriminator_union() {
    let Ok(TextEditOrInsertReplaceEdit::TextEdit(edit)) = decode::<TextEditOrInsertReplaceEdit>(
        r#"{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "newText": "x"
        }"#,
    ) else {
        panic!("text edit");
    };
    assert_eq!(edit.new_text, "x");
    let Ok(TextEditOrInsertReplaceEdit::InsertReplaceEdit(edit)) =
        decode::<TextEditOrInsertReplaceEdit>(
            r#"{
            "insert": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "replace": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2}},
            "newText": "y"
        }"#,
        )
    else {
        panic!("insert replace edit");
    };
    assert_eq!(edit.new_text, "y");
}

#[test]
fn unmarshal_string_or_array_union() {
    assert_eq!(
        decode::<StringOrInlayHintLabelParts>(r#""hello""#),
        Ok(StringOrInlayHintLabelParts::String("hello".to_owned()))
    );
    let Ok(StringOrInlayHintLabelParts::InlayHintLabelParts(parts)) =
        decode::<StringOrInlayHintLabelParts>(
            r#"[{"value": "param"}, {"value": ": "}, {"value": "string"}]"#,
        )
    else {
        panic!("parts");
    };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].value, "param");
}

#[test]
fn unmarshal_document_edit_union() {
    type Edit = TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile;
    assert!(matches!(
        decode::<Edit>(
            r#"{
                "textDocument": {"uri": "file:///a.ts", "version": 1},
                "edits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "x"}]
            }"#
        ),
        Ok(Edit::TextDocumentEdit(_))
    ));
    assert!(matches!(
        decode::<Edit>(
            r#"{
                "kind": null,
                "textDocument": {"uri": "file:///a.ts", "version": 1},
                "edits": []
            }"#
        ),
        Ok(Edit::TextDocumentEdit(_))
    ));
    let Ok(Edit::CreateFile(create)) =
        decode::<Edit>(r#"{"kind": "create", "uri": "file:///new.ts"}"#)
    else {
        panic!("create");
    };
    assert_eq!(create.uri, DocumentUri::from("file:///new.ts"));
    let Ok(Edit::CreateFile(create)) =
        decode::<Edit>(r#"{"uri": "file:///new.ts", "kind": "create"}"#)
    else {
        panic!("create after fields");
    };
    assert_eq!(create.uri, DocumentUri::from("file:///new.ts"));
    let Ok(Edit::RenameFile(rename)) = decode::<Edit>(
        r#"{"kind": "rename", "oldUri": "file:///old.ts", "newUri": "file:///new.ts"}"#,
    ) else {
        panic!("rename");
    };
    assert_eq!(rename.old_uri, DocumentUri::from("file:///old.ts"));
    let Ok(Edit::DeleteFile(delete)) =
        decode::<Edit>(r#"{"kind": "delete", "uri": "file:///gone.ts"}"#)
    else {
        panic!("delete");
    };
    assert_eq!(delete.uri, DocumentUri::from("file:///gone.ts"));
}

#[test]
fn unmarshal_field_ordering() {
    let location = decode::<Location>(
        r#"{
            "range": {"start": {"line": 1, "character": 2}, "end": {"line": 3, "character": 4}},
            "uri": "file:///test.ts"
        }"#,
    )
    .unwrap();
    assert_eq!(location.uri, DocumentUri::from("file:///test.ts"));
    assert_eq!(location.range.start.line, 1);
    let hint = decode::<InlayHint>(
        r#"{
            "kind": 1,
            "label": "x",
            "position": {"line": 0, "character": 0}
        }"#,
    )
    .unwrap();
    assert_eq!(hint.kind, Some(InlayHintKind::TYPE));
}

#[test]
fn unmarshal_completion_item_data_file_name() {
    let data =
        decode::<CompletionItemData>(r#"{"fileName":"/src/index.ts","position":1,"name":"value"}"#)
            .unwrap();
    assert_eq!(data.file_name, "/src/index.ts");
}

#[test]
fn unmarshal_empty_object() {
    assert_eq!(
        decode::<WorkDoneProgressOptions>("{}")
            .unwrap()
            .work_done_progress,
        None
    );
    decode::<InitializationOptions>("{}").unwrap();
    decode::<ClientCapabilities>("{}").unwrap();
    decode::<ServerCapabilities>("{}").unwrap();
}

#[test]
fn marshal_omits_zero_optional_fields() {
    let text = encode(&InlayHint {
        position: Position::default(),
        label: StringOrInlayHintLabelParts::String("x".to_owned()),
        ..InlayHint::default()
    });
    for absent in ["kind", "textEdits", "paddingLeft"] {
        assert!(!text.contains(absent), "{text}");
    }
    for present in ["position", "label"] {
        assert!(text.contains(present), "{text}");
    }
    let text = encode(&FoldingRange {
        start_line: 1,
        end_line: 10,
        ..FoldingRange::default()
    });
    for absent in ["kind", "startCharacter"] {
        assert!(!text.contains(absent), "{text}");
    }
    for present in ["startLine", "endLine"] {
        assert!(text.contains(present), "{text}");
    }
}

#[test]
fn literal_types() {
    assert_eq!(encode(&StringLiteralCreate), r#""create""#);
    assert_eq!(
        decode::<StringLiteralCreate>(r#""create""#),
        Ok(StringLiteralCreate)
    );
    assert!(decode::<StringLiteralCreate>(r#""delete""#).is_err());
    assert!(decode::<StringLiteralCreate>("42").is_err());
}

#[test]
fn enum_string_values() {
    assert_eq!(InlayHintKind::TYPE.to_string(), "Type");
    assert_eq!(InlayHintKind::PARAMETER.to_string(), "Parameter");
    assert_eq!(SymbolKind::FILE.to_string(), "File");
    assert_eq!(SymbolKind::FUNCTION.to_string(), "Function");
    assert_eq!(SymbolKind::VARIABLE.to_string(), "Variable");
    assert!(InlayHintKind(999).to_string().contains("999"));
}

/// tsgo `TestRoundTrip`: every value survives marshal, unmarshal and marshal
/// again unchanged.
#[test]
fn round_trip_keeps_the_wire_format() {
    fn again<T: Decode + Serialize>(value: &T) {
        let text = encode(value);
        assert_eq!(encode(&decode::<T>(&text).unwrap()), text);
    }
    again(&Range {
        start: Position {
            line: 1,
            character: 2,
        },
        end: Position {
            line: 3,
            character: 4,
        },
    });
    again(&TextEdit {
        range: Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 3,
                character: 4,
            },
        },
        new_text: "hello".to_owned(),
    });
    again(&MarkupContent {
        kind: MarkupKind::MARKDOWN,
        value: "**x**".to_owned(),
    });
    again(&DidChangeConfigurationParams {
        settings: json!({"js/ts": {"x": 1.0}}),
    });
    again(&DidChangeConfigurationParams {
        settings: serde_json::Value::Null,
    });
    again(&CompletionItem {
        label: "pageXOffset".to_owned(),
        kind: Some(CompletionItemKind::FIELD),
        sort_text: Some("15".to_owned()),
        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
        ..CompletionItem::default()
    });
    again(&ParameterInformation {
        label: StringOrTuple::String("p: number".to_owned()),
        ..ParameterInformation::default()
    });
    again(&ParameterInformation {
        label: StringOrTuple::Tuple([0, 4]),
        ..ParameterInformation::default()
    });
}

#[test]
fn strictness_missing_required() {
    assert!(error_of::<TextEdit>(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}}"#
    )
    .contains("missing required properties"));
    assert!(error_of::<Range>(r#"{"start":{"line":0,"character":0}}"#)
        .contains("missing required properties"));
    assert!(error_of::<Position>(r#"{"line":0}"#).contains("missing required properties"));
}

#[test]
fn strictness_not_object() {
    let error = error_of::<TextEdit>(r#""oops""#);
    assert!(
        error.contains("object") || error.contains("cannot unmarshal"),
        "{error}"
    );
}

/// tsgo `TestUnmarshalCompletionItem`.
#[test]
fn unmarshal_completion_item() {
    let message = r#"{
    "label": "pageXOffset",
    "insertTextFormat": 1,
    "textEdit": {
        "newText": "pageXOffset",
        "insert": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 4}},
        "replace": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 4}}
    },
    "kind": 6,
    "sortText": "15",
    "commitCharacters": [".", ",", ";"]
}"#;
    let range = Range {
        start: Position {
            line: 4,
            character: 0,
        },
        end: Position {
            line: 4,
            character: 4,
        },
    };
    assert_eq!(
        decode::<CompletionItem>(message),
        Ok(CompletionItem {
            label: "pageXOffset".to_owned(),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            text_edit: Some(TextEditOrInsertReplaceEdit::InsertReplaceEdit(
                InsertReplaceEdit {
                    new_text: "pageXOffset".to_owned(),
                    insert: range.clone(),
                    replace: range,
                }
            )),
            kind: Some(CompletionItemKind::VARIABLE),
            sort_text: Some("15".to_owned()),
            commit_characters: Some(vec![".".to_owned(), ",".to_owned(), ";".to_owned()]),
            ..CompletionItem::default()
        })
    );
}

#[test]
fn string_enums_keep_unknown_values() {
    assert_eq!(
        decode::<MarkupKind>(r#""rst""#),
        Ok(MarkupKind(Cow::Owned("rst".to_owned())))
    );
}
