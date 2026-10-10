//! The program's diagnostics and emit (P5-4d): each kind of diagnostics for
//! some files (in turn, a repeated file repeated) or for the whole program
//! (sorted), the program, config file parsing and global rows, with nested
//! entries located as tsgo's (a checker's at its diagnostic, the program's
//! without a location) and carrying their own related information; the
//! emit through the session's file system, to strings and of selected files
//! forced. Pinned to tsgo's responses at 19dadef8 for the same files (the
//! JSON texts).

use super::*;

const FILES: &[(&str, &str)] = &[
    (
        "/tsconfig.json",
        r#"{ "compilerOptions": { "strict": true, "declaration": true, "allowJs": true, "target": "es2020", "lib": ["es2020"], "outDir": "dist" }, "include": ["src"] }"#,
    ),
    ("/src/syntax.ts", "const s: = 1;\nlet t = ;\n"),
    ("/src/bind.ts", "let x = 1;\nlet x = 2;\n"),
    (
        "/src/semantic.ts",
        "interface Props { callback: (x: string) => void }\nexport const p: Props = { callback: (x: number) => {} };\n",
    ),
    (
        "/src/chain.ts",
        "declare let src: { x: {} };\nexport const a: { x: { y: number } } = src;\n",
    ),
    (
        "/src/suggest.ts",
        "/** @deprecated */ export function old() {}\nconst _unused = 1;\nold();\n",
    ),
    (
        "/src/unicode.ts",
        "const \u{65e5} = \"\u{1F600}\"; export const z: number = \"x\";\r\nexport const w: string = 1;\n",
    ),
    (
        "/src/decl.ts",
        "export const Z = class { private secret = 1; };\n",
    ),
    ("/src/check.js", "// @ts-check\nlet a = 1;\na = \"x\";\n"),
    ("/src/ignore.ts", "// @ts-expect-error\nexport const ok = 1;\n"),
    ("/nolib/tsconfig.json", r#"{ "compilerOptions": { "noLib": true } }"#),
    ("/nolib/index.ts", "export const x = [1];\n"),
    (
        "/inc/tsconfig.json",
        r#"{ "compilerOptions": { "rootDir": "src", "outDir": "dist", "importsNotUsedAsValues": "remove" }, "files": ["src/a.ts", "other/b.ts"] }"#,
    ),
    ("/inc/src/a.ts", "export const a = 1;\n"),
    ("/inc/other/b.ts", "export const b = 2;\n"),
    (
        "/emit/tsconfig.json",
        r#"{ "compilerOptions": { "outDir": "dist", "rootDir": "src", "declaration": true, "sourceMap": true, "declarationMap": true }, "files": ["src/z.ts", "src/a.ts"] }"#,
    ),
    (
        "/emit/src/z.ts",
        "export const Z = class { private secret = 1; };\n",
    ),
    (
        "/emit/src/a.ts",
        "import { Z } from \"./z\";\nexport const A = class { protected hidden = 2; };\nexport const b = Z;\n",
    ),
    (
        "/gate/tsconfig.json",
        r#"{ "compilerOptions": { "noEmitOnError": true, "declaration": true, "outDir": "dist", "rootDir": "src" } }"#,
    ),
    (
        "/gate/src/z.ts",
        "export const Z = class { private secret = 1; };\n",
    ),
    ("/gate/src/a.ts", "export const a = 1;\n"),
    (
        "/forced/tsconfig.json",
        r#"{ "compilerOptions": { "declarationMap": true, "emitDeclarationOnly": true, "noEmit": true, "noEmitOnError": true, "sourceMap": true, "resolveJsonModule": true, "emitBOM": true, "newLine": "crlf" } }"#,
    ),
    (
        "/forced/src/a.ts",
        "import data from \"./data.json\";\nexport const a: string = 1;\nexport const d = data;\n",
    ),
    ("/forced/src/b.d.ts", "export declare const b: number;\n"),
    ("/forced/src/data.json", "{ \"n\": 1 }\n"),
];

/// A session with every project of [`FILES`] open: the session, its file
/// system and the request base of each project by config file name.
fn projects() -> (Session, Arc<MemFs>, impl Fn(&str) -> Value) {
    let (session, fs) = session(FILES);
    let snapshot = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": [
            "/tsconfig.json",
            "/nolib/tsconfig.json",
            "/inc/tsconfig.json",
            "/emit/tsconfig.json",
            "/gate/tsconfig.json",
            "/forced/tsconfig.json",
        ] }),
    )
    .unwrap()["snapshot"]
        .clone();
    let base = move |project: &str| json!({ "snapshot": snapshot, "project": project });
    (session, fs, base)
}

