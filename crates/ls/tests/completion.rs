//! Completions as tsgo answers them, for the cases the completion port's
//! fixes reach and the conformance baselines do not show. Each case is a
//! fourslash test's input with tsgo's answer, which the lsp suite's
//! transcripts record (`scripts/lsp_transcripts.py`). The items are compared
//! in any order, as the transcripts' digests compare them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tsc_checker::ImportModuleSpecifierEndingPreference;
use tsc_compiler::LiveProgram;
use tsc_diagnostics::{JsStr, JsString};
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_host::{CompilerHost, MemoryCompilerHost};
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::{
    CompletionPreferences, JsxAttributeCompletionStyle, QuotePreference, UserPreferences,
};
use tsc_ls::LanguageService;
use tsc_lsproto::{
    CompletionContext, CompletionParams, CompletionTriggerKind, DocumentUri, MarkupKind, Position,
    PositionEncodingKind, ResolvedClientCapabilities, TextDocumentIdentifier,
};
use tsc_program::{
    load_program, CompilerConfigHost, ConfigParseHost, LibraryCatalog, ProgramLoadLimits,
    ProgramOptions,
};
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

/// The test's files as the language service's host, and as its file
/// system (without the libraries, which tsgo bundles).
struct Files(BTreeMap<String, String>, MemoryCompilerHost);

impl SourceMapperHost for Files {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.0.get(file_name).cloned()
    }
}

impl tsc_ls::Host for Files {
    fn read_directory(
        &self,
        path: &str,
        extensions: &[&str],
        excludes: &[&str],
        includes: &[&str],
        depth: Option<usize>,
    ) -> Vec<String> {
        let patterns = |patterns: &[&str]| {
            patterns
                .iter()
                .map(|&pattern| JsString::from(pattern))
                .collect::<Vec<_>>()
        };
        let (excludes, includes) = (patterns(excludes), patterns(includes));
        CompilerConfigHost::new(&self.1)
            .read_directory(
                JsStr::from_str(path),
                extensions,
                Some(&excludes),
                Some(&includes),
                depth,
            )
            .expect("the memory host's listing")
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect()
    }

    /// The names of the directories (the memory host lists their paths).
    fn get_directories(&self, path: &str) -> Vec<String> {
        self.1
            .get_directories_js(JsStr::from_str(path))
            .unwrap_or_default()
            .iter()
            .map(|directory| {
                tsc_program::base_file_name(directory.as_js())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.1
            .directory_exists_js(JsStr::from_str(path))
            .unwrap_or(false)
    }

    fn file_exists(&self, path: &str) -> bool {
        self.1
            .file_exists_js(JsStr::from_str(path))
            .unwrap_or(false)
    }
}

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
    with_service(files, options, preferences, snippets, |service| {
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
    })
}

/// tsgo's `completionItem/resolve` of `item` (an item tsgo's completion
/// gave, with its data), for the fourslash client.
fn resolve(files: &[(&str, &str)], options: CompilerOptions, item: &str) -> Value {
    with_service(
        files,
        options,
        UserPreferences::default(),
        false,
        |service| {
            let item = tsc_lsproto::from_str(item).expect("tsgo's item");
            let response = service.resolve_completion_item(item).expect("an answer");
            serde_json::to_value(&response).expect("the response as JSON")
        },
    )
}

/// The language service over the files, for the fourslash client (which
/// takes snippets or not).
fn with_service(
    files: &[(&str, &str)],
    options: CompilerOptions,
    preferences: UserPreferences,
    snippets: bool,
    run: impl FnOnce(&mut LanguageService<'_>) -> Value,
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
    let mut file_system = MemoryCompilerHost::builder("/").case_sensitive(true);
    for (name, text) in &texts {
        file_system = file_system.file(PathBuf::from(name), text.as_bytes());
    }
    let files = Files(texts, file_system.build().expect("memory host"));
    let mut capabilities = ResolvedClientCapabilities::default();
    let completion = &mut capabilities.text_document.completion;
    completion.completion_item.commit_characters_support = true;
    completion.completion_item.insert_replace_support = true;
    completion.completion_item.preselect_support = true;
    completion.completion_item.snippet_support = snippets;
    completion.completion_item.documentation_format =
        vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT];
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
    run(&mut service)
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
/// The string literal types of a contextual union, at a variable's initializer.
#[test]
fn string_literal_types_of_a_contextual_union() {
    let files = [
        ("/completionForStringLiteral.ts", "type Options = \"Option 1\" | \"Option 2\" | \"Option 3\";\nvar x: Options = \"Option 3\";\n\nfunction f(a: Options) { };\nf(\""),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionForStringLiteral.ts",
            (1, 18),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "Option 1", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 18}, "end": {"line": 1, "character": 26}}, "newText": "Option 1"}, "data": {"fileName": "/completionForStringLiteral.ts", "position": 71, "name": "Option 1"}}, {"label": "Option 2", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 18}, "end": {"line": 1, "character": 26}}, "newText": "Option 2"}, "data": {"fileName": "/completionForStringLiteral.ts", "position": 71, "name": "Option 2"}}, {"label": "Option 3", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 18}, "end": {"line": 1, "character": 26}}, "newText": "Option 3"}, "data": {"fileName": "/completionForStringLiteral.ts", "position": 71, "name": "Option 3"}}]}"#
        ),
    );
}

/// The quoted property names of an object literal's contextual type.
#[test]
fn quoted_property_names_of_an_object_literal() {
    let files = [
        ("/completionForQuotedPropertyInPropertyAssignment1.ts", "export interface Configfiles {\n  jspm: string;\n  'jspm:browser': string;\n}\nlet files: Configfiles;\nfiles = {\n   : '',\n   '': ''\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionForQuotedPropertyInPropertyAssignment1.ts",
            (7, 4),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 4}}, "replace": {"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 4}}}}, "items": [{"label": "jspm", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 4}}, "newText": "jspm"}, "data": {"fileName": "/completionForQuotedPropertyInPropertyAssignment1.ts", "position": 122, "name": "jspm"}}, {"label": "jspm:browser", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 4}}, "newText": "jspm:browser"}, "data": {"fileName": "/completionForQuotedPropertyInPropertyAssignment1.ts", "position": 122, "name": "jspm:browser"}}]}"#
        ),
    );
}

/// The property names of an element access, a new identifier with an index signature.
#[test]
fn property_names_of_an_element_access() {
    let files = [
        ("/completionForStringLiteral2.ts", "var o = {\n    foo() { },\n    bar: 0,\n    \"some other name\": 1\n};\ndeclare const p: { [s: string]: any, a: number };\n\no[\"bar\"];\no[\" ;\np[\"\"];"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionForStringLiteral2.ts",
            (7, 3),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 3}}, "replace": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 6}}}}, "items": [{"label": "foo", "kind": 2, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 6}}, "newText": "foo"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 119, "name": "foo"}}, {"label": "bar", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 6}}, "newText": "bar"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 119, "name": "bar"}}, {"label": "some other name", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 6}}, "newText": "some other name"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 119, "name": "some other name"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionForStringLiteral2.ts",
            (8, 3),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 8, "character": 3}, "end": {"line": 8, "character": 3}}, "replace": {"start": {"line": 8, "character": 3}, "end": {"line": 8, "character": 3}}}}, "items": [{"label": "foo", "kind": 2, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 3}, "end": {"line": 8, "character": 3}}, "newText": "foo"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 129, "name": "foo"}}, {"label": "bar", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 3}, "end": {"line": 8, "character": 3}}, "newText": "bar"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 129, "name": "bar"}}, {"label": "some other name", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 3}, "end": {"line": 8, "character": 3}}, "newText": "some other name"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 129, "name": "some other name"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionForStringLiteral2.ts",
            (9, 3),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [], "editRange": {"insert": {"start": {"line": 9, "character": 3}, "end": {"line": 9, "character": 3}}, "replace": {"start": {"line": 9, "character": 3}, "end": {"line": 9, "character": 3}}}}, "items": [{"label": "a", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 3}, "end": {"line": 9, "character": 3}}, "newText": "a"}, "data": {"fileName": "/completionForStringLiteral2.ts", "position": 135, "name": "a"}}]}"#
        ),
    );
}

