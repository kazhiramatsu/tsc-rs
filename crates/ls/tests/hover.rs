//! Hover as tsgo answers it, for the cases the hover port's fixes reach and
//! the conformance baselines do not show. Each case is a fourslash test's
//! input with tsgo's answer, which the lsp suite's transcripts record
//! (`scripts/lsp_transcripts.py`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tsc_compiler::LiveProgram;
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_host::MemoryCompilerHost;
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::UserPreferences;
use tsc_ls::LanguageService;
use tsc_lsproto::{
    DocumentUri, Hover, HoverParams,
    MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings, MarkupKind, Position,
    PositionEncodingKind, ResolvedClientCapabilities, TextDocumentIdentifier,
};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// A protocol range: (line, character) to (line, character).
type Range = ((u32, u32), (u32, u32));

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

/// tsgo's hover at `(line, character)` of `file` for a client with or
/// without Visual Studio extensions.
fn provide_hover(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    position: (u32, u32),
    verbosity_level: Option<i32>,
    vs_capability: bool,
) -> Option<Hover> {
    provide_hovers(
        files,
        options,
        file,
        &[position],
        verbosity_level,
        vs_capability,
    )
    .pop()
    .expect("one answer")
}

/// tsgo's hovers at `positions` of `file`, asked in turn of one program (a
/// session's requests share its checker).
fn provide_hovers(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    positions: &[(u32, u32)],
    verbosity_level: Option<i32>,
    vs_capability: bool,
) -> Vec<Option<Hover>> {
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
        .filter(|(name, _)| !name.ends_with(".map"))
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
    let preferences = UserPreferences::default();
    let mut capabilities = ResolvedClientCapabilities::default();
    capabilities.text_document.hover.content_format =
        vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT];
    capabilities.experimental.hover_verbosity_level = true;
    capabilities._vs_supports_visual_studio_extensions = vs_capability;
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    positions
        .iter()
        .map(|&(line, character)| {
            let params = HoverParams {
                text_document: TextDocumentIdentifier {
                    uri: DocumentUri(format!("file://{file}")),
                },
                position: Position { line, character },
                work_done_token: None,
                verbosity_level,
            };
            service.provide_hover(&params).expect("an answer")
        })
        .collect()
}

/// tsgo's hover at `(line, character)` of `file`: the markdown value, the
/// range and whether a higher verbosity level shows more.
fn hover(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    position: (u32, u32),
    verbosity_level: Option<i32>,
) -> Option<(String, Range, bool)> {
    let hover = provide_hover(files, options, file, position, verbosity_level, false)?;
    let MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(content) =
        hover.contents
    else {
        panic!("markup content");
    };
    let range = hover.range.expect("a range");
    Some((
        content.value,
        (
            (range.start.line, range.start.character),
            (range.end.line, range.end.character),
        ),
        hover.can_increase_verbosity,
    ))
}

/// tsgo's hover for a client with Visual Studio extensions: the markdown
/// value and the `_vs_rawContent` it shows instead, as JSON.
fn hover_vs(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    position: (u32, u32),
) -> Option<(String, serde_json::Value)> {
    let hover = provide_hover(files, options, file, position, None, true)?;
    let MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(content) =
        hover.contents
    else {
        panic!("markup content");
    };
    Some((
        content.value,
        serde_json::to_value(&hover._vs_raw_content).expect("raw content as JSON"),
    ))
}

/// A `_vs_rawContent` tsgo answered, as JSON.
fn raw(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("tsgo's raw content")
}