/// The JSON text of a request's response.
fn text(session: &Session, method: &str, base: &Value, fields: Value) -> String {
    let mut params = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        params[key] = value.clone();
    }
    call_text(session, method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))
}

/// A request's error.
fn error(session: &Session, method: &str, base: &Value, fields: Value) -> String {
    let mut params = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        params[key] = value.clone();
    }
    call_text(session, method, &params).unwrap_err()
}

#[test]
fn diagnostics_of_each_kind_for_a_file() {
    let (session, _, base) = projects();
    let base = base("/tsconfig.json");
    let of = |method: &str, file: &str| text(&session, method, &base, json!({ "files": [file] }));
    assert_eq!(
        of("getSyntacticDiagnostics", "/src/syntax.ts"),
        r##"[{"fileName":"/src/syntax.ts","pos":9,"end":10,"startPosition":{"line":0,"character":9},"endPosition":{"line":0,"character":10},"sourceLines":[{"line":0,"text":"const s: = 1;\n"}],"code":1110,"category":1,"text":"Type expected."},{"fileName":"/src/syntax.ts","pos":22,"end":23,"startPosition":{"line":1,"character":8},"endPosition":{"line":1,"character":9},"sourceLines":[{"line":1,"text":"let t = ;\n"}],"code":1109,"category":1,"text":"Expression expected."}]"##
    );
    assert_eq!(
        of("getBindDiagnostics", "/src/bind.ts"),
        r##"[{"fileName":"/src/bind.ts","pos":4,"end":5,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":5},"sourceLines":[{"line":0,"text":"let x = 1;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/bind.ts","pos":15,"end":16,"startPosition":{"line":1,"character":4},"endPosition":{"line":1,"character":5},"sourceLines":[{"line":1,"text":"let x = 2;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."}]"##
    );
    // A checker's chain: each nested entry at its diagnostic's location.
    assert_eq!(
        of("getSemanticDiagnostics", "/src/semantic.ts"),
        r##"[{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2322,"category":1,"text":"Type '(x: number) => void' is not assignable to type '(x: string) => void'.","messageChain":[{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2328,"category":1,"text":"Types of parameters 'x' and 'x' are incompatible.","messageChain":[{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."}]}],"relatedInformation":[{"fileName":"/src/semantic.ts","pos":18,"end":26,"startPosition":{"line":0,"character":18},"endPosition":{"line":0,"character":26},"sourceLines":[{"line":0,"text":"interface Props { callback: (x: string) => void }\n"}],"code":6500,"category":3,"text":"The expected type comes from property 'callback' which is declared here on type 'Props'"}]}]"##
    );
    // The relater's related information on every level of its chain.
    assert_eq!(
        of("getSemanticDiagnostics", "/src/chain.ts"),
        r##"[{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2322,"category":1,"text":"Type '{ x: {}; }' is not assignable to type '{ x: { y: number; }; }'.","messageChain":[{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2326,"category":1,"text":"Types of property 'x' are incompatible.","messageChain":[{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2741,"category":1,"text":"Property 'y' is missing in type '{}' but required in type '{ y: number; }'.","relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]}],"relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]}],"relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]}]"##
    );
    assert_eq!(
        of("getSuggestionDiagnostics", "/src/suggest.ts"),
        r##"[{"fileName":"/src/suggest.ts","pos":50,"end":57,"startPosition":{"line":1,"character":6},"endPosition":{"line":1,"character":13},"sourceLines":[{"line":1,"text":"const _unused = 1;\n"}],"code":6133,"category":2,"text":"'_unused' is declared but its value is never read.","reportsUnnecessary":true},{"fileName":"/src/suggest.ts","pos":63,"end":66,"startPosition":{"line":2,"character":0},"endPosition":{"line":2,"character":3},"sourceLines":[{"line":2,"text":"old();\n"}],"code":6387,"category":2,"text":"The signature '(): void' of 'old' is deprecated.","reportsDeprecated":true,"relatedInformation":[{"fileName":"/src/suggest.ts","pos":4,"end":16,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":16},"sourceLines":[{"line":0,"text":"/** @deprecated */ export function old() {}\n"}],"code":2798,"category":1,"text":"The declaration was marked as deprecated here."}]}]"##
    );
    // UTF-16 positions, a CRLF line.
    assert_eq!(
        of("getSemanticDiagnostics", "/src/unicode.ts"),
        r##"[{"fileName":"/src/unicode.ts","pos":29,"end":30,"startPosition":{"line":0,"character":29},"endPosition":{"line":0,"character":30},"sourceLines":[{"line":0,"text":"const 日 = \"😀\"; export const z: number = \"x\";\r\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."},{"fileName":"/src/unicode.ts","pos":60,"end":61,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const w: string = 1;\n"}],"code":2322,"category":1,"text":"Type 'number' is not assignable to type 'string'."}]"##
    );
    assert_eq!(
        of("getSuggestionDiagnostics", "/src/unicode.ts"),
        r##"[{"fileName":"/src/unicode.ts","pos":6,"end":7,"startPosition":{"line":0,"character":6},"endPosition":{"line":0,"character":7},"sourceLines":[{"line":0,"text":"const 日 = \"😀\"; export const z: number = \"x\";\r\n"}],"code":6133,"category":2,"text":"'日' is declared but its value is never read.","reportsUnnecessary":true}]"##
    );
    assert_eq!(
        of("getDeclarationDiagnostics", "/src/decl.ts"),
        r##"[{"fileName":"/src/decl.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/src/decl.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}]"##
    );
    assert_eq!(
        of("getSemanticDiagnostics", "/src/check.js"),
        r##"[{"fileName":"/src/check.js","pos":24,"end":25,"startPosition":{"line":2,"character":0},"endPosition":{"line":2,"character":1},"sourceLines":[{"line":2,"text":"a = \"x\";\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."}]"##
    );
    for method in [
        "getSyntacticDiagnostics",
        "getBindDiagnostics",
        "getSuggestionDiagnostics",
        "getDeclarationDiagnostics",
    ] {
        assert_eq!(of(method, "/src/check.js"), "[]", "{method}");
    }
}