/// The property names of an indexed access type, less those in the union already.
#[test]
fn property_names_of_an_indexed_access_type() {
    let files = [
        ("/stringLiteralCompletionsForTypeIndexedAccess.ts", "type Foo = { a: string; b: number; c: boolean; };\ntype A = Foo[\"\"];\ntype AorB = Foo[\"a\" | \"\"];"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringLiteralCompletionsForTypeIndexedAccess.ts",
            (1, 14),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}, "replace": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}}}, "items": [{"label": "a", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}, "newText": "a"}, "data": {"fileName": "/stringLiteralCompletionsForTypeIndexedAccess.ts", "position": 64, "name": "a"}}, {"label": "b", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}, "newText": "b"}, "data": {"fileName": "/stringLiteralCompletionsForTypeIndexedAccess.ts", "position": 64, "name": "b"}}, {"label": "c", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}, "newText": "c"}, "data": {"fileName": "/stringLiteralCompletionsForTypeIndexedAccess.ts", "position": 64, "name": "c"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringLiteralCompletionsForTypeIndexedAccess.ts",
            (2, 23),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"], "editRange": {"insert": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 23}}, "replace": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 23}}}}, "items": [{"label": "b", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 23}}, "newText": "b"}, "data": {"fileName": "/stringLiteralCompletionsForTypeIndexedAccess.ts", "position": 91, "name": "b"}}, {"label": "c", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 23}}, "newText": "c"}, "data": {"fileName": "/stringLiteralCompletionsForTypeIndexedAccess.ts", "position": 91, "name": "c"}}]}"#
        ),
    );
}

/// The string literals of a switch's type that no case clause has.
#[test]
fn string_literals_of_case_clauses() {
    let files = [
        ("/switchCompletions.ts", "enum E { A, B }\ndeclare const e: E;\nswitch (e) {\n    case E.A:\n        return 0;\n    case E.\n}\ndeclare const f: 1 | 2 | 3;\nswitch (f) {\n    case 1:\n        return 1;\n    case \n}\ndeclare const f2: 'foo' | 'bar' | 'baz';\nswitch (f2) {\n    case 'bar':\n        return 1;\n    case ''\n}\n\n// repro from #52874\ndeclare let x: \"foo\" | \"bar\";\nswitch (x) {\n    case ('')\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/switchCompletions.ts",
            (17, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "baz", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 17, "character": 10}, "end": {"line": 17, "character": 10}}, "newText": "baz"}, "data": {"fileName": "/switchCompletions.ts", "position": 277, "name": "baz"}}, {"label": "foo", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 17, "character": 10}, "end": {"line": 17, "character": 10}}, "newText": "foo"}, "data": {"fileName": "/switchCompletions.ts", "position": 277, "name": "foo"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/switchCompletions.ts",
            (23, 11),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "bar", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 23, "character": 11}, "end": {"line": 23, "character": 11}}, "newText": "bar"}, "data": {"fileName": "/switchCompletions.ts", "position": 357, "name": "bar"}}, {"label": "foo", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 23, "character": 11}, "end": {"line": 23, "character": 11}}, "newText": "foo"}, "data": {"fileName": "/switchCompletions.ts", "position": 357, "name": "foo"}}]}"#
        ),
    );
}

/// String literal types escaped for the quote of the literal.
#[test]
fn string_literals_escaped_for_their_quote() {
    let files = [
        ("/stringCompletionsVsEscaping.ts", "type Value<P extends string> = `var(--\\\\\\\\, ${P})`\nexport const value: Value<'one' | 'two'> = \"\"\n\nexport const test: `\\ntest\\n` = ''\n\nexport const doubleQuoted1: `\"double-quoted\"` = ''\nexport const doubleQuoted2: `\"double-quoted\"` = \"\"\n\nexport const singleQuoted2: `'single-quoted'` = \"\"\nexport const singleQuoted2: `'single-quoted'` = ''\n\nexport const backtickQuoted1: '`backtick-quoted`' = \"\"\nexport const backtickQuoted2: '`backtick-quoted`' = ``"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringCompletionsVsEscaping.ts",
            (1, 44),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "var(--\\\\\\\\, one)", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 44}, "end": {"line": 1, "character": 44}}, "newText": "var(--\\\\\\\\, one)"}, "data": {"fileName": "/stringCompletionsVsEscaping.ts", "position": 95, "name": "var(--\\\\\\\\, one)"}}, {"label": "var(--\\\\\\\\, two)", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 44}, "end": {"line": 1, "character": 44}}, "newText": "var(--\\\\\\\\, two)"}, "data": {"fileName": "/stringCompletionsVsEscaping.ts", "position": 95, "name": "var(--\\\\\\\\, two)"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringCompletionsVsEscaping.ts",
            (3, 33),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "\\ntest\\n", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 3, "character": 33}, "end": {"line": 3, "character": 33}}, "newText": "\\ntest\\n"}, "data": {"fileName": "/stringCompletionsVsEscaping.ts", "position": 131, "name": "\\ntest\\n"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringCompletionsVsEscaping.ts",
            (5, 49),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "\\\"double-quoted\\\"", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 5, "character": 49}, "end": {"line": 5, "character": 49}}, "newText": "\\\"double-quoted\\\""}, "data": {"fileName": "/stringCompletionsVsEscaping.ts", "position": 183, "name": "\\\"double-quoted\\\""}}]}"#
        ),
    );
}

/// The string literals of a JSX attribute, and the names of the props in an element's attribute initializer.
#[test]
fn string_literals_of_a_jsx_attribute() {
    let files = [
        ("/a.tsx", "type Props = { a: number } | { b: \"somethingelse\", c: 0 | 1 };\ndeclare function Foo(args: Props): any\n\nconst a1 = <Foo b={\"\"} />\nconst a2 = <Foo b=\"\" />\nconst a3 = <Foo b=\"somethingelse\" />\nconst a4 = <Foo b={\"somethingelse\"}  />\nconst a5 = <Foo b={\"somethingelse\"} c={0}  />"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (3, 20), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "somethingelse", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 3, "character": 20}, "end": {"line": 3, "character": 20}}, "newText": "somethingelse"}, "data": {"fileName": "/a.tsx", "position": 123, "name": "somethingelse"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (4, 19), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "somethingelse", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 4, "character": 19}, "end": {"line": 4, "character": 19}}, "newText": "somethingelse"}, "data": {"fileName": "/a.tsx", "position": 148, "name": "somethingelse"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (5, 33), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 186, "name": "a"}}, {"label": "b", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 186, "name": "b"}}, {"label": "c", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 186, "name": "c"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (6, 36), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 226, "name": "a"}}, {"label": "c", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 226, "name": "c"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (7, 42), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 5, "sortText": "11", "data": {"fileName": "/a.tsx", "position": 272, "name": "a"}}]}"#
        ),
    );
}

