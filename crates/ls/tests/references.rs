//! Find all references as tsgo answers them, for the cases the fixes of the
//! references port reach. Each case is a fourslash test's input with
//! tsgo's answer, which the lsp suite's transcripts record
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
    DocumentUri, LocationsOrNull, Position, PositionEncodingKind, ReferenceContext,
    ReferenceParams, ResolvedClientCapabilities, TextDocumentIdentifier,
};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// A protocol range: (line, character) to (line, character).
type Range = ((u32, u32), (u32, u32));

/// A reference: its file and range.
type Reference = (&'static str, Range);

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

/// tsgo's references at `(line, character)` of `file`.
fn references(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    (line, character): (u32, u32),
) -> Vec<Reference> {
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
        .filter(|(name, _)| !name.ends_with("tsconfig.json"))
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
    let capabilities = ResolvedClientCapabilities::default();
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    let params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: DocumentUri(format!("file://{file}")),
        },
        position: Position { line, character },
        work_done_token: None,
        partial_result_token: None,
        context: ReferenceContext {
            include_declaration: true,
        },
    };
    let LocationsOrNull::Locations(locations) =
        service.provide_references(&params).expect("an answer")
    else {
        panic!("locations");
    };
    locations
        .iter()
        .map(|location| {
            let target = location
                .uri
                .as_str()
                .strip_prefix("file://")
                .expect("a file URI");
            let target = files
                .0
                .keys()
                .find(|name| name.as_str() == target)
                .map_or("<other>", |name| leak(name));
            (
                target,
                (
                    (location.range.start.line, location.range.start.character),
                    (location.range.end.line, location.range.end.character),
                ),
            )
        })
        .collect()
}

