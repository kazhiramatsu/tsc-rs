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
    DocumentUri, HoverParams, MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings,
    MarkupKind, Position, PositionEncodingKind, ResolvedClientCapabilities, TextDocumentIdentifier,
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

impl tsc_ls::Host for Files {}

/// tsgo's hover at `(line, character)` of `file`: the markdown value, the
/// range and whether a higher verbosity level shows more.
fn hover(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    (line, character): (u32, u32),
    verbosity_level: Option<i32>,
) -> Option<(String, Range, bool)> {
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
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    let params = HoverParams {
        text_document: TextDocumentIdentifier {
            uri: DocumentUri(format!("file://{file}")),
        },
        position: Position { line, character },
        work_done_token: None,
        verbosity_level,
    };
    let hover = service.provide_hover(&params).expect("an answer")?;
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