#[test]
fn diagnostics_of_several_files_or_of_the_program() {
    let (session, _, base) = projects();
    let base = base("/tsconfig.json");
    // Each file's in turn, a repeated file repeated.
    assert_eq!(
        text(
            &session,
            "getSemanticDiagnostics",
            &base,
            json!({ "files": ["/src/bind.ts", "/src/check.js", "/src/bind.ts"] }),
        ),
        r##"[{"fileName":"/src/bind.ts","pos":4,"end":5,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":5},"sourceLines":[{"line":0,"text":"let x = 1;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/bind.ts","pos":15,"end":16,"startPosition":{"line":1,"character":4},"endPosition":{"line":1,"character":5},"sourceLines":[{"line":1,"text":"let x = 2;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/check.js","pos":24,"end":25,"startPosition":{"line":2,"character":0},"endPosition":{"line":2,"character":1},"sourceLines":[{"line":2,"text":"a = \"x\";\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."},{"fileName":"/src/bind.ts","pos":4,"end":5,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":5},"sourceLines":[{"line":0,"text":"let x = 1;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/bind.ts","pos":15,"end":16,"startPosition":{"line":1,"character":4},"endPosition":{"line":1,"character":5},"sourceLines":[{"line":1,"text":"let x = 2;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."}]"##
    );
    assert_eq!(
        text(
            &session,
            "getSemanticDiagnostics",
            &base,
            json!({ "files": [] })
        ),
        "[]"
    );
    assert_eq!(
        error(
            &session,
            "getSemanticDiagnostics",
            &base,
            json!({ "files": ["/src/missing.ts"] }),
        ),
        "api: client error: source file not found: /src/missing.ts"
    );
    // Without files (or with `null`), the whole program's, sorted.
    assert_eq!(
        text(&session, "getSyntacticDiagnostics", &base, json!({})),
        r##"[{"fileName":"/src/syntax.ts","pos":9,"end":10,"startPosition":{"line":0,"character":9},"endPosition":{"line":0,"character":10},"sourceLines":[{"line":0,"text":"const s: = 1;\n"}],"code":1110,"category":1,"text":"Type expected."},{"fileName":"/src/syntax.ts","pos":22,"end":23,"startPosition":{"line":1,"character":8},"endPosition":{"line":1,"character":9},"sourceLines":[{"line":1,"text":"let t = ;\n"}],"code":1109,"category":1,"text":"Expression expected."}]"##
    );
    assert_eq!(
        text(
            &session,
            "getBindDiagnostics",
            &base,
            json!({ "files": null })
        ),
        r##"[{"fileName":"/src/bind.ts","pos":4,"end":5,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":5},"sourceLines":[{"line":0,"text":"let x = 1;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/bind.ts","pos":15,"end":16,"startPosition":{"line":1,"character":4},"endPosition":{"line":1,"character":5},"sourceLines":[{"line":1,"text":"let x = 2;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."}]"##
    );
    assert_eq!(
        text(&session, "getSemanticDiagnostics", &base, json!({})),
        r##"[{"fileName":"/src/bind.ts","pos":4,"end":5,"startPosition":{"line":0,"character":4},"endPosition":{"line":0,"character":5},"sourceLines":[{"line":0,"text":"let x = 1;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/bind.ts","pos":15,"end":16,"startPosition":{"line":1,"character":4},"endPosition":{"line":1,"character":5},"sourceLines":[{"line":1,"text":"let x = 2;\n"}],"code":2451,"category":1,"text":"Cannot redeclare block-scoped variable 'x'."},{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2322,"category":1,"text":"Type '{ x: {}; }' is not assignable to type '{ x: { y: number; }; }'.","messageChain":[{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2326,"category":1,"text":"Types of property 'x' are incompatible.","messageChain":[{"fileName":"/src/chain.ts","pos":41,"end":42,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2741,"category":1,"text":"Property 'y' is missing in type '{}' but required in type '{ y: number; }'.","relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]}],"relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]}],"relatedInformation":[{"fileName":"/src/chain.ts","pos":51,"end":52,"startPosition":{"line":1,"character":23},"endPosition":{"line":1,"character":24},"sourceLines":[{"line":1,"text":"export const a: { x: { y: number } } = src;\n"}],"code":2728,"category":3,"text":"'y' is declared here."}]},{"fileName":"/src/check.js","pos":24,"end":25,"startPosition":{"line":2,"character":0},"endPosition":{"line":2,"character":1},"sourceLines":[{"line":2,"text":"a = \"x\";\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."},{"fileName":"/src/ignore.ts","pos":0,"end":19,"startPosition":{"line":0,"character":0},"endPosition":{"line":0,"character":19},"sourceLines":[{"line":0,"text":"// @ts-expect-error\n"}],"code":2578,"category":1,"text":"Unused '@ts-expect-error' directive."},{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2322,"category":1,"text":"Type '(x: number) => void' is not assignable to type '(x: string) => void'.","messageChain":[{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2328,"category":1,"text":"Types of parameters 'x' and 'x' are incompatible.","messageChain":[{"fileName":"/src/semantic.ts","pos":76,"end":84,"startPosition":{"line":1,"character":26},"endPosition":{"line":1,"character":34},"sourceLines":[{"line":1,"text":"export const p: Props = { callback: (x: number) => {} };\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."}]}],"relatedInformation":[{"fileName":"/src/semantic.ts","pos":18,"end":26,"startPosition":{"line":0,"character":18},"endPosition":{"line":0,"character":26},"sourceLines":[{"line":0,"text":"interface Props { callback: (x: string) => void }\n"}],"code":6500,"category":3,"text":"The expected type comes from property 'callback' which is declared here on type 'Props'"}]},{"fileName":"/src/unicode.ts","pos":29,"end":30,"startPosition":{"line":0,"character":29},"endPosition":{"line":0,"character":30},"sourceLines":[{"line":0,"text":"const 日 = \"😀\"; export const z: number = \"x\";\r\n"}],"code":2322,"category":1,"text":"Type 'string' is not assignable to type 'number'."},{"fileName":"/src/unicode.ts","pos":60,"end":61,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const w: string = 1;\n"}],"code":2322,"category":1,"text":"Type 'number' is not assignable to type 'string'."}]"##
    );
    assert_eq!(
        text(&session, "getDeclarationDiagnostics", &base, json!({})),
        r##"[{"fileName":"/src/decl.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/src/decl.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}]"##
    );
}

