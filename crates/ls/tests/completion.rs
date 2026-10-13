//! Completions as tsgo answers them, for the cases the completion port's
//! fixes reach and the conformance baselines do not show. Each case is a
//! fourslash test's input with tsgo's answer, which the lsp suite's
//! transcripts record (`scripts/lsp_transcripts.py`). The items are compared
//! in any order, as the transcripts' digests compare them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tsc_compiler::LiveProgram;
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_host::MemoryCompilerHost;
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::{
    CompletionPreferences, JsxAttributeCompletionStyle, QuotePreference, UserPreferences,
};
use tsc_ls::LanguageService;
use tsc_lsproto::{
    CompletionContext, CompletionParams, CompletionTriggerKind, DocumentUri, Position,
    PositionEncodingKind, ResolvedClientCapabilities, TextDocumentIdentifier,
};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// The fourslash options with `configure` applied.
fn options(configure: impl FnOnce(&mut CompilerOptions)) -> CompilerOptions {
    let mut options = CompilerOptions {
        target: Some(LATEST_STANDARD),
        jsx: Some(JSX_PRESERVE),
        skip_default_lib_check: Some(true),
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    configure(&mut options);
    options
}

fn libraries() -> &'static [(String, Vec<u8>)] {
    static FILES: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    FILES.get_or_init(|| {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
        );
        let mut files = Vec::new();
        for entry in std::fs::read_dir(directory).expect("vendored TypeScript libraries") {
            let entry = entry.expect("library directory entry");
            let name = entry.file_name().into_string().expect("library filename");
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                files.push((
                    format!("/lib/{name}"),
                    std::fs::read(entry.path()).expect("library bytes"),
                ));
            }
        }
        files.sort();
        files
    })
}

/// The test's files as the language service's host.
struct Files(BTreeMap<String, String>);

impl SourceMapperHost for Files {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.0.get(file_name).cloned()
    }
}

impl tsc_ls::Host for Files {}

/// tsgo's completion at `(line, character)` of `file` (with the trigger
/// character, if any), as JSON with the items sorted, for the client the
/// fourslash tests have.
fn completion(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    position: (u32, u32),
    trigger_character: Option<&str>,
) -> Value {
    completion_with(
        files,
        options,
        UserPreferences::default(),
        false,
        file,
        position,
        trigger_character,
    )
}

/// [`completion`] with the user's preferences, for a client that takes
/// snippets or not.
fn completion_with(
    files: &[(&str, &str)],
    options: CompilerOptions,
    preferences: UserPreferences,
    snippets: bool,
    file: &str,
    (line, character): (u32, u32),
    trigger_character: Option<&str>,
) -> Value {
    let mut builder = MemoryCompilerHost::builder("/").case_sensitive(true);
    for (name, text) in files {
        builder = builder.file(PathBuf::from(name), text.as_bytes());
    }
    for (path, bytes) in libraries() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let roots = files
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect::<Vec<_>>();
    let prepared = load_program(
        &host,
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("load the program");
    let mut live = LiveProgram::new(prepared).expect("the live program");
    let texts: BTreeMap<String, String> = files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect();
    let line_maps = texts.clone();
    let converters = Converters::new(PositionEncodingKind::UTF16, move |file_name| {
        Arc::new(LspLineMap::new(
            line_maps.get(file_name).map_or("", String::as_str),
        ))
    });
    let files = Files(texts);
    let mut capabilities = ResolvedClientCapabilities::default();
    let completion = &mut capabilities.text_document.completion;
    completion.completion_item.commit_characters_support = true;
    completion.completion_item.insert_replace_support = true;
    completion.completion_item.preselect_support = true;
    completion.completion_item.snippet_support = snippets;
    completion.completion_list.item_defaults =
        vec!["commitCharacters".to_owned(), "editRange".to_owned()];
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    let params = CompletionParams {
        text_document: TextDocumentIdentifier {
            uri: DocumentUri(format!("file://{file}")),
        },
        position: Position { line, character },
        work_done_token: None,
        partial_result_token: None,
        context: Some(CompletionContext {
            trigger_kind: if trigger_character.is_some() {
                CompletionTriggerKind::TRIGGER_CHARACTER
            } else {
                CompletionTriggerKind::INVOKED
            },
            trigger_character: trigger_character.map(str::to_owned),
        }),
    };
    let response = service.provide_completion(&params).expect("an answer");
    sorted(serde_json::to_value(&response).expect("the response as JSON"))
}

/// A response with its items in label order.
fn sorted(mut value: Value) -> Value {
    if let Some(Value::Array(items)) = value.get_mut("items") {
        items.sort_by_key(|item| item.to_string());
    }
    value
}

/// The answer tsgo gave, as JSON with its items sorted.
fn tsgo(json: &str) -> Value {
    sorted(serde_json::from_str(json).expect("tsgo's answer"))
}

/// The item of a response with `label`, or null.
fn item(response: &Value, label: &str) -> Value {
    response["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["label"] == label))
        .cloned()
        .unwrap_or(Value::Null)
}

/// TestCompletionsOptionalMethod: an optional member of a property access is labeled
/// with `?` and inserts its name.
#[test]
fn optional_member_label() {
    let files = [(
        "/completionsOptionalMethod.ts",
        "declare const x: { m?(): void };\nx.",
    )];
    let compiler_options = || {
        options(|options| {
            options.strict_null_checks = Some(true);
        })
    };
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsOptionalMethod.ts",
            (1, 2),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "m?", "kind": 2, "sortText": "11", "filterText": "m", "insertText": "m", "data": {"fileName": "/completionsOptionalMethod.ts", "position": 35, "name": "m"}}]}"#
        ),
    );
}