/// The keys a generic parameter takes, in a call, a JSX element and a JavaScript file.
#[test]
fn string_literals_of_a_generic_parameter() {
    let files = [
        ("/a.tsx", "interface Events {\n  \"\": any;\n  drag: any;\n  dragenter: any;\n}\ndeclare function addListener<K extends keyof Events>(type: K, listener: (ev: Events[K]) => any): void;\n\ndeclare function ListenerComponent<K extends keyof Events>(props: { type: K, onWhatever: (ev: Events[K]) => void }): JSX.Element;\n\naddListener(\"\");\n(<ListenerComponent type=\"\" />);"),
        ("/b.js", "addListener(\"\");"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (9, 13), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 13}, "end": {"line": 9, "character": 13}}, "newText": ""}, "data": {"fileName": "/a.tsx", "position": 311}}, {"label": "drag", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 13}, "end": {"line": 9, "character": 13}}, "newText": "drag"}, "data": {"fileName": "/a.tsx", "position": 311, "name": "drag"}}, {"label": "dragenter", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 13}, "end": {"line": 9, "character": 13}}, "newText": "dragenter"}, "data": {"fileName": "/a.tsx", "position": 311, "name": "dragenter"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/a.tsx", (10, 26), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 26}, "end": {"line": 10, "character": 26}}, "newText": ""}, "data": {"fileName": "/a.tsx", "position": 341}}, {"label": "drag", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 26}, "end": {"line": 10, "character": 26}}, "newText": "drag"}, "data": {"fileName": "/a.tsx", "position": 341, "name": "drag"}}, {"label": "dragenter", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 26}, "end": {"line": 10, "character": 26}}, "newText": "dragenter"}, "data": {"fileName": "/a.tsx", "position": 341, "name": "dragenter"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/b.js", (0, 13), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 13}, "end": {"line": 0, "character": 13}}, "newText": ""}, "data": {"fileName": "/b.js", "position": 13}}, {"label": "drag", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 13}, "end": {"line": 0, "character": 13}}, "newText": "drag"}, "data": {"fileName": "/b.js", "position": 13, "name": "drag"}}, {"label": "dragenter", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 13}, "end": {"line": 0, "character": 13}}, "newText": "dragenter"}, "data": {"fileName": "/b.js", "position": 13, "name": "dragenter"}}]}"#
        ),
    );
}

/// The literals a parameter takes as inferred from the arguments before it.
#[test]
fn string_literals_inferred_from_a_previous_argument() {
    let files = [
        ("/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "// https://github.com/microsoft/TypeScript/issues/55545\nenum myEnum {\n  valA = \"valA\",\n  valB = \"valB\",\n}\n\ninterface myEnumParamMapping {\n  [\"valA\"]: \"1\" | \"2\";\n  [\"valB\"]: \"3\" | \"4\";\n}\n\nfunction myFunction<K extends keyof typeof myEnum>(\n  a: K,\n  b: myEnumParamMapping[K],\n) {}\n\nmyFunction(\"valA\", \"\");\nmyFunction(\"valA\", ``);\n\nfunction myFunction2<K extends keyof typeof myEnum>(\n  a: K,\n  { b }: { b: myEnumParamMapping[K] },\n) {}\n\nmyFunction2(\"valA\", { b: \"\" });\nmyFunction2(\"valA\", { b: `` });"),
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
            "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts",
            (16, 20),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "1", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 16, "character": 20}, "end": {"line": 16, "character": 20}}, "newText": "1"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 301, "name": "1"}}, {"label": "2", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 16, "character": 20}, "end": {"line": 16, "character": 20}}, "newText": "2"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 301, "name": "2"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts",
            (17, 20),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "1", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 17, "character": 20}, "end": {"line": 17, "character": 20}}, "newText": "1"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 325, "name": "1"}}, {"label": "2", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 17, "character": 20}, "end": {"line": 17, "character": 20}}, "newText": "2"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 325, "name": "2"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts",
            (24, 26),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "1", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 24, "character": 26}, "end": {"line": 24, "character": 26}}, "newText": "1"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 462, "name": "1"}}, {"label": "2", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 24, "character": 26}, "end": {"line": 24, "character": 26}}, "newText": "2"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 462, "name": "2"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts",
            (25, 26),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "1", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 25, "character": 26}, "end": {"line": 25, "character": 26}}, "newText": "1"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 494, "name": "1"}}, {"label": "2", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 25, "character": 26}, "end": {"line": 25, "character": 26}}, "newText": "2"}, "data": {"fileName": "/stringLiteralCompletionsInArgUsingInferenceResultFromPreviousArg.ts", "position": 494, "name": "2"}}]}"#
        ),
    );
}

/// The string literals of a type argument's property, from its type parameter's constraint.
#[test]
fn string_literals_of_a_type_argument_property() {
    let files = [
        ("/completionListInTypeLiteralInTypeParameter18.ts", "class Foo<T extends { x: 'one' | 'two' }> {}\nfunction foo<T extends { x: 'one' | 'two' }>() {}\ndeclare function tag<T extends { x: 'one' | 'two' }>(x: TemplateStringsArray): void;\ndeclare function decorator<T extends { x: 'one' | 'two' }>(...args: unknown[]): never\n\ntype A = Foo<{ x: '' }>;\nnew Foo<{ x: '' }>();\nfoo<{ x: '' }>();\nfoo<{ x: '' }>;\nFoo<{ x: '' }>;\ntag<{ x: '' }>``;\nclass { @decorator<{ x: '' }>; method() {} }"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (5, 19),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 5, "character": 19}, "end": {"line": 5, "character": 19}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 286, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 5, "character": 19}, "end": {"line": 5, "character": 19}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 286, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (6, 14),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 6, "character": 14}, "end": {"line": 6, "character": 14}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 306, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 6, "character": 14}, "end": {"line": 6, "character": 14}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 306, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (7, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 10}, "end": {"line": 7, "character": 10}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 324, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 10}, "end": {"line": 7, "character": 10}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 324, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (8, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 10}, "end": {"line": 8, "character": 10}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 342, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 10}, "end": {"line": 8, "character": 10}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 342, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (9, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 10}, "end": {"line": 9, "character": 10}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 358, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 10}, "end": {"line": 9, "character": 10}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 358, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (10, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 10}, "end": {"line": 10, "character": 10}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 374, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 10}, "end": {"line": 10, "character": 10}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 374, "name": "two"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionListInTypeLiteralInTypeParameter18.ts",
            (11, 25),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "one", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 25}, "end": {"line": 11, "character": 25}}, "newText": "one"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 407, "name": "one"}}, {"label": "two", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 25}, "end": {"line": 11, "character": 25}}, "newText": "two"}, "data": {"fileName": "/completionListInTypeLiteralInTypeParameter18.ts", "position": 407, "name": "two"}}]}"#
        ),
    );
}