fn leak(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// findAllReferencesLinkTag1: a member's references include the names of
/// `{@link}` and `@see` tags: unqualified (the class's members), qualified
/// and `C#m` (tsgo parses both as qualified names).
#[test]
fn link_tag_names_are_references() {
    let text = "class C {\n    m() { }\n    n = 1\n    static s() { }\n    /**\n     * {@link m}\n     * @see {m}\n     * {@link C.m}\n     * @see {C.m}\n     * {@link C#m}\n     * @see {C#m}\n     * {@link C.prototype.m}\n     * @see {C.prototype.m}\n     */\n    p() { }\n    /**\n     * {@link n}\n     * @see {n}\n     * {@link C.n}\n     * @see {C.n}\n     * {@link C#n}\n     * @see {C#n}\n     * {@link C.prototype.n}\n     * @see {C.prototype.n}\n     */\n    q() { }\n    /**\n     * {@link s}\n     * @see {s}\n     * {@link C.s}\n     * @see {C.s}\n     */\n    r() { }\n}\n\ninterface I {\n    a()\n    b: 1\n    /**\n     * {@link a}\n     * @see {a}\n     * {@link I.a}\n     * @see {I.a}\n     * {@link I#a}\n     * @see {I#a}\n     */\n    c()\n    /**\n     * {@link b}\n     * @see {b}\n     * {@link I.b}\n     * @see {I.b}\n     */\n    d()\n}\n\nfunction nestor() {\n    /** {@link r2} */\n    function ref() { }\n    /** @see {r2} */\n    function d3() { }\n    function r2() { }\n}";
    assert_eq!(
        references(
            &[("/findAllReferencesLinkTag1.ts", text)],
            options(|_| {}),
            "/findAllReferencesLinkTag1.ts",
            (1, 5)
        ),
        vec![
            ("/findAllReferencesLinkTag1.ts", ((1, 4), (1, 5))),
            ("/findAllReferencesLinkTag1.ts", ((5, 14), (5, 15))),
            ("/findAllReferencesLinkTag1.ts", ((6, 13), (6, 14))),
            ("/findAllReferencesLinkTag1.ts", ((7, 16), (7, 17))),
            ("/findAllReferencesLinkTag1.ts", ((8, 15), (8, 16))),
            ("/findAllReferencesLinkTag1.ts", ((9, 16), (9, 17))),
            ("/findAllReferencesLinkTag1.ts", ((10, 15), (10, 16))),
            ("/findAllReferencesLinkTag1.ts", ((11, 26), (11, 27))),
            ("/findAllReferencesLinkTag1.ts", ((12, 25), (12, 26))),
        ]
    );
}

/// findAllReferencesJsDocTypeLiteral: the last name of a nested `@param`
/// is the reparsed property's name (tsgo `GetReparsedNodeForNode`).
#[test]
fn a_nested_param_name_is_its_property() {
    assert_eq!(
        references(
            &[("/foo.js", "/**\n * @param {object} o - very important!\n * @param {string} o.x - a thing, its ok\n * @param {number} o.y - another thing\n * @param {Object} o.nested - very nested\n * @param {boolean} o.nested.great - much greatness\n * @param {number} o.nested.times - twice? probably!??\n */\n function f(o) { return o.nested.great; }")],
            options(|options| options.allow_js = true),
            "/foo.js",
            (5, 29)
        ),
        vec![
            ("/foo.js", ((5, 29), (5, 34))),
            ("/foo.js", ((8, 33), (8, 38))),
        ]
    );
}

/// findAllReferencesJsOverloadedFunctionParameter: an `@overload` signature's\n/// parameters are the reparsed overload's, not the implementation's.
#[test]
fn overload_parameters_are_their_own() {
    assert_eq!(
        references(
            &[
                ("/foo.js", "/**\n * @overload\n * @param {number} x\n * @returns {number}\n *\n * @overload\n * @param {string} x\n * @returns {string} \n *\n * @param {unknown} x\n * @returns {unknown} \n */\nfunction foo(x) {\n  return x;\n}")
            ],
            options(|options| options.allow_js = true),
            "/foo.js",
            (12, 14)
        ),
        vec![
            ("/foo.js", ((9, 20), (9, 21))),
            ("/foo.js", ((12, 13), (12, 14))),
            ("/foo.js", ((13, 9), (13, 10))),
        ]
    );
}

/// findAllRefsJSDocNamespacedTypedef: a namespaced typedef's name and the\n/// qualified names that refer to it.
#[test]
fn a_namespaced_typedef_is_referenced_by_its_qualified_name() {
    assert_eq!(
        references(
            &[
                ("/index.js", "// Namespaced typedef\n/** @typedef {string} NS.T */\n\n// Namespaced typedef aliased to qualified namespaced typedef.\n/** @typedef {NS.T} NS.U */\n\n// Namespaced typedef aliased to implicitly-resolved typedef.\n/** @typedef {U} NS.V */\n")
            ],
            options(|options| options.allow_js = true),
            "/index.js",
            (1, 25)
        ),
        vec![
            ("/index.js", ((1, 25), (1, 26))),
            ("/index.js", ((4, 17), (4, 18))),
        ]
    );
}

/// findAllRefsJsDocImportTag2: an `@import` of a default export and its uses.
#[test]
fn a_jsdoc_import_imports_a_default_export() {
    assert_eq!(
        references(
            &[
                ("/component.js", "export default class Component {\n  constructor() {\n    this.id_ = Math.random();\n  }\n  id() {\n    return this.id_;\n  }\n}"),
                ("/spatial-navigation.js", "/** @import Component from './component.js' */\n\nexport class SpatialNavigation {\n  /**\n   * @param {Component} component\n   */\n  add(component) {}\n}"),
                ("/player.js", "import Component from './component.js';\n\n/**\n * @extends Component\n */\nexport class Player extends Component {}")
            ],
            options(|options| options.allow_js = true),
            "/player.js",
            (3, 21)
        ),
        vec![
            ("/player.js", ((0, 7), (0, 16))),
            ("/player.js", ((3, 12), (3, 21))),
            ("/player.js", ((5, 28), (5, 37))),
            ("/component.js", ((0, 21), (0, 30))),
            ("/spatial-navigation.js", ((0, 12), (0, 21))),
            ("/spatial-navigation.js", ((4, 13), (4, 22))),
        ]
    );
}

/// findAllRefsTypedef_importType: an import type's qualifier names a\n/// CommonJS module's typedef.
#[test]
fn an_import_type_refers_to_a_commonjs_typedef() {
    assert_eq!(
        references(
            &[
                (
                    "/a.js",
                    "module.exports = 0;\n/** @typedef {number} Foo */\nconst dummy = 0;"
                ),
                ("/b.js", "/** @type {import('./a').Foo} */\nconst x = 0;")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (1, 22)
        ),
        vec![("/a.js", ((1, 22), (1, 25))), ("/b.js", ((0, 25), (0, 28))),]
    );
}

/// findAllRefs_importType_js4.
#[test]
fn import_type_js4() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * @callback A\n * @param {unknown} response\n */\n\nmodule.exports = {};"),
                ("/b.js", "/** @typedef {import(\"./a\").A} A */")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (1, 13)
        ),
        vec![
            ("/a.js", ((1, 13), (1, 14))),
            ("/b.js", ((0, 28), (0, 29))),
        ]
    );
}