#[test]
fn program_config_and_global_diagnostics() {
    let (session, _, base) = projects();
    let root = base("/tsconfig.json");
    // The program's chain entry has no location (tsgo NewCompilerDiagnostic).
    assert_eq!(
        text(&session, "getProgramDiagnostics", &root, json!({})),
        r##"[{"fileName":"/tsconfig.json","pos":116,"end":124,"startPosition":{"line":0,"character":116},"endPosition":{"line":0,"character":124},"sourceLines":[{"line":0,"text":"{ \"compilerOptions\": { \"strict\": true, \"declaration\": true, \"allowJs\": true, \"target\": \"es2020\", \"lib\": [\"es2020\"], \"outDir\": \"dist\" }, \"include\": [\"src\"] }"}],"code":5011,"category":1,"text":"The common source directory of 'tsconfig.json' is './src'. The 'rootDir' setting must be explicitly set to this or another path to adjust your output's file layout.","messageChain":[{"pos":-1,"end":-1,"code":5111,"category":3,"text":"Visit https://aka.ms/ts6 for migration information."}]}]"##
    );
    assert_eq!(
        text(
            &session,
            "getConfigFileParsingDiagnostics",
            &root,
            json!({})
        ),
        "[]"
    );
    assert_eq!(
        text(&session, "getGlobalDiagnostics", &root, json!({})),
        "[]"
    );
    // The checker's file-less rows are at 0.
    assert_eq!(
        text(
            &session,
            "getGlobalDiagnostics",
            &base("/nolib/tsconfig.json"),
            json!({})
        ),
        r##"[{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'Array'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'Boolean'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'CallableFunction'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'Function'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'IArguments'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'NewableFunction'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'Number'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'Object'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'RegExp'."},{"pos":0,"end":0,"code":2318,"category":1,"text":"Cannot find global type 'String'."}]"##
    );
    // An include reason's chain, the project's file-less rows.
    let inc = base("/inc/tsconfig.json");
    assert_eq!(
        text(&session, "getConfigFileParsingDiagnostics", &inc, json!({})),
        r##"[{"fileName":"/inc/tsconfig.json","pos":59,"end":83,"startPosition":{"line":0,"character":59},"endPosition":{"line":0,"character":83},"sourceLines":[{"line":0,"text":"{ \"compilerOptions\": { \"rootDir\": \"src\", \"outDir\": \"dist\", \"importsNotUsedAsValues\": \"remove\" }, \"files\": [\"src/a.ts\", \"other/b.ts\"] }"}],"code":5023,"category":1,"text":"Unknown compiler option 'importsNotUsedAsValues'."}]"##
    );
    assert_eq!(
        text(&session, "getProgramDiagnostics", &inc, json!({})),
        r##"[{"pos":-1,"end":-1,"code":6059,"category":1,"text":"File '/inc/other/b.ts' is not under 'rootDir' '/inc/src'. 'rootDir' is expected to contain all source files.","messageChain":[{"pos":-1,"end":-1,"code":1430,"category":3,"text":"The file is in the program because:","messageChain":[{"pos":-1,"end":-1,"code":1409,"category":3,"text":"Part of 'files' list in tsconfig.json"}]}],"relatedInformation":[{"fileName":"/inc/tsconfig.json","pos":119,"end":131,"startPosition":{"line":0,"character":119},"endPosition":{"line":0,"character":131},"sourceLines":[{"line":0,"text":"{ \"compilerOptions\": { \"rootDir\": \"src\", \"outDir\": \"dist\", \"importsNotUsedAsValues\": \"remove\" }, \"files\": [\"src/a.ts\", \"other/b.ts\"] }"}],"code":1410,"category":3,"text":"File is matched by 'files' list specified here."}]}]"##
    );
    assert_eq!(
        text(&session, "getGlobalDiagnostics", &inc, json!({})),
        r##"[{"pos":-1,"end":-1,"code":6059,"category":1,"text":"File '/inc/other/b.ts' is not under 'rootDir' '/inc/src'. 'rootDir' is expected to contain all source files.","messageChain":[{"pos":-1,"end":-1,"code":1430,"category":3,"text":"The file is in the program because:","messageChain":[{"pos":-1,"end":-1,"code":1409,"category":3,"text":"Part of 'files' list in tsconfig.json"}]}],"relatedInformation":[{"fileName":"/inc/tsconfig.json","pos":119,"end":131,"startPosition":{"line":0,"character":119},"endPosition":{"line":0,"character":131},"sourceLines":[{"line":0,"text":"{ \"compilerOptions\": { \"rootDir\": \"src\", \"outDir\": \"dist\", \"importsNotUsedAsValues\": \"remove\" }, \"files\": [\"src/a.ts\", \"other/b.ts\"] }"}],"code":1410,"category":3,"text":"File is matched by 'files' list specified here."}]}]"##
    );
}

