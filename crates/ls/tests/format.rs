//! Formatting as tsgo answers it (`textDocument/formatting`,
//! `rangeFormatting` and `onTypeFormatting`): each case is a fourslash
//! test's document at the request, its format settings and request, with
//! tsgo's text edits, which the lsp suite's transcripts record
//! (`scripts/lsp_transcripts.py`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};
use tsc_compiler::LiveProgram;
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_host::MemoryCompilerHost;
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::parse_user_preferences;
use tsc_ls::LanguageService;
use tsc_lsproto::{PositionEncodingKind, ResolvedClientCapabilities};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// The format settings of the fourslash client's configuration.
const FOURSLASH_FORMAT: &str = r#"{"convertTabsToSpaces": true, "enabled": true, "indentSize": 4, "indentStyle": 2, "indentSwitchCase": true, "insertSpaceAfterCommaDelimiter": true, "insertSpaceAfterConstructor": false, "insertSpaceAfterFunctionKeywordForAnonymousFunctions": false, "insertSpaceAfterKeywordsInControlFlowStatements": true, "insertSpaceAfterOpeningAndBeforeClosingJsxExpressionBraces": false, "insertSpaceAfterOpeningAndBeforeClosingNonemptyBraces": true, "insertSpaceAfterOpeningAndBeforeClosingNonemptyBrackets": false, "insertSpaceAfterOpeningAndBeforeClosingNonemptyParenthesis": false, "insertSpaceAfterOpeningAndBeforeClosingTemplateStringBraces": false, "insertSpaceAfterSemicolonInForStatements": true, "insertSpaceBeforeAndAfterBinaryOperators": true, "insertSpaceBeforeFunctionParenthesis": false, "newLineCharacter": "\n", "placeOpenBraceOnNewLineForControlBlocks": false, "placeOpenBraceOnNewLineForFunctions": false, "semicolons": "ignore", "tabSize": 4, "trimTrailingWhitespace": true}"#;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

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

/// Only module specifier completions read the file system.
impl tsc_ls::Host for Files {
    fn read_directory(
        &self,
        _: &str,
        _: &[&str],
        _: &[&str],
        _: &[&str],
        _: Option<usize>,
    ) -> Vec<String> {
        unreachable!("only module specifier completions read directories")
    }

    fn get_directories(&self, _: &str) -> Vec<String> {
        unreachable!("only module specifier completions read directories")
    }

    fn directory_exists(&self, _: &str) -> bool {
        unreachable!("only module specifier completions read directories")
    }

    fn file_exists(&self, _: &str) -> bool {
        unreachable!("only module specifier completions read directories")
    }
}

/// The answer to the formatting request `method` with `params` (JSON), with
/// the fourslash client's format settings and `format_settings` (a JSON
/// object of the ones that differ, null for one the client leaves out), as
/// JSON.
fn format(files: &[(&str, &str)], format_settings: &str, method: &str, params: &str) -> Value {
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
    let options = CompilerOptions {
        target: Some(LATEST_STANDARD),
        jsx: Some(JSX_PRESERVE),
        skip_default_lib_check: Some(true),
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
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

    let mut settings: Map<String, Value> =
        serde_json::from_str(FOURSLASH_FORMAT).expect("the fourslash format settings");
    let overrides: Map<String, Value> =
        serde_json::from_str(format_settings).expect("the test's format settings");
    for (key, value) in overrides {
        if value.is_null() {
            settings.remove(&key);
        } else {
            settings.insert(key, value);
        }
    }
    let mut sections = Map::new();
    sections.insert(
        "js/ts".to_owned(),
        serde_json::json!({ "format": Value::Object(settings) }),
    );
    let preferences = parse_user_preferences(&sections);
    let capabilities = ResolvedClientCapabilities::default();
    let service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    let response = match method {
        "textDocument/formatting" => serde_json::to_value(
            service
                .provide_format_document(&tsc_lsproto::from_str(params).expect("params"))
                .expect("an answer"),
        ),
        "textDocument/rangeFormatting" => serde_json::to_value(
            service
                .provide_format_document_range(&tsc_lsproto::from_str(params).expect("params"))
                .expect("an answer"),
        ),
        "textDocument/onTypeFormatting" => serde_json::to_value(
            service
                .provide_format_document_on_type(&tsc_lsproto::from_str(params).expect("params"))
                .expect("an answer"),
        ),
        _ => panic!("not a formatting request: {method}"),
    };
    response.expect("the response as JSON")
}

/// The answer tsgo gave, as JSON.
fn tsgo(json: &str) -> Value {
    serde_json::from_str(json).expect("tsgo's answer")
}
/// TestFormattingOnSemiColon: a `;` formats its statement: spaces around binary operators and after `var`.
#[test]
fn on_semicolon_formats_its_statement() {
    let files = [("/formattingOnSemiColon.ts", "var  a=b+c^d-e*++f;")];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingOnSemiColon.ts"}, "position": {"line": 0, "character": 19}, "ch": ";", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 5}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 6}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 7}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 8}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 10}, "end": {"line": 0, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 11}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 12}, "end": {"line": 0, "character": 12}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 13}, "end": {"line": 0, "character": 13}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 14}, "end": {"line": 0, "character": 14}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 15}, "end": {"line": 0, "character": 15}}, "newText": " "}]"#
        ),
    );
}

/// TestFormattingOnCloseBrace: a `}` formats the lines of the outermost node it closes.
#[test]
fn on_close_brace_formats_the_block() {
    let files = [("/formattingOnCloseBrace.ts", "class foo    {\n    }")];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingOnCloseBrace.ts"}, "position": {"line": 1, "character": 5}, "ch": "}", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 13}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}, "newText": ""}]"#
        ),
    );
}