/// quickInfoDisplayPartsTypeParameterInClass: a symbol's qualified name writes
/// its parent's type parameters or instantiated type arguments, and a call's
/// signature its type arguments.
#[test]
fn type_arguments_in_qualified_names_and_signatures() {
    let files = [
        ("/quickInfoDisplayPartsTypeParameterInClass.ts", "class c<T> {\n    constructor(a: T) {\n    }\n    method<U>(a: U, b: T) {\n        return a;\n    }\n}\nvar cInstance = new c(\"Hello\");\nvar cVal = c;\ncInstance.method(\"hello\", \"cello\");\nclass c2<T extends c<string>> {\n    constructor(a: T) {\n    }\n    method<U extends c<string>>(a: U, b: T) {\n        return a;\n    }\n}\nvar cInstance1 = new c2(cInstance);\nvar cVal2 = c2;\ncInstance1.method(cInstance, cInstance);"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsTypeParameterInClass.ts",
            (3, 4),
            None
        ),
        Some((
            "```typescript\n(method) c<T>.method<U>(a: U, b: T): U\n```\n".to_owned(),
            ((3, 4), (3, 10)),
            false,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsTypeParameterInClass.ts",
            (3, 11),
            None
        ),
        Some((
            "```typescript\n(type parameter) U in c<T>.method<U>(a: U, b: T): U\n```\n".to_owned(),
            ((3, 11), (3, 12)),
            false,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsTypeParameterInClass.ts",
            (7, 20),
            None
        ),
        Some((
            "```typescript\nconstructor c<string>(a: string): c<string>\n```\n".to_owned(),
            ((7, 20), (7, 21)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickInfoDisplayPartsTypeParameterInClass.ts", (9, 10), None),
        Some((
            "```typescript\n(method) c<string>.method<\"hello\">(a: \"hello\", b: string): \"hello\"\n```\n".to_owned(),
            ((9, 10), (9, 16)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickInfoDisplayPartsTypeParameterInClass.ts", (13, 11), None),
        Some((
            "```typescript\n(type parameter) U extends c<string> in c2<T extends c<string>>.method<U extends c<string>>(a: U, b: T): U\n```\n".to_owned(),
            ((13, 11), (13, 12)),
            true,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsTypeParameterInClass.ts",
            (17, 21),
            None
        ),
        Some((
            "```typescript\nconstructor c2<c<string>>(a: c<string>): c2<c<string>>\n```\n"
                .to_owned(),
            ((17, 21), (17, 23)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickInfoDisplayPartsTypeParameterInClass.ts", (19, 11), None),
        Some((
            "```typescript\n(method) c2<c<string>>.method<c<string>>(a: c<string>, b: c<string>): c<string>\n```\n".to_owned(),
            ((19, 11), (19, 17)),
            true,
        ))
    );
}

/// quickInfoGenericTypePath: a class declared in a generic function writes the
/// function's type arguments as an instantiation expression.
#[test]
fn instantiated_outer_type_arguments() {
    let files = [
        ("/quickInfoGenericTypePath.ts", "\nfunction f<T>(x: T) {\n  class C {\n    value = x\n  }\n  return new C()\n}\n\nclass Box<T> {\n  public value: T;\n  constructor(value: T) {\n    this.value = value;\n  }\n}\n\nconst instance = f(\"hello\");\nconst b1 = new Box(instance);\ndeclare const b2: Box<typeof instance>;\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoGenericTypePath.ts",
            (15, 18),
            None
        ),
        Some((
            "```typescript\nfunction f<string>(x: string): f<string>.C\n```\n".to_owned(),
            ((15, 17), (15, 18)),
            true,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoGenericTypePath.ts",
            (16, 8),
            None
        ),
        Some((
            "```typescript\nconst b1: Box<f<string>.C>\n```\n".to_owned(),
            ((16, 6), (16, 8)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickInfoGenericTypePath.ts", (16, 18), None),
        Some((
            "```typescript\nconstructor Box<f<string>.C>(value: f<string>.C): Box<f<string>.C>\n```\n".to_owned(),
            ((16, 15), (16, 18)),
            true,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoGenericTypePath.ts",
            (17, 16),
            None
        ),
        Some((
            "```typescript\nconst b2: Box<f<string>.C>\n```\n".to_owned(),
            ((17, 14), (17, 16)),
            true,
        ))
    );
}

/// quickInfoOnFunctionPropertyReturnedFromGenericFunction1: a function's
/// outer type parameters instantiate its qualified name.
#[test]
fn instantiated_function_qualifier() {
    let files = [
        ("/quickInfoOnFunctionPropertyReturnedFromGenericFunction1.ts", "function createProps<T>(t: T) {\n  function getProps() {}\n  function createVariants() {}\n\n  getProps.createVariants = createVariants;\n  return getProps;\n}\n\ncreateProps({}).createVariants();"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoOnFunctionPropertyReturnedFromGenericFunction1.ts",
            (8, 16),
            None
        ),
        Some((
            "```typescript\n(property) getProps<{}>.createVariants: () => void\n```\n".to_owned(),
            ((8, 16), (8, 30)),
            false,
        ))
    );
}

/// hoverSelfReExportedNamespaceGenericNoCrash: a property of a generic
/// interface reached through a self re-exported namespace.
#[test]
fn self_re_exported_namespace_generic() {
    let files = [
        ("/main.ts", "import { Box } from \"./mod\"\ndeclare const b: Box<string>\nconst x = b.content\n"),
        ("/mod.ts", "export interface Box<A> { content: Content<A> }\nexport type Content<A> = A\nexport * as Box from \"./mod\""),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/main.ts", (2, 12), None),
        Some((
            "```typescript\n(property) Box<string>.content: string\n```\n".to_owned(),
            ((2, 12), (2, 19)),
            false,
        ))
    );
}

/// quickInfoJsDocTagsTypedef: a typedef with a type is documented by its whole
/// comment.
#[test]
fn typedef_comment() {
    let files = [
        ("/quickInfoJsDocTagsTypedef.js", "/**\n * Bar comment\n * @typedef {Object} Bar\n * @property {string} baz - baz comment\n * @property {string} qux - qux comment\n */\n\n/**\n * foo comment\n * @param {Bar} x - x comment\n * @returns {Bar}\n */\nfunction foo(x) {\n    return x;\n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoJsDocTagsTypedef.js",
            (2, 21),
            None
        ),
        Some((
            "```typescript\ntype Bar = {\n    baz: string;\n    qux: string;\n}\n```\nBar comment"
                .to_owned(),
            ((2, 21), (2, 24)),
            false,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoJsDocTagsTypedef.js",
            (9, 11),
            None
        ),
        Some((
            "```typescript\ntype Bar = {\n    baz: string;\n    qux: string;\n}\n```\nBar comment"
                .to_owned(),
            ((9, 11), (9, 14)),
            false,
        ))
    );
}

/// quickInfoPropertyTag: a typedef's property is documented by its `@property`
/// tag's comment.
#[test]
fn typedef_property_comment() {
    let files = [
        ("/a.js", "/**\n * @typedef I\n * @property {number} x Doc\n *                      More doc\n */\n\n/** @type {I} */\nconst obj = { x: 10 };"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (7, 14), None),
        Some((
            "```typescript\n(property) x: number\n```\nDoc\nMore doc".to_owned(),
            ((7, 14), (7, 15)),
            false,
        ))
    );
}

/// quickInfoJSDocTypedefPropertyWithInvalidTag: an `@` before something other
/// than an identifier stays comment text.
#[test]
fn typedef_property_comment_with_at_signs() {
    let files = [
        ("/a.js", "/**\n * @typedef {Object} MyType1\n * @property {string} name\n * @-rule\n * @property {number} age\n */\n\n/**\n * @typedef {Object} MyType2\n * @property {string} name\n * some comment\n * @property {number} age\n */\n\n/**\n * @typedef {Object} MyType3\n * @property {string} name\n * @*stars\n * @property {number} age\n */\n\n/**\n * @typedef {Object} MyType4\n * @property {string} name\n * @(parens)\n * @property {number} age\n */\n\n/**\n * @typedef {Object} MyType5\n * @property {string} name\n * @foo*bar\n * @property {number} age\n */\n\n/** @type {MyType1} */\nconst obj1 = { name: \"\", age: 10 };\n\n/** @type {MyType2} */\nconst obj2 = { name: \"\", age: 10 };\n\n/** @type {MyType3} */\nconst obj3 = { name: \"\", age: 10 };\n\n/** @type {MyType4} */\nconst obj4 = { name: \"\", age: 10 };\n\n/** @type {MyType5} */\nconst obj5 = { name: \"\", age: 10 };\n"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (36, 15), None),
        Some((
            "```typescript\n(property) name: string\n```\n@-rule".to_owned(),
            ((36, 15), (36, 19)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (39, 15), None),
        Some((
            "```typescript\n(property) name: string\n```\nsome comment".to_owned(),
            ((39, 15), (39, 19)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (42, 15), None),
        Some((
            "```typescript\n(property) name: string\n```\n@*stars".to_owned(),
            ((42, 15), (42, 19)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (45, 15), None),
        Some((
            "```typescript\n(property) name: string\n```\n@(parens)".to_owned(),
            ((45, 15), (45, 19)),
            false,
        ))
    );
}

/// quickInfoJSDocParamWithTrailingAtBeforeCommentEnd: a trailing `@` is text.
#[test]
fn trailing_at_sign() {
    let files = [(
        "/a.js",
        "/** @param {string} x trailing @*/\nfunction foo(x) {}\n",
    )];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (1, 9), None),
        Some((
            "```typescript\nfunction foo(x: string): void\n```\n\n\n*@param* `x` \u{2014} trailing @".to_owned(),
            ((1, 9), (1, 12)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (1, 13), None),
        Some((
            "```typescript\n(parameter) x: string\n```\ntrailing @".to_owned(),
            ((1, 13), (1, 14)),
            false,
        ))
    );
}

/// commentsLinePreservation: only the comment's last piece loses its trailing
/// white space.
#[test]
fn trailing_blank_comment_line() {
    let files = [
        ("/commentsLinePreservation.ts", "/** This is firstLine\n  * This is second Line\n  * \n  * This is fourth Line\n  */\nvar a: string;\n/** \n  * This is firstLine\n  * This is second Line\n  * \n  * This is fourth Line\n  */\nvar b: string;\n/** \n  * This is firstLine\n  * This is second Line\n  * \n  * This is fourth Line\n  *\n  */\nvar c: string;\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param\n  * @random tag This should be third line\n  */\nfunction d(param: string) { param = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param\n  */\nfunction e(param: string) { param = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1 first line of param\n  *\n  *  param information third line\n  * @random tag This should be third line\n  */\nfunction f(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1\n  *\n  *  param information first line\n  * @random tag This should be third line\n  */\nfunction g(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1\n  *\n  *  param information first line\n  *\n  *  param information third line\n  * @random tag This should be third line\n  */\nfunction h(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1\n  *\n  *  param information first line\n  *\n  *  param information third line\n  *\n  */\nfunction i(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1\n  *\n  *  param information first line\n  *\n  *  param information third line\n  */\nfunction j(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1 hello   @randomtag \n  *\n  *  random information first line\n  *\n  *  random information third line\n  */\nfunction k(param1: string) { param1 = \"hello\"; }\n/** \n  * This is firstLine\n  * This is second Line\n  * @param param1 first Line text\n  *\n  * @param param1 \n  *\n  * blank line that shouldnt be shown when starting this \n  * second time information about the param again\n  */\nfunction l(param1: string) { param1 = \"hello\"; }\n     /** \n       * This is firstLine\n This is second Line\n [1]: third * line\n @param param1 first Line text\n second line text\n */\nfunction m(param1: string) { param1 = \"hello\"; }"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/commentsLinePreservation.ts", (5, 4), None),
        Some((
            "```typescript\nvar a: string\n```\nThis is firstLine\nThis is second Line\n\nThis is fourth Line".to_owned(),
            ((5, 4), (5, 5)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/commentsLinePreservation.ts", (12, 4), None),
        Some((
            "```typescript\nvar b: string\n```\nThis is firstLine\nThis is second Line\n\nThis is fourth Line".to_owned(),
            ((12, 4), (12, 5)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/commentsLinePreservation.ts", (20, 4), None),
        Some((
            "```typescript\nvar c: string\n```\nThis is firstLine\nThis is second Line\n\nThis is fourth Line\n".to_owned(),
            ((20, 4), (20, 5)),
            false,
        ))
    );
}

/// quickInfoInheritedLinkTag: a link's `A#b` names `A.b`.
#[test]
fn link_member_names() {
    let files = [
        ("/quickInfoInheritedLinkTag.ts", "export class C {\n     /**\n      * @deprecated Use {@link PerspectiveCamera#setFocalLength .setFocalLength()} and {@link PerspectiveCamera#filmGauge .filmGauge} instead.\n      */\n    m() { }\n}\nexport class D extends C {\n    m() { } // crashes here\n}\nnew C().m // and here (with a different thing trying to access undefined)"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickInfoInheritedLinkTag.ts", (9, 9), None),
        Some((
            "```typescript\n(method) C.m(): void\n```\n\n\n*@deprecated* \u{2014} Use PerspectiveCamera.setFocalLength .setFocalLength() and PerspectiveCamera.filmGauge .filmGauge instead.".to_owned(),
            ((9, 8), (9, 9)),
            false,
        ))
    );
}

/// jsDocTagsWithHyphen: a typedef property named by an invalid identifier has
/// no symbol; its type is `any`.
#[test]
fn invalid_property_name_has_no_symbol() {
    let files = [
        ("/dummy.js", "/**\n * @typedef Product\n * @property {string} title\n * @property {boolean} high-top some-comments\n */\n\n/**\n * @type {Product}\n */\nconst product = {\n    \n}"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/dummy.js", (3, 24), None),
        Some((
            "```typescript\nany\n```\n".to_owned(),
            ((3, 23), (3, 31)),
            false,
        ))
    );
}

/// quickInfoJsdocTypedefMissingType: a JSDoc tag's name is no declaration name.
#[test]
fn typedef_name_without_type() {
    let files = [("/a.js", "/**\n * @typedef A\n */\nvar x;")];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (1, 12), None),
        Some((
            "```typescript\nany\n```\n".to_owned(),
            ((1, 12), (1, 13)),
            false,
        ))
    );
}

/// quickInfoPatternAmbientModuleWithImportAttributes: an ambient module's
/// import attributes, on one line.
#[test]
fn pattern_ambient_module_import_attributes() {
    let files = [
        ("/quickInfoPatternAmbientModuleWithImportAttributes.ts", "declare module \"*.css\" with { type: \"css\" } {\n    const styles: { readonly [className: string]: string };\n    export default styles;\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickInfoPatternAmbientModuleWithImportAttributes.ts",
            (0, 22),
            None
        ),
        Some((
            "```typescript\nmodule \"*.css\" with { type: \"css\"; }\n```\n".to_owned(),
            ((0, 16), (0, 21)),
            true,
        ))
    );
}

/// quickInfoMergedPatternAmbientModuleWithImportAttributes: pattern ambient
/// modules merge by their import attributes.
#[test]
fn merged_pattern_ambient_modules_by_import_attributes() {
    let files = [
        ("/first.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export const cssOnly: \"css\";\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export const textOnly: \"text\";\n}\n"),
        ("/second.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export const cssAlso: \"css-also\";\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export const textAlso: \"text-also\";\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/first.d.ts", (0, 24), None),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"css\"; }\n```\n".to_owned(),
            ((0, 16), (0, 23)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/first.d.ts", (3, 24), None),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"text\"; }\n```\n".to_owned(),
            ((3, 16), (3, 23)),
            true,
        ))
    );
}

/// patternAmbientModuleWithImportAttributesLanguageService: an import's
/// attributes pick the pattern ambient module.
#[test]
fn pattern_ambient_modules_imported_with_attributes() {
    let files = [
        ("/index.ts", "import * as css from \"./style.asset\" with { type: \"css\" };\nimport * as text from \"./copy.asset\" with { type: \"text\" };\ncss.shared;\ntext.shared;"),
        ("/types.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export interface CssPayload { kind: \"css\" }\n    export const shared: CssPayload;\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export interface TextPayload { kind: \"text\" }\n    export const shared: TextPayload;\n}\n"),
    ];
    let compiler_options = || {
        options(|options| {
            options.module = Some(200);
            options.module_resolution = Some(100);
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/index.ts", (0, 21), None),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"css\"; }\n```\n".to_owned(),
            ((0, 22), (0, 35)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/index.ts", (1, 22), None),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"text\"; }\n```\n".to_owned(),
            ((1, 23), (1, 35)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/index.ts", (2, 4), None),
        Some((
            "```typescript\nconst shared: css.CssPayload\n```\n".to_owned(),
            ((2, 4), (2, 10)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/index.ts", (3, 5), None),
        Some((
            "```typescript\nconst shared: text.TextPayload\n```\n".to_owned(),
            ((3, 5), (3, 11)),
            true,
        ))
    );
}

/// quickInfoVerbosityJSDocNamespacedTypedef: a namespaced typedef's innermost
/// name is the alias's name, and names in its type resolve in its namespace.
#[test]
fn namespaced_typedef_names() {
    let files = [
        ("/index.js", "// Namespaced typedef\n/** @typedef {string} NS.T */\n\n// Namespaced typedef aliased to qualified namespaced typedef.\n/** @typedef {NS.T} NS.U */\n\n// Namespaced typedef aliased to implicitly-resolved typedef.\n/** @typedef {U} NS.V */\n"),
    ];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/index.js", (1, 25), None),
        Some((
            "```typescript\ntype T = string\n```\n".to_owned(),
            ((1, 25), (1, 26)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/index.js", (4, 23), None),
        Some((
            "```typescript\ntype U = string\n```\n".to_owned(),
            ((4, 23), (4, 24)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/index.js", (7, 20), None),
        Some((
            "```typescript\ntype V = string\n```\n".to_owned(),
            ((7, 20), (7, 21)),
            false,
        ))
    );
}

/// quickinfoVerbosityEmptyEnum: an empty enum is no union, so it does not
/// expand.
#[test]
fn empty_enum_does_not_expand() {
    let files = [(
        "/quickinfoVerbosityEmptyEnum.ts",
        "\nenum Degree {}\n\ndeclare const e: Degree;\n",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityEmptyEnum.ts",
            (3, 15),
            None
        ),
        Some((
            "```typescript\nconst e: Degree\n```\n".to_owned(),
            ((3, 14), (3, 15)),
            false,
        ))
    );
}

/// quickinfoVerbosityEnum: an expanded enum lists its members with their
/// written initializers or constant values; a const enum keeps `const`.
#[test]
fn expanded_enums() {
    let files = [
        ("/a.ts", "export {};\nenum Color {\n    Red,\n    Green,\n    Blue,\n}\nconst x: Color = Color.Red;\nconst enum Direction {\n    Up,\n    Down,\n}\nconst y: Direction = Direction.Up;\nenum Flags {\n    None = 0,\n    IsDirectory = 1 << 0,\n    IsFile = 1 << 1,\n    IsSymlink = 1 << 2,\n}"),
        ("/b.ts", "export enum Color {\n    Red = \"red\"\n}"),
        ("/c.ts", "import { Color } from \"./b\";\nconst c: Color = Color.Red;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/a.ts", (1, 10), Some(1)),
        Some((
            "```typescript\nenum Color {\n    Red = 0,\n    Green = 1,\n    Blue = 2\n}\n```\n"
                .to_owned(),
            ((1, 5), (1, 10)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.ts", (6, 7), Some(1)),
        Some((
            "```typescript\nconst x: Color.Red | Color.Green | Color.Blue\n```\n".to_owned(),
            ((6, 6), (6, 7)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.ts", (7, 20), Some(1)),
        Some((
            "```typescript\nconst enum Direction {\n    Up = 0,\n    Down = 1\n}\n```\n".to_owned(),
            ((7, 11), (7, 20)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.ts", (11, 7), Some(1)),
        Some((
            "```typescript\nconst y: Direction.Up | Direction.Down\n```\n".to_owned(),
            ((11, 6), (11, 7)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.ts", (12, 10), Some(1)),
        Some((
            "```typescript\nenum Flags {\n    None = 0,\n    IsDirectory = 1 << 0,\n    IsFile = 1 << 1,\n    IsSymlink = 1 << 2\n}\n```\n".to_owned(),
            ((12, 5), (12, 10)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/c.ts", (1, 14), Some(1)),
        Some((
            "```typescript\n(alias) enum Color {\n    Red = \"red\"\n}\n```\n".to_owned(),
            ((1, 9), (1, 14)),
            false,
        ))
    );
}

/// quickinfoVerbosityEmptyEnum: an empty enum's type stays its name at a
/// higher level too.
#[test]
fn expanded_empty_enum() {
    let files = [(
        "/quickinfoVerbosityEmptyEnum.ts",
        "\nenum Degree {}\n\ndeclare const e: Degree;\n",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityEmptyEnum.ts",
            (3, 15),
            Some(1)
        ),
        Some((
            "```typescript\nconst e: Degree\n```\n".to_owned(),
            ((3, 14), (3, 15)),
            false,
        ))
    );
}

/// quickinfoVerbosityClass1: an expanded class lists its members, its
/// constructors and heritage clauses.
#[test]
fn expanded_classes() {
    let files = [
        ("/quickinfoVerbosityClass1.ts", "{\n    class Foo {\n        a!: \"a\" | \"c\";\n    }\n    const f = new Foo();\n}\n{\n    type FooParam = \"a\" | \"b\";\n    class Foo {\n        constructor(public x: string) {\n            this.x = \"a\";\n        }\n        foo(p: FooParam): void {}\n    }\n    const f = new Foo(\"\");\n}\n{\n    class Bar {\n        a!: string;\n        bar(): void {}\n        baz(param: string): void {}\n    }\n    class Foo extends Bar {\n        b!: boolean;\n        override baz(param: string | number): void {}\n    }\n    const f = new Foo();\n}\n{\n    class Bar<B extends string> {\n        bar(param: B): void {}\n        baz(): this { return this; }\n    }\n    class Foo extends Bar<\"foo\"> {\n        foo(): this { return this; }\n    }\n    const b = new Bar();\n    const f = new Foo();\n}\n{\n    class Bar<B extends string> {\n        bar(param: B): void {}\n        baz(): this { return this; }\n    }\n    const noname = new (class extends Bar<\"foo\"> {\n        foo(): this { return this; }\n    })();\n    const klass = class extends Bar<\"foo\"> {\n        foo(): this { return this; }\n    };\n    const k = new klass();\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClass1.ts",
            (4, 11),
            Some(1)
        ),
        Some((
            "```typescript\nconst f: {\n    a: \"a\" | \"c\";\n}\n```\n".to_owned(),
            ((4, 10), (4, 11)),
            false,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClass1.ts",
            (14, 11),
            Some(1)
        ),
        Some((
            "```typescript\nconst f: {\n    x: string;\n    foo(p: FooParam): void;\n}\n```\n"
                .to_owned(),
            ((14, 10), (14, 11)),
            true,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClass1.ts",
            (14, 11),
            Some(2)
        ),
        Some((
            "```typescript\nconst f: {\n    x: string;\n    foo(p: \"a\" | \"b\"): void;\n}\n```\n"
                .to_owned(),
            ((14, 10), (14, 11)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (17, 13), Some(1)),
        Some((
            "```typescript\nclass Bar {\n    a: string;\n    bar(): void;\n    baz(param: string): void;\n}\n```\n".to_owned(),
            ((17, 10), (17, 13)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (26, 11), Some(1)),
        Some((
            "```typescript\nconst f: {\n    b: boolean;\n    baz(param: string | number): void;\n    a: string;\n    bar(): void;\n}\n```\n".to_owned(),
            ((26, 10), (26, 11)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (36, 11), Some(1)),
        Some((
            "```typescript\nconst b: {\n    bar(param: string): void;\n    baz(): Bar<string>;\n}\n```\n".to_owned(),
            ((36, 10), (36, 11)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (37, 11), Some(1)),
        Some((
            "```typescript\nconst f: {\n    foo(): Foo;\n    bar(param: \"foo\"): void;\n    baz(): Foo;\n}\n```\n".to_owned(),
            ((37, 10), (37, 11)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (44, 16), Some(1)),
        Some((
            "```typescript\nconst noname: {\n    foo(): (Anonymous class);\n    bar(param: \"foo\"): void;\n    baz(): (Anonymous class);\n}\n```\n".to_owned(),
            ((44, 10), (44, 16)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityClass1.ts", (50, 11), Some(1)),
        Some((
            "```typescript\nconst k: {\n    foo(): klass;\n    bar(param: \"foo\"): void;\n    baz(): klass;\n}\n```\n".to_owned(),
            ((50, 10), (50, 11)),
            false,
        ))
    );
}

/// quickinfoVerbosityAbstractClass: an expanded class keeps `abstract`.
#[test]
fn expanded_abstract_class() {
    let files = [
        ("/quickinfoVerbosityAbstractClass.ts", "\ndeclare abstract class Shape {\n    abstract area(): number;\n    abstract perimeter(): number;\n    toString(): string;\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityAbstractClass.ts", (1, 28), Some(1)),
        Some((
            "```typescript\nabstract class Shape {\n    abstract area(): number;\n    abstract perimeter(): number;\n    toString(): string;\n}\n```\n".to_owned(),
            ((1, 23), (1, 28)),
            false,
        ))
    );
}

/// quickinfoVerbosityClassWithMixinBase: a class whose base is a mixin call.
#[test]
fn expanded_class_with_mixin_base() {
    let files = [
        ("/quickinfoVerbosityClassWithMixinBase.ts", "\nclass Base {}\n\ndeclare const Mixin: new () => Base & { mixed: string };\n\nclass Derived extends Mixin {}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClassWithMixinBase.ts",
            (5, 13),
            Some(1)
        ),
        Some((
            "```typescript\nclass Derived extends Mixin {\n}\n```\n".to_owned(),
            ((5, 6), (5, 13)),
            false,
        ))
    );
}

/// quickinfoVerbosityClassInterfaceMerge: a class merged with an interface
/// expands as the class.
#[test]
fn expanded_class_and_interface() {
    let files = [
        ("/quickinfoVerbosityClassInterfaceMerge.ts", "\ndeclare class Foo {\n    x: number;\n}\ndeclare interface Foo {\n    y: string;\n}\nconst f: Foo = { x: 1, y: \"hello\" };\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClassInterfaceMerge.ts",
            (1, 17),
            Some(1)
        ),
        Some((
            "```typescript\nclass Foo {\n    x: number;\n    y: string;\n}\n```\n".to_owned(),
            ((1, 14), (1, 17)),
            false,
        ))
    );
    assert_eq!(
        hover(
            &files,
            compiler_options(),
            "/quickinfoVerbosityClassInterfaceMerge.ts",
            (7, 12),
            Some(1)
        ),
        Some((
            "```typescript\nclass Foo {\n    x: number;\n    y: string;\n}\n```\n".to_owned(),
            ((7, 9), (7, 12)),
            false,
        ))
    );
}

/// quickinfoVerbosityInterfaceMemberOrdering: index, construct and call
/// signatures come before properties.
#[test]
fn expanded_interface_member_order() {
    let files = [
        ("/quickinfoVerbosityInterfaceMemberOrdering.ts", "\ninterface Callable {\n    (x: string): boolean;\n    new (x: string): Callable;\n    [key: string]: any;\n    name: string;\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityInterfaceMemberOrdering.ts", (1, 18), Some(1)),
        Some((
            "```typescript\ninterface Callable {\n    [key: string]: any;\n    new (x: string): Callable;\n    (x: string): boolean;\n    name: string;\n}\n```\n".to_owned(),
            ((1, 10), (1, 18)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespace: an expanded namespace lists its exports;
/// an alias of a local is declared first and exported by name.
#[test]
fn expanded_namespaces() {
    let files = [
        ("/1.ts", "export {};\nclass Foo<T> {\n    y: string;\n}\nnamespace Foo {\n    export var y: number = 1;\n    export var x: string = \"hello\";\n    export var w = \"world\";\n    var z = 2;\n}"),
        ("/2.ts", "export namespace Foo {\n    export var y: number = 1;\n    export var x: string = \"hello\";\n}"),
        ("/3.ts", "import * as Foo_1 from \"./b\";\nexport declare namespace ns {\n    import Foo = Foo_1.Foo;\n    export { Foo };\n    export const c: number;\n    export const d = 1;\n    let e: Apple;\n    export let f: Apple;\n}\ninterface Apple {\n    a: string;\n}"),
        ("/4.ts", "class Foo<T> {\n    y!: T;\n}\nnamespace Two {\n    export const f = new Foo<number>();\n}"),
        ("/5.ts", "namespace Two {\n    export const g = new Foo<string>();\n}"),
        ("/6.ts", "namespace OnlyLocal {\n    const bar: number;\n}"),
        ("/foo.ts", "export function foo() { return \"foo\"; }\nimport(\"./foo\")\nvar x = import(\"./foo\")"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/1.ts", (4, 13), Some(1)),
        Some((
            "```typescript\nclass Foo<T> {\n    y: string;\n}\nnamespace Foo {\n    let y: number;\n    let x: string;\n    let w: string;\n}\n\n```\n".to_owned(),
            ((4, 10), (4, 13)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/3.ts", (1, 27), Some(1)),
        Some((
            "```typescript\nnamespace ns {\n    export { Foo };\n    export let c: number;\n    export let d: 1;\n    export let f: Apple;\n}\n```\n".to_owned(),
            ((1, 25), (1, 27)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/3.ts", (1, 27), Some(2)),
        Some((
            "```typescript\nnamespace ns {\n    export { Foo };\n    export let c: number;\n    export let d: 1;\n    export let f: {\n        a: string;\n    };\n}\n```\n".to_owned(),
            ((1, 25), (1, 27)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/4.ts", (3, 13), Some(1)),
        Some((
            "```typescript\nnamespace Two {\n    let f: Foo<number>;\n    let g: Foo<string>;\n}\n```\n".to_owned(),
            ((3, 10), (3, 13)),
            true,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/4.ts", (3, 13), Some(2)),
        Some((
            "```typescript\nnamespace Two {\n    let f: {\n        y: number;\n    };\n    let g: {\n        y: string;\n    };\n}\n```\n".to_owned(),
            ((3, 10), (3, 13)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/6.ts", (0, 19), Some(1)),
        Some((
            "```typescript\nnamespace OnlyLocal { }\n```\n".to_owned(),
            ((0, 10), (0, 19)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/foo.ts", (1, 8), Some(1)),
        Some((
            "```typescript\nmodule \"/foo\" {\n    function foo(): string;\n    namespace foo { }\n}\n```\n".to_owned(),
            ((1, 8), (1, 13)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceDefaultExport: a namespace's default export
/// is no identifier and is left out.
#[test]
fn expanded_namespace_default_export() {
    let files = [
        ("/quickinfoVerbosityNamespaceDefaultExport.ts", "\ndeclare namespace ns {\n    interface Shape {\n        sides: number;\n    }\n    const circle: Shape;\n    export default circle;\n    export { Shape };\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceDefaultExport.ts", (1, 20), Some(1)),
        Some((
            "```typescript\nnamespace ns {\n    export let default: any;\n    export { Shape };\n}\n```\n".to_owned(),
            ((1, 18), (1, 20)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceTypeAliases: type aliases in an expanded
/// namespace.
#[test]
fn expanded_namespace_type_aliases() {
    let files = [
        ("/quickinfoVerbosityNamespaceTypeAliases.ts", "\ntype BaseConfig = { host: string; port: number };\n\ndeclare namespace Config {\n    type Readonly<T> = { readonly [K in keyof T]: T[K] };\n    type Optional<T> = { [K in keyof T]?: T[K] };\n    type ServerConfig = Readonly<BaseConfig>;\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceTypeAliases.ts", (3, 24), Some(1)),
        Some((
            "```typescript\nnamespace Config {\n    type Readonly<T> = { readonly [K in keyof T]: T[K]; };\n    type Optional<T> = { [K in keyof T]?: T[K] | undefined; };\n    type ServerConfig = {\n        readonly host: string;\n        readonly port: number;\n    };\n}\n```\n".to_owned(),
            ((3, 18), (3, 24)),
            false,
        ))
    );
}

/// quickInfoVerbosityNamespaceBindingWithDefaultExportedFunction1.
#[test]
fn expanded_namespace_binding_with_default_function() {
    let files = [
        (
            "/a.ts",
            "export default function fn() {}\nexport { fn as default };",
        ),
        ("/b.ts", "import * as ns from \"./a\";\n\nns;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/b.ts", (2, 2), Some(1)),
        Some((
            "```typescript\n(alias) module \"/a\" {\n    export { default };\n}\n```\n".to_owned(),
            ((2, 0), (2, 2)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceInterfaceHeritageCrash: an interface's heritage
/// clause as written.
#[test]
fn expanded_namespace_interface_heritage() {
    let files = [(
        "/quickinfoVerbosityNamespaceInterfaceHeritageCrash.ts",
        "\ndeclare namespace NS {\n    interface Config extends Record<string, any> {}\n}\n",
    )];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceInterfaceHeritageCrash.ts", (1, 20), Some(1)),
        Some((
            "```typescript\nnamespace NS {\n    interface Config extends Record<string, any> {\n    }\n}\n```\n".to_owned(),
            ((1, 18), (1, 20)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceInterfaceHeritageIntersectionCrash.
#[test]
fn expanded_namespace_interface_heritage_intersection() {
    let files = [
        ("/quickinfoVerbosityNamespaceInterfaceHeritageIntersectionCrash.ts", "\ndeclare namespace NS {\n    type Mixin = { a: string } & { b: number };\n    interface Config extends Mixin {}\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceInterfaceHeritageIntersectionCrash.ts", (1, 20), Some(1)),
        Some((
            "```typescript\nnamespace NS {\n    type Mixin = {\n        a: string;\n    } & {\n        b: number;\n    };\n    interface Config extends Mixin {\n    }\n}\n```\n".to_owned(),
            ((1, 18), (1, 20)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceErrorClassHeritage1: a class extending an
/// unresolved name.
#[test]
fn expanded_namespace_error_class_heritage() {
    let files = [
        ("/quickinfoVerbosityNamespaceErrorClassHeritage1.ts", "\nnamespace NS {\n    export class Derived extends NonExistentClass {\n        derivedField: number;\n    }\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceErrorClassHeritage1.ts", (1, 12), Some(1)),
        Some((
            "```typescript\nnamespace NS {\n    class Derived extends NonExistentClass {\n        derivedField: number;\n    }\n}\n```\n".to_owned(),
            ((1, 10), (1, 12)),
            false,
        ))
    );
}

/// quickinfoVerbosityNamespaceAnonymousClassHeritage1.
#[test]
fn expanded_namespace_anonymous_class_heritage() {
    let files = [
        ("/quickinfoVerbosityNamespaceAnonymousClassHeritage1.ts", "\nnamespace NS {\n    export class Derived extends class {\n        baseField: string;\n    } {\n        derivedField: number;\n    }\n}\n"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityNamespaceAnonymousClassHeritage1.ts", (1, 12), Some(1)),
        Some((
            "```typescript\nnamespace NS {\n    class Derived extends class {\n        baseField: string;\n    } {\n        derivedField: number;\n    }\n}\n```\n".to_owned(),
            ((1, 10), (1, 12)),
            false,
        ))
    );
}

/// quickinfoVerbosityToplevelTruncation1: a long expansion is cut with a
/// count of the members left out.
#[test]
fn expanded_truncation() {
    let files = [
        ("/quickinfoVerbosityToplevelTruncation1.ts", "export enum LargeEnum {\n    Member1,\n    Member2,\n    Member3,\n    Member4,\n    Member5,\n    Member6,\n    Member7,\n    Member8,\n    Member9,\n    Member10,\n    Member11,\n    Member12,\n    Member13,\n    Member14,\n    Member15,\n    Member16,\n    Member17,\n    Member18,\n    Member19,\n    Member20,\n    Member21,\n    Member22,\n    Member23,\n    Member24,\n    Member25,\n}\nexport interface LargeInterface {\n    property1: string;\n    property2: number;\n    property3: boolean;\n    property4: Date;\n    property5: string[];\n    property6: number[];\n    property7: boolean[];\n    property8: { [key: string]: unknown };\n    property9: string | null;\n    property10: number | null;\n    property11: boolean | null;\n    property12: Date | null;\n    property13: string | number;\n    property14: number | boolean;\n    property15: string | boolean;\n    property16: Array<{ id: number; name: string }>;\n    property17: Array<{ key: string; value: unknown }>;\n    property18: { nestedProp1: string; nestedProp2: number };\n    property19: { nestedProp3: boolean; nestedProp4: Date };\n    property20: () => void;\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityToplevelTruncation1.ts", (0, 21), Some(1)),
        Some((
            "```typescript\nenum LargeEnum {\n    Member1 = 0,\n    Member2 = 1,\n    Member3 = 2,\n    Member4 = 3,\n    Member5 = 4,\n    Member6 = 5,\n    Member7 = 6,\n    Member8 = 7,\n    Member9 = 8,\n    Member10 = 9,\n    Member11 = 10,\n    Member12 = 11,\n    Member13 = 12,\n    Member14 = 13,\n    Member15 = 14,\n    Member16 = 15,\n    Member17 = 16,\n    Member18 = 17,\n    Member19 = 18,\n    Member20 = 19,\n    Member21 = 20,\n    Member22 = 21,\n    Member23 = 22,\n    Member24 = 23,\n    Member25 = 24\n}\n```\n".to_owned(),
            ((0, 12), (0, 21)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/quickinfoVerbosityToplevelTruncation1.ts", (27, 31), Some(1)),
        Some((
            "```typescript\ninterface LargeInterface {\n    property1: string;\n    property2: number;\n    property3: boolean;\n    property4: Date;\n    property5: string[];\n    property6: number[];\n    property7: boolean[];\n    property8: {\n        [key: string]: unknown;\n    };\n    property9: string | null;\n    property10: number | null;\n    property11: boolean | null;\n    property12: Date | null;\n    property13: string | number;\n    property14: number | boolean;\n    property15: string | boolean;\n    property16: {\n        id: number;\n        name: string;\n    }[];\n    property17: {\n        key: string;\n        value: unknown;\n    }[];\n    property18: {\n        nestedProp1: string;\n        nestedProp2: number;\n    };\n    property19: {\n        nestedProp3: boolean;\n        nestedProp4: Date;\n    };\n    property20: ...;\n}\n```\n".to_owned(),
            ((27, 17), (27, 31)),
            false,
        ))
    );
}

/// quickInfoMergedPatternAmbientModuleWithImportAttributes: an expanded
/// pattern ambient module keeps its import attributes on one line.
#[test]
fn expanded_pattern_ambient_modules() {
    let files = [
        ("/first.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export const cssOnly: \"css\";\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export const textOnly: \"text\";\n}\n"),
        ("/second.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export const cssAlso: \"css-also\";\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export const textAlso: \"text-also\";\n}"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover(&files, compiler_options(), "/first.d.ts", (0, 24), Some(1)),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"css\"; } {\n    let cssOnly: \"css\";\n    let cssAlso: \"css-also\";\n}\n```\n".to_owned(),
            ((0, 16), (0, 23)),
            false,
        ))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/first.d.ts", (3, 24), Some(1)),
        Some((
            "```typescript\nmodule \"*.asset\" with { type: \"text\"; } {\n    let textOnly: \"text\";\n    let textAlso: \"text-also\";\n}\n```\n".to_owned(),
            ((3, 16), (3, 23)),
            false,
        ))
    );
}

/// quickInfoAliasVS: an alias shows its target's icon and the declaration's
/// documentation summary.
#[test]
fn vs_alias() {
    let files = [
        ("/a.ts", "/**\n * Doc\n * @tag Tag text\n */\nexport const x = 0;"),
        ("/b.ts", "import { x } from \"./a\";\nx;"),
        ("/c.ts", "/**\n * Doc 2\n * @tag Tag text 2\n */\nimport {\n    /**\n     * Doc 3\n     * @tag Tag text 3\n     */\n    x\n} from \"./a\";\nx;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover_vs(&files, compiler_options(), "/b.ts", (1, 1)),
        Some((
            "```typescript\n(alias) const x: 0\n```\nDoc\n\n*@tag* \u{2014} Tag text".to_owned(),
            raw(
                r#"{"Style":1,"Elements":[{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":620,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"alias","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"const ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"local name","Text":"x","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":": ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"string","Text":"0","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"},{"Runs":[{"ClassificationTypeName":"text","Text":"Doc","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
    assert_eq!(
        hover_vs(&files, compiler_options(), "/c.ts", (11, 1)),
        Some((
            "```typescript\n(alias) const x: 0\n```\nDoc\n\n*@tag* \u{2014} Tag text".to_owned(),
            raw(
                r#"{"Style":1,"Elements":[{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":620,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"alias","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"const ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"local name","Text":"x","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":": ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"string","Text":"0","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"},{"Runs":[{"ClassificationTypeName":"text","Text":"Doc","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
}

/// quickInfoDisplayPartsFunctionVS: parameter names, optional and rest
/// parameters, and a signature's trailing semicolon.
#[test]
fn vs_function_parameters() {
    let files = [
        ("/quickInfoDisplayPartsFunctionVS.ts", "function foo(param: string, optionalParam?: string, paramWithInitializer = \"hello\", ...restParam: string[]) {\n}\nfunction foowithoverload(a: string): string;\nfunction foowithoverload(a: number): number;\nfunction foowithoverload(a: any): any {\n    return a;\n}\nfunction foowith3overload(a: string): string;\nfunction foowith3overload(a: number): number;\nfunction foowith3overload(a: boolean): boolean;\nfunction foowith3overload(a: any): any {\n    return a;\n}\nfoo(\"hello\");\nfoowithoverload(\"hello\");\nfoowithoverload(10);\nfoowith3overload(\"hello\");\nfoowith3overload(10);\nfoowith3overload(true);"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover_vs(&files, compiler_options(), "/quickInfoDisplayPartsFunctionVS.ts", (0, 9)),
        Some((
            "```typescript\nfunction foo(param: string, optionalParam?: string, paramWithInitializer?: string, ...restParam: string[]): void;\n```\n".to_owned(),
            raw(r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1880,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"keyword","Text":"function ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"foo","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"param","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":",","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"optionalParam","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"?","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":",","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"paramWithInitializer","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"?","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":",","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"...","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"restParam","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"[","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"]","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#),
        ))
    );
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsFunctionVS.ts",
            (2, 9)
        ),
        Some((
            "```typescript\nfunction foowithoverload(a: string): string;\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1880,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"keyword","Text":"function ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"foowithoverload","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"a","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
}

/// quickInfoDisplayPartsClassMethodVS: a method's icon by its accessibility.
#[test]
fn vs_class_method_icons() {
    let files = [
        ("/quickInfoDisplayPartsClassMethodVS.ts", "class c {\n    public publicMethod() { }\n    private privateMethod() { }\n    protected protectedMethod() { }\n    static staticMethod() { }\n    private static privateStaticMethod() { }\n    protected static protectedStaticMethod() { }\n    method() {\n        this.publicMethod();\n        this.privateMethod();\n        this.protectedMethod();\n        c.staticMethod();\n        c.privateStaticMethod();\n        c.protectedStaticMethod();\n    }\n}\nvar cInstance = new c();\ncInstance.publicMethod();\nc.staticMethod();"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassMethodVS.ts",
            (1, 11)
        ),
        Some((
            "```typescript\n(method) c.publicMethod(): void;\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1880,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"method","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"c.publicMethod","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassMethodVS.ts",
            (2, 12)
        ),
        Some((
            "```typescript\n(method) c.privateMethod(): void;\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1878,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"method","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"c.privateMethod","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassMethodVS.ts",
            (3, 14)
        ),
        Some((
            "```typescript\n(method) c.protectedMethod(): void;\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1879,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"method","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"c.protectedMethod","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassMethodVS.ts",
            (4, 11)
        ),
        Some((
            "```typescript\n(method) c.staticMethod(): void;\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1880,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"method","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"c.staticMethod","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
}

/// quickInfoDisplayPartsClassPropertyVS: a property's runs and icon.
#[test]
fn vs_class_properties() {
    let files = [
        ("/quickInfoDisplayPartsClassPropertyVS.ts", "class c {\n    public publicProperty: string;\n    private privateProperty: string;\n    protected protectedProperty: string;\n    static staticProperty: string;\n    private static privateStaticProperty: string;\n    protected static protectedStaticProperty: string;\n    method() {\n        this.publicProperty;\n        this.privateProperty;\n        this.protectedProperty;\n        c.staticProperty;\n        c.privateStaticProperty;\n        c.protectedStaticProperty;\n    }\n}\nvar cInstance = new c();\ncInstance.publicProperty;\nc.staticProperty;"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassPropertyVS.ts",
            (1, 11)
        ),
        Some((
            "```typescript\n(property) c.publicProperty: string\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":2436,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"property","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"property name","Text":"c.publicProperty","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":": ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
    assert_eq!(
        hover_vs(
            &files,
            compiler_options(),
            "/quickInfoDisplayPartsClassPropertyVS.ts",
            (2, 12)
        ),
        Some((
            "```typescript\n(property) c.privateProperty: string\n```\n".to_owned(),
            raw(
                r#"{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":2434,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"text","Text":"property","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":") ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"property name","Text":"c.privateProperty","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":": ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#
            ),
        ))
    );
}

/// quickInfoCommentsFunctionDeclarationVS: the documentation is the plain
/// comment, without its tags.
#[test]
fn vs_documentation_summary() {
    let files = [
        ("/quickInfoCommentsFunctionDeclarationVS.ts", "/** This comment should appear for foo*/\nfunction foo() {\n}\nfoo();\n/** This is comment for function signature*/\nfunction fooWithParameters(/** this is comment about a*/a: string,\n    /** this is comment for b*/\n    b: number) {\n    var d = a;\n}\nfooWithParameters(\"a\",10);\n/**\n* Does something\n* @param a a string\n*/\ndeclare function fn(a: string);\nfn(\"hello\");"),
    ];
    let compiler_options = || options(|_| {});
    assert_eq!(
        hover_vs(&files, compiler_options(), "/quickInfoCommentsFunctionDeclarationVS.ts", (5, 11)),
        Some((
            "```typescript\nfunction fooWithParameters(a: string, b: number): void;\n```\nThis is comment for function signature".to_owned(),
            raw(r#"{"Style":1,"Elements":[{"Style":0,"Elements":[{"ImageId":{"Guid":"ae27a6b0-e345-4288-96df-5eaf394ee369","Id":1880,"_vs_type":"ImageId"},"_vs_type":"ImageElement"},{"Runs":[{"ClassificationTypeName":"keyword","Text":"function ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"method name","Text":"fooWithParameters","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":"(","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"a","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"string","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":",","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"parameter name","Text":"b","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"number","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":")","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":":","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"whitespace","Text":" ","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"keyword","Text":"void","_vs_type":"ClassifiedTextRun"},{"ClassificationTypeName":"punctuation","Text":";","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"},{"Runs":[{"ClassificationTypeName":"text","Text":"This is comment for function signature","_vs_type":"ClassifiedTextRun"}],"_vs_type":"ClassifiedTextElement"}],"_vs_type":"ContainerElement"}"#),
        ))
    );
}

/// tsgo `finalizeBoundary` drops the length a reuse of an existing type node
/// counted, failed or not (a reused node counts as its source text): a
/// property's union whose reuse fails on an inaccessible alias counts its
/// expanded members once, and the hover is not truncated (found by the LSP
/// test on hono, which truncated hono's `ClientRequestOptions`). Pinned to
/// tsgo's answer at aa814927.
#[test]
fn a_failed_type_node_reuse_counts_no_length() {
    let globals = "declare function send(input: string): Promise<string>;\n";
    // Ten more properties bring the count near hover's limit (500).
    let extra: String = (0..10)
        .map(|index| format!("  extra{index}?: string\n"))
        .collect();
    let types = format!("class Server {{\n  request = (input: string | number | boolean, init?: object, env?: object, context?: object): string | number | Promise<string> => \"\"\n}}\ntype ServerRequest = (typeof Server.prototype)['request']\nexport type Options<T = unknown> = {{\n  send?: typeof send | ServerRequest\n{extra}  init?: object\n  buildSearchParams?: (query: Record<string, string | string[]>) => object\n}} & (keyof T extends never\n  ? {{ headers?: Record<string, string> | (() => Record<string, string> | Promise<Record<string, string>>) }}\n  : {{ headers: T | (() => T | Promise<T>) }})\n");
    let index = "export type { Options } from './types'\n";
    let files = [
        ("/index.ts", index),
        ("/types.ts", types.as_str()),
        ("/globals.ts", globals),
    ];
    assert_eq!(
        hover(&files, options(|_| {}), "/index.ts", (0, 14), None),
        Some((
            "```typescript\n(alias) type Options<T = unknown> = {\n    send?: typeof send | ((input: string | number | boolean, init?: object, env?: object, context?: object) => string | number | Promise<string>);\n    extra0?: string;\n    extra1?: string;\n    extra2?: string;\n    extra3?: string;\n    extra4?: string;\n    extra5?: string;\n    extra6?: string;\n    extra7?: string;\n    extra8?: string;\n    extra9?: string;\n    init?: object;\n    buildSearchParams?: (query: Record<string, string | string[]>) => object;\n} & (keyof T extends never ? {\n    headers?: Record<string, string> | (() => Record<string, string> | Promise<Record<string, string>>);\n} : {\n    headers: T | (() => T | Promise<T>);\n})\n```\n".to_owned(),
            ((0, 14), (0, 21)),
            true,
        ))
    );
}

/// tsgo marks a hover truncated only where it cuts an expansion
/// (`checkTruncationLengthIfExpanding`), not where `mapToTypeNodes` or
/// `createTypeNodesFromResolvedType` cut a type: a type parameter's hover
/// whose signature is cut (`... 2 more ...`) still offers a higher
/// verbosity (found by the LSP test on hono, in its factory's handlers).
/// Pinned to tsgo's answer at aa814927.
#[test]
fn a_cut_type_still_offers_a_higher_verbosity() {
    let text = "interface Env { Variables?: object; Bindings?: object }\ntype IfAnyThenEmptyObject<T> = 0 extends 1 & T ? {} : T\ntype ProcessHead<T> = IfAnyThenEmptyObject<T extends Env ? (Env extends T ? {} : T) : T>\ntype IntersectNonAnyTypes<T extends any[]> = T extends [infer Head, ...infer Rest]\n  ? ProcessHead<Head> & IntersectNonAnyTypes<Rest>\n  : {}\ndeclare function f<\n  E extends Env,\n  I extends object = {},\n  I2 extends object = I,\n  I3 extends object = I & I2,\n  E2 extends Env = E,\n  E3 extends Env = IntersectNonAnyTypes<[E, E2]>,\n  E4 extends Env = IntersectNonAnyTypes<[E, E2, E3]>,\n>(a: E2, b: E3, c: E4): [E2, E3, E4]\n";
    assert_eq!(
        hover(&[("/a.ts", text)], options(|_| {}), "/a.ts", (12, 2), None),
        Some((
            "```typescript\n(type parameter) E3 extends Env in f<E extends Env, I extends object = {}, I2 extends object = I, I3 extends object = I & I2, E2 extends Env = E, E3 extends Env = IfAnyThenEmptyObject<E extends Env ? Env extends E ? {} : E : E> & IfAnyThenEmptyObject<E2 extends Env ? Env extends E2 ? {} : E2 : E2> & {}, E4 extends Env = IfAnyThenEmptyObject<...> & ... 2 more ... & ...>(a: E2, b: E3, c: E4): [E2, E3, E4]\n```\n".to_owned(),
            ((12, 2), (12, 4)),
            true,
        ))
    );
}

/// The `this` of a `this` type is a token (tsgo synthesizes one) in a type
/// reference, so the hover's meaning is a type's: an interface merged with
/// a `const` of its name shows `this`, not the `const` (found by the LSP
/// test on zod, whose `ZodType` interface merges with a `const`). Pinned to
/// tsgo's answer at aa814927.
#[test]
fn a_this_type_of_an_interface_merged_with_a_const_shows_this() {
    let text = "interface Box<T> { t: T }\nexport interface Foo { m(): Box<this>; }\nexport const Foo = 1;\n";
    assert_eq!(
        hover(&[("/a.ts", text)], options(|_| {}), "/a.ts", (1, 32), None),
        Some((
            "```typescript\nthis\n```\n".to_owned(),
            ((1, 32), (1, 36)),
            false,
        ))
    );
}

/// tsgo always copies a `this` type when it reuses a declaration's type
/// node (nodecopy.go: Strada's `canReuseTypeNode` check never failed for
/// it): an inherited method's reused return annotation keeps `this` (found
/// by the LSP test on zod, in `meta()`'s `$replace<GlobalMeta, this>`).
/// Pinned to tsgo's answer at aa814927.
#[test]
fn a_reused_return_annotation_keeps_its_this_type() {
    let text = "type Replace<M, S> = M extends string ? S : M;\ninterface Base<T> {\n  meta(): Replace<T, this> | undefined;\n  meta2(): Replace<number, this> | undefined;\n  meta3(): Replace<T, T> | undefined;\n}\ninterface Str extends Base<string> { x: number }\ndeclare const A: Str;\nA.meta();\nA.meta2();\nA.meta3();\n";
    assert_eq!(
        hover(&[("/a.ts", text)], options(|_| {}), "/a.ts", (9, 2), None),
        Some((
            "```typescript\n(method) Base<string>.meta2(): Replace<number, this> | undefined\n```\n"
                .to_owned(),
            ((9, 2), (9, 7)),
            false,
        ))
    );
}

/// tsgo records an access answered by an index signature with its index
/// symbol as the access's resolved symbol, so `getTypeOfSymbolAtLocation`
/// takes the access for a reference and answers its narrowed type (found
/// by the LSP test on effect, `Array.isArray(annotations.brands) ?
/// annotations.brands : []`). Pinned to tsgo's answer at aa814927.
#[test]
fn an_index_signature_access_hovers_its_narrowed_type() {
    let text = "interface Annotations { readonly [x: string]: unknown }\nfunction g(a: Annotations) {\n  if (Array.isArray(a.brands)) { a.brands; }\n}\n";
    assert_eq!(
        hover(&[("/a.ts", text)], options(|_| {}), "/a.ts", (2, 35), None),
        Some((
            "```typescript\nany[]\n```\n".to_owned(),
            ((2, 35), (2, 41)),
            false,
        ))
    );
}

/// The node builder counts a `readonly` index signature's modifier
/// (tsgo `indexInfoToIndexSignatureDeclarationHelper`), which moves where
/// a long hover is cut (found by the LSP test on effect, in its devtools'
/// schema types). Pinned to tsgo's answer at aa814927.
#[test]
fn a_readonly_index_signature_moves_the_hover_cut() {
    let properties = (0..60)
        .map(|index| format!("a{index}: string"))
        .collect::<Vec<_>>()
        .join("; ");
    let text = format!("declare const v: {{ readonly [x: string]: string; readonly [y: number]: string; {properties} }};\nconst w = v;\n");
    let shown = (0..42)
        .map(|index| format!("    a{index}: string;\n"))
        .collect::<String>();
    assert_eq!(
        hover(&[("/a.ts", text.as_str())], options(|_| {}), "/a.ts", (1, 6), None),
        Some((
            format!("```typescript\nconst w: {{\n    readonly [x: string]: string;\n    readonly [y: number]: string;\n{shown}    ... 17 more ...;\n    a59: string;\n}}\n```\n"),
            ((1, 6), (1, 7)),
            false,
        ))
    );
}

/// tsgo `GetHostSignatureFromJSDoc` takes only the declaration the comment
/// is attached to (Strada's `getEffectiveJSDocHost` is not ported): a
/// `{@link effect}` above `const effectContext = (effect) => …` resolves at
/// the statement, to the module's `effect`, not the parameter (found by the
/// LSP test on effect, in `Layer.effectContext`'s `@see`). Pinned to tsgo's
/// answer at aa814927.
#[test]
fn a_jsdoc_link_above_a_variable_statement_skips_its_initializer() {
    let text = "export const effect = 1;\n/**\n * @see {@link effect} for a single service\n */\nexport const effectContext = (effect: number) => effect;\neffectContext(1);\n";
    assert_eq!(
        hover(&[("/a.ts", text)], options(|_| {}), "/a.ts", (5, 0), None),
        Some((
            "```typescript\nconst effectContext: (effect: number) => number\n```\n\n\n*@see* \u{2014} [effect](file:///a.ts#1,14-1,20) for a single service".to_owned(),
            ((5, 0), (5, 13)),
            false,
        ))
    );
}

/// The first line of a hover's markdown code block.
fn hover_head(hover: Option<Hover>) -> String {
    let MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(content) =
        hover.expect("a hover").contents
    else {
        panic!("markup content");
    };
    content
        .value
        .lines()
        .nth(1)
        .expect("a code line")
        .to_owned()
}

/// tsgo's checker keeps one node builder for its displays
/// (`typeToStringNodebuilder`), whose cache of built nodes outlives a
/// request: once a hover's qualifier has built `typeof a` alone, a later
/// qualifier reuses that whole node where a fresh build would cut it (found
/// by the LSP test on zod). Pinned to tsgo's answers at aa814927.
#[test]
fn a_later_hover_reuses_the_checker_s_built_nodes() {
    let a = (0..12)
        .map(|index| format!("a{index}: string"))
        .collect::<Vec<_>>()
        .join("; ");
    let b = (0..12)
        .map(|index| format!("b{index}: number"))
        .collect::<Vec<_>>()
        .join("; ");
    let text = format!("declare const a: {{ {a} }};\ninterface Box<T> {{ get(): T }}\ninterface Pair<U, T> {{ get(): T }}\ndeclare const y: Box<typeof a>;\ndeclare const x: Pair<{{ {b} }}, typeof a>;\ny.get();\nx.get();\n");
    let files = [("/a.ts", text.as_str())];
    let pair = format!("(method) Pair<{{ {}; }}, ", b);
    let alone = hover_head(provide_hover(
        &files,
        options(|_| {}),
        "/a.ts",
        (6, 2),
        None,
        false,
    ));
    assert_eq!(
        alone,
        format!("{pair}{{ a0: string; a1: string; a2: string; ... 8 more ...; a11: string; }}>.get(): {{")
    );
    let mut hovers = provide_hovers(
        &files,
        options(|_| {}),
        "/a.ts",
        &[(5, 2), (6, 2)],
        None,
        false,
    );
    let after = hover_head(hovers.pop().expect("two answers"));
    assert_eq!(after, format!("{pair}{{ {a}; }}>.get(): {{"));
}

/// A JavaScript function's `@returns {name is T}` is its signature's type
/// predicate, which the hover prints as tsgo does (found by the LSP test on
/// svelte; tsc-rs printed `boolean`).
#[test]
fn javascript_return_type_predicate() {
    let files = [(
        "/a.js",
        "/**\n * @param {string} name\n * @returns {name is \"a\"}\n */\nexport function is_a(name) {\n\treturn name === 'a';\n}\n",
    )];
    assert_eq!(
        hover(
            &files,
            options(|options| {
                options.allow_js = true;
                options.check_js = Some(true);
            }),
            "/a.js",
            (4, 16),
            None
        ),
        Some((
            "```typescript\nfunction is_a(name: string): name is \"a\"\n```\n\n\n*@param* `name`\n\n*@returns*"
                .to_owned(),
            ((4, 16), (4, 20)),
            false,
        ))
    );
}

/// In a TypeScript file tsgo does not reparse JSDoc, so a `@typedef`'s
/// types stay under their host and see its type parameters: the `Response`
/// of a `@property` type in a declaration file generated from JavaScript is
/// the host function's type parameter, not the global `Response` (tsgo
/// aa814927; webpack-dev-middleware's `middleware.d.ts` in nuxt's session,
/// where tsc-rs counted it among the global's references).
#[test]
fn a_jsdoc_typedef_in_a_declaration_file_sees_its_host_s_type_parameters() {
    let files = [(
        "/mw.d.ts",
        "/**\n * @template {ServerResponse} Response\n * @typedef {object} SendErrorOptions\n * @property {Modify<Request, Response>=} modify\n */\nexport function send<Response>(res: Response): void;\nexport interface ServerResponse { status: number }\nexport type Modify<A, B> = (a: A, b: B) => void;\n",
    )];
    assert_eq!(
        hover(&files, options(|_| {}), "/mw.d.ts", (3, 30), None),
        Some((
            "```typescript\n(type parameter) Response in send<Response>(res: Response): void\n```\n"
                .to_owned(),
            ((3, 30), (3, 38)),
            false,
        ))
    );
}

/// A reused type annotation whose `import()` names a module reachable only
/// through a `node_modules` path fails its reuse boundary, and the boundary
/// restores the builder's error state: the property is serialized afresh
/// without the import (tsgo aa814927 `finalizeBoundary`; remix's hover over
/// a `createClient()` result, whose redis module re-exports pnpm-nested
/// RESP types, panicked with "should always get typenode").
#[test]
fn a_reused_import_type_through_node_modules_falls_back_to_a_fresh_type() {
    let files = [
        (
            "/node_modules/pkg/package.json",
            "{\"name\": \"pkg\", \"version\": \"1.0.0\", \"types\": \"index.d.ts\"}\n",
        ),
        (
            "/node_modules/pkg/index.d.ts",
            "export declare const x: {\n    f: () => import(\"other/dist/inner\").T;\n    g: (a: import(\"other/dist/inner\").T) => void;\n};\n",
        ),
        (
            "/node_modules/pkg/node_modules/other/package.json",
            "{\"name\": \"other\", \"version\": \"1.0.0\"}\n",
        ),
        (
            "/node_modules/pkg/node_modules/other/dist/inner.d.ts",
            "export interface T { t: number }\n",
        ),
        ("/src/a.ts", "import { x } from 'pkg'\nconst y = x\nexport { y }\n"),
    ];
    assert_eq!(
        hover(
            &files,
            options(|options| {
                options.module = Some(199);
                options.module_resolution = Some(99);
            }),
            "/src/a.ts",
            (1, 6),
            None,
        ),
        Some((
            "```typescript\nconst y: {\n    f: () => T;\n    g: (a: T) => void;\n}\n```\n"
                .to_owned(),
            ((1, 6), (1, 7)),
            true,
        ))
    );
}

/// A nested `@param options.fs file system` tag documents the property it
/// declares: tsgo's reparser makes a property signature of it with the tag
/// as its JSDoc, like a typedef's `@property` (tsgo aa814927; webpack's
/// `processRuntimeRequirements({ chunks })` hover showed "chunks").
#[test]
fn a_nested_param_tag_documents_its_property() {
    let files = [(
        "/docs.js",
        "/**\n * @param {object} options options\n * @param {string} options.fs file system\n */\nexport function run(options) {}\nrun({ fs: 'memory' });\n",
    )];
    assert_eq!(
        hover(
            &files,
            options(|options| {
                options.allow_js = true;
                options.check_js = Some(true);
            }),
            "/docs.js",
            (5, 6),
            None,
        ),
        Some((
            "```typescript\n(property) fs: string\n```\nfile system".to_owned(),
            ((5, 6), (5, 8)),
            false,
        ))
    );
}

/// The property of an instantiated interface is qualified by the type
/// arguments it was instantiated with, not by the interface's type parameter
/// declarations with their defaults (svelte's `Value<V = unknown>`; tsgo
/// aa814927 prints `Value<V>`).
#[test]
fn javascript_instantiated_qualifier_has_no_defaults() {
    let files = [
        (
            "/types.d.ts",
            "export interface Signal {\n\tf: number;\n}\nexport type Equals = (this: Value, value: unknown) => boolean;\nexport interface Value<V = unknown> extends Signal {\n\tequals: Equals;\n\tv: V;\n}\n",
        ),
        (
            "/a.js",
            "/** @import { Value } from './types' */\n\n/**\n * @template V\n * @param {Value<V>} source\n * @param {V} value\n * @returns {V}\n */\nexport function internal_set(source, value) {\n\tif (!source.equals(value)) {\n\t\treturn value;\n\t}\n\treturn value;\n}\n",
        ),
    ];
    assert_eq!(
        hover(
            &files,
            options(|options| {
                options.allow_js = true;
                options.check_js = Some(true);
            }),
            "/a.js",
            (9, 13),
            None
        ),
        Some((
            "```typescript\n(property) Value<V>.equals: (this: Value, value: unknown) => boolean\n```\n"
                .to_owned(),
            ((9, 13), (9, 19)),
            true,
        ))
    );
}

/// A JavaScript function with `@overload` tags is declared first by the
/// overloads tsgo's reparser makes statements of, which have no JSDoc of
/// their own: its hover shows the signatures without the implementation's
/// documentation, at the `function` keyword and through a re-export alike
/// (found by the LSP test on svelte; pinned to tsgo aa814927).
#[test]
fn javascript_overloads_have_no_documentation() {
    let files = [("/a.js", "/**\n * @template V\n * @overload\n * @param {() => V} get\n * @param {(v: V) => void} set\n * @returns {V}\n */\n/**\n * @template V\n * @overload\n * @param {() => V} get\n * @returns {V}\n */\n/**\n * Create a store from a function that returns state.\n * @template V\n * @param {() => V} get\n * @param {(v: V) => void} [set]\n * @returns {V}\n */\nexport function toStore(get, set) {\n\treturn get();\n}\n\nexport { toStore as derived };\n")];
    let compiler_options = || {
        options(|options| {
            options.allow_js = true;
            options.check_js = Some(true);
        })
    };
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (20, 7), None),
        Some(("```typescript\nfunction toStore<V>(get: () => V, set: (v: V) => void): V\nfunction toStore<V>(get: () => V): V\n```\n".to_owned(), ((20, 7), (20, 15)), false))
    );
    assert_eq!(
        hover(&files, compiler_options(), "/a.js", (24, 9), None),
        Some(("```typescript\nfunction toStore<V>(get: () => V, set: (v: V) => void): V\nfunction toStore<V>(get: () => V): V\n```\n".to_owned(), ((24, 9), (24, 16)), false))
    );
}

/// tsgo's token search cannot reach a nested cast's own JSDoc (the
/// reparsed `as` expression has none and starts at its operand's full
/// position), so the inner cast's type name has no hover while the outer
/// one does (tsgo aa814927; the user decided to match tsgo here).
#[test]
fn a_nested_cast_s_type_name_has_no_hover() {
    let files = [("/nested.js", "/** @typedef {{ kind: string, body: Node[] }} Node */\n/** @typedef {{ parent: Scope | null }} Scope */\n/**\n * @param {{ statements: Node[], scope: Scope }} state state\n * @param {Node} previous previous\n * @returns {Node[]} nodes\n */\nfunction reset(state, previous) {\n\tstate.statements = /** @type {Node[]} */ (\n\t\t/** @type {Node[] | null} */ (null)\n\t);\n\tstate.scope = /** @type {Scope} */ (/** @type {Scope | null} */ (null));\n\tprevious.body = [\n\t\t.../** @type {Node[]} */ (previous.body),\n\t\t.../** @type {Node[]} */ (state.statements)\n\t];\n\treturn previous.body;\n}\nmodule.exports = reset;\n")];
    let javascript = || {
        options(|options| {
            options.allow_js = true;
            options.check_js = Some(true);
        })
    };
    assert!(hover(&files, javascript(), "/nested.js", (11, 51), None).is_none());
    let outer = hover(&files, javascript(), "/nested.js", (11, 27), None).expect("the outer cast");
    assert_eq!(outer.1, ((11, 26), (11, 31)));
    assert!(hover(&files, javascript(), "/nested.js", (9, 14), None).is_none());
}
