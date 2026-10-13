//! Diagnostics the conformance baselines do not show and tsgo's language
//! service does: suggestions (deprecations, an unchecked JavaScript file's
//! spelling), related rows, declaration diagnostics and messages only
//! reached through them. Each case is a fourslash test's input with tsgo's
//! answer to `textDocument/diagnostic`, which the lsp suite's transcripts
//! record (`scripts/lsp_transcripts.py`): the document's syntactic,
//! semantic, suggestion and, when declarations are emitted, declaration
//! diagnostics (tsgo `getAllDiagnostics`).

use std::path::PathBuf;

use tsc_compiler::LiveProgram;
use tsc_diagnostics::{flattened_message, Diagnostic, DiagnosticCategory};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};
use tsc_types::CompilerOptions;

#[allow(dead_code)]
#[path = "support/witness_libraries.rs"]
mod witness_libraries;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// A protocol range: (line, character) to (line, character).
type Range = ((u32, u32), (u32, u32));

/// A diagnostic as `textDocument/diagnostic` reports it.
#[derive(Debug, PartialEq)]
struct Row {
    range: Range,
    code: u32,
    category: DiagnosticCategory,
    message: String,
    related: Vec<(Range, String)>,
}

fn row(range: Range, code: u32, category: DiagnosticCategory, message: &str) -> Row {
    Row {
        range,
        code,
        category,
        message: message.to_owned(),
        related: Vec::new(),
    }
}

fn related(mut row: Row, rows: &[(Range, &str)]) -> Row {
    row.related = rows
        .iter()
        .map(|(range, message)| (*range, (*message).to_owned()))
        .collect();
    row
}

/// The fourslash options with `configure` applied.
fn options(configure: impl FnOnce(&mut CompilerOptions)) -> CompilerOptions {
    let mut options = CompilerOptions {
        target: Some(LATEST_STANDARD),
        jsx: Some(JSX_PRESERVE),
        skip_default_lib_check: Some(true),
        ..CompilerOptions::default()
    };
    configure(&mut options);
    options
}

/// Which of a file's diagnostics a case compares.
#[derive(Clone, Copy, PartialEq)]
enum Kinds {
    /// tsgo `getAllDiagnostics`: syntactic, semantic, suggestion and, when
    /// declarations are emitted, declaration diagnostics.
    All,
    /// tsgo `GetDeclarationDiagnostics`.
    Declaration,
}

/// tsgo `getAllDiagnostics` of `file` in a program of `files` with the
/// standard library.
fn diagnostics(files: &[(&str, &str)], options: CompilerOptions, file: &str) -> Vec<Row> {
    diagnostics_of(files, options, file, Kinds::All)
}