/// TestFormatOnOpenCurlyBraceRemoveNewLine: a `{` on its own line moves up after the signature.
#[test]
fn on_open_curly_brace_joins_the_line() {
    let files = [("/formatOnOpenCurlyBraceRemoveNewLine.ts", "if(true)\n{ }")];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formatOnOpenCurlyBraceRemoveNewLine.ts"}, "position": {"line": 1, "character": 1}, "ch": "{", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 2}, "end": {"line": 0, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 8}, "end": {"line": 1, "character": 0}}, "newText": " "}]"#
        ),
    );
}

/// TestFormattingOnEnter: Enter formats the previous line and indents the new one.
#[test]
fn on_enter_formats_the_previous_line() {
    let files = [(
        "/formattingOnEnter.ts",
        "class foo { }\nclass bar {\n }\n// new line here",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingOnEnter.ts"}, "position": {"line": 2, "character": 0}, "ch": "\n", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 1}}, "newText": ""}]"#
        ),
    );
}

/// TestFormattingWithMultilineComments: Enter next to multi-line comments, which the rules leave alone.
#[test]
fn on_enter_after_multiline_comments() {
    let files = [(
        "/formattingWithMultilineComments.ts",
        "f(/*\n         */() => { \n });",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingWithMultilineComments.ts"}, "position": {"line": 2, "character": 0}, "ch": "\n", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 18}, "end": {"line": 1, "character": 19}}, "newText": ""}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 1}}, "newText": "    "}]"#
        ),
    );
}

/// TestWhiteSpaceTrimming: Enter trims the trailing white space of the previous line.
#[test]
fn on_enter_trims_trailing_white_space() {
    let files = [(
        "/whiteSpaceTrimming.ts",
        "if (true) {     \n  //    \n   \n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///whiteSpaceTrimming.ts"}, "position": {"line": 3, "character": 0}, "ch": "\n", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 3}}, "newText": ""}]"#
        ),
    );
}

/// TestTabbingAfterNewlineInsertedBeforeWhile: Enter before the `while` of a do statement indents it as the do's.
#[test]
fn on_enter_before_while() {
    let files = [(
        "/tabbingAfterNewlineInsertedBeforeWhile.ts",
        "function foo() {\n    \nwhile (true) { }\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///tabbingAfterNewlineInsertedBeforeWhile.ts"}, "position": {"line": 2, "character": 0}, "ch": "\n", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}, "newText": ""}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestSemicolonFormattingNestedStatements: a `;` in nested statements indents its statement's lines.
#[test]
fn on_semicolon_in_nested_statements() {
    let files = [(
        "/semicolonFormattingNestedStatements.ts",
        "if (true)\nif (true)\nif (true) {\nif (true)\nvar x = 0;\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///semicolonFormattingNestedStatements.ts"}, "position": {"line": 4, "character": 10}, "ch": ";", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 0}}, "newText": "        "}]"#
        ),
    );
}

/// TestFormattingInExpressionsInTsx: a `;` after a JSX expression's arrow function in a TSX file.
#[test]
fn on_semicolon_in_tsx() {
    let files = [
        ("/test.tsx", "import * as React from \"react\";\n<div\n    autoComplete={(function () {\nreturn true;\n    })() }\n    >\n</div>"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///test.tsx"}, "position": {"line": 3, "character": 12}, "ch": ";", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "        "}]"#
        ),
    );
}

/// TestFormatDocumentPreserveTrailingWhitespace: without `trimTrailingWhitespace`, only the rules' edits.
#[test]
fn document_keeps_trailing_white_space() {
    let files = [
        ("/formatDocumentPreserveTrailingWhitespace.ts", "\nvar a;     \nvar b     \n     \n//     \nfunction b(){     \n    while(true){     \n    }     \n}     \n"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{"trimTrailingWhitespace": false}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatDocumentPreserveTrailingWhitespace.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": false}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 5, "character": 12}, "end": {"line": 5, "character": 12}}, "newText": " "}, {"range": {"start": {"line": 6, "character": 9}, "end": {"line": 6, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 6, "character": 15}, "end": {"line": 6, "character": 15}}, "newText": " "}]"#
        ),
    );
}

/// TestFormatDocumentZeroTabSize: a tab size of 0 indents with nothing.
#[test]
fn document_with_zero_tab_size() {
    let files = [(
        "/formatDocumentZeroTabSize.ts",
        "function foo() {\n    if (true) {\n        var x = 1;\n    }\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{"indentSize": null, "tabSize": null}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatDocumentZeroTabSize.ts"}, "options": {"tabSize": 0, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}, "newText": ""}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 8}}, "newText": ""}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 4}}, "newText": ""}]"#
        ),
    );
}

/// TestFormatIfTryCatchBlocks: `placeOpenBraceOnNewLineForControlBlocks` moves a control block's `{` to its own line.
#[test]
fn document_control_block_braces_on_new_lines() {
    let files = [(
        "/formatIfTryCatchBlocks.ts",
        "try {\n}\ncatch {\n}\n\ntry {\n}\ncatch (e) {\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{"placeOpenBraceOnNewLineForControlBlocks": true}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatIfTryCatchBlocks.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 4}}, "newText": "\n"}, {"range": {"start": {"line": 2, "character": 5}, "end": {"line": 2, "character": 6}}, "newText": "\n"}, {"range": {"start": {"line": 5, "character": 3}, "end": {"line": 5, "character": 4}}, "newText": "\n"}, {"range": {"start": {"line": 7, "character": 9}, "end": {"line": 7, "character": 10}}, "newText": "\n"}]"#
        ),
    );
}