/// TestCompletionAfterQuestionDot: after `?.`, a name that is no identifier is an
/// element access, and an optional property is labeled with `?`.
#[test]
fn question_dot_and_element_access() {
    let files = [
        ("/completionAfterQuestionDot.ts", "class User {\n    #foo: User;\n    bar: User;\n    address?: {\n        city: string;\n        \"postal code\": string;\n    };\n    constructor() {\n        this.address?.;\n        this?.;\n        this?.bar?.;\n    }\n};"),
    ];
    let compiler_options = || {
        options(|options| {
            options.strict = Some(true);
        })
    };
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionAfterQuestionDot.ts",
            (8, 22),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "city", "kind": 5, "sortText": "11", "data": {"fileName": "/completionAfterQuestionDot.ts", "position": 162, "name": "city"}}, {"label": "postal code", "kind": 5, "sortText": "11", "filterText": "?.postal code", "insertText": "?.[\"postal code\"]", "textEdit": {"range": {"start": {"line": 8, "character": 20}, "end": {"line": 8, "character": 22}}, "newText": "?.[\"postal code\"]"}, "data": {"fileName": "/completionAfterQuestionDot.ts", "position": 162, "name": "postal code"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionAfterQuestionDot.ts",
            (9, 14),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "bar", "kind": 5, "sortText": "11", "data": {"fileName": "/completionAfterQuestionDot.ts", "position": 178, "name": "bar"}}, {"label": "address?", "kind": 5, "sortText": "11", "filterText": "address", "insertText": "address", "data": {"fileName": "/completionAfterQuestionDot.ts", "position": 178, "name": "address"}}]}"#
        ),
    );
}

/// TestCompletionFilterText2: a member of a nullable value inserts `?.` over the
/// dot, filtered without it.
#[test]
fn inserted_question_dot() {
    let files = [
        ("/completionFilterText2.ts", "declare const foo1: { bar: string } | undefined;\nif (true) {\n    foo1.\n}\nelse {\n    foo1?.\n}\n"),
    ];
    let compiler_options = || {
        options(|options| {
            options.strict = Some(true);
        })
    };
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionFilterText2.ts",
            (2, 9),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "bar", "kind": 5, "sortText": "11", "filterText": ".bar", "insertText": "?.bar", "textEdit": {"range": {"start": {"line": 2, "character": 8}, "end": {"line": 2, "character": 9}}, "newText": "?.bar"}, "data": {"fileName": "/completionFilterText2.ts", "position": 70, "name": "bar"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionFilterText2.ts",
            (5, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "bar", "kind": 5, "sortText": "11", "data": {"fileName": "/completionFilterText2.ts", "position": 90, "name": "bar"}}]}"#
        ),
    );
}

/// TestCompletionForStringLiteral_quotePreference2: the element access of a member
/// that is no identifier quotes it in the file's style.
#[test]
fn element_access_quote_preference() {
    let files = [(
        "/completionForStringLiteral_quotePreference2.ts",
        "const a = {\n    '#': 'a'\n};\na.",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            UserPreferences {
                quote_preference: QuotePreference::SINGLE,
                ..UserPreferences::default()
            },
            false,
            "/completionForStringLiteral_quotePreference2.ts",
            (3, 2),
            None
        ),
        tsgo(
            r##"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "#", "kind": 5, "sortText": "11", "filterText": ".#", "insertText": "['#']", "textEdit": {"range": {"start": {"line": 3, "character": 1}, "end": {"line": 3, "character": 2}}, "newText": "['#']"}, "data": {"fileName": "/completionForStringLiteral_quotePreference2.ts", "position": 30, "name": "#"}}]}"##
        ),
    );
}

/// TestMemberListInWithBlock: in a method, the class's members insert `this.`.
#[test]
fn this_members_in_a_with_block() {
    let files = [
        ("/memberListInWithBlock.ts", "class c {\n    static x: number;\n    public foo() {\n        with ({}) {\n            function f() { }\n            var d = this.foo;\n            \n        }\n    }\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/memberListInWithBlock.ts",
            (5, 25),
            None
        ),
        tsgo(r#"null"#),
    );
    let response = completion(
        &files,
        compiler_options(),
        "/memberListInWithBlock.ts",
        (6, 12),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(48));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "foo"),
        tsgo(
            r#"{"label": "foo", "kind": 2, "sortText": "14", "insertText": "this.foo", "data": {"fileName": "/memberListInWithBlock.ts", "position": 142, "source": "ThisProperty/", "name": "foo"}}"#
        )
    );
    assert_eq!(
        item(&response, "this"),
        tsgo(
            r#"{"label": "this", "kind": 14, "sortText": "15", "data": {"fileName": "/memberListInWithBlock.ts", "position": 142, "name": "this"}}"#
        )
    );
    assert_eq!(item(&response, "d"), Value::Null);
    assert_eq!(item(&response, "f"), Value::Null);
}