#[test]
fn a_created_program_reports_its_config_file_parsing_diagnostics() {
    // createProgram includes config file parsing diagnostics.
    let (session, _) = session(&[("/src/index.ts", "export const value = 1;")]);
    let diagnostic = json!({ "pos": 0, "end": 0, "code": 9001, "category": 1, "text": "Synthetic config parsing error." });
    let response = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{
            "rootFiles": ["/src/index.ts"],
            "compilerOptions": { "noLib": true },
            "options": { "configFileParsingDiagnostics": [diagnostic] },
        }] }),
    )
    .unwrap();
    let base =
        json!({ "snapshot": response["snapshot"], "project": response["projects"][0]["id"] });
    let expected =
        r#"[{"pos":0,"end":0,"code":9001,"category":1,"text":"Synthetic config parsing error."}]"#;
    assert_eq!(
        text(
            &session,
            "getConfigFileParsingDiagnostics",
            &base,
            json!({})
        ),
        expected
    );
    // A file-less row of the project, after the checker's (code 2318).
    let global = text(&session, "getGlobalDiagnostics", &base, json!({}));
    assert!(
        global.starts_with(r#"[{"pos":0,"end":0,"code":2318,"#),
        "{global}"
    );
    assert!(global.ends_with(&expected[1..]), "{global}");
}