/// TestFormattingSingleLineWithNewLineOptionSet: `placeOpenBraceOnNewLineForFunctions` leaves a single-line body's `{` on its line.
#[test]
fn document_function_braces_on_new_lines() {
    let files = [(
        "/formattingSingleLineWithNewLineOptionSet.ts",
        "namespace Default{}\nfunction foo(){}\nif (true){}\nfunction boo() {\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{"placeOpenBraceOnNewLineForControlBlocks": true, "placeOpenBraceOnNewLineForFunctions": true}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingSingleLineWithNewLineOptionSet.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 17}, "end": {"line": 0, "character": 17}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 18}, "end": {"line": 0, "character": 18}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 14}, "end": {"line": 1, "character": 14}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 15}, "end": {"line": 1, "character": 15}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 9}, "end": {"line": 2, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 10}, "end": {"line": 2, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 3, "character": 14}, "end": {"line": 3, "character": 15}}, "newText": "\n"}]"#
        ),
    );
}

/// TestFormattingObjectLiteralOpenCurlyNewline: `indentMultiLineObjectLiteralBeginningOnBlankLine` indents an object literal that starts on the next line.
#[test]
fn document_object_literal_on_a_blank_line() {
    let files = [
        ("/formattingObjectLiteralOpenCurlyNewline.ts", "\nvar clear =\n{\n    outerKey:\n    {\n        innerKey: 1,\n        innerKey2:\n            2\n    }\n};\n"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{"indentMultiLineObjectLiteralBeginningOnBlankLine": true}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingObjectLiteralOpenCurlyNewline.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 4}}, "newText": "        "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 8}}, "newText": "                "}, {"range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 8}}, "newText": "                "}, {"range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 12}}, "newText": "                    "}, {"range": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormatSimulatingScriptBlocks: `baseIndentSize` indents a script block's top-level statements.
#[test]
fn range_with_base_indent_size() {
    let files = [
        ("/formatSimulatingScriptBlocks.ts", "/* BEGIN EXTERNAL SOURCE */\n\n                        var a = 1;\n                        alert(\"/********/\");\n                    \n/* END EXTERNAL SOURCE */\n\n/* BEGIN EXTERNAL SOURCE */\n\n                            var b = 1;\n\n                        var c = \"/********/\";\n       var d = 1;\n\n            var e = \"/********/\";\n            var f = 1;\n        \n/* END EXTERNAL SOURCE */"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{"baseIndentSize": 12}"#,
            "textDocument/rangeFormatting",
            r#"{"textDocument": {"uri": "file:///formatSimulatingScriptBlocks.ts"}, "range": {"start": {"line": 14, "character": 31}, "end": {"line": 16, "character": 8}}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 16, "character": 0}, "end": {"line": 16, "character": 8}}, "newText": ""}]"#
        ),
    );
}

/// TestFormatComments: comments keep their place and indent with their tokens.
#[test]
fn document_comments() {
    let files = [(
        "/formatComments.ts",
        "_.chain()\n// wow\n  .then()\n// waa\n    .then();\nwow(\n  3,\n// uaa\n    4\n// wua\n);",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatComments.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 2}}, "newText": "    "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 2}}, "newText": "    "}, {"range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormattingMultilineCommentsWithTabs1: a multi-line comment's lines shift with its first line, tabs counted.
#[test]
fn document_multiline_comments_with_tabs() {
    let files = [
        ("/formattingMultilineCommentsWithTabs1.ts", "var f = function (j) {\n\n\tswitch (j) {\n\t\tcase 1:\n\t\t\t\t/* when current checkbox has focus, Firefox has changed check state already\n\t\t\t\ton SPACE bar press only\n\t\t\t\tIE does not have issue, use the CSS class\n\t\t\t\tinput:focus[type=checkbox] (z-index = 31290)\n\t\t\t\tto determine whether checkbox has focus or not\n\t\t\t\t*/\n\t\t\tbreak;\n\t\tcase 2:\n\t\tbreak;\n\t}\n}"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingMultilineCommentsWithTabs1.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 17}}, "newText": ""}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 1}}, "newText": "    "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 2}}, "newText": "        "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 4}}, "newText": "            "}, {"range": {"start": {"line": 10, "character": 0}, "end": {"line": 10, "character": 3}}, "newText": "            "}, {"range": {"start": {"line": 11, "character": 0}, "end": {"line": 11, "character": 2}}, "newText": "        "}, {"range": {"start": {"line": 12, "character": 0}, "end": {"line": 12, "character": 2}}, "newText": "            "}, {"range": {"start": {"line": 13, "character": 0}, "end": {"line": 13, "character": 1}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormattingReplaceTabsWithSpaces: indentation in tabs becomes spaces.
#[test]
fn document_replaces_tabs_with_spaces() {
    let files = [
        ("/formattingReplaceTabsWithSpaces.ts", "namespace Foo {\n\t\t\t\tclass Test { }\n\t\t\tclass Test { }\nclass Test { }\n\t\t\t class Test { }\n   class Test { }\n    class Test { }\n     class Test { }\n}"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingReplaceTabsWithSpaces.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 4}}, "newText": "    "}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 3}}, "newText": "    "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 4}}, "newText": "    "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 3}}, "newText": "    "}, {"range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 5}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormatTsx: a TSX file's elements, attributes and expressions.
#[test]
fn document_tsx() {
    let files = [(
        "/foo.tsx",
        "<div><p>'</p><p>{function(){return 1;}]}</p></div>",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///foo.tsx"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 27}, "end": {"line": 0, "character": 27}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 28}, "end": {"line": 0, "character": 28}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 37}, "end": {"line": 0, "character": 37}}, "newText": " "}]"#
        ),
    );
}

/// TestFormatJsxDottedTagName: a dotted tag name with a dash is read again as a JSX identifier.
#[test]
fn document_jsx_dotted_tag_name() {
    let files = [(
        "/file.tsx",
        "const x = (\n<a-b.c>\n<a-b.c></a-b.c>\n</a-b.c>\n);",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///file.tsx"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormattingJsxTexts3: JSX text is read again as JSX text and not indented.
#[test]
fn document_jsx_texts() {
    let files = [(
        "/file.tsx",
        "function foo() {\nconst bar = \"Oh no\";\n\nreturn (\n<div>\"{bar}\"</div>\n)\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///file.tsx"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormatDotAfterNumber: the space before the `.` of an integer literal stays.
#[test]
fn document_dot_after_number() {
    let files = [
        ("/formatDotAfterNumber.ts", "1+ 2 .toString() +3\n1+ 2. .toString() +3\n1+ 2.0 .toString() +3\n1+ (2) .toString() +3\n1+ 2_000 .toString() +3"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatDotAfterNumber.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 18}, "end": {"line": 0, "character": 18}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 1}, "end": {"line": 1, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 19}, "end": {"line": 1, "character": 19}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 1}, "end": {"line": 2, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 20}, "end": {"line": 2, "character": 20}}, "newText": " "}, {"range": {"start": {"line": 3, "character": 1}, "end": {"line": 3, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 3, "character": 6}, "end": {"line": 3, "character": 7}}, "newText": ""}, {"range": {"start": {"line": 3, "character": 20}, "end": {"line": 3, "character": 20}}, "newText": " "}, {"range": {"start": {"line": 4, "character": 1}, "end": {"line": 4, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 4, "character": 22}, "end": {"line": 4, "character": 22}}, "newText": " "}]"#
        ),
    );
}

/// TestFormattingSkippedTokens: no edit touches a range with a parse error.
#[test]
fn document_skips_parse_errors() {
    let files = [
        ("/formattingSkippedTokens.ts", "foo(): Bar { }\nfunction Foo      () #   { }\n4+:5\n namespace M {\nfunction a(\n    : T) { }\n}\nvar x       ="),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingSkippedTokens.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 12}, "end": {"line": 1, "character": 18}}, "newText": ""}, {"range": {"start": {"line": 2, "character": 1}, "end": {"line": 2, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 3}, "end": {"line": 2, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 1}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 7, "character": 5}, "end": {"line": 7, "character": 12}}, "newText": " "}]"#
        ),
    );
}

