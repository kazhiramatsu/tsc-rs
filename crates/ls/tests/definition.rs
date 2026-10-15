//! Go to definition and type definition as tsgo answers them, for the cases
//! the fixes of the definition port reach and the conformance baselines do
//! not show: a decorator where none can be, a JSDoc `@import`, a
//! declaration map, and an object literal's property whose contextual type
//! comes from the literal itself. Each case is a fourslash test's input with
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
    DocumentUri, LocationOrLocationsOrDefinitionLinksOrNull, Position, PositionEncodingKind,
    ResolvedClientCapabilities,
};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// A protocol range: (line, character) to (line, character).
type Range = ((u32, u32), (u32, u32));

/// A definition link: the origin, the target's file, its range and its
/// selection range.
type Link = (Range, &'static str, Range, Range);

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

#[derive(Clone, Copy)]
enum Request {
    Definition,
    TypeDefinition,
}

/// tsgo's answer to `request` at `(line, character)` of `file`, in a
/// program of the files that are not source maps.
fn links(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    (line, character): (u32, u32),
    request: Request,
) -> Vec<Link> {
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
    capabilities.text_document.definition.link_support = true;
    capabilities.text_document.type_definition.link_support = true;
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    let uri = DocumentUri(format!("file://{file}"));
    let position = Position { line, character };
    let response = match request {
        Request::Definition => service.provide_definition(&uri, &position),
        Request::TypeDefinition => service.provide_type_definition(&uri, &position),
    }
    .expect("an answer");
    let LocationOrLocationsOrDefinitionLinksOrNull::DefinitionLinks(links) = response else {
        panic!("definition links: {response:?}");
    };
    let range = |range: &tsc_lsproto::Range| {
        (
            (range.start.line, range.start.character),
            (range.end.line, range.end.character),
        )
    };
    links
        .iter()
        .map(|link| {
            let target = link
                .target_uri
                .as_str()
                .strip_prefix("file://")
                .expect("a file URI");
            let target = files
                .0
                .keys()
                .find(|name| name.as_str() == target)
                .map_or("<other>", |name| leak(name));
            (
                range(link.origin_selection_range.as_ref().expect("an origin")),
                target,
                range(&link.target_range),
                range(&link.target_selection_range),
            )
        })
        .collect()
}

fn leak(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// goToDefinitionDecoratorNoCrashOnFunctionDeclaration1: a decorator of a
/// function declaration resolves as an error call (tsgo's
/// `CanHaveDecorators` guard), and its name goes to the decorator.
#[test]
fn a_decorator_where_none_can_be_goes_to_its_function() {
    let text = "function dec(target: any) { return target; }\n\n@dec\nfunction foo() {}";
    assert_eq!(
        links(
            &[("/a.ts", text)],
            options(|_| {}),
            "/a.ts",
            (2, 1),
            Request::Definition
        ),
        vec![(
            ((2, 1), (2, 4)),
            "/a.ts",
            ((0, 0), (0, 44)),
            ((0, 9), (0, 12))
        )]
    );
}

/// goToDefinitionJsDocImportTag1: an `@import` tag's module specifier is
/// the module (tsgo resolves it through the declaration it reparses the
/// tag into).
#[test]
fn a_jsdoc_import_specifier_goes_to_the_module() {
    let files = [
        ("/a.js", "/**\n * @import { A } from      \"./b\"\n */"),
        ("/b.ts", "export interface A { }"),
    ];
    let options = options(|options| {
        options.allow_js = true;
        options.check_js = Some(true);
    });
    assert_eq!(
        links(&files, options, "/a.js", (1, 31), Request::Definition),
        vec![(
            ((1, 27), (1, 32)),
            "/b.ts",
            ((0, 0), (0, 22)),
            ((0, 0), (0, 22))
        )]
    );
}

/// declarationMapGoToDefinition: a declaration file's location is its
/// source's, through the declaration map.
#[test]
fn a_declaration_goes_to_its_source_through_the_declaration_map() {
    let files = [
        (
            "/index.ts",
            "export class Foo {\n    member: string;\n    methodName(propName: SomeType): void {}\n    otherMethod() {\n        if (Math.random() > 0.5) {\n            return {x: 42};\n        }\n        return {y: \"yes\"};\n    }\n}\n\nexport interface SomeType {\n    member: number;\n}",
        ),
        (
            "/indexdef.d.ts",
            "export declare class Foo {\n    member: string;\n    methodName(propName: SomeType): void;\n    otherMethod(): {\n        x: number;\n        y?: undefined;\n    } | {\n        y: string;\n        x?: undefined;\n    };\n}\nexport interface SomeType {\n    member: number;\n}\n//# sourceMappingURL=indexdef.d.ts.map",
        ),
        (
            "/indexdef.d.ts.map",
            r#"{"version":3,"file":"indexdef.d.ts","sourceRoot":"","sources":["index.ts"],"names":[],"mappings":"AAAA;IACI,MAAM,EAAE,MAAM,CAAC;IACf,UAAU,CAAC,QAAQ,EAAE,QAAQ,GAAG,IAAI;IACpC,WAAW;;;;;;;CAMd;AAED,MAAM,WAAW,QAAQ;IACrB,MAAM,EAAE,MAAM,CAAC;CAClB"}"#,
        ),
        (
            "/mymodule.ts",
            "import * as mod from \"./indexdef\";\nconst instance = new mod.Foo();\ninstance.methodName({member: 12});",
        ),
    ];
    let options = options(|options| options.lib = Some(vec!["es5".into()]));
    assert_eq!(
        links(&files, options, "/mymodule.ts", (2, 9), Request::Definition),
        vec![(
            ((2, 9), (2, 19)),
            "/index.ts",
            ((2, 4), (3, 4)),
            ((2, 4), (2, 14))
        )]
    );
}

const OBJECT_LITERAL_PROPERTIES_2: &str = "type C = {\n  foo: string;\n  bar: number;\n};\n\ndeclare function fn<T extends C>(arg: T): T;\n\nfn({\n  foo: \"\",\n  bar: true,\n});\n\nconst result = fn({\n  foo: \"\",\n  bar: 1,\n});\n\n// this one shouldn't go to the constraint type\nresult.foo;";

/// goToDefinitionObjectLiteralProperties2: a property whose contextual
/// type is inferred from the literal itself goes to the constraint's (tsgo
/// `GetContextualType` with `IgnoreNodeInferences`); a property access of
/// the result goes to the literal's.
#[test]
fn an_inferred_literal_property_goes_to_the_constraint() {
    let file = [("/a.ts", OBJECT_LITERAL_PROPERTIES_2)];
    let at = |position| {
        links(
            &file,
            options(|_| {}),
            "/a.ts",
            position,
            Request::Definition,
        )
    };
    assert_eq!(
        at((8, 5)),
        vec![(
            ((8, 2), (8, 5)),
            "/a.ts",
            ((1, 2), (1, 14)),
            ((1, 2), (1, 5))
        )]
    );
    assert_eq!(
        at((9, 5)),
        vec![(
            ((9, 2), (9, 5)),
            "/a.ts",
            ((2, 2), (2, 14)),
            ((2, 2), (2, 5))
        )]
    );
    assert_eq!(
        at((18, 10)),
        vec![(
            ((18, 7), (18, 10)),
            "/a.ts",
            ((13, 2), (13, 9)),
            ((13, 2), (13, 5))
        )]
    );
}

/// goToDefinitionObjectLiteralProperties3: a union's members that the
/// literal's discriminants do not rule out each give their property.
#[test]
fn a_literal_property_goes_to_each_union_member() {
    let text = "type A = {\n  foo: unknown;\n};\n\ntype B = {\n  foo?: unknown;\n  bar: unknown;\n};\n\nfunction test1(arg: A | B) {}\n\ntest1({\n  foo: 1,\n});\n\nfunction test2<T extends A>(arg: T | B) {}\n\ntest2({\n  foo: 2,\n});";
    let file = [("/a.ts", text)];
    for line in [12, 18] {
        assert_eq!(
            links(
                &file,
                options(|_| {}),
                "/a.ts",
                (line, 5),
                Request::Definition
            ),
            vec![
                (
                    ((line, 2), (line, 5)),
                    "/a.ts",
                    ((1, 2), (1, 15)),
                    ((1, 2), (1, 5))
                ),
                (
                    ((line, 2), (line, 5)),
                    "/a.ts",
                    ((5, 2), (5, 16)),
                    ((5, 2), (5, 5))
                ),
            ]
        );
    }
}

/// goToTypeDefinition_returnType: a function's inferred type goes to its
/// return type (go to definition goes to the function), its union's
/// members each, and an overloaded function's type to its declarations.
#[test]
fn a_function_type_definition_goes_to_its_return_type() {
    let text = "interface I { x: number; }\ninterface J { y: number; }\n\nfunction f0(): I { return { x: 0 }; }\n\ntype T = (i: I) => I;\nconst f1: T = i => ({ x: i.x + 1 });\n\nconst f2 = (i: I): I => ({ x: i.x + 1 });\n\nconst f3 = (i: I) => ({ x: i.x + 1 });\n\nconst f4 = (i: I) => i;\n\nconst f5 = (i: I): I | J => ({ x: i.x + 1 });\n\nconst f6 = (i: I, j: J, b: boolean) => b ? i : j;\n\nconst f7 = (i: I) => {};\n\nfunction f8(i: I): I;\nfunction f8(j: J): J;\nfunction f8(ij: any): any { return ij; }\n\nf0();\nf1();\nf2();\nf3();\nf4();\nf5();\nf6();\nf7();\nf8();";
    let file = [("/a.ts", text)];
    let at = |line| {
        links(
            &file,
            options(|_| {}),
            "/a.ts",
            (line, 0),
            Request::TypeDefinition,
        )
        .into_iter()
        .map(|(origin, _, target, selection)| {
            assert_eq!(origin, ((line, 0), (line, 2)));
            (target, selection)
        })
        .collect::<Vec<_>>()
    };
    let i = (((0, 0), (0, 26)), ((0, 10), (0, 11)));
    let j = (((1, 0), (1, 26)), ((1, 10), (1, 11)));
    assert_eq!(at(24), vec![i]);
    assert_eq!(at(25), vec![(((5, 9), (5, 20)), ((5, 9), (5, 20)))]);
    assert_eq!(at(26), vec![i]);
    assert_eq!(at(27), vec![(((10, 22), (10, 36)), ((10, 22), (10, 36)))]);
    assert_eq!(at(28), vec![i]);
    assert_eq!(at(29), vec![i, j]);
    assert_eq!(at(30), vec![i, j]);
    assert_eq!(at(31), vec![]);
    assert_eq!(
        at(32),
        vec![
            (((20, 0), (20, 21)), ((20, 9), (20, 11))),
            (((21, 0), (21, 21)), ((21, 9), (21, 11))),
            (((22, 0), (22, 40)), ((22, 9), (22, 11))),
        ]
    );
}

/// tsgo `getTypeOfSymbolAtLocation` compares a variable's missing
/// initializer with its type's missing value declaration and finds them
/// equal (Go's nil == nil, as TypeScript's `undefined === undefined`): a
/// `declare var` typed by an interface of one call signature goes to that
/// signature's return type, as the libraries' `Function`, `Symbol` and
/// `String` do (found by the LSP test on hono). Pinned to tsgo's answers at
/// aa814927.
#[test]
fn a_declared_constructor_variable_goes_to_its_call_return_type() {
    let text = "interface Box { b: number; }\ninterface BoxConstructor { (n: number): Box; }\ndeclare var Box: BoxConstructor;\ninterface MakerConstructor { (n: number): string; }\ndeclare var Maker: MakerConstructor;\nlet a: Box;\nconst b = Box(1);\nconst c = Maker(1);\n";
    let file = [("/a.ts", text)];
    let at = |line, character| {
        links(
            &file,
            options(|_| {}),
            "/a.ts",
            (line, character),
            Request::TypeDefinition,
        )
    };
    let box_declarations = |origin: Range| {
        vec![
            (origin, "/a.ts", ((0, 0), (0, 28)), ((0, 10), (0, 13))),
            (origin, "/a.ts", ((2, 0), (2, 32)), ((2, 12), (2, 15))),
        ]
    };
    // `Box` as a type and as a callee: the return type `Box`, whose symbol
    // is the interface and the variable.
    assert_eq!(at(5, 7), box_declarations(((5, 7), (5, 10))));
    assert_eq!(at(6, 10), box_declarations(((6, 10), (6, 13))));
    // A primitive return type has no declarations.
    assert_eq!(at(7, 10), vec![]);
}