/// The string literals a property takes in the members of a union the other properties allow.
#[test]
fn string_literals_of_a_discriminated_union_property() {
    let files = [
        ("/completionsUnionStringLiteralProperty.ts", "type Foo = { a: 0, b: 'x' } | { a: 0, b: 'y' } | { a: 1, b: 'z' };\nconst foo: Foo = { a: 0, b: '' }\n\ntype Bar = { a: 0, b: 'fx' } | { a: 0, b: 'fy' } | { a: 1, b: 'fz' };\nconst bar: Bar = { a: 0, b: 'f' }\n\ntype Baz = { x: 0, y: 0, z: 'a' } | { x: 0, y: 1, z: 'b' } | { x: 1, y: 0, z: 'c' } | { x: 1, y: 1, z: 'd' };\nconst baz1: Baz = { z: '' };\nconst baz2: Baz = { x: 0, z: '' };\nconst baz3: Baz = { x: 0, y: 1, z: '' };\nconst baz4: Baz = { x: 2, y: 1, z: '' };"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (1, 29),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "x", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 29}, "end": {"line": 1, "character": 29}}, "newText": "x"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 96, "name": "x"}}, {"label": "y", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 29}, "end": {"line": 1, "character": 29}}, "newText": "y"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 96, "name": "y"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (4, 30),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "fx", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 4, "character": 29}, "end": {"line": 4, "character": 30}}, "newText": "fx"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 201, "name": "fx"}}, {"label": "fy", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 4, "character": 29}, "end": {"line": 4, "character": 30}}, "newText": "fy"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 201, "name": "fy"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (7, 24),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 24}, "end": {"line": 7, "character": 24}}, "newText": "a"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 340, "name": "a"}}, {"label": "b", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 24}, "end": {"line": 7, "character": 24}}, "newText": "b"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 340, "name": "b"}}, {"label": "c", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 24}, "end": {"line": 7, "character": 24}}, "newText": "c"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 340, "name": "c"}}, {"label": "d", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 7, "character": 24}, "end": {"line": 7, "character": 24}}, "newText": "d"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 340, "name": "d"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (8, 30),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "a", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 30}, "end": {"line": 8, "character": 30}}, "newText": "a"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 375, "name": "a"}}, {"label": "b", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 8, "character": 30}, "end": {"line": 8, "character": 30}}, "newText": "b"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 375, "name": "b"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (9, 36),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "b", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 9, "character": 36}, "end": {"line": 9, "character": 36}}, "newText": "b"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 416, "name": "b"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/completionsUnionStringLiteralProperty.ts",
            (10, 36),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "b", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 36}, "end": {"line": 10, "character": 36}}, "newText": "b"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 457, "name": "b"}}, {"label": "d", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 10, "character": 36}, "end": {"line": 10, "character": 36}}, "newText": "d"}, "data": {"fileName": "/completionsUnionStringLiteralProperty.ts", "position": 457, "name": "d"}}]}"#
        ),
    );
}
/// The files a triple-slash path reference may name, only inside its unterminated string.
#[test]
fn triple_slash_reference_paths() {
    let files = [
        ("/f.ts", ""),
        (
            "/test.ts",
            "/// <reference path=\"\n/// <reference path=\"\" />",
        ),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (0, 19), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (0, 20), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 19), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 20), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 22), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 23), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 24), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 25), None),
        tsgo(r#"null"#),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (0, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "f.ts", "kind": 17, "detail": "f.ts", "sortText": "11", "data": {"fileName": "/test.ts", "position": 21, "name": "f.ts"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test.ts", (1, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "f.ts", "kind": 17, "detail": "f.ts", "sortText": "11", "data": {"fileName": "/test.ts", "position": 43, "name": "f.ts"}}]}"#
        ),
    );
}

/// The files of a triple-slash path reference with their extensions, JavaScript files with allowJs.
#[test]
fn triple_slash_reference_paths_with_javascript() {
    let files = [
        ("/f1.cs", ""),
        ("/f1.d.ts", ""),
        ("/f1.js", ""),
        ("/f1.jsx", ""),
        ("/f1.ts", ""),
        ("/f1.tsx", ""),
        (
            "/test0.ts",
            "/// <reference path=\"\n/// <reference path=\"./\n/// <reference path=\"./f",
        ),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/test0.ts", (0, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "f1.jsx", "kind": 17, "detail": "f1.jsx", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 21, "name": "f1.jsx"}}, {"label": "f1.ts", "kind": 17, "detail": "f1.ts", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 21, "name": "f1.ts"}}, {"label": "f1.js", "kind": 17, "detail": "f1.js", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 21, "name": "f1.js"}}, {"label": "f1.d.ts", "kind": 17, "detail": "f1.d.ts", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 21, "name": "f1.d.ts"}}, {"label": "f1.tsx", "kind": 17, "detail": "f1.tsx", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 21, "name": "f1.tsx"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test0.ts", (1, 23), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "f1.jsx", "kind": 17, "detail": "f1.jsx", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 45, "name": "f1.jsx"}}, {"label": "f1.ts", "kind": 17, "detail": "f1.ts", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 45, "name": "f1.ts"}}, {"label": "f1.js", "kind": 17, "detail": "f1.js", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 45, "name": "f1.js"}}, {"label": "f1.d.ts", "kind": 17, "detail": "f1.d.ts", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 45, "name": "f1.d.ts"}}, {"label": "f1.tsx", "kind": 17, "detail": "f1.tsx", "sortText": "11", "data": {"fileName": "/test0.ts", "position": 45, "name": "f1.tsx"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/test0.ts", (2, 24), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "f1.ts", "kind": 17, "detail": "f1.ts", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 24}}, "newText": "f1.ts"}, "data": {"fileName": "/test0.ts", "position": 70, "name": "f1.ts"}}, {"label": "f1.js", "kind": 17, "detail": "f1.js", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 24}}, "newText": "f1.js"}, "data": {"fileName": "/test0.ts", "position": 70, "name": "f1.js"}}, {"label": "f1.d.ts", "kind": 17, "detail": "f1.d.ts", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 24}}, "newText": "f1.d.ts"}, "data": {"fileName": "/test0.ts", "position": 70, "name": "f1.d.ts"}}, {"label": "f1.tsx", "kind": 17, "detail": "f1.tsx", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 24}}, "newText": "f1.tsx"}, "data": {"fileName": "/test0.ts", "position": 70, "name": "f1.tsx"}}, {"label": "f1.jsx", "kind": 17, "detail": "f1.jsx", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 23}, "end": {"line": 2, "character": 24}}, "newText": "f1.jsx"}, "data": {"fileName": "/test0.ts", "position": 70, "name": "f1.jsx"}}]}"#
        ),
    );
}

/// A package's typesVersions mapping redirects the directory a specifier completes in.
#[test]
fn paths_of_a_types_versions_wildcard() {
    let files = [
        ("/a.ts", "import { } from \"foo/\";"),
        ("/node_modules/foo/dist/blah.d.ts", "export const blah = 0;"),
        ("/node_modules/foo/dist/index.d.ts", "export const index = 0;"),
        ("/node_modules/foo/dist/subfolder/one.d.ts", "export const one = 0;"),
        ("/node_modules/foo/nope.d.ts", "export const nope = 0;"),
        ("/node_modules/foo/package.json", "{\n  \"types\": \"index.d.ts\",\n  \"typesVersions\": {\n    \"*\": {\n      \"*\": [\"dist/*\"]\n    }\n  }\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.module = Some(1);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/a.ts", (0, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "index", "kind": 17, "detail": "index.d.ts", "sortText": "11", "data": {"fileName": "/a.ts", "position": 21, "name": "index"}}, {"label": "blah", "kind": 17, "detail": "blah.d.ts", "sortText": "11", "data": {"fileName": "/a.ts", "position": 21, "name": "blah"}}, {"label": "subfolder", "kind": 19, "detail": "subfolder", "sortText": "11", "data": {"fileName": "/a.ts", "position": 21, "name": "subfolder"}}]}"#
        ),
    );
}