/// TestFormatNoSpaceBetweenClosingParenAndTemplateString: no space between a tag and its template.
#[test]
fn document_tagged_template() {
    let files = [(
        "/formatNoSpaceBetweenClosingParenAndTemplateString.ts",
        "foo() `abc`;\nbar()`def`;\nbaz()`a${x}b`;",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatNoSpaceBetweenClosingParenAndTemplateString.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 5}, "end": {"line": 0, "character": 6}}, "newText": ""}]"#
        ),
    );
}

/// TestFormattingDoubleLessThan: nested type arguments and a shift operator.
#[test]
fn document_double_less_than() {
    let files = [(
        "/formattingDoubleLessThan.ts",
        "if (<number>foo < <number>bar) {}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingDoubleLessThan.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 32}, "end": {"line": 0, "character": 32}}, "newText": " "}]"#
        ),
    );
}

/// TestGenericsFormatting: type parameters and arguments lose their inner spaces.
#[test]
fn document_type_arguments() {
    let files = [
        ("/genericsFormatting.ts", "class Foo   <    T1   ,  T2    >  {\n    public method    <   T3,    T4   >   ( a: T1,   b: Array    < T4 > ):   Map < T1  ,   T2, Array < T3    >    > {\n    }\n}\nvar foo = new Foo   <  number, Array <   number  >   >  (  );\nfoo = new Foo  <  {   bar  :  number }, Array   < {   baz :  string   }  >  >  (  );\n\ninterface IFoo {\nnew < T  > ( a: T);\nop?< T , M > (a: T, b : M );\n}\n\nfoo()<number, string, T >();\n(a + b)<number, string, T >();\n\nfunction bar <T> () {\n    return class  <  T2 > {\n    }\n}\nclass A < T > extends bar <  T >( )  <  T > {\n}"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///genericsFormatting.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 12}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 13}, "end": {"line": 0, "character": 17}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 19}, "end": {"line": 0, "character": 22}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 23}, "end": {"line": 0, "character": 25}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 27}, "end": {"line": 0, "character": 31}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 32}, "end": {"line": 0, "character": 34}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 17}, "end": {"line": 1, "character": 21}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 22}, "end": {"line": 1, "character": 25}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 28}, "end": {"line": 1, "character": 32}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 34}, "end": {"line": 1, "character": 37}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 38}, "end": {"line": 1, "character": 41}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 42}, "end": {"line": 1, "character": 43}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 49}, "end": {"line": 1, "character": 52}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 60}, "end": {"line": 1, "character": 64}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 65}, "end": {"line": 1, "character": 66}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 68}, "end": {"line": 1, "character": 69}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 70}, "end": {"line": 1, "character": 71}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 73}, "end": {"line": 1, "character": 76}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 79}, "end": {"line": 1, "character": 80}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 81}, "end": {"line": 1, "character": 82}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 84}, "end": {"line": 1, "character": 86}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 87}, "end": {"line": 1, "character": 90}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 99}, "end": {"line": 1, "character": 100}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 101}, "end": {"line": 1, "character": 102}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 104}, "end": {"line": 1, "character": 108}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 109}, "end": {"line": 1, "character": 113}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 17}, "end": {"line": 4, "character": 20}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 21}, "end": {"line": 4, "character": 23}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 36}, "end": {"line": 4, "character": 37}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 38}, "end": {"line": 4, "character": 41}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 47}, "end": {"line": 4, "character": 49}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 50}, "end": {"line": 4, "character": 53}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 54}, "end": {"line": 4, "character": 56}}, "newText": ""}, {"range": {"start": {"line": 4, "character": 57}, "end": {"line": 4, "character": 59}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 13}, "end": {"line": 5, "character": 15}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 16}, "end": {"line": 5, "character": 18}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 19}, "end": {"line": 5, "character": 22}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 25}, "end": {"line": 5, "character": 27}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 28}, "end": {"line": 5, "character": 30}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 45}, "end": {"line": 5, "character": 48}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 49}, "end": {"line": 5, "character": 50}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 51}, "end": {"line": 5, "character": 54}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 57}, "end": {"line": 5, "character": 58}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 59}, "end": {"line": 5, "character": 61}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 67}, "end": {"line": 5, "character": 70}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 71}, "end": {"line": 5, "character": 73}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 74}, "end": {"line": 5, "character": 76}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 77}, "end": {"line": 5, "character": 79}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 80}, "end": {"line": 5, "character": 82}}, "newText": ""}, {"range": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 8, "character": 5}, "end": {"line": 8, "character": 6}}, "newText": ""}, {"range": {"start": {"line": 8, "character": 7}, "end": {"line": 8, "character": 9}}, "newText": ""}, {"range": {"start": {"line": 8, "character": 10}, "end": {"line": 8, "character": 11}}, "newText": ""}, {"range": {"start": {"line": 8, "character": 12}, "end": {"line": 8, "character": 13}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 9, "character": 4}, "end": {"line": 9, "character": 5}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 6}, "end": {"line": 9, "character": 7}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 10}, "end": {"line": 9, "character": 11}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 12}, "end": {"line": 9, "character": 13}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 21}, "end": {"line": 9, "character": 22}}, "newText": ""}, {"range": {"start": {"line": 9, "character": 25}, "end": {"line": 9, "character": 26}}, "newText": ""}, {"range": {"start": {"line": 12, "character": 23}, "end": {"line": 12, "character": 24}}, "newText": ""}, {"range": {"start": {"line": 13, "character": 25}, "end": {"line": 13, "character": 26}}, "newText": ""}, {"range": {"start": {"line": 15, "character": 12}, "end": {"line": 15, "character": 13}}, "newText": ""}, {"range": {"start": {"line": 15, "character": 16}, "end": {"line": 15, "character": 17}}, "newText": ""}, {"range": {"start": {"line": 16, "character": 16}, "end": {"line": 16, "character": 18}}, "newText": " "}, {"range": {"start": {"line": 16, "character": 19}, "end": {"line": 16, "character": 21}}, "newText": ""}, {"range": {"start": {"line": 16, "character": 23}, "end": {"line": 16, "character": 24}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 7}, "end": {"line": 19, "character": 8}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 9}, "end": {"line": 19, "character": 10}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 11}, "end": {"line": 19, "character": 12}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 25}, "end": {"line": 19, "character": 26}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 27}, "end": {"line": 19, "character": 29}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 30}, "end": {"line": 19, "character": 31}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 33}, "end": {"line": 19, "character": 34}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 35}, "end": {"line": 19, "character": 37}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 38}, "end": {"line": 19, "character": 40}}, "newText": ""}, {"range": {"start": {"line": 19, "character": 41}, "end": {"line": 19, "character": 42}}, "newText": ""}]"#
        ),
    );
}