/// TestCompletionOfAwaitPromise2: a promise's value's members wrap the
/// expression in `(await ...)`.
#[test]
fn await_promise_members() {
    let files = [(
        "/completionOfAwaitPromise2.ts",
        "interface Foo { foo: string }\nasync function foo(x: Promise<Foo>) {\n   x.\n}",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionOfAwaitPromise2.ts",
            (2, 5),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "then", "kind": 2, "sortText": "11", "data": {"fileName": "/completionOfAwaitPromise2.ts", "position": 73, "name": "then"}}, {"label": "catch", "kind": 2, "sortText": "11", "data": {"fileName": "/completionOfAwaitPromise2.ts", "position": 73, "name": "catch"}}, {"label": "Symbol", "kind": 6, "sortText": "15", "filterText": ".Symbol", "insertText": "[Symbol]", "textEdit": {"range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 5}}, "newText": "[Symbol]"}, "data": {"fileName": "/completionOfAwaitPromise2.ts", "position": 73, "name": "Symbol"}}, {"label": "finally", "kind": 2, "sortText": "11", "data": {"fileName": "/completionOfAwaitPromise2.ts", "position": 73, "name": "finally"}}, {"label": "foo", "kind": 5, "sortText": "11", "filterText": "(await x).foo", "insertText": "(await x).foo", "textEdit": {"range": {"start": {"line": 2, "character": 3}, "end": {"line": 2, "character": 5}}, "newText": "(await x).foo"}, "data": {"fileName": "/completionOfAwaitPromise2.ts", "position": 73, "name": "foo"}}]}"#
        ),
    );
}

/// TestCompletionsRecommended_equals: after `===`, the left side's enum is
/// recommended.
#[test]
fn recommended_enum_after_equality() {
    let files = [(
        "/completionsRecommended_equals.ts",
        "enum Enu {}\ndeclare const e: Enu;\ne === ;\ne === E",
    )];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionsRecommended_equals.ts",
        (2, 6),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1077));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "Enu"),
        tsgo(
            r#"{"label": "Enu", "kind": 13, "preselect": true, "sortText": "11", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 40, "name": "Enu"}}"#
        )
    );
    assert_eq!(
        item(&response, "e"),
        tsgo(
            r#"{"label": "e", "kind": 6, "sortText": "11", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 40, "name": "e"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 40, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionsRecommended_equals.ts",
        (3, 7),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1077));
    assert_eq!(
        response["itemDefaults"],
        tsgo(
            r#"{"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 3, "character": 6}, "end": {"line": 3, "character": 7}}, "replace": {"start": {"line": 3, "character": 6}, "end": {"line": 3, "character": 7}}}}"#
        )
    );
    assert_eq!(
        item(&response, "Enu"),
        tsgo(
            r#"{"label": "Enu", "kind": 13, "preselect": true, "sortText": "11", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 49, "name": "Enu"}}"#
        )
    );
    assert_eq!(
        item(&response, "e"),
        tsgo(
            r#"{"label": "e", "kind": 6, "sortText": "11", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 49, "name": "e"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionsRecommended_equals.ts", "position": 49, "name": "undefined"}}"#
        )
    );
}

/// TestCompletionsDeprecatedTags: deprecated members sort last and are tagged.
#[test]
fn deprecated_members() {
    let files = [(
        "/completionsDeprecatedTags.ts",
        "const o = {\n    /** @deprecated */\n    a: 1,\n    b: 2,\n    c: 3,\n}\no.",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsDeprecatedTags.ts",
            (6, 2),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 5, "tags": [1], "sortText": "z11", "data": {"fileName": "/completionsDeprecatedTags.ts", "position": 69, "name": "a"}}, {"label": "b", "kind": 5, "sortText": "11", "data": {"fileName": "/completionsDeprecatedTags.ts", "position": 69, "name": "b"}}, {"label": "c", "kind": 5, "sortText": "11", "data": {"fileName": "/completionsDeprecatedTags.ts", "position": 69, "name": "c"}}]}"#
        ),
    );
}

/// TestCompletionsLiterals: the contextual type's literals, in the file's quotes.
#[test]
fn literals_of_the_contextual_type() {
    let files = [
        ("/completionsLiterals.ts", "const x: 0 | \"one\" = ;\nconst y: 0 | \"one\" | 1n = ;\nconst y2: 0 | \"one\" | 1n = 'one';"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionsLiterals.ts",
        (0, 21),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1080));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "0"),
        tsgo(
            r#"{"label": "0", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "0"}}"#
        )
    );
    assert_eq!(item(&response, "1n"), Value::Null);
    assert_eq!(
        item(&response, "x"),
        tsgo(
            r#"{"label": "x", "kind": 6, "sortText": "11", "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "x"}}"#
        )
    );
    assert_eq!(
        item(&response, "y"),
        tsgo(
            r#"{"label": "y", "kind": 6, "sortText": "11", "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "y"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "undefined"}}"#
        )
    );
    assert_eq!(
        item(&response, "Audio"),
        tsgo(
            r#"{"label": "Audio", "kind": 6, "sortText": "15", "data": {"fileName": "/completionsLiterals.ts", "position": 21, "name": "Audio"}}"#
        )
    );
}

/// TestCompletionExportFrom: only `from` after an export clause.
#[test]
fn from_after_an_export_clause() {
    let files = [("/completionExportFrom.ts", "export * ;\nexport {} ;")];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionExportFrom.ts",
            (0, 9),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "from", "kind": 14, "sortText": "15", "data": {"fileName": "/completionExportFrom.ts", "position": 9, "name": "from"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionExportFrom.ts",
            (1, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "from", "kind": 14, "sortText": "15", "data": {"fileName": "/completionExportFrom.ts", "position": 21, "name": "from"}}]}"#
        ),
    );
}

/// TestSymbolCompletionLowerPriority: a property named by a unique symbol
/// completes the symbol's name, among the globals.
#[test]
fn computed_symbol_property() {
    let files = [
        ("/symbolCompletionLowerPriority.ts", "declare const Symbol: (s: string) => symbol;\nconst mySymbol = Symbol(\"test\");\ninterface TestInterface { \n    [mySymbol]: string;\n    normalProperty: number;\n}\nconst obj: TestInterface = {} as any;\nobj."),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/symbolCompletionLowerPriority.ts",
            (7, 4),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "mySymbol", "kind": 6, "sortText": "15", "filterText": ".mySymbol", "insertText": "[mySymbol]", "textEdit": {"range": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 4}}, "newText": "[mySymbol]"}, "data": {"fileName": "/symbolCompletionLowerPriority.ts", "position": 201, "name": "mySymbol"}}, {"label": "normalProperty", "kind": 5, "sortText": "11", "data": {"fileName": "/symbolCompletionLowerPriority.ts", "position": 201, "name": "normalProperty"}}]}"#
        ),
    );
}