/// The subpath imports of the nearest package.json, its wildcard expanded.
#[test]
fn paths_of_package_json_imports() {
    let files = [
        ("/dist/arguments/index.d.ts", "export const arguments = 0;"),
        ("/dist/blah.d.ts", "export const blah = 0;"),
        ("/dist/index.d.ts", "export const index = 0;"),
        ("/index.mts", "import { } from \"\";"),
        ("/package.json", "{\n  \"name\": \"foo\",\n  \"main\": \"dist/index.js\",\n  \"module\": \"dist/index.mjs\",\n  \"types\": \"dist/index.d.ts\",\n  \"imports\": {\n    \"#*\": {\n      \"types\": \"./dist/*.d.ts\",\n      \"import\": \"./dist/*.mjs\",\n      \"default\": \"./dist/*.js\"\n    },\n    \"#arguments\": {\n      \"types\": \"./dist/arguments/index.d.ts\",\n      \"import\": \"./dist/arguments/index.mjs\",\n      \"default\": \"./dist/arguments/index.js\"\n    }\n  }\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.module = Some(101);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/index.mts", (0, 17), None),
        tsgo(
            r##"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "#index", "kind": 17, "detail": "#index.d.ts", "sortText": "11", "filterText": "index", "data": {"fileName": "/index.mts", "position": 17, "name": "#index"}}, {"label": "#blah", "kind": 17, "detail": "#blah.d.ts", "sortText": "11", "filterText": "blah", "data": {"fileName": "/index.mts", "position": 17, "name": "#blah"}}, {"label": "#arguments", "kind": 17, "detail": "#arguments.d.ts", "sortText": "11", "filterText": "arguments", "data": {"fileName": "/index.mts", "position": 17, "name": "#arguments"}}]}"##
        ),
    );
}

/// A package's exports, its wildcard expanded over the target directory.
#[test]
fn paths_of_package_json_exports() {
    let files = [
        ("/index.mts", "import { } from \"foo/\";"),
        ("/node_modules/foo/dist/arguments/index.d.ts", "export const arguments = 0;"),
        ("/node_modules/foo/dist/blah.d.ts", "export const blah = 0;"),
        ("/node_modules/foo/dist/index.d.ts", "export const index = 0;"),
        ("/node_modules/foo/package.json", "{\n  \"name\": \"foo\",\n  \"main\": \"dist/index.js\",\n  \"module\": \"dist/index.mjs\",\n  \"types\": \"dist/index.d.ts\",\n  \"exports\": {\n    \".\": {\n      \"types\": \"./dist/index.d.ts\",\n      \"import\": \"./dist/index.mjs\",\n      \"default\": \"./dist/index.js\"\n    },\n    \"./*\": {\n      \"types\": \"./dist/*.d.ts\",\n      \"import\": \"./dist/*.mjs\",\n      \"default\": \"./dist/*.js\"\n    },\n    \"./arguments\": {\n      \"types\": \"./dist/arguments/index.d.ts\",\n      \"import\": \"./dist/arguments/index.mjs\",\n      \"default\": \"./dist/arguments/index.js\"\n    }\n  }\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.module = Some(101);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/index.mts", (0, 21), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "index", "kind": 17, "detail": "index.d.ts", "sortText": "11", "data": {"fileName": "/index.mts", "position": 21, "name": "index"}}, {"label": "blah", "kind": 17, "detail": "blah.d.ts", "sortText": "11", "data": {"fileName": "/index.mts", "position": 21, "name": "blah"}}, {"label": "arguments", "kind": 17, "detail": "arguments.d.ts", "sortText": "11", "data": {"fileName": "/index.mts", "position": 21, "name": "arguments"}}]}"#
        ),
    );
}

/// The dependencies of every package.json above the script.
#[test]
fn packages_of_the_package_json_dependencies() {
    let files = [
        (
            "/dir1/dir2/dir3/dir4/test0.ts",
            "import * as foo1 from \"f\nimport foo4 = require(\"f\nvar foo7 = require(\"f",
        ),
        ("/dir1/dir2/dir3/node_modules/fake-module3/ts.ts", ""),
        (
            "/dir1/dir2/dir3/package.json",
            "{ \"dependencies\": { \"fake-module3\": \"latest\" } }",
        ),
        ("/dir1/node_modules/fake-module2/index.ts", ""),
        (
            "/dir1/package.json",
            "{ \"dependencies\": { \"fake-module2\": \"latest\" } }",
        ),
        ("/node_modules/fake-module/ts.ts", ""),
        (
            "/package.json",
            "{ \"dependencies\": { \"fake-module\": \"latest\" } }",
        ),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/dir1/dir2/dir3/dir4/test0.ts",
            (0, 24),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "fake-module2", "kind": 9, "detail": "fake-module2", "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 23}, "end": {"line": 0, "character": 24}}, "newText": "fake-module2"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 24, "name": "fake-module2"}}, {"label": "fake-module", "kind": 9, "detail": "fake-module", "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 23}, "end": {"line": 0, "character": 24}}, "newText": "fake-module"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 24, "name": "fake-module"}}, {"label": "fake-module3", "kind": 9, "detail": "fake-module3", "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 23}, "end": {"line": 0, "character": 24}}, "newText": "fake-module3"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 24, "name": "fake-module3"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/dir1/dir2/dir3/dir4/test0.ts",
            (1, 24),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "fake-module3", "kind": 9, "detail": "fake-module3", "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 23}, "end": {"line": 1, "character": 24}}, "newText": "fake-module3"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 49, "name": "fake-module3"}}, {"label": "fake-module2", "kind": 9, "detail": "fake-module2", "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 23}, "end": {"line": 1, "character": 24}}, "newText": "fake-module2"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 49, "name": "fake-module2"}}, {"label": "fake-module", "kind": 9, "detail": "fake-module", "sortText": "11", "textEdit": {"range": {"start": {"line": 1, "character": 23}, "end": {"line": 1, "character": 24}}, "newText": "fake-module"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 49, "name": "fake-module"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/dir1/dir2/dir3/dir4/test0.ts",
            (2, 21),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "fake-module", "kind": 9, "detail": "fake-module", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 20}, "end": {"line": 2, "character": 21}}, "newText": "fake-module"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 71, "name": "fake-module"}}, {"label": "fake-module3", "kind": 9, "detail": "fake-module3", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 20}, "end": {"line": 2, "character": 21}}, "newText": "fake-module3"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 71, "name": "fake-module3"}}, {"label": "fake-module2", "kind": 9, "detail": "fake-module2", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 20}, "end": {"line": 2, "character": 21}}, "newText": "fake-module2"}, "data": {"fileName": "/dir1/dir2/dir3/dir4/test0.ts", "position": 71, "name": "fake-module2"}}]}"#
        ),
    );
}