/// TestFormattingMappedType: a mapped type's brackets and braces.
#[test]
fn document_mapped_type() {
    let files = [(
        "/formattingMappedType.ts",
        "type t  < T  > =   {\n   [   P   in   keyof    T  ]   :   T  [  P  ]\n};",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingMappedType.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 8}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 10}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 13}}, "newText": ""}, {"range": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 19}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 3}}, "newText": "    "}, {"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 7}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 8}, "end": {"line": 1, "character": 11}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 13}, "end": {"line": 1, "character": 16}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 21}, "end": {"line": 1, "character": 25}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 26}, "end": {"line": 1, "character": 28}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 29}, "end": {"line": 1, "character": 32}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 33}, "end": {"line": 1, "character": 36}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 37}, "end": {"line": 1, "character": 39}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 40}, "end": {"line": 1, "character": 42}}, "newText": ""}, {"range": {"start": {"line": 1, "character": 43}, "end": {"line": 1, "character": 45}}, "newText": ""}]"#
        ),
    );
}

/// TestFormattingConditionalTypes: a conditional type's `?` and `:`.
#[test]
fn document_conditional_types() {
    let files = [
        ("/formattingConditionalTypes.ts", "type Diff1<T, U> = T extends U?never:T;\ntype Diff2<T, U> = T    extends    U  ?    never   :     T;"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingConditionalTypes.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 30}, "end": {"line": 0, "character": 30}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 31}, "end": {"line": 0, "character": 31}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 36}, "end": {"line": 0, "character": 36}}, "newText": " "}, {"range": {"start": {"line": 0, "character": 37}, "end": {"line": 0, "character": 37}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 20}, "end": {"line": 1, "character": 24}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 31}, "end": {"line": 1, "character": 35}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 36}, "end": {"line": 1, "character": 38}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 39}, "end": {"line": 1, "character": 43}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 48}, "end": {"line": 1, "character": 51}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 52}, "end": {"line": 1, "character": 57}}, "newText": " "}]"#
        ),
    );
}