/// TestSwitchCompletions: after `case E.`, the enum's members.
#[test]
fn enum_members_in_a_case() {
    let files = [
        ("/switchCompletions.ts", "enum E { A, B }\ndeclare const e: E;\nswitch (e) {\n    case E.A:\n        return 0;\n    case E.\n}\ndeclare const f: 1 | 2 | 3;\nswitch (f) {\n    case 1:\n        return 1;\n    case \n}\ndeclare const f2: 'foo' | 'bar' | 'baz';\nswitch (f2) {\n    case 'bar':\n        return 1;\n    case ''\n}\n\n// repro from #52874\ndeclare let x: \"foo\" | \"bar\";\nswitch (x) {\n    case ('')\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/switchCompletions.ts",
            (5, 11),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "B", "kind": 20, "sortText": "11", "data": {"fileName": "/switchCompletions.ts", "position": 92, "name": "B"}}]}"#
        ),
    );
}

/// TestCompletionInFunctionLikeBody_includesPrimitiveTypes: `new Bar<` is a type
/// argument position even for a class without type parameters, as tsgo's class
/// construct signatures always have a type parameter list.
#[test]
fn type_arguments_of_a_new_expression() {
    let files = [
        ("/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "class Foo<T> { }\nclass Bar { }\nfunction includesTypes() {\n    new Foo<\n}\nfunction excludesTypes1() {\n    new Bar<\n}\nfunction excludesTypes2() {\n    1<\n}"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionInFunctionLikeBody_includesPrimitiveTypes.ts",
        (3, 12),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2055));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "Foo"),
        tsgo(
            r#"{"label": "Foo", "kind": 7, "sortText": "11", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 70, "name": "Foo"}}"#
        )
    );
    assert_eq!(
        item(&response, "Bar"),
        tsgo(
            r#"{"label": "Bar", "kind": 7, "sortText": "11", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 70, "name": "Bar"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 70, "name": "undefined"}}"#
        )
    );
    assert_eq!(item(&response, "Audio"), Value::Null);
    assert_eq!(
        item(&response, "ANGLE_instanced_arrays"),
        tsgo(
            r#"{"label": "ANGLE_instanced_arrays", "kind": 8, "sortText": "15", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 70, "name": "ANGLE_instanced_arrays"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionInFunctionLikeBody_includesPrimitiveTypes.ts",
        (6, 12),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2055));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "Foo"),
        tsgo(
            r#"{"label": "Foo", "kind": 7, "sortText": "11", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 113, "name": "Foo"}}"#
        )
    );
    assert_eq!(
        item(&response, "Bar"),
        tsgo(
            r#"{"label": "Bar", "kind": 7, "sortText": "11", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 113, "name": "Bar"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 113, "name": "undefined"}}"#
        )
    );
    assert_eq!(item(&response, "Audio"), Value::Null);
    assert_eq!(
        item(&response, "ANGLE_instanced_arrays"),
        tsgo(
            r#"{"label": "ANGLE_instanced_arrays", "kind": 8, "sortText": "15", "data": {"fileName": "/completionInFunctionLikeBody_includesPrimitiveTypes.ts", "position": 113, "name": "ANGLE_instanced_arrays"}}"#
        )
    );
}

/// TestCompletionListAfterFunction: after a function declaration without a body,
/// every keyword.
#[test]
fn keywords_after_a_bodyless_function() {
    let files = [
        ("/completionListAfterFunction.ts", "// Outside the function\ndeclare function f1(a: number);\n\n// inside the function\ndeclare function f2(b: number, b2 = \n\n// Outside the function\nfunction f3(c: number) { }\n\n// inside the function\nfunction f4(d: number) { }"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionListAfterFunction.ts",
        (1, 31),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1078));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "f1"),
        tsgo(
            r#"{"label": "f1", "kind": 3, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 55, "name": "f1"}}"#
        )
    );
    assert_eq!(item(&response, "b"), Value::Null);
    assert_eq!(
        item(&response, "abstract"),
        tsgo(
            r#"{"label": "abstract", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 55, "name": "abstract"}}"#
        )
    );
    assert_eq!(
        item(&response, "declare"),
        tsgo(
            r#"{"label": "declare", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 55, "name": "declare"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 55, "name": "undefined"}}"#
        )
    );
    assert_eq!(item(&response, "arguments"), Value::Null);
    let response = completion(
        &files,
        compiler_options(),
        "/completionListAfterFunction.ts",
        (4, 36),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1080));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "f1"),
        tsgo(
            r#"{"label": "f1", "kind": 3, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "f1"}}"#
        )
    );
    assert_eq!(
        item(&response, "b"),
        tsgo(
            r#"{"label": "b", "kind": 6, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "b"}}"#
        )
    );
    assert_eq!(
        item(&response, "abstract"),
        tsgo(
            r#"{"label": "abstract", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "abstract"}}"#
        )
    );
    assert_eq!(
        item(&response, "declare"),
        tsgo(
            r#"{"label": "declare", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "declare"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "undefined"}}"#
        )
    );
    assert_eq!(
        item(&response, "arguments"),
        tsgo(
            r#"{"label": "arguments", "kind": 6, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 116, "name": "arguments"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListAfterFunction.ts",
        (7, 26),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1063));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "f1"),
        tsgo(
            r#"{"label": "f1", "kind": 3, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 168, "name": "f1"}}"#
        )
    );
    assert_eq!(
        item(&response, "b"),
        tsgo(
            r#"{"label": "b", "kind": 6, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 168, "name": "b"}}"#
        )
    );
    assert_eq!(item(&response, "abstract"), Value::Null);
    assert_eq!(item(&response, "declare"), Value::Null);
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 168, "name": "undefined"}}"#
        )
    );
    assert_eq!(
        item(&response, "arguments"),
        tsgo(
            r#"{"label": "arguments", "kind": 6, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 168, "name": "arguments"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListAfterFunction.ts",
        (10, 25),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1062));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "f1"),
        tsgo(
            r#"{"label": "f1", "kind": 3, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 218, "name": "f1"}}"#
        )
    );
    assert_eq!(item(&response, "b"), Value::Null);
    assert_eq!(item(&response, "abstract"), Value::Null);
    assert_eq!(item(&response, "declare"), Value::Null);
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionListAfterFunction.ts", "position": 218, "name": "undefined"}}"#
        )
    );
    assert_eq!(
        item(&response, "arguments"),
        tsgo(
            r#"{"label": "arguments", "kind": 6, "sortText": "11", "data": {"fileName": "/completionListAfterFunction.ts", "position": 218, "name": "arguments"}}"#
        )
    );
}