#[test]
fn the_emit_writes_through_each_file_in_turn() {
    let (session, fs, base) = projects();
    let base = base("/emit/tsconfig.json");
    // Each file's diagnostics in program order (z.ts, then a.ts).
    assert_eq!(
        text(&session, "emit", &base, json!({})),
        r##"{"emitSkipped":true,"diagnostics":[{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]},{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":4094,"category":1,"text":"Property 'hidden' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable A."}]},{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable b."}]}],"emittedFiles":["/emit/dist/z.js.map","/emit/dist/z.js","/emit/dist/a.js.map","/emit/dist/a.js"],"emittedFilesContents":[]}"##
    );
    let file = |name: &str| String::from_utf8(fs.read(name).unwrap()).unwrap();
    assert_eq!(
        file("/emit/dist/a.js"),
        "import { Z } from \"./z\";\nexport const A = class {\n    hidden = 2;\n};\nexport const b = Z;\n//# sourceMappingURL=a.js.map"
    );
    assert_eq!(
        file("/emit/dist/z.js.map"),
        r#"{"version":3,"file":"z.js","sourceRoot":"","sources":["../src/z.ts"],"names":[],"mappings":"AAAA,MAAM,CAAC,MAAM,CAAC,GAAG;IAAgB,MAAM,GAAG,CAAC,CAAC;CAAE,CAAC"}"#
    );
    // The declarations were blocked by their diagnostics.
    assert!(fs.read("/emit/dist/z.d.ts").is_err());
    assert_eq!(
        text(&session, "emitToString", &base, json!({})),
        r##"{"emitSkipped":true,"diagnostics":[{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]},{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":4094,"category":1,"text":"Property 'hidden' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable A."}]},{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable b."}]}],"outputFiles":[{"fileName":"/emit/dist/a.js","text":"import { Z } from \"./z\";\nexport const A = class {\n    hidden = 2;\n};\nexport const b = Z;\n//# sourceMappingURL=a.js.map","sourceFileName":"/emit/src/a.ts"},{"fileName":"/emit/dist/a.js.map","text":"{\"version\":3,\"file\":\"a.js\",\"sourceRoot\":\"\",\"sources\":[\"../src/a.ts\"],\"names\":[],\"mappings\":\"AAAA,OAAO,EAAE,CAAC,EAAE,MAAM,KAAK,CAAC;AACxB,MAAM,CAAC,MAAM,CAAC,GAAG;IAAkB,MAAM,GAAG,CAAC,CAAC;CAAE,CAAC;AACjD,MAAM,CAAC,MAAM,CAAC,GAAG,CAAC,CAAC\"}","sourceFileName":"/emit/src/a.ts"},{"fileName":"/emit/dist/z.js","text":"export const Z = class {\n    secret = 1;\n};\n//# sourceMappingURL=z.js.map","sourceFileName":"/emit/src/z.ts"},{"fileName":"/emit/dist/z.js.map","text":"{\"version\":3,\"file\":\"z.js\",\"sourceRoot\":\"\",\"sources\":[\"../src/z.ts\"],\"names\":[],\"mappings\":\"AAAA,MAAM,CAAC,MAAM,CAAC,GAAG;IAAgB,MAAM,GAAG,CAAC,CAAC;CAAE,CAAC\"}","sourceFileName":"/emit/src/z.ts"}]}"##
    );
    assert_eq!(
        text(&session, "emitToString", &base, json!({ "emitOnly": 1 })),
        r##"{"emitSkipped":false,"diagnostics":[],"outputFiles":[{"fileName":"/emit/dist/a.js","text":"import { Z } from \"./z\";\nexport const A = class {\n    hidden = 2;\n};\nexport const b = Z;\n//# sourceMappingURL=a.js.map","sourceFileName":"/emit/src/a.ts"},{"fileName":"/emit/dist/a.js.map","text":"{\"version\":3,\"file\":\"a.js\",\"sourceRoot\":\"\",\"sources\":[\"../src/a.ts\"],\"names\":[],\"mappings\":\"AAAA,OAAO,EAAE,CAAC,EAAE,MAAM,KAAK,CAAC;AACxB,MAAM,CAAC,MAAM,CAAC,GAAG;IAAkB,MAAM,GAAG,CAAC,CAAC;CAAE,CAAC;AACjD,MAAM,CAAC,MAAM,CAAC,GAAG,CAAC,CAAC\"}","sourceFileName":"/emit/src/a.ts"},{"fileName":"/emit/dist/z.js","text":"export const Z = class {\n    secret = 1;\n};\n//# sourceMappingURL=z.js.map","sourceFileName":"/emit/src/z.ts"},{"fileName":"/emit/dist/z.js.map","text":"{\"version\":3,\"file\":\"z.js\",\"sourceRoot\":\"\",\"sources\":[\"../src/z.ts\"],\"names\":[],\"mappings\":\"AAAA,MAAM,CAAC,MAAM,CAAC,GAAG;IAAgB,MAAM,GAAG,CAAC,CAAC;CAAE,CAAC\"}","sourceFileName":"/emit/src/z.ts"}]}"##
    );
    assert_eq!(
        text(
            &session,
            "getDeclarationDiagnostics",
            &base,
            json!({ "files": ["/emit/src/a.ts", "/emit/src/z.ts"] }),
        ),
        r##"[{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":4094,"category":1,"text":"Property 'hidden' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":38,"end":39,"startPosition":{"line":1,"character":13},"endPosition":{"line":1,"character":14},"sourceLines":[{"line":1,"text":"export const A = class { protected hidden = 2; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable A."}]},{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/a.ts","pos":88,"end":89,"startPosition":{"line":2,"character":13},"endPosition":{"line":2,"character":14},"sourceLines":[{"line":2,"text":"export const b = Z;\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable b."}]},{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/emit/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}]"##
    );
}