/// TestFormattingOnChainedCallbacksAndPropertyAccesses: chained calls and callbacks indent from the chain.
#[test]
fn document_chained_calls() {
    let files = [
        ("/formattingOnChainedCallbacksAndPropertyAccesses.ts", "var x = 1;\nx\n.toFixed\nx\n.toFixed()\nx\n.toFixed()\n.length\n.toString();\nx\n.toFixed\n.toString()\n.length;"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingOnChainedCallbacksAndPropertyAccesses.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 7, "character": 0}, "end": {"line": 7, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 10, "character": 0}, "end": {"line": 10, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 11, "character": 0}, "end": {"line": 11, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 12, "character": 0}, "end": {"line": 12, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestFormattingOnStatementsWithNoSemicolon: statements without semicolons.
#[test]
fn document_statements_without_semicolons() {
    let files = [
        ("/formattingOnStatementsWithNoSemicolon.ts", "do\n     { var a\n}   while (1)\nfunction f() {\n    var s = 1\n            }\nswitch (t) {\n    case 1:\n{\ntest\n}\n}\ndo{do{do{}while(a!==b)}while(a!==b)}while(a!==b)\ndo{\ndo{\ndo{\n}while(a!==b)\n}while(a!==b)\n}while(a!==b)\nfor(var i=0;i<10;i++){\nfor(var j=0;j<10;j++){\nj-=i\n}}\nfunction foo() {\ntry {\nx+=2\n}\ncatch( e){\nx+=2\n}finally {\nx+=2\n}\n}\ndo     { var a }   while (1)\n    foo(function (file) {\n        return 0\n    }).then(function (doc) {\n        return 1\n    });\nif(1)\nif(1)\nx++\nelse\nif(1)\nx+=2\nelse\nx+=2\n\n\n\n;\n         do do do do\n                test;\n            while (0)\n         while (0)\n            while (0)\n         while (0)"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formattingOnStatementsWithNoSemicolon.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 2}, "end": {"line": 1, "character": 5}}, "newText": " "}, {"range": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 7}}, "newText": "\n"}, {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 7}}, "newText": "    "}, {"range": {"start": {"line": 2, "character": 1}, "end": {"line": 2, "character": 4}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 12}}, "newText": ""}, {"range": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 0}}, "newText": "            "}, {"range": {"start": {"line": 10, "character": 0}, "end": {"line": 10, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 12, "character": 2}, "end": {"line": 12, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 3}, "end": {"line": 12, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 5}, "end": {"line": 12, "character": 5}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 6}, "end": {"line": 12, "character": 6}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 8}, "end": {"line": 12, "character": 8}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 9}, "end": {"line": 12, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 10}, "end": {"line": 12, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 15}, "end": {"line": 12, "character": 15}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 17}, "end": {"line": 12, "character": 17}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 20}, "end": {"line": 12, "character": 20}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 22}, "end": {"line": 12, "character": 22}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 23}, "end": {"line": 12, "character": 23}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 28}, "end": {"line": 12, "character": 28}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 30}, "end": {"line": 12, "character": 30}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 33}, "end": {"line": 12, "character": 33}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 35}, "end": {"line": 12, "character": 35}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 36}, "end": {"line": 12, "character": 36}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 41}, "end": {"line": 12, "character": 41}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 43}, "end": {"line": 12, "character": 43}}, "newText": " "}, {"range": {"start": {"line": 12, "character": 46}, "end": {"line": 12, "character": 46}}, "newText": " "}, {"range": {"start": {"line": 13, "character": 2}, "end": {"line": 13, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 14, "character": 0}, "end": {"line": 14, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 14, "character": 2}, "end": {"line": 14, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 15, "character": 0}, "end": {"line": 15, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 15, "character": 2}, "end": {"line": 15, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 16, "character": 0}, "end": {"line": 16, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 16, "character": 1}, "end": {"line": 16, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 16, "character": 6}, "end": {"line": 16, "character": 6}}, "newText": " "}, {"range": {"start": {"line": 16, "character": 8}, "end": {"line": 16, "character": 8}}, "newText": " "}, {"range": {"start": {"line": 16, "character": 11}, "end": {"line": 16, "character": 11}}, "newText": " "}, {"range": {"start": {"line": 17, "character": 0}, "end": {"line": 17, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 17, "character": 1}, "end": {"line": 17, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 17, "character": 6}, "end": {"line": 17, "character": 6}}, "newText": " "}, {"range": {"start": {"line": 17, "character": 8}, "end": {"line": 17, "character": 8}}, "newText": " "}, {"range": {"start": {"line": 17, "character": 11}, "end": {"line": 17, "character": 11}}, "newText": " "}, {"range": {"start": {"line": 18, "character": 1}, "end": {"line": 18, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 18, "character": 6}, "end": {"line": 18, "character": 6}}, "newText": " "}, {"range": {"start": {"line": 18, "character": 8}, "end": {"line": 18, "character": 8}}, "newText": " "}, {"range": {"start": {"line": 18, "character": 11}, "end": {"line": 18, "character": 11}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 3}, "end": {"line": 19, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 9}, "end": {"line": 19, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 10}, "end": {"line": 19, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 12}, "end": {"line": 19, "character": 12}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 13}, "end": {"line": 19, "character": 13}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 14}, "end": {"line": 19, "character": 14}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 17}, "end": {"line": 19, "character": 17}}, "newText": " "}, {"range": {"start": {"line": 19, "character": 21}, "end": {"line": 19, "character": 21}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 0}, "end": {"line": 20, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 20, "character": 3}, "end": {"line": 20, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 9}, "end": {"line": 20, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 10}, "end": {"line": 20, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 12}, "end": {"line": 20, "character": 12}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 13}, "end": {"line": 20, "character": 13}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 14}, "end": {"line": 20, "character": 14}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 17}, "end": {"line": 20, "character": 17}}, "newText": " "}, {"range": {"start": {"line": 20, "character": 21}, "end": {"line": 20, "character": 21}}, "newText": " "}, {"range": {"start": {"line": 21, "character": 0}, "end": {"line": 21, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 21, "character": 1}, "end": {"line": 21, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 21, "character": 3}, "end": {"line": 21, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 22, "character": 0}, "end": {"line": 22, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 22, "character": 1}, "end": {"line": 22, "character": 1}}, "newText": "\n"}, {"range": {"start": {"line": 24, "character": 0}, "end": {"line": 24, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 25, "character": 0}, "end": {"line": 25, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 25, "character": 1}, "end": {"line": 25, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 25, "character": 3}, "end": {"line": 25, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 26, "character": 0}, "end": {"line": 26, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 27, "character": 0}, "end": {"line": 27, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 27, "character": 5}, "end": {"line": 27, "character": 5}}, "newText": " "}, {"range": {"start": {"line": 27, "character": 6}, "end": {"line": 27, "character": 7}}, "newText": ""}, {"range": {"start": {"line": 27, "character": 9}, "end": {"line": 27, "character": 9}}, "newText": " "}, {"range": {"start": {"line": 28, "character": 0}, "end": {"line": 28, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 28, "character": 1}, "end": {"line": 28, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 28, "character": 3}, "end": {"line": 28, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 29, "character": 0}, "end": {"line": 29, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 29, "character": 1}, "end": {"line": 29, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 30, "character": 0}, "end": {"line": 30, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 30, "character": 1}, "end": {"line": 30, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 30, "character": 3}, "end": {"line": 30, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 31, "character": 0}, "end": {"line": 31, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 33, "character": 2}, "end": {"line": 33, "character": 7}}, "newText": " "}, {"range": {"start": {"line": 33, "character": 16}, "end": {"line": 33, "character": 19}}, "newText": " "}, {"range": {"start": {"line": 34, "character": 0}, "end": {"line": 34, "character": 4}}, "newText": ""}, {"range": {"start": {"line": 34, "character": 16}, "end": {"line": 34, "character": 17}}, "newText": ""}, {"range": {"start": {"line": 35, "character": 0}, "end": {"line": 35, "character": 8}}, "newText": "    "}, {"range": {"start": {"line": 36, "character": 0}, "end": {"line": 36, "character": 4}}, "newText": ""}, {"range": {"start": {"line": 36, "character": 20}, "end": {"line": 36, "character": 21}}, "newText": ""}, {"range": {"start": {"line": 37, "character": 0}, "end": {"line": 37, "character": 8}}, "newText": "    "}, {"range": {"start": {"line": 38, "character": 0}, "end": {"line": 38, "character": 4}}, "newText": ""}, {"range": {"start": {"line": 39, "character": 2}, "end": {"line": 39, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 40, "character": 0}, "end": {"line": 40, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 40, "character": 2}, "end": {"line": 40, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 41, "character": 0}, "end": {"line": 41, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 42, "character": 0}, "end": {"line": 42, "character": 0}}, "newText": "    "}, {"range": {"start": {"line": 43, "character": 0}, "end": {"line": 43, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 43, "character": 2}, "end": {"line": 43, "character": 2}}, "newText": " "}, {"range": {"start": {"line": 44, "character": 0}, "end": {"line": 44, "character": 0}}, "newText": "            "}, {"range": {"start": {"line": 44, "character": 1}, "end": {"line": 44, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 44, "character": 3}, "end": {"line": 44, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 45, "character": 0}, "end": {"line": 45, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 46, "character": 0}, "end": {"line": 46, "character": 0}}, "newText": "            "}, {"range": {"start": {"line": 46, "character": 1}, "end": {"line": 46, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 46, "character": 3}, "end": {"line": 46, "character": 3}}, "newText": " "}, {"range": {"start": {"line": 50, "character": 0}, "end": {"line": 50, "character": 0}}, "newText": "                "}, {"range": {"start": {"line": 51, "character": 0}, "end": {"line": 51, "character": 9}}, "newText": ""}, {"range": {"start": {"line": 52, "character": 0}, "end": {"line": 52, "character": 16}}, "newText": "    "}, {"range": {"start": {"line": 53, "character": 0}, "end": {"line": 53, "character": 12}}, "newText": ""}, {"range": {"start": {"line": 54, "character": 0}, "end": {"line": 54, "character": 9}}, "newText": ""}, {"range": {"start": {"line": 55, "character": 0}, "end": {"line": 55, "character": 12}}, "newText": ""}, {"range": {"start": {"line": 56, "character": 0}, "end": {"line": 56, "character": 9}}, "newText": ""}]"#
        ),
    );
}