/// TestCompletionListBuilderLocations_parameters: at a constructor parameter, the
/// parameter property modifiers, `override` among them.
#[test]
fn constructor_parameter_keywords() {
    let files = [
        ("/completionListBuilderLocations_parameters.ts", "var aa = 1;\nclass bar1{ constructor(\nclass bar2{ constructor(a\nclass bar3{ constructor(a, \nclass bar4{ constructor(a, b\nclass bar6{ constructor(public a, \nclass bar7{ constructor(private a, "),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (1, 24),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 36, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 36, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 36, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 36, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 36, "name": "override"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (2, 25),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [], "editRange": {"insert": {"start": {"line": 2, "character": 24}, "end": {"line": 2, "character": 25}}, "replace": {"start": {"line": 2, "character": 24}, "end": {"line": 2, "character": 25}}}}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 62, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 62, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 62, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 62, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 62, "name": "override"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (3, 27),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 90, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 90, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 90, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 90, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 90, "name": "override"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (4, 28),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [], "editRange": {"insert": {"start": {"line": 4, "character": 27}, "end": {"line": 4, "character": 28}}, "replace": {"start": {"line": 4, "character": 27}, "end": {"line": 4, "character": 28}}}}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 119, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 119, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 119, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 119, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 119, "name": "override"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (5, 34),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 154, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 154, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 154, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 154, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 154, "name": "override"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListBuilderLocations_parameters.ts",
            (6, 35),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 190, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 190, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 190, "name": "public"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 190, "name": "readonly"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListBuilderLocations_parameters.ts", "position": 190, "name": "override"}}]}"#
        ),
    );
}

/// TestDoubleUnderscoreCompletions: in an unchecked JavaScript file, the file's
/// identifiers follow the members.
#[test]
fn javascript_identifiers() {
    let files = [
        ("/a.js", "function MyObject(){\n    this.__property = 1;\n}\nvar instance = new MyObject();\ninstance."),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/a.js", (4, 9), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "instance", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/a.js", "position": 88, "name": "instance"}}, {"label": "MyObject", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/a.js", "position": 88, "name": "MyObject"}}, {"label": "__property", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/a.js", "position": 88, "name": "__property"}}]}"#
        ),
    );
}