/// The module names of an import type, in TypeScript and in JSDoc.
#[test]
fn module_names_of_import_types() {
    let files = [
        ("/node_modules/package/index.ts", "file content not read"),
        ("/ns.ts", "file content not read"),
        (
            "/usage.ts",
            "type A = typeof import(\"p\");\ntype B = import(\"./\");",
        ),
        ("/user.js", "/** @type {import(\"\")} */"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
            options.module_resolution = Some(100);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/usage.ts", (0, 25), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "package", "kind": 19, "detail": "package", "sortText": "11", "textEdit": {"range": {"start": {"line": 0, "character": 24}, "end": {"line": 0, "character": 25}}, "newText": "package"}, "data": {"fileName": "/usage.ts", "position": 25, "name": "package"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/user.js", (0, 19), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "package", "kind": 19, "detail": "package", "sortText": "11", "data": {"fileName": "/user.js", "position": 19, "name": "package"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/usage.ts", (1, 19), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "ns", "kind": 17, "detail": "ns.ts", "sortText": "11", "data": {"fileName": "/usage.ts", "position": 48, "name": "ns"}}, {"label": "user", "kind": 17, "detail": "user.js", "sortText": "11", "data": {"fileName": "/usage.ts", "position": 48, "name": "user"}}, {"label": "node_modules", "kind": 19, "detail": "node_modules", "sortText": "11", "data": {"fileName": "/usage.ts", "position": 48, "name": "node_modules"}}]}"#
        ),
    );
}

/// The module specifiers of node_modules files with the endings the resolution mode allows.
#[test]
fn paths_in_node_modules_by_extension() {
    let files = [
        ("/main.cts", "import {} from \"./src/\";\nimport mod = require(\"./src/\");\nconst m = import(\"./src/\");"),
        ("/main.mts", "import {} from \"./src/\";\nimport mod = require(\"./src/\");\nconst m = import(\"./src/\");"),
        ("/main.ts", "import {} from \"./src/\";\nimport mod = require(\"./src/\");\nconst m = import(\"./src/\");"),
        ("/src/decl.d.cts", "export {}"),
        ("/src/decl.d.mts", "export {}"),
        ("/src/decl.d.ts", "export {}"),
        ("/src/js.cjs", "export {}"),
        ("/src/js.js", "export {}"),
        ("/src/js.mjs", "export {}"),
        ("/src/module.cts", "export {}"),
        ("/src/module.js", "export {}"),
        ("/src/module.mts", "export {}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
            options.module = Some(101);
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/main.mts", (0, 22), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "module.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "js.mjs"}}, {"label": "js.js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "js.js"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "module.mjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "decl.mjs"}}, {"label": "module.js", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "module.js"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "js.cjs"}}, {"label": "decl.js", "kind": 17, "detail": "decl.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "decl.js"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 22, "name": "decl.cjs"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.mts", (2, 24), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "js.js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "js.js"}}, {"label": "module.js", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "module.js"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "js.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "js.mjs"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "decl.cjs"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "module.mjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "decl.mjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "module.cjs"}}, {"label": "decl.js", "kind": 17, "detail": "decl.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 81, "name": "decl.js"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.cts", (2, 24), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "js.cjs"}}, {"label": "decl.js", "kind": 17, "detail": "decl.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "decl.js"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "decl.cjs"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "module.mjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "decl.mjs"}}, {"label": "module.js", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "module.js"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "module.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "js.mjs"}}, {"label": "js.js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 81, "name": "js.js"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.ts", (2, 24), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "js.js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "js.js"}}, {"label": "decl.js", "kind": 17, "detail": "decl.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "decl.js"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "decl.cjs"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "module.mjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "decl.mjs"}}, {"label": "module.js", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "module.js"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "js.cjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "module.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 81, "name": "js.mjs"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.mts", (1, 28), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "decl.mjs"}}, {"label": "module", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "module"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "js.cjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "module.cjs"}}, {"label": "decl", "kind": 17, "detail": "decl.d.ts", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "decl"}}, {"label": "js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "js"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "module.mjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "js.mjs"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.mts", "position": 53, "name": "decl.cjs"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.cts", (0, 22), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "module.cjs"}}, {"label": "decl", "kind": 17, "detail": "decl.d.ts", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "decl"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "js.mjs"}}, {"label": "js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "js"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "module.mjs"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "decl.cjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "decl.mjs"}}, {"label": "module", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "module"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 22, "name": "js.cjs"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.cts", (1, 28), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "decl.mjs"}}, {"label": "module", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "module"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "js.cjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "module.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "js.mjs"}}, {"label": "js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "js"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "module.mjs"}}, {"label": "decl", "kind": 17, "detail": "decl.d.ts", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "decl"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.cts", "position": 53, "name": "decl.cjs"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.ts", (0, 22), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "module.mjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "decl.mjs"}}, {"label": "module", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "module"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "js.mjs"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "decl.cjs"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "js.cjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "module.cjs"}}, {"label": "decl", "kind": 17, "detail": "decl.d.ts", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "decl"}}, {"label": "js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 22, "name": "js"}}]}"#
        ),
    );
    assert_eq!(
        completion(&files, compiler_options(), "/main.ts", (1, 28), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "decl", "kind": 17, "detail": "decl.d.ts", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "decl"}}, {"label": "module.mjs", "kind": 17, "detail": "module.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "module.mjs"}}, {"label": "module.cjs", "kind": 17, "detail": "module.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "module.cjs"}}, {"label": "js.mjs", "kind": 17, "detail": "js.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "js.mjs"}}, {"label": "js", "kind": 17, "detail": "js.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "js"}}, {"label": "decl.cjs", "kind": 17, "detail": "decl.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "decl.cjs"}}, {"label": "decl.mjs", "kind": 17, "detail": "decl.mjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "decl.mjs"}}, {"label": "module", "kind": 17, "detail": "module.js", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "module"}}, {"label": "js.cjs", "kind": 17, "detail": "js.cjs", "sortText": "11", "data": {"fileName": "/main.ts", "position": 53, "name": "js.cjs"}}]}"#
        ),
    );
}

/// The preferences with an `importModuleSpecifierEnding`.
fn ending(preference: ImportModuleSpecifierEndingPreference) -> UserPreferences {
    UserPreferences {
        import_module_specifier_ending: preference,
        ..UserPreferences::default()
    }
}

/// A file's module specifier with the preferred ending: its JavaScript
/// extension, or none (`index`).
#[test]
fn file_names_by_module_specifier_ending() {
    let files = [
        ("/module.js", "import { f } from \"./\""),
        ("/test.js", "export function f(){\n    return 1\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            ending(ImportModuleSpecifierEndingPreference::JS),
            false,
            "/module.js",
            (0, 21),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "test.js", "kind": 17, "detail": "test.js", "sortText": "11", "data": {"fileName": "/module.js", "position": 21, "name": "test.js"}}]}"#
        ),
    );
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            ending(ImportModuleSpecifierEndingPreference::INDEX),
            false,
            "/module.js",
            (0, 21),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "test", "kind": 17, "detail": "test.js", "sortText": "11", "data": {"fileName": "/module.js", "position": 21, "name": "test"}}]}"#
        ),
    );
}

/// With allowImportingTsExtensions, a TypeScript file's specifier has no
/// extension by default and its own with the `js` ending preference.
#[test]
fn file_names_with_typescript_extensions() {
    let files = [
        ("/project/foo.ts", "export const foo = 0;"),
        ("/project/main.ts", "import {} from \"./\""),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_importing_ts_extensions = Some(true);
            options.module_resolution = Some(100);
        })
    };
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/project/main.ts",
            (0, 18),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "foo", "kind": 17, "detail": "foo.ts", "sortText": "11", "data": {"fileName": "/project/main.ts", "position": 18, "name": "foo"}}]}"#
        ),
    );
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            ending(ImportModuleSpecifierEndingPreference::JS),
            false,
            "/project/main.ts",
            (0, 18),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": []}, "items": [{"label": "foo.ts", "kind": 17, "detail": "foo.ts", "sortText": "11", "data": {"fileName": "/project/main.ts", "position": 18, "name": "foo.ts"}}]}"#
        ),
    );
}
/// The JSDoc tag names after an at sign.
#[test]
fn jsdoc_tag_names_after_an_at_sign() {
    let files = [
        ("/Foo.js", "/** @ */\nvar v1;\n\n/** @p */\nvar v2;\n\n/** @param  */\nvar v3;\n\n/** @param { n } bar */\nvar v4;\n\n/** @type { n } */\nvar v5;\n\n// @\nvar v6;\n\n// @pa\nvar v7;\n\n/** @return { n } */\nvar v8;\n\n/**  */\n\n/**\n \n*/\n\n/**\n * \n */\n\n/**\n          \n */\n\n/**\n  *       \n  */\n\n/**\n  * some comment \n  */\n\n/**\n  * @param \n  */\n\n/** @param  */\n\n/**\n  * jsdoc inline tag {@}\n  */"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    let response = completion(&files, compiler_options(), "/Foo.js", (3, 6), None);
    assert_eq!(response["items"].as_array().map(Vec::len), Some(84));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "abstract"),
        tsgo(
            r#"{"label": "abstract", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 24, "name": "abstract"}}"#
        )
    );
    assert_eq!(
        item(&response, "param"),
        tsgo(
            r#"{"label": "param", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 24, "name": "param"}}"#
        )
    );
    assert_eq!(
        item(&response, "yields"),
        tsgo(
            r#"{"label": "yields", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 24, "name": "yields"}}"#
        )
    );
}