#[test]
fn no_emit_on_error_stops_the_emit_but_not_a_forced_one() {
    let (session, fs, base) = projects();
    let base = base("/gate/tsconfig.json");
    assert_eq!(
        text(&session, "emit", &base, json!({})),
        r##"{"emitSkipped":true,"diagnostics":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}],"emittedFiles":[],"emittedFilesContents":[]}"##
    );
    assert!(fs.read("/gate/dist/a.js").is_err());
    assert_eq!(
        text(&session, "emitToString", &base, json!({ "emitOnly": 1 })),
        r##"{"emitSkipped":true,"diagnostics":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}],"outputFiles":[]}"##
    );
    assert_eq!(
        text(
            &session,
            "getDeclarationEmit",
            &base,
            json!({ "files": ["/gate/src/z.ts"] }),
        ),
        r##"{"emitSkipped":false,"diagnostics":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":4094,"category":1,"text":"Property 'secret' of exported anonymous class type may not be private or protected.","relatedInformation":[{"fileName":"/gate/src/z.ts","pos":13,"end":14,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":14},"sourceLines":[{"line":0,"text":"export const Z = class { private secret = 1; };\n"}],"code":9027,"category":1,"text":"Add a type annotation to the variable Z."}]}],"outputFiles":[{"fileName":"/gate/dist/z.d.ts","text":"export declare const Z: {\n    new (): {\n        secret: number;\n    };\n};\n","sourceFileName":"/gate/src/z.ts"}]}"##
    );
}