/// TestAutoFormattingOnPasting: a pasted range is indented from its enclosing node.
#[test]
fn range_indents_pasted_lines() {
    let files = [(
        "/autoFormattingOnPasting.ts",
        "namespace TestModule {\n class TestClass{\nprivate   foo;\npublic testMethod( )\n{}\n}\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/rangeFormatting",
            r#"{"textDocument": {"uri": "file:///autoFormattingOnPasting.ts"}, "range": {"start": {"line": 1, "character": 0}, "end": {"line": 5, "character": 1}}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}}, "newText": "    "}, {"range": {"start": {"line": 1, "character": 16}, "end": {"line": 1, "character": 16}}, "newText": " "}, {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 2, "character": 7}, "end": {"line": 2, "character": 10}}, "newText": " "}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "        "}, {"range": {"start": {"line": 3, "character": 18}, "end": {"line": 3, "character": 19}}, "newText": ""}, {"range": {"start": {"line": 3, "character": 20}, "end": {"line": 4, "character": 0}}, "newText": " "}, {"range": {"start": {"line": 4, "character": 1}, "end": {"line": 4, "character": 1}}, "newText": " "}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 0}}, "newText": "    "}]"#
        ),
    );
}

/// TestRangeFormatStartingInsideJSDocComment: a range that starts in a JSDoc comment scans from the token before it.
#[test]
fn range_starting_in_a_js_doc_comment() {
    let files = [(
        "/a.ts",
        "/**\n * @a\n * `\n * @b\n */\nexport function f() {}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/rangeFormatting",
            r#"{"textDocument": {"uri": "file:///a.ts"}, "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 22}}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 5, "character": 21}, "end": {"line": 5, "character": 21}}, "newText": " "}]"#
        ),
    );
}