/// The JSDoc tags, at sign and name, where a line of the comment begins.
#[test]
fn jsdoc_tags_at_the_start_of_a_line() {
    let files = [
        ("/Foo.js", "/** @ */\nvar v1;\n\n/** @p */\nvar v2;\n\n/** @param  */\nvar v3;\n\n/** @param { n } bar */\nvar v4;\n\n/** @type { n } */\nvar v5;\n\n// @\nvar v6;\n\n// @pa\nvar v7;\n\n/** @return { n } */\nvar v8;\n\n/**  */\n\n/**\n \n*/\n\n/**\n * \n */\n\n/**\n          \n */\n\n/**\n  *       \n  */\n\n/**\n  * some comment \n  */\n\n/**\n  * @param \n  */\n\n/** @param  */\n\n/**\n  * jsdoc inline tag {@}\n  */"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    let response = completion(&files, compiler_options(), "/Foo.js", (24, 4), None);
    assert_eq!(response["items"].as_array().map(Vec::len), Some(84));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "@abstract"),
        tsgo(
            r#"{"label": "@abstract", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 186, "name": "@abstract"}}"#
        )
    );
    assert_eq!(
        item(&response, "@param"),
        tsgo(
            r#"{"label": "@param", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 186, "name": "@param"}}"#
        )
    );
    assert_eq!(
        item(&response, "@yields"),
        tsgo(
            r#"{"label": "@yields", "kind": 14, "sortText": "11", "data": {"fileName": "/Foo.js", "position": 186, "name": "@yields"}}"#
        )
    );
}

/// The names after an @param tag of no function.
#[test]
fn no_jsdoc_completions_after_a_parameter_tag() {
    let files = [
        ("/Foo.js", "/** @ */\nvar v1;\n\n/** @p */\nvar v2;\n\n/** @param  */\nvar v3;\n\n/** @param { n } bar */\nvar v4;\n\n/** @type { n } */\nvar v5;\n\n// @\nvar v6;\n\n// @pa\nvar v7;\n\n/** @return { n } */\nvar v8;\n\n/**  */\n\n/**\n \n*/\n\n/**\n * \n */\n\n/**\n          \n */\n\n/**\n  *       \n  */\n\n/**\n  * some comment \n  */\n\n/**\n  * @param \n  */\n\n/** @param  */\n\n/**\n  * jsdoc inline tag {@}\n  */"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        completion(&files, compiler_options(), "/Foo.js", (6, 11), None),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": []}"#
        ),
    );
}

/// The parameter names an @param tag may take, less those tagged and those not starting with the name written.
#[test]
fn jsdoc_parameter_names() {
    let files = [
        ("/jsdocParameterNameCompletion.ts", "/**\n * @param \n */\nfunction f(foo, bar) {}\n/**\n * @param foo\n * @param \n */\nfunction g(foo, bar) {}\n/**\n * @param can\n * @param cantaloupe\n */\nfunction h(cat, canary, canoodle, cantaloupe, zebra) {}\n/**\n * @param  {string} \n */\nfunction i(foo, bar) {}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/jsdocParameterNameCompletion.ts",
            (1, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 14, "name": "foo"}}, {"label": "bar", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 14, "name": "bar"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/jsdocParameterNameCompletion.ts",
            (15, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 213, "name": "foo"}}, {"label": "bar", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 213, "name": "bar"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/jsdocParameterNameCompletion.ts",
            (15, 20),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "foo", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 223, "name": "foo"}}, {"label": "bar", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 223, "name": "bar"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/jsdocParameterNameCompletion.ts",
            (6, 10),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "bar", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 71, "name": "bar"}}]}"#
        ),
    );
    assert_eq!(
        completion(
            &files,
            compiler_options(),
            "/jsdocParameterNameCompletion.ts",
            (10, 13),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "itemDefaults": {"commitCharacters": [".", ",", ";"]}, "items": [{"label": "canary", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 117, "name": "canary"}}, {"label": "canoodle", "kind": 6, "sortText": "11", "data": {"fileName": "/jsdocParameterNameCompletion.ts", "position": 117, "name": "canoodle"}}]}"#
        ),
    );
}

/// The @param tags of a JavaScript function's untagged parameters: the type of a default, a destructured parameter's properties.
#[test]
fn jsdoc_parameter_tags_of_javascript_parameters() {
    let files = [
        ("/file.js", "/**\n * @\n */\nfunction foo(x) {\n  return x + 1;\n}\n  \n/**\n * \n */\nfunction bar(x, { y }) {\n  return x + y;\n}\n\n/**\n * @param {number} x\n * \n */\nfunction baz(x, { y }) {\n  return x + y;\n}\n\n/**\n * @param {number} x\n * @param {object} param1 \n * @param {n} param1.y \n */\nfunction baz(x, { y }) {\n  return x + y;\n}\n\n/**\n * @\n */\nfunction baz(x = 0) {\n  return x * 2;\n}\n"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    let response = completion(&files, compiler_options(), "/file.js", (1, 4), None);
    assert_eq!(response["items"].as_array().map(Vec::len), Some(85));
    assert_eq!(
        response["itemDefaults"],
        tsgo(r#"{"commitCharacters": [".", ",", ";"]}"#)
    );
    assert_eq!(
        item(&response, "param {*} x "),
        tsgo(
            r#"{"label": "param {*} x ", "kind": 6, "sortText": "11", "data": {"fileName": "/file.js", "position": 8, "name": "param {*} x "}}"#
        )
    );
}

/// The doc comment template after /** before a variable statement, the file parsed again with the comment closed.
#[test]
fn doc_comment_template_of_a_variable_statement() {
    let files = [
        ("/docCommentTemplateVariableStatements01.ts", "/**\nvar a = 10;\n\n\nlet b = \"\";\n\n\nconst c = 30;\n\n\nlet d = {\n    foo: 10,\n    bar: \"20\"\n};\n\n\nlet e = function e(x, y, z) {\n    return +(x + y + z);\n};\n\n\nlet f = class F {\n    constructor(a, b, c) {\n        this.a = a;\n        this.b = b || (this.c = c);\n    }\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            UserPreferences::new_default(),
            false,
            "/docCommentTemplateVariableStatements01.ts",
            (0, 3),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "items": [{"label": "/** */", "kind": 1, "detail": "JSDoc comment", "sortText": "\u0000", "textEdit": {"newText": "/** */", "insert": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}, "replace": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}}, "commitCharacters": [], "data": {"fileName": "/docCommentTemplateVariableStatements01.ts", "position": 3, "name": "/** */"}}]}"#
        ),
    );
}

/// The doc comment template of a function as a snippet with its parameters.
#[test]
fn doc_comment_template_snippet_of_a_function() {
    let files = [(
        "/jSDocSnippetCompletionForFunction.ts",
        "/** */\nfunction abcdef(x, y) { }\n",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            UserPreferences::default(),
            true,
            "/jSDocSnippetCompletionForFunction.ts",
            (0, 3),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "items": [{"label": "/** */", "kind": 1, "detail": "JSDoc comment", "sortText": "\u0000", "insertTextFormat": 2, "textEdit": {"newText": "/**\n * $0\n * @param x ${1}\n * @param y ${2}\n */", "insert": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}, "replace": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}}, "commitCharacters": [], "data": {"fileName": "/jSDocSnippetCompletionForFunction.ts", "position": 3, "name": "/** */"}}]}"#
        ),
    );
}