/// jsdocSatisfiesTagFindAllReferences: a `@satisfies` tag's type refers to a\n/// typedef.
#[test]
fn a_satisfies_tag_refers_to_a_typedef() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * @typedef {Object} T\n * @property {number} a\n */\n\n/** @satisfies {T} comment */\nconst foo = { a: 1 };")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (5, 16)
        ),
        vec![
            ("/a.js", ((1, 21), (1, 22))),
            ("/a.js", ((5, 16), (5, 17))),
        ]
    );
}

/// jsdocTypedefTagSemanticMeaning0.
#[test]
fn typedef_semantic_meaning_0_2() {
    assert_eq!(
        references(
            &[(
                "/a.js",
                "/** @typedef {number} T */\nconst T = 1;\n/** @type {T} */\nconst n = T;"
            )],
            options(|options| options.allow_js = true),
            "/a.js",
            (0, 22)
        ),
        vec![("/a.js", ((0, 22), (0, 23))), ("/a.js", ((2, 11), (2, 12))),]
    );
}

/// jsdocTypedefTagServices.
#[test]
fn typedef_tag_services_2() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * Doc comment\n * @typedef Product\n * @property {string} title\n */\n/**\n * @type {Product}\n */\nconst product = null;")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (6, 10)
        ),
        vec![
            ("/a.js", ((2, 12), (2, 19))),
            ("/a.js", ((6, 10), (6, 17))),
        ]
    );
}

/// patternAmbientModuleWithImportAttributesLanguageService: a pattern\n/// module's export is searched in its file, then in the importers'.
#[test]
fn a_pattern_module_member_is_searched_in_its_file_first() {
    assert_eq!(
        references(
            &[
                ("/tsconfig.json", "{ \"compilerOptions\": { \"module\": \"preserve\", \"moduleResolution\": \"bundler\" } }\n"),
                ("/types.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export interface CssPayload { kind: \"css\" }\n    export const shared: CssPayload;\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export interface TextPayload { kind: \"text\" }\n    export const shared: TextPayload;\n}\n"),
                ("/index.ts", "import * as css from \"./style.asset\" with { type: \"css\" };\nimport * as text from \"./copy.asset\" with { type: \"text\" };\ncss.shared;\ntext.shared;")
            ],
            options(|_| {}),
            "/index.ts",
            (2, 4)
        ),
        vec![
            ("/types.d.ts", ((2, 17), (2, 23))),
            ("/index.ts", ((2, 4), (2, 10))),
        ]
    );
}