/// TestFormatSelectionEditAtEndOfRange: the pair after the range's end gets its edit (`semicolons: remove`).
#[test]
fn range_edit_after_its_end() {
    let files = [("/formatSelectionEditAtEndOfRange.ts", "var x = 1;\nvoid 0;")];
    assert_eq!(
        format(
            &files,
            r#"{"semicolons": "remove"}"#,
            "textDocument/rangeFormatting",
            r#"{"textDocument": {"uri": "file:///formatSelectionEditAtEndOfRange.ts"}, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 10}}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 10}}, "newText": ""}]"#
        ),
    );
}

/// TestFormatSelectionWithTrivia: a range that ends in trivia trims and indents it.
#[test]
fn range_with_trivia() {
    let files = [(
        "/formatSelectionWithTrivia.ts",
        "if (true) {     \n  //   \n      \n     //    \n     ;    \n       \n      }",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/rangeFormatting",
            r#"{"textDocument": {"uri": "file:///formatSelectionWithTrivia.ts"}, "range": {"start": {"line": 2, "character": 3}, "end": {"line": 6, "character": 7}}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(
            r#"[{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 6}}, "newText": ""}, {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 5}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 5}}, "newText": "    "}, {"range": {"start": {"line": 4, "character": 6}, "end": {"line": 4, "character": 10}}, "newText": ""}, {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 7}}, "newText": ""}, {"range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 6}}, "newText": ""}]"#
        ),
    );
}

/// TestFormattingInComment: a `;` typed in a line comment formats nothing.
#[test]
fn on_type_in_a_line_comment() {
    let files = [(
        "/formattingInComment.ts",
        "class A {\nfoo(              ); // ;\n}\nfunction foo() {       var x;       } // ",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingInComment.ts"}, "position": {"line": 1, "character": 25}, "ch": ";", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(r#"[]"#),
    );
}

/// TestFormattingInComment: nor does a `}` typed in one.
#[test]
fn on_close_brace_in_a_line_comment() {
    let files = [(
        "/formattingInComment.ts",
        "class A {\nfoo(              ); // ;\n}\nfunction foo() {       var x;       } // }",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///formattingInComment.ts"}, "position": {"line": 3, "character": 42}, "ch": "}", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(r#"[]"#),
    );
}

/// TestBasicReplaceLine: a character other than `{`, `}`, `;` and Enter formats nothing (a `:` here).
#[test]
fn on_type_of_another_character() {
    let files = [(
        "/basicReplaceLine.ts",
        "export {};\ninterface Point {\n\tx: number;\n\tz:\n}\ndeclare const p: Point;\np.",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/onTypeFormatting",
            r#"{"textDocument": {"uri": "file:///basicReplaceLine.ts"}, "position": {"line": 3, "character": 3}, "ch": ":", "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(r#"[]"#),
    );
}

/// TestFormatConflictMarker1: conflict markers are trivia, and their errors keep the lines between them as they are.
#[test]
fn document_with_conflict_markers() {
    let files = [(
        "/formatConflictMarker1.ts",
        "class C {\n<<<<<<< HEAD\nv = 1;\n=======\nv = 2;\n>>>>>>> Branch - a\n}",
    )];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatConflictMarker1.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(r#"[]"#),
    );
}

/// TestFormatConflictDiff3Marker1: the same with a diff3 marker.
#[test]
fn document_with_diff3_conflict_markers() {
    let files = [
        ("/formatConflictDiff3Marker1.ts", "class C {\n<<<<<<< HEAD\nv = 1;\n||||||| merged common ancestors\nv = 3;\n=======\nv = 2;\n>>>>>>> Branch - a\n}"),
    ];
    assert_eq!(
        format(
            &files,
            r#"{}"#,
            "textDocument/formatting",
            r#"{"textDocument": {"uri": "file:///formatConflictDiff3Marker1.ts"}, "options": {"tabSize": 4, "insertSpaces": true, "trimTrailingWhitespace": true}}"#,
        ),
        tsgo(r#"[]"#),
    );
}