#[test]
fn a_forced_emit_of_selected_files() {
    let (session, fs, base) = projects();
    let base = base("/forced/tsconfig.json");
    // noEmit: nothing, and not skipped.
    assert_eq!(
        text(&session, "emit", &base, json!({})),
        r##"{"emitSkipped":false,"diagnostics":[],"emittedFiles":[],"emittedFilesContents":[]}"##
    );
    // JavaScript and its map whatever emitDeclarationOnly and noEmit say,
    // with the BOM and CRLF; no output for a declaration file or JSON.
    assert_eq!(
        text(
            &session,
            "getJavaScriptEmit",
            &base,
            json!({ "files": ["/forced/src/a.ts", "/forced/src/b.d.ts", "/forced/src/data.json"] }),
        ),
        r##"{"emitSkipped":false,"diagnostics":[],"outputFiles":[{"fileName":"/forced/src/a.js","text":"﻿import data from \"./data.json\";\r\nexport const a = 1;\r\nexport const d = data;\r\n//# sourceMappingURL=a.js.map","sourceFileName":"/forced/src/a.ts"},{"fileName":"/forced/src/a.js.map","text":"{\"version\":3,\"file\":\"a.js\",\"sourceRoot\":\"\",\"sources\":[\"a.ts\"],\"names\":[],\"mappings\":\"AAAA,OAAO,IAAI,MAAM,aAAa,CAAC;AAC/B,MAAM,CAAC,MAAM,CAAC,GAAW,CAAC,CAAC;AAC3B,MAAM,CAAC,MAAM,CAAC,GAAG,IAAI,CAAC\"}","sourceFileName":"/forced/src/a.ts"}]}"##
    );
    // Declarations with their maps whatever `declaration` says, a JSON
    // file's empty one; sorted by name.
    assert_eq!(
        text(
            &session,
            "getDeclarationEmit",
            &base,
            json!({ "files": ["/forced/src/data.json", "/forced/src/a.ts"] }),
        ),
        r##"{"emitSkipped":false,"diagnostics":[],"outputFiles":[{"fileName":"/forced/src/a.d.ts","text":"﻿export declare const a: string;\r\nexport declare const d: {\r\n    n: number;\r\n};\r\n//# sourceMappingURL=a.d.ts.map","sourceFileName":"/forced/src/a.ts"},{"fileName":"/forced/src/a.d.ts.map","text":"{\"version\":3,\"file\":\"a.d.ts\",\"sourceRoot\":\"\",\"sources\":[\"a.ts\"],\"names\":[],\"mappings\":\"AACA,eAAO,MAAM,CAAC,EAAE,MAAU,CAAC;AAC3B,eAAO,MAAM,CAAC;;CAAO,CAAC\"}","sourceFileName":"/forced/src/a.ts"},{"fileName":"/forced/src/data.d.json.ts","text":"﻿","sourceFileName":"/forced/src/data.json"}]}"##
    );
    assert!(fs.read("/forced/src/a.js").is_err());
    assert_eq!(
        error(&session, "getDeclarationEmit", &base, json!({})),
        "api: client error: files is required"
    );
    assert_eq!(
        error(
            &session,
            "getJavaScriptEmit",
            &base,
            json!({ "files": ["/forced/src/missing.ts"] }),
        ),
        "api: client error: source file not found: /forced/src/missing.ts"
    );
    assert_eq!(
        error(&session, "emitToString", &base, json!({ "emitOnly": 3 })),
        "api: client error: invalid emitOnly value: 3"
    );
}

#[test]
fn a_full_file_system_emit_returns_the_outputs() {
    // full filesystem emit returns outputs without mutating the host.
    let (session, fs) = session(&[]);
    let response = call(
        &session,
        "createSnapshot",
        json!({
            "openProjects": ["/tsconfig.json"],
            "fileSystem": {
                "kind": "full",
                "files": {
                    "/tsconfig.json": r#"{ "compilerOptions": { "noLib": true, "outDir": "/out", "rootDir": "/src" }, "files": ["src/main.ts"] }"#,
                    "/src/main.ts": "export const value: number = 1;",
                },
            },
        }),
    )
    .unwrap();
    let base = json!({ "snapshot": response["snapshot"], "project": "/tsconfig.json" });
    assert_eq!(
        text(&session, "emit", &base, json!({})),
        r#"{"emitSkipped":false,"diagnostics":[],"emittedFiles":["/out/main.js"],"emittedFilesContents":["export const value = 1;\n"]}"#
    );
    assert!(fs.read("/out/main.js").is_err());
}

#[test]
fn an_emit_from_a_layer_over_a_full_file_system_returns_the_outputs() {
    // TestEmitFromLayerOverFullFileSystemReturnsFileContents.
    let (session, _) = session(&[]);
    let base = call(
        &session,
        "createSnapshot",
        json!({
            "openProjects": ["/tsconfig.json"],
            "fileSystem": {
                "kind": "full",
                "files": {
                    "/tsconfig.json": r#"{ "compilerOptions": { "noLib": true, "outDir": "/out" }, "files": ["src/main.ts"] }"#,
                    "/src/main.ts": "export const value: number = 1;",
                },
            },
        }),
    )
    .unwrap();
    let layered = call(
        &session,
        "updateSnapshot",
        json!({
            "snapshot": base["snapshot"],
            "changes": { "fileSystem": { "kind": "layer", "files": {} } },
        }),
    )
    .unwrap();
    assert_eq!(layered["projects"], json!([]));
    let params = json!({ "snapshot": layered["snapshot"], "project": base["projects"][0]["id"] });
    let emitted = call(&session, "emit", params.clone()).unwrap();
    assert_eq!(emitted["emittedFiles"], json!(["/out/src/main.js"]));
    assert_eq!(
        emitted["emittedFilesContents"],
        json!(["export const value = 1;\n"])
    );
    call(&session, "release", json!({ "snapshot": base["snapshot"] })).unwrap();
    assert_eq!(call(&session, "emit", params).unwrap(), emitted);
}