/// The doc comment template of a class method, indented like the line.
#[test]
fn doc_comment_template_of_a_method() {
    let files = [
        ("/docCommentTemplateClassDeclMethods02.ts", "class C {\n    /**\n    [Symbol.iterator]() {\n        return undefined;\n    }\n    \n    [1 + 2 + 3 + Math.rand()](x: number, y: string, z = true) { }\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion_with(
            &files,
            compiler_options(),
            UserPreferences::new_default(),
            false,
            "/docCommentTemplateClassDeclMethods02.ts",
            (1, 7),
            None
        ),
        tsgo(
            r#"{"isIncomplete": false, "items": [{"label": "/** */", "kind": 1, "detail": "JSDoc comment", "sortText": "\u0000", "textEdit": {"newText": "/**\n     * \n     * @returns\n     */", "insert": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 7}}, "replace": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 7}}}, "commitCharacters": [], "data": {"fileName": "/docCommentTemplateClassDeclMethods02.ts", "position": 17, "name": "/** */"}}]}"#
        ),
    );
}
/// A /** that ends the file is no JSDoc comment (tsgo parses none under four characters): no completions.
#[test]
fn no_completions_in_a_short_unterminated_comment() {
    let files = [("/emptyFile.ts", "/**")];
    let compiler_options = || options(|_| {});
    assert_eq!(
        completion(&files, compiler_options(), "/emptyFile.ts", (0, 3), None),
        tsgo(r#"null"#),
    );
}
/// A keyword's details: its name.
#[test]
fn resolve_a_keyword() {
    let files = [("/completionResolveKeyword.ts", "class C {\n\t\n}")];
    let compiler_options = || options(|_| {});
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "abstract", "kind": 14, "sortText": "15", "data": {"fileName": "/completionResolveKeyword.ts", "position": 11, "name": "abstract"}}"#
        ),
        tsgo(
            r#"{"label": "abstract", "kind": 14, "detail": "abstract", "sortText": "15", "data": {"fileName": "/completionResolveKeyword.ts", "position": 11, "name": "abstract"}}"#
        ),
    );
}

/// A JSDoc tag's details: its name.
#[test]
fn resolve_a_jsdoc_tag() {
    let files = [(
        "/completionsJsdocTag.ts",
        "/**\n * @typedef {object} T\n * \n */",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "@property", "kind": 14, "sortText": "11", "data": {"fileName": "/completionsJsdocTag.ts", "position": 30, "name": "@property"}}"#
        ),
        tsgo(
            r#"{"label": "@property", "kind": 14, "detail": "@property", "sortText": "11", "data": {"fileName": "/completionsJsdocTag.ts", "position": 30, "name": "@property"}}"#
        ),
    );
}

/// The details of string literal completions: a literal type's name, a property's quick info and documentation.
#[test]
fn resolve_string_literal_completions() {
    let files = [
        ("/a.ts", "import {} from \"./\";\n\nconst x: \"a\" = \"\";\n\ninterface I {\n    /** Prop doc */\n    x: number;\n    /** Method doc */\n    m(): void;\n}\ndeclare const o: I;\no[\"\"];"),
        ("/other.ts", "export const x = 0;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "a", "kind": 21, "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 16}, "end": {"line": 2, "character": 16}}, "newText": "a"}, "data": {"fileName": "/a.ts", "position": 38, "name": "a"}}"#
        ),
        tsgo(
            r#"{"label": "a", "kind": 21, "detail": "a", "sortText": "11", "textEdit": {"range": {"start": {"line": 2, "character": 16}, "end": {"line": 2, "character": 16}}, "newText": "a"}, "data": {"fileName": "/a.ts", "position": 38, "name": "a"}}"#
        ),
    );
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "m", "kind": 2, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 3}}, "newText": "m"}, "data": {"fileName": "/a.ts", "position": 153, "name": "m"}}"#
        ),
        tsgo(
            r#"{"label": "m", "kind": 2, "detail": "(method) I.m(): void", "documentation": {"kind": "markdown", "value": "Method doc"}, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 3}}, "newText": "m"}, "data": {"fileName": "/a.ts", "position": 153, "name": "m"}}"#
        ),
    );
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "x", "kind": 5, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 3}}, "newText": "x"}, "data": {"fileName": "/a.ts", "position": 153, "name": "x"}}"#
        ),
        tsgo(
            r#"{"label": "x", "kind": 5, "detail": "(property) I.x: number", "documentation": {"kind": "markdown", "value": "Prop doc"}, "sortText": "11", "textEdit": {"range": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 3}}, "newText": "x"}, "data": {"fileName": "/a.ts", "position": 153, "name": "x"}}"#
        ),
    );
}

/// A variable's quick info and its JSDoc documentation.
#[test]
fn resolve_a_documented_variable() {
    let files = [
        ("/commentsVariables.ts", "/** This is my variable*/\nvar myVariable = 10;\n\n/** d variable*/\nvar d = 10;\nmyVariable = d;\n\n/** foos comment*/\nfunction foo() {\n}\n/** fooVar comment*/\nvar fooVar: () => void;\n\nfoo();\nfooVar();\nfooVar = foo;\n\nfoo();\nfooVar();\nvar fooVarVar = fooVar;\n/**class comment*/\nclass c {\n    /** constructor comment*/\n    constructor() {\n    }\n}\n/**instance comment*/\nvar i = new c();\n\n/** interface comments*/\ninterface i1 {\n}\n/**interface instance comments*/\nvar i1_i: i1;\n\nfunction foo2(a: number): void;\nfunction foo2(b: string): void;\nfunction foo2(aOrb) {\n}\nvar x = foo2;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "myVariable", "kind": 6, "sortText": "11", "data": {"fileName": "/commentsVariables.ts", "position": 47, "name": "myVariable"}}"#
        ),
        tsgo(
            r#"{"label": "myVariable", "kind": 6, "detail": "var myVariable: number", "documentation": {"kind": "markdown", "value": "This is my variable"}, "sortText": "11", "data": {"fileName": "/commentsVariables.ts", "position": 47, "name": "myVariable"}}"#
        ),
    );
}

/// The quick info and documentation of a library type's members.
#[test]
fn resolve_members_of_a_library_type() {
    let files = [("/completionListOfGenericSymbol.ts", "var a = [1,2,3];\na.")];
    let compiler_options = || options(|_| {});
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "length", "kind": 5, "sortText": "11", "data": {"fileName": "/completionListOfGenericSymbol.ts", "position": 19, "name": "length"}}"#
        ),
        tsgo(
            r#"{"label": "length", "kind": 5, "detail": "(property) Array<number>.length: number", "documentation": {"kind": "markdown", "value": "Gets or sets the length of the array. This is a number one higher than the highest index in the array."}, "sortText": "11", "data": {"fileName": "/completionListOfGenericSymbol.ts", "position": 19, "name": "length"}}"#
        ),
    );
    assert_eq!(
        resolve(
            &files,
            compiler_options(),
            r#"{"label": "toString", "kind": 2, "sortText": "11", "data": {"fileName": "/completionListOfGenericSymbol.ts", "position": 19, "name": "toString"}}"#
        ),
        tsgo(
            r#"{"label": "toString", "kind": 2, "detail": "(method) Array<number>.toString(): string", "documentation": {"kind": "markdown", "value": "Returns a string representation of an array."}, "sortText": "11", "data": {"fileName": "/completionListOfGenericSymbol.ts", "position": 19, "name": "toString"}}"#
        ),
    );
}