fn diagnostics_of(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    kinds: Kinds,
) -> Vec<Row> {
    let mut builder = MemoryCompilerHost::builder("/").case_sensitive(true);
    for (name, text) in files {
        builder = builder.file(PathBuf::from(name), text.as_bytes());
    }
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let roots = files
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect::<Vec<_>>();
    let emit_declarations = options.declaration == Some(true);
    // The loaders take a program that emits and one that does not.
    let load = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    let prepared = load(
        &host,
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("load the program");
    let mut live = LiveProgram::new(prepared).expect("the live program");
    let index = live.file_index(file).expect("the file");
    let mut all = Vec::new();
    if kinds == Kinds::All {
        all.extend(live.syntactic_diagnostics(index));
        all.extend(live.semantic_diagnostics(index).expect("semantic"));
        all.extend(live.suggestion_diagnostics(index).expect("suggestion"));
    }
    if emit_declarations {
        for rows in live.declaration_diagnostics(&[index]).expect("declaration") {
            all.extend(rows);
        }
    }
    let text = files
        .iter()
        .find(|(name, _)| *name == file)
        .map(|(_, text)| *text)
        .expect("the file's text");
    all.iter()
        .map(|diagnostic| to_row(diagnostic, text))
        .collect()
}

/// The (line, character) of a UTF-16 offset in an ASCII text.
fn position(text: &str, offset: u32) -> (u32, u32) {
    let before = &text[..offset as usize];
    let line = before.matches('\n').count() as u32;
    let character = before.len() - before.rfind('\n').map_or(0, |index| index + 1);
    (line, character as u32)
}

fn range(text: &str, start: Option<u32>, length: Option<u32>) -> Range {
    let start = start.unwrap_or(0);
    (
        position(text, start),
        position(text, start + length.unwrap_or(0)),
    )
}

fn to_row(diagnostic: &Diagnostic, text: &str) -> Row {
    Row {
        range: range(text, diagnostic.start, diagnostic.length),
        code: diagnostic.code(),
        category: diagnostic.category(),
        message: flattened_message(&diagnostic.message, "\n", None)
            .to_string_lossy()
            .into_owned(),
        related: diagnostic
            .related
            .iter()
            .map(|related| {
                (
                    range(text, related.start, related.length),
                    related.message.text_in(None).to_string_lossy().into_owned(),
                )
            })
            .collect(),
    }
}

const SUGGESTION: DiagnosticCategory = DiagnosticCategory::Suggestion;
const ERROR: DiagnosticCategory = DiagnosticCategory::Error;
const DEPRECATED_HERE: &str = "The declaration was marked as deprecated here.";

#[test]
fn contextual_types_report_the_object_literals_deprecated_properties() {
    // TestJsdocDeprecated_suggestion3: tsgo `checkContextualDeprecations`
    // reports the object literal's `timeout` against its contextual type.
    let text = "interface RequestOptions {\n    /** @deprecated use signal instead */\n    timeout?: number;\n}\ndeclare function request(url: string, opts: RequestOptions): void;\n\nrequest(\"/api\", { timeout: 5000 });\ndeclare const opts: RequestOptions;\nopts.timeout;";
    let file = "/jsdocDeprecated_suggestion3.ts";
    let deprecated = |range| {
        related(
            row(range, 6385, SUGGESTION, "'timeout' is deprecated."),
            &[(((1, 8), (1, 39)), DEPRECATED_HERE)],
        )
    };
    assert_eq!(
        diagnostics(&[(file, text)], options(|_| {}), file),
        vec![
            deprecated(((6, 18), (6, 25))),
            deprecated(((8, 5), (8, 12)))
        ]
    );
}

#[test]
fn contextual_deprecations_follow_the_chosen_overload_and_every_name_kind() {
    // TestDeprecatedContextualPropertyOverload: the overload's contextual
    // type, accessors, string and numeric names.
    let text = "interface DeprecatedOptions {\n    kind: \"deprecated\";\n    /** @deprecated */\n    value: number;\n}\ninterface CurrentOptions {\n    kind: \"current\";\n    value: number;\n}\ndeclare function select(options: DeprecatedOptions): void;\ndeclare function select(options: CurrentOptions): void;\n\nselect({ kind: \"current\", value: 1 });\n\n/** @deprecated */\ndeclare const deprecatedValue: number;\nselect({ kind: \"current\", value: deprecatedValue });\n\ninterface DeprecatedContainer {\n    /** @deprecated */\n    value: number;\n}\ndeclare const deprecatedContainer: DeprecatedContainer;\nselect({ kind: \"current\", value: deprecatedContainer.value });\n\n/** @deprecated */\ndeclare function deprecatedCall(): number;\nselect({ kind: \"current\", value: deprecatedCall() });\n\ninterface DeprecatedAccessorOptions {\n    /** @deprecated */\n    value: number;\n}\ndeclare function accessor(options: DeprecatedAccessorOptions): void;\naccessor({ get value() { return 1; } });\naccessor({ set value(_value: number) {} });\n\ninterface DeprecatedNamedOptions {\n    /** @deprecated */\n    \"string-name\": number;\n    /** @deprecated */\n    1: number;\n}\ndeclare function named(options: DeprecatedNamedOptions): void;\nnamed({ \"string-name\": 1, 1: 1 });\n\ninterface FirstDeprecatedOptions {\n    kind: \"first\";\n    /** @deprecated */\n    value: number;\n}\ninterface SecondDeprecatedOptions {\n    kind: \"second\";\n    /** @deprecated */\n    value: number;\n}\ndeclare function selectDeprecated(options: FirstDeprecatedOptions): void;\ndeclare function selectDeprecated(options: SecondDeprecatedOptions): void;\n\nselectDeprecated({ kind: \"second\", value: 1 });";
    let file = "/deprecatedContextualPropertyOverload.ts";
    let deprecated = |range, message: &str, declaration| {
        related(
            row(range, 6385, SUGGESTION, message),
            &[(declaration, DEPRECATED_HERE)],
        )
    };
    assert_eq!(
        diagnostics(&[(file, text)], options(|_| {}), file),
        vec![
            row(
                ((16, 33), (16, 48)),
                6385,
                SUGGESTION,
                "'deprecatedValue' is deprecated."
            ),
            deprecated(
                ((23, 53), (23, 58)),
                "'value' is deprecated.",
                ((19, 8), (19, 20))
            ),
            related(
                row(
                    ((27, 33), (27, 47)),
                    6387,
                    SUGGESTION,
                    "The signature '(): number' of 'deprecatedCall' is deprecated."
                ),
                &[(((25, 4), (25, 16)), DEPRECATED_HERE)],
            ),
            deprecated(
                ((34, 15), (34, 20)),
                "'value' is deprecated.",
                ((30, 8), (30, 20))
            ),
            deprecated(
                ((35, 15), (35, 20)),
                "'value' is deprecated.",
                ((30, 8), (30, 20))
            ),
            deprecated(
                ((44, 8), (44, 21)),
                "'string-name' is deprecated.",
                ((38, 8), (38, 20))
            ),
            deprecated(
                ((44, 26), (44, 27)),
                "'1' is deprecated.",
                ((40, 8), (40, 20))
            ),
            deprecated(
                ((59, 35), (59, 40)),
                "'value' is deprecated.",
                ((53, 8), (53, 20))
            ),
        ]
    );
}

#[test]
fn a_deprecated_construct_signature_prints_as_one() {
    // TestJsdocDeprecated_suggestion1 (a.ts): tsgo's `signatureToString`
    // writes a construct signature `new (): C`.
    let text = "export namespace foo {\n    /** @deprecated */\n    export function faff () { }\n    faff()\n}\nconst a = foo.faff()\nfoo[\"faff\"]\nconst { faff } = foo\nfaff()\n/** @deprecated */\nexport function bar () {\n    foo?.faff()\n}\nfoo?.[\"faff\"]?.()\nbar();\n/** @deprecated */\nexport interface Foo {\n    /** @deprecated */\n    zzz: number\n}\n/** @deprecated */\nexport type QW = Foo[\"zzz\"]\nexport type WQ = QW\nclass C {\n    /** @deprecated */\n    constructor() {\n    }\n    /** @deprecated */\n    m() { }\n}\n/** @deprecated */\nclass D {\n    constructor() {\n    }\n}\nvar c = new C()\nc.m()\nc.m\nnew D()\nC\nD";
    let file = "/a.ts";
    let rows = diagnostics(
        &[(file, text)],
        options(|options| options.experimental_decorators = true),
        file,
    );
    let signature = |range, message: &str, declaration| {
        related(
            row(range, 6387, SUGGESTION, message),
            &[(declaration, DEPRECATED_HERE)],
        )
    };
    let entity = |range, message: &str, declaration| {
        related(
            row(range, 6385, SUGGESTION, message),
            &[(declaration, DEPRECATED_HERE)],
        )
    };
    let faff = ((1, 8), (1, 20));
    let unused = row(
        ((5, 6), (5, 7)),
        6133,
        SUGGESTION,
        "'a' is declared but its value is never read.",
    );
    assert_eq!(
        rows,
        vec![
            signature(
                ((3, 4), (3, 8)),
                "The signature '(): void' of 'faff' is deprecated.",
                faff
            ),
            unused,
            signature(
                ((5, 14), (5, 18)),
                "The signature '(): void' of 'foo.faff' is deprecated.",
                faff
            ),
            entity(((6, 4), (6, 10)), "'faff' is deprecated.", faff),
            entity(((7, 8), (7, 12)), "'faff' is deprecated.", faff),
            signature(
                ((8, 0), (8, 4)),
                "The signature '(): void' of 'faff' is deprecated.",
                faff
            ),
            signature(
                ((11, 9), (11, 13)),
                "The signature '(): void' of 'foo.faff' is deprecated.",
                faff
            ),
            signature(
                ((13, 6), (13, 12)),
                "The signature '(): void' of 'foo.faff' is deprecated.",
                faff
            ),
            signature(
                ((14, 0), (14, 3)),
                "The signature '(): void' of 'bar' is deprecated.",
                ((9, 4), (9, 16))
            ),
            entity(
                ((21, 17), (21, 20)),
                "'Foo' is deprecated.",
                ((15, 4), (15, 16))
            ),
            entity(
                ((21, 21), (21, 26)),
                "'zzz' is deprecated.",
                ((17, 8), (17, 20))
            ),
            entity(
                ((22, 17), (22, 19)),
                "'QW' is deprecated.",
                ((20, 4), (20, 16))
            ),
            signature(
                ((35, 12), (35, 13)),
                "The signature 'new (): C' of 'C' is deprecated.",
                ((24, 8), (24, 20))
            ),
            signature(
                ((36, 2), (36, 3)),
                "The signature '(): void' of 'c.m' is deprecated.",
                ((27, 8), (27, 20))
            ),
            entity(
                ((37, 2), (37, 3)),
                "'m' is deprecated.",
                ((27, 8), (27, 20))
            ),
            entity(
                ((38, 4), (38, 5)),
                "'D' is deprecated.",
                ((30, 4), (30, 16))
            ),
            entity(
                ((40, 0), (40, 1)),
                "'D' is deprecated.",
                ((30, 4), (30, 16))
            ),
        ]
    );
}

#[test]
fn an_unchecked_javascript_spelling_suggestion_keeps_its_related_row() {
    // TestCodeFixSpellingJs8: tsgo adds "'locals' is declared here." before
    // `addErrorOrSuggestion` makes the row a suggestion.
    let text = "var locals = {}\n// @ts-expect-error\nObject.keys(locale)";
    let file = "/a.js";
    assert_eq!(
        diagnostics(
            &[(file, text)],
            options(|options| {
                options.allow_js = true;
                options.allow_js_specified = Some(true);
                options.no_emit = Some(true);
            }),
            file,
        ),
        vec![related(
            row(
                ((2, 12), (2, 18)),
                2570,
                SUGGESTION,
                "Could not find name 'locale'. Did you mean 'locals'?"
            ),
            &[(((0, 4), (0, 10)), "'locals' is declared here.")],
        )]
    );
}

#[test]
fn an_overridden_late_bound_property_prints_its_computed_name() {
    // TestCodeFixPropertyOverrideAccess4: the base property through
    // tsgo's `symbolToString` (`[prop]`), not its internal name.
    let text = "const prop = Symbol.for('foo');\n\nclass A {\n    [prop] = 1;\n}\nclass B extends A {\n    get [prop]() { return 2; }\n}";
    let file = "/codeFixPropertyOverrideAccess4.ts";
    assert_eq!(
        diagnostics(
            &[(file, text)],
            options(|options| {
                options.target = Some(99);
                options.lib = Some(vec!["esnext".to_owned()]);
                options.strict = Some(true);
            }),
            file,
        ),
        vec![row(
            ((6, 8), (6, 14)),
            2611,
            ERROR,
            "'[prop]' is defined as a property in class 'A', but is overridden here in 'B' as an accessor."
        )]
    );
}

#[test]
fn a_truncated_type_counts_its_property_names_not_their_quoted_display() {
    // TestCodeFixAddMissingAttributes10: tsgo's node builder adds
    // `len(ast.SymbolName(propertySymbol)) + 1` per property, so fifteen
    // quoted names fit before `... 134 more ...`.
    let text = "type A = 'a' | 'b' | 'c' | 'd' | 'e';\ntype B = 1 | 2 | 3;\ntype C = '@' | '!';\ntype D = `${A}${Uppercase<A>}${B}${C}`;\nconst A = (props: { [K in D]: K }) =>\n   <div {...props}></div>;\n\nconst Bar = () =>\n   <A></A>";
    let file = "/foo.tsx";
    let intrinsic =
        "JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.";
    assert_eq!(
        diagnostics(&[(file, text)], options(|_| {}), file),
        vec![
            row(((5, 3), (5, 19)), 7026, ERROR, intrinsic),
            row(((5, 19), (5, 25)), 7026, ERROR, intrinsic),
            row(
                ((8, 4), (8, 5)),
                2740,
                ERROR,
                "Type '{}' is missing the following properties from type '{ \"aA1!\": \"aA1!\"; \"aA1@\": \"aA1@\"; \"aA2!\": \"aA2!\"; \"aA2@\": \"aA2@\"; \"aA3!\": \"aA3!\"; \"aA3@\": \"aA3@\"; \"aB1!\": \"aB1!\"; \"aB1@\": \"aB1@\"; \"aB2!\": \"aB2!\"; \"aB2@\": \"aB2@\"; \"aB3!\": \"aB3!\"; \"aB3@\": \"aB3@\"; \"aC1!\": \"aC1!\"; \"aC1@\": \"aC1@\"; \"aC2!\": \"aC2!\"; ... 134 more ...; \"eE3@\": \"eE3@\"; }': \"aA1!\", \"aA1@\", \"aA2!\", \"aA2@\", and 146 more."
            ),
        ]
    );
}

#[test]
fn an_anonymous_function_expressions_declaration_error_names_its_variable() {
    // TestCodeFixMissingTypeAnnotationOnExports8: tsgo's
    // `GetErrorRangeForNode` names an anonymous function expression by the
    // variable it is assigned to (`g`).
    let text = "function foo() {return 42;}\nexport const g = function () { return foo(); };";
    let file = "/codeFixMissingTypeAnnotationOnExports8.ts";
    assert_eq!(
        diagnostics(
            &[(file, text)],
            options(|options| {
                options.declaration = Some(true);
                options.isolated_declarations = Some(true);
            }),
            file,
        ),
        vec![related(
            row(
                ((1, 38), (1, 43)),
                9013,
                ERROR,
                "Expression type can't be inferred with --isolatedDeclarations."
            ),
            &[
                (
                    ((1, 13), (1, 14)),
                    "Add a return type to the function expression."
                ),
                (
                    ((1, 38), (1, 43)),
                    "Add satisfies and a type assertion to this expression (satisfies T as T) to make the type explicit."
                ),
            ],
        )]
    );
}

#[test]
fn duplicate_accessor_declaration_errors_merge_and_sort_their_related_rows() {
    // The declaration transformer reports each accessor's error twice;
    // tsgo's `filterAndSortDiagnostics` (compactAndMergeRelatedInfos)
    // merges the copies and sorts their related rows by position: the get
    // accessor's before the set accessor's.
    let text = "function foo() { return 42;}\nexport class A {\n  get a() {\n    return foo();\n  }\n  set a(value) {}\n}\n";
    let file = "/code.ts";
    let accessor = |range| {
        related(
            row(
                range,
                9009,
                ERROR,
                "At least one accessor must have an explicit type annotation with --isolatedDeclarations.",
            ),
            &[
                (
                    ((2, 6), (2, 7)),
                    "Add a return type to the get accessor declaration.",
                ),
                (
                    ((5, 6), (5, 7)),
                    "Add a type to parameter of the set accessor declaration.",
                ),
            ],
        )
    };
    assert_eq!(
        diagnostics_of(
            &[(file, text)],
            options(|options| {
                options.declaration = Some(true);
                options.isolated_declarations = Some(true);
            }),
            file,
            Kinds::Declaration,
        ),
        vec![accessor(((2, 6), (2, 7))), accessor(((5, 8), (5, 13)))]
    );
}

#[test]
fn jsx_attributes_report_their_deprecated_contextual_properties() {
    // tsgo `checkJsxAttributes` defers `checkContextualDeprecations` too.
    let text = "declare namespace JSX {\n    interface IntrinsicElements {\n        div: {\n            /** @deprecated */\n            old?: number;\n            fresh?: number;\n        };\n    }\n}\nconst a = <div old={1} fresh={2} />;\n";
    let file = "/app.tsx";
    assert_eq!(
        diagnostics(&[(file, text)], options(|_| {}), file),
        vec![related(
            row(((9, 15), (9, 18)), 6385, SUGGESTION, "'old' is deprecated."),
            &[(((3, 16), (3, 28)), DEPRECATED_HERE)],
        )]
    );
}