/// TestCompletionInJsDoc: in a JSDoc type of a JavaScript file, the file's
/// identifiers too.
#[test]
fn javascript_identifiers_in_a_jsdoc_type() {
    let files = [
        ("/Foo.js", "/** @ */\nvar v1;\n\n/** @p */\nvar v2;\n\n/** @param  */\nvar v3;\n\n/** @param { n } bar */\nvar v4;\n\n/** @type { n } */\nvar v5;\n\n// @\nvar v6;\n\n// @pa\nvar v7;\n\n/** @return { n } */\nvar v8;\n\n/**  */\n\n/**\n \n*/\n\n/**\n * \n */\n\n/**\n          \n */\n\n/**\n  *       \n  */\n\n/**\n  * some comment \n  */\n\n/**\n  * @param \n  */\n\n/** @param  */\n\n/**\n  * jsdoc inline tag {@}\n  */"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    let response = completion(&files, compiler_options(), "/Foo.js", (9, 14), None);
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2063));
    assert_eq!(
        response["itemDefaults"],
        tsgo(
            r#"{"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 9, "character": 13}, "end": {"line": 9, "character": 14}}, "replace": {"start": {"line": 9, "character": 13}, "end": {"line": 9, "character": 14}}}}"#
        )
    );
    assert_eq!(
        item(&response, "n"),
        tsgo(
            r#"{"label": "n", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/Foo.js", "position": 75, "name": "n"}}"#
        )
    );
    assert_eq!(
        item(&response, "v1"),
        tsgo(
            r#"{"label": "v1", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/Foo.js", "position": 75, "name": "v1"}}"#
        )
    );
    assert_eq!(
        item(&response, "v8"),
        tsgo(
            r#"{"label": "v8", "kind": 1, "sortText": "18", "commitCharacters": [], "data": {"fileName": "/Foo.js", "position": 75, "name": "v8"}}"#
        )
    );
    assert_eq!(
        item(&response, "number"),
        tsgo(
            r#"{"label": "number", "kind": 14, "sortText": "15", "data": {"fileName": "/Foo.js", "position": 75, "name": "number"}}"#
        )
    );
}
/// TestArgumentCompletions: after an argument's comma (a spread tuple's
/// elements counted), the parameter's literals.
#[test]
fn literals_of_an_argument_after_a_comma() {
    let files = [
        ("/argumentCompletions.ts", "\nfunction foo(a: \"a\", b: \"b\") {}\nfoo(\"a\", );\n\n\nconst t3 = ['x', 'y', 'z'] as const;\nconst x: [string, string, string, 'a' | 'b'] = [...t3, ];\n"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/argumentCompletions.ts",
        (2, 9),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1079));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(item(&response, "\"a\""), Value::Null);
    assert_eq!(
        item(&response, "\"b\""),
        tsgo(
            r#"{"label": "\"b\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/argumentCompletions.ts", "position": 42, "name": "\"b\""}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/argumentCompletions.ts", "position": 42, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/argumentCompletions.ts",
        (6, 55),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1079));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"a\""),
        tsgo(
            r#"{"label": "\"a\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/argumentCompletions.ts", "position": 139, "name": "\"a\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"b\""),
        tsgo(
            r#"{"label": "\"b\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/argumentCompletions.ts", "position": 139, "name": "\"b\""}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/argumentCompletions.ts", "position": 139, "name": "undefined"}}"#
        )
    );
}
/// TestCompletionEntryForArgumentConstrainedToString: after `(`, the literals of
/// the argument's constraint.
#[test]
fn literals_of_a_type_parameter_constraint() {
    let files = [(
        "/completionEntryForArgumentConstrainedToString.ts",
        "declare function test<P extends \"a\" | \"b\">(p: P): void;\n\ntest()\n",
    )];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionEntryForArgumentConstrainedToString.ts",
        (2, 5),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1078));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"a\""),
        tsgo(
            r#"{"label": "\"a\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionEntryForArgumentConstrainedToString.ts", "position": 62, "name": "\"a\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"b\""),
        tsgo(
            r#"{"label": "\"b\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionEntryForArgumentConstrainedToString.ts", "position": 62, "name": "\"b\""}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionEntryForArgumentConstrainedToString.ts", "position": 62, "name": "undefined"}}"#
        )
    );
}
/// TestCompletionsInArrayLiteralWithContextualType: after an element's comma, the
/// next tuple element's literals.
#[test]
fn literals_of_a_tuple_element() {
    let files = [(
        "/completionsInArrayLiteralWithContextualType.ts",
        "let z: [\"a\", \"b\" | \"c\"] = [\"a\", ];",
    )];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionsInArrayLiteralWithContextualType.ts",
        (0, 32),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1077));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"b\""),
        tsgo(
            r#"{"label": "\"b\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionsInArrayLiteralWithContextualType.ts", "position": 32, "name": "\"b\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"c\""),
        tsgo(
            r#"{"label": "\"c\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionsInArrayLiteralWithContextualType.ts", "position": 32, "name": "\"c\""}}"#
        )
    );
    assert_eq!(item(&response, "\"a\""), Value::Null);
}
/// TestCompletionForStringLiteral_quotePreference1: after a property's colon (a
/// token of the property assignment), the property's literals.
#[test]
fn literals_of_an_object_literal_property() {
    let files = [
        ("/completionForStringLiteral_quotePreference1.ts", "enum A {\n    A,\n    B,\n    C\n}\ninterface B {\n    a: keyof typeof A;\n}\nconst b: B = {\n    a: \n}"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionForStringLiteral_quotePreference1.ts",
        (9, 7),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1079));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"A\""),
        tsgo(
            r#"{"label": "\"A\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionForStringLiteral_quotePreference1.ts", "position": 92, "name": "\"A\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"B\""),
        tsgo(
            r#"{"label": "\"B\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionForStringLiteral_quotePreference1.ts", "position": 92, "name": "\"B\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"C\""),
        tsgo(
            r#"{"label": "\"C\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionForStringLiteral_quotePreference1.ts", "position": 92, "name": "\"C\""}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionForStringLiteral_quotePreference1.ts", "position": 92, "name": "undefined"}}"#
        )
    );
}
/// TestCompletionEntryForPropertyConstrainedToString: a property of an argument
/// takes the literals of its type parameter's constraint.
#[test]
fn literals_of_an_inferred_property() {
    let files = [(
        "/completionEntryForPropertyConstrainedToString.ts",
        "declare function test<P extends \"a\" | \"b\">(p: { type: P }): void;\n\ntest({ type:  })",
    )];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionEntryForPropertyConstrainedToString.ts",
        (2, 13),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1078));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"a\""),
        tsgo(
            r#"{"label": "\"a\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionEntryForPropertyConstrainedToString.ts", "position": 80, "name": "\"a\""}}"#
        )
    );
    assert_eq!(
        item(&response, "\"b\""),
        tsgo(
            r#"{"label": "\"b\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionEntryForPropertyConstrainedToString.ts", "position": 80, "name": "\"b\""}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 6, "sortText": "15", "data": {"fileName": "/completionEntryForPropertyConstrainedToString.ts", "position": 80, "name": "undefined"}}"#
        )
    );
}
/// TestCompletionListInTypeLiteralInTypeParameter17: at a property of an object
/// type in a type argument, the literals of the type parameter's constraint.
#[test]
fn literals_of_a_type_argument_property() {
    let files = [
        ("/completionListInTypeLiteralInTypeParameter17.ts", "class Foo<T extends { x: 'one' | 2 }> {}\nfunction foo<T extends { x: 'one' | 2 }>() {}\n\ntype A = Foo<{ x:  }>;\nnew Foo<{ x:  }>();\nfoo<{ x:  }>();\nfoo<{ x:  }>;\nFoo<{ x:  }>;"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter17.ts",
        (3, 18),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 106, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 106, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 106, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter17.ts",
        (4, 13),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 124, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 124, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 124, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter17.ts",
        (5, 9),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 140, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 140, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 140, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter17.ts",
        (6, 9),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 156, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 156, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 156, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter17.ts",
        (7, 9),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 170, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 170, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter17.ts", "position": 170, "name": "undefined"}}"#
        )
    );
}
/// TestCompletionListInTypeLiteralInTypeParameter21: after `[` of a type argument,
/// the element literals of the constraint.
#[test]
fn literals_of_a_type_argument_element() {
    let files = [
        ("/completionListInTypeLiteralInTypeParameter21.ts", "class Foo<T extends ('one' | 2)[]> {}\nfunction foo<T extends ('one' | 2)[]>() {}\n\ntype A = Foo<[]>;\nnew Foo<[]>();\nfoo<[]>();\nfoo<[]>;\nFoo<[]>;"),
    ];
    let compiler_options = || options(|_| {});
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter21.ts",
        (3, 14),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 96, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 96, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 96, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter21.ts",
        (4, 9),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 109, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 109, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 109, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter21.ts",
        (5, 5),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 120, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 120, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 120, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter21.ts",
        (6, 5),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 131, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 131, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 131, "name": "undefined"}}"#
        )
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListInTypeLiteralInTypeParameter21.ts",
        (7, 5),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(2057));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "\"one\""),
        tsgo(
            r#"{"label": "\"one\"", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 140, "name": "\"one\""}}"#
        )
    );
    assert_eq!(
        item(&response, "2"),
        tsgo(
            r#"{"label": "2", "kind": 21, "sortText": "11", "commitCharacters": [], "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 140, "name": "2"}}"#
        )
    );
    assert_eq!(
        item(&response, "undefined"),
        tsgo(
            r#"{"label": "undefined", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter21.ts", "position": 140, "name": "undefined"}}"#
        )
    );
}

/// TestCompletionListInObjectLiteral7: in an object literal argument, the
/// parameter type's properties.
#[test]
fn object_literal_properties() {
    let files = [
        ("/completionListInObjectLiteral7.ts", "type Foo = { foo: boolean };\nfunction f<T>(shape: Foo): any;\nfunction f<T>(shape: () => Foo): any;\nfunction f(arg: any) {\n  return arg;\n}\n\nf({  });\nf(() => ({  }));\nf(() => (({  })));"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInObjectLiteral7.ts",
            (7, 4),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListInObjectLiteral7.ts", "position": 143, "name": "foo"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInObjectLiteral7.ts",
            (8, 11),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListInObjectLiteral7.ts", "position": 159, "name": "foo"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInObjectLiteral7.ts",
            (9, 12),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListInObjectLiteral7.ts", "position": 177, "name": "foo"}}]}"#
        ),
    );
}
/// TestCompletionListInObjectLiteral8: an optional property sorts after the others
/// and is labeled with `?`.
#[test]
fn optional_object_literal_property() {
    let files = [
        ("/completionListInObjectLiteral8.ts", "declare function test<\n  Variants extends Partial<Record<'hover' | 'pressed', string>>,\n>(v: Variants): void\n\ntest({\n  hover: \"\",\n  \n});"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInObjectLiteral8.ts",
            (6, 2),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "pressed?", "kind": 5, "sortText": "12", "filterText": "pressed", "insertText": "pressed", "data": {"fileName": "/completionListInObjectLiteral8.ts", "position": 132, "name": "pressed"}}]}"#
        ),
    );
}
/// TestCompletionListInObjectBindingPattern16: a destructured JavaScript
/// parameter takes its type from its `@param` tag.
#[test]
fn binding_pattern_of_a_jsdoc_parameter() {
    let files = [
        ("/a.js", "/**\n * @typedef Foo\n * @property {number} a\n * @property {string} b\n */\n\n/**\n * @param {Foo} options\n */\nfunction f({  }) {}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
            options.check_js = Some(true);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/a.js", (9, 13), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 5, "sortText": "11", "data": {"fileName": "/a.js", "position": 118, "name": "a"}}, {"label": "b", "kind": 5, "sortText": "11", "data": {"fileName": "/a.js", "position": 118, "name": "b"}}]}"#
        ),
    );
}
/// TestCompletionListForExportEquals2: in named imports, the module's exports and
/// its `export =` value's properties.
#[test]
fn named_imports_of_an_export_equals() {
    let files = [
        ("/a.ts", "import {  } from \"foo\";"),
        ("/node_modules/foo/index.d.ts", "export = Foo;\ninterface Foo { bar: number; }\ndeclare namespace Foo {\n    interface Static {}\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/a.ts", (0, 9), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "Static", "kind": 8, "sortText": "11", "data": {"fileName": "/a.ts", "position": 9, "name": "Static"}}, {"label": "type", "kind": 14, "sortText": "15", "data": {"fileName": "/a.ts", "position": 9, "name": "type"}}]}"#
        ),
    );
}
/// TestCompletionsGenericIndexedAccess6: a JSX element's attributes.
#[test]
fn jsx_attributes() {
    let files = [
        ("/component.tsx", "interface CustomElements {\n  'component-one': {\n      foo?: string;\n  },\n  'component-two': {\n      bar?: string;\n  }\n}\n\ntype Options<T extends keyof CustomElements> = { kind: T } & Required<{ x: CustomElements[(T extends string ? T : never) & string] }['x']>;\n\ndeclare function Component<T extends keyof CustomElements>(props: Options<T>): void;\n\nconst c = <Component  kind=\"component-one\" />"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/component.tsx", (13, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 5, "sortText": "11", "data": {"fileName": "/component.tsx", "position": 369, "name": "foo"}}]}"#
        ),
    );
}
/// TestTsxCompletion10: at a closing tag, the opening tag's name.
#[test]
fn jsx_closing_tag() {
    let files = [
        ("/file.tsx", "declare namespace JSX {\n    interface Element { }\n    interface IntrinsicElements {\n        div: { ONE: string; TWO: number; }\n    }\n}\nvar x1 = <div></"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/file.tsx", (6, 16), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 6, "character": 16}, "end": {"line": 6, "character": 16}}, "replace": {"start": {"line": 6, "character": 16}, "end": {"line": 6, "character": 16}}}}, "items": [{"label": "div>", "kind": 7, "sortText": "11", "data": {"fileName": "/file.tsx", "position": 151, "name": "div>"}}]}"#
        ),
    );
}
/// TestCompletionListWithLabel: after `break` or `continue`, the labels around.
#[test]
fn labels() {
    let files = [
        ("/completionListWithLabel.ts", " label: while (true) {\n    break \n    continue \n    testlabel: while (true) {\n        break \n        continue \n        break tes\n        continue tes\n    }\n    break \n    break; \n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (1, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 33, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (2, 13),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 47, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (9, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 166, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (4, 14),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "testlabel", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 92, "name": "testlabel"}}, {"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 92, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (5, 17),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "testlabel", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 110, "name": "testlabel"}}, {"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 110, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (6, 17),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 6, "character": 14}, "end": {"line": 6, "character": 17}}, "replace": {"start": {"line": 6, "character": 14}, "end": {"line": 6, "character": 17}}}}, "items": [{"label": "testlabel", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 128, "name": "testlabel"}}, {"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 128, "name": "label"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListWithLabel.ts",
            (7, 20),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 7, "character": 17}, "end": {"line": 7, "character": 20}}, "replace": {"start": {"line": 7, "character": 17}, "end": {"line": 7, "character": 20}}}}, "items": [{"label": "testlabel", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 149, "name": "testlabel"}}, {"label": "label", "kind": 10, "sortText": "11", "data": {"fileName": "/completionListWithLabel.ts", "position": 149, "name": "label"}}]}"#
        ),
    );
    let response = completion(
        &files,
        compiler_options(),
        "/completionListWithLabel.ts",
        (10, 11),
        None,
    );
    assert_eq!(response["items"].as_array().map(Vec::len), Some(1075));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
}
/// TestCompletionListInNamedClassExpressionWithShadowing: in a class body, the
/// class member keywords.
#[test]
fn class_member_keywords() {
    let files = [
        ("/completionListInNamedClassExpressionWithShadowing.ts", "class myClass {  }\n\nvar x = class myClass {\n   getClassName (){\n       m\n   }\n   \n}\nvar y = class {\n   getSomeName() {\n       \n   }\n   \n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInNamedClassExpressionWithShadowing.ts",
            (0, 16),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "private", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "private"}}, {"label": "protected", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "protected"}}, {"label": "public", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "public"}}, {"label": "static", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "static"}}, {"label": "abstract", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "abstract"}}, {"label": "accessor", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "accessor"}}, {"label": "async", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "async"}}, {"label": "constructor", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "constructor"}}, {"label": "declare", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "declare"}}, {"label": "get", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "get"}}, {"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "readonly"}}, {"label": "set", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "set"}}, {"label": "override", "kind": 14, "sortText": "15", "data": {"fileName": "/completionListInNamedClassExpressionWithShadowing.ts", "position": 16, "name": "override"}}]}"#
        ),
    );
}
/// TestCompletionsAfterJSDoc: in an interface body, `readonly` only.
#[test]
fn interface_member_keyword() {
    let files = [(
        "/completionsAfterJSDoc.ts",
        "export interface Foo {\n  /** JSDoc */\n  foo(): void;\n}",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsAfterJSDoc.ts",
            (2, 2),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [], "editRange": {"insert": {"start": {"line": 2, "character": 2}, "end": {"line": 2, "character": 2}}, "replace": {"start": {"line": 2, "character": 2}, "end": {"line": 2, "character": 5}}}}, "items": [{"label": "readonly", "kind": 14, "sortText": "15", "data": {"fileName": "/completionsAfterJSDoc.ts", "position": 40, "name": "readonly"}}]}"#
        ),
    );
}
/// TestCompletionListInTypeLiteralInTypeParameter8: in an object type argument, the
/// constraint's members not written yet.
#[test]
fn type_argument_object_type_members() {
    let files = [
        ("/completionListInTypeLiteralInTypeParameter8.ts", "interface Foo {\n    one: string;\n    two: {\n        three: {\n            four: number;\n            five: string;\n        }\n    }\n}\n\ninterface Bar<T extends Foo> {\n    foo: T;\n}\n\nvar foobar: Bar<{\n    two: {\n        three: {\n            five: string,\n            \n        },\n        \n    },\n    \n}>;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter8.ts",
            (18, 12),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "four", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter8.ts", "position": 262, "name": "four"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter8.ts",
            (20, 8),
            None
        ),
        tsgo(r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": []}"#),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter8.ts",
            (22, 4),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "one", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListInTypeLiteralInTypeParameter8.ts", "position": 294, "name": "one"}}]}"#
        ),
    );
}
/// TestJsxAttributeSnippetCompletionUnclosed: with braces preferred, an attribute
/// inserts `name={$1}`.
#[test]
fn jsx_attribute_snippets() {
    let files = [
        ("/file.tsx", "interface NestedInterface {\n    Foo: NestedInterface;\n    (props: {className?: string}): any;\n}\n\ndeclare const Foo: NestedInterface;\n\nfunction fn1() {\n    return <Foo>\n        <Foo \n    </Foo>\n}\nfunction fn2() {\n    return <Foo>\n        <Foo.Foo \n    </Foo>\n}\nfunction fn3() {\n    return <Foo>\n        <Foo.Foo cla\n    </Foo>\n}\nfunction fn4() {\n    return <Foo>\n        <Foo.Foo cla something\n    </Foo>\n}\nfunction fn5() {\n    return <Foo>\n        <Foo.Foo something \n    </Foo>\n}\nfunction fn6() {\n    return <Foo>\n        <Foo.Foo something cla\n    </Foo>\n}\nfunction fn7() {\n    return <Foo \n}\nfunction fn8() {\n    return <Foo cla\n}\nfunction fn9() {\n    return <Foo cla something\n}\nfunction fn10() {\n    return <Foo something \n}\nfunction fn11() {\n    return <Foo something cla\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.strict = Some(false);
        })
    };
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            UserPreferences {
                completion_preferences: CompletionPreferences {
                    jsx_attribute_completion_style: JsxAttributeCompletionStyle::BRACES,
                    ..CompletionPreferences::default()
                },
                ..UserPreferences::default()
            },
            true,
            "/file.tsx",
            (9, 13),
            None,
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "className?", "kind": 5, "sortText": "12", "filterText": "className", "insertText": "className={$1}", "insertTextFormat": 2, "data": {"fileName": "/file.tsx", "position": 181, "name": "className"}}]}"#
        ),
    );
}
