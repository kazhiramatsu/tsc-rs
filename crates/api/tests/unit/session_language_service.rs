//! The language service requests (L9): completions (with their symbols,
//! through prepared auto-imports), the referenced symbols of a node, a
//! signature's usages, the references to a symbol in a file, a symbol's
//! JSDoc tags and documentation comment, and the import adder's edits.
//! Pinned to tsgo's responses (aa8149273) for the same files; the symbol
//! handles are each implementation's own, so the tests compare what they
//! identify.

use super::*;

/// The symbol at `text`'s first occurrence in a file of the snapshot.
fn symbol_in(session: &Session, base: &Value, file: &str, source: &str, text: &str) -> Value {
    let at = source.find(text).unwrap_or_else(|| panic!("{text:?}"));
    call(
        session,
        "getSymbolAtPosition",
        with(base, json!({ "file": file, "position": at })),
    )
    .unwrap()
}

/// A symbol response's handle.
fn handle(symbol: &Value) -> Value {
    symbol["reference"].clone()
}

/// A symbol response without its handles.
fn identified(symbol: &Value) -> Value {
    let mut symbol = symbol.clone();
    let object = symbol.as_object_mut().unwrap();
    object.remove("reference");
    object.remove("parent");
    symbol
}

fn names(completions: &Value) -> Vec<&str> {
    completions["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect()
}

const OBJECT: &str = "\nconst obj = { name: \"hello\", age: 42 };\nobj.\n";

#[test]
fn member_completions_carry_their_symbols_when_asked() {
    let (session, base) = checker(
        &[("/tsconfig.json", "{}"), ("/src/main.ts", OBJECT)],
        "/tsconfig.json",
    );
    let position = OBJECT.find("obj.").unwrap() + 4;
    let plain = call(
        &session,
        "getCompletionsAtPosition",
        with(
            &base,
            json!({ "file": "/src/main.ts", "position": position, "triggerCharacter": "." }),
        ),
    )
    .unwrap();
    assert_eq!(
        plain,
        json!({ "isIncomplete": false, "entries": [
            { "name": "name", "kind": 5, "sortText": "11" },
            { "name": "age", "kind": 5, "sortText": "11" },
        ] })
    );
    let with_symbols = call(
        &session,
        "getCompletionsAtPosition",
        with(
            &base,
            json!({ "file": "/src/main.ts", "position": position, "triggerCharacter": ".", "includeSymbol": true }),
        ),
    )
    .unwrap();
    let entries = with_symbols["entries"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| identified(&entry["symbol"]))
            .collect::<Vec<_>>(),
        vec![
            json!({ "name": "name", "flags": 33554436, "checkFlags": 0, "declarations": ["10.304./src/main.ts"], "valueDeclaration": "10.304./src/main.ts" }),
            json!({ "name": "age", "flags": 33554436, "checkFlags": 0, "declarations": ["13.304./src/main.ts"], "valueDeclaration": "13.304./src/main.ts" }),
        ]
    );
    // A snapshot's symbol (the checker's own), which the checker requests
    // take back.
    assert_eq!(entries[0]["symbol"]["reference"]["kind"], 1);
    let ty = call(
        &session,
        "getTypeOfSymbol",
        with(&base, json!({ "symbol": handle(&entries[0]["symbol"]) })),
    )
    .unwrap();
    assert!(ty["id"].is_u64(), "{ty}");
}

#[test]
fn completions_of_unions_strings_and_missing_files() {
    let union = "declare const u: { a: 1, b: string } | { a: 2, c: number };\nu.";
    let strings = "declare const o: { alpha: 1, beta: 2 };\no[\"\"]";
    let (session, base) = checker(
        &[
            ("/tsconfig.json", "{}"),
            ("/src/union.ts", union),
            ("/src/strings.ts", strings),
        ],
        "/tsconfig.json",
    );
    let completions = call(
        &session,
        "getCompletionsAtPosition",
        with(
            &base,
            json!({ "file": "/src/union.ts", "position": union.len(), "triggerCharacter": ".", "includeSymbol": true }),
        ),
    )
    .unwrap();
    assert_eq!(
        identified(&completions["entries"][0]["symbol"]),
        json!({ "name": "a", "flags": 33554436, "checkFlags": 2498, "declarations": ["14.173./src/union.ts", "23.173./src/union.ts"] })
    );
    assert_eq!(names(&completions), ["a"]);
    let completions = call(
        &session,
        "getCompletionsAtPosition",
        with(
            &base,
            json!({ "file": "/src/strings.ts", "position": strings.find("o[\"").unwrap() + 3, "triggerCharacter": "\"", "includeSymbol": true }),
        ),
    )
    .unwrap();
    assert_eq!(names(&completions), ["alpha", "beta"]);
    assert_eq!(
        identified(&completions["entries"][1]["symbol"]),
        json!({ "name": "beta", "flags": 4, "checkFlags": 0, "declarations": ["16.173./src/strings.ts"], "valueDeclaration": "16.173./src/strings.ts" })
    );
    // A file the program does not have.
    assert_eq!(
        call(
            &session,
            "getCompletionsAtPosition",
            with(&base, json!({ "file": "/src/missing.ts", "position": 0 })),
        )
        .unwrap(),
        Value::Null
    );
}

#[test]
fn completions_need_prepared_auto_imports_for_their_symbols() {
    let (session, _) = session(&[
        (
            "/tsconfig.json",
            r#"{ "compilerOptions": { "module": "esnext", "target": "esnext" }, "include": ["src"] }"#,
        ),
        ("/src/export.ts", "export const someValue = 1;"),
        ("/src/main.ts", "someV"),
    ]);
    let created = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/tsconfig.json"], "userPreferences": { "includeCompletionsForModuleExports": true } }),
    )
    .unwrap();
    let base = json!({ "snapshot": created["snapshot"], "project": "/tsconfig.json" });
    let request = |base: &Value, include_symbol: bool| {
        call(
            &session,
            "getCompletionsAtPosition",
            with(
                base,
                json!({ "file": "/src/main.ts", "position": 5, "includeSymbol": include_symbol }),
            ),
        )
    };
    assert_eq!(
        request(&base, true).unwrap_err(),
        "api: client error: snapshot is not prepared for auto-imports for /src/main.ts"
    );
    // Without symbols, through a snapshot with them prepared.
    assert!(names(&request(&base, false).unwrap()).contains(&"someValue"));
    let prepared = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": { "prepareAutoImports": "/src/main.ts" } }),
    )
    .unwrap();
    let prepared_base = json!({ "snapshot": prepared["snapshot"], "project": "/tsconfig.json" });
    let completions = request(&prepared_base, true).unwrap();
    let entry = completions["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "someValue")
        .unwrap();
    assert_eq!(
        entry,
        &json!({ "name": "someValue", "kind": 6, "sortText": "16", "labelDetails": { "description": "./export" } })
    );
    // The snapshot's preferences turn them off.
    let disabled = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/tsconfig.json"], "userPreferences": { "includeCompletionsForModuleExports": false } }),
    )
    .unwrap();
    let disabled_base = json!({ "snapshot": disabled["snapshot"], "project": "/tsconfig.json" });
    assert!(!names(&request(&disabled_base, true).unwrap()).contains(&"someValue"));
    // A file without a project cannot be prepared.
    assert_eq!(
        call(
            &session,
            "createSnapshot",
            json!({ "openProjects": ["/tsconfig.json"], "prepareAutoImports": "/nowhere/missing.ts" }),
        )
        .unwrap_err(),
        "api: client error: could not prepare auto-imports for /nowhere/missing.ts"
    );
}

const GREET: &str = "function greet(name: string) { return name; }\ngreet(\"world\");\nconst o = { greet };\no.greet(\"x\");\nexport { greet as hello };\n";
const OTHER: &str = "import { hello } from \"./index\";\nhello(\"y\");\n";

fn greet() -> (Session, Value) {
    checker(
        &[
            ("/tsconfig.json", "{}"),
            ("/src/index.ts", GREET),
            ("/src/other.ts", OTHER),
        ],
        "/tsconfig.json",
    )
}

#[test]
fn referenced_symbols_and_signature_usages_of_a_function() {
    let (session, base) = greet();
    let references = call(
        &session,
        "getReferencedSymbolsForNode",
        with(
            &base,
            json!({ "node": "4.79./src/index.ts", "position": 8 }),
        ),
    )
    .unwrap();
    let entries = references.as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| (
                entry["definition"].clone(),
                identified(&entry["symbol"]),
                entry["references"].clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                json!("3.264./src/index.ts"),
                json!({ "name": "greet", "flags": 16, "checkFlags": 0, "declarations": ["3.264./src/index.ts"], "valueDeclaration": "3.264./src/index.ts" }),
                json!([
                    "4.79./src/index.ts",
                    "15.79./src/index.ts",
                    "26.79./src/index.ts",
                    "38.79./src/index.ts"
                ]),
            ),
            (
                json!("37.283./src/index.ts"),
                json!({ "name": "hello", "flags": 2097152, "checkFlags": 0, "declarations": ["37.283./src/index.ts"] }),
                json!(["39.79./src/index.ts"]),
            ),
            (
                json!("7.278./src/other.ts"),
                json!({ "name": "hello", "flags": 2097152, "checkFlags": 0, "declarations": ["7.278./src/other.ts"] }),
                json!(["8.79./src/other.ts", "12.79./src/other.ts"]),
            ),
        ]
    );
    assert_eq!(
        call(
            &session,
            "getSignatureUsages",
            with(&base, json!({ "signatureDecl": "3.264./src/index.ts" })),
        )
        .unwrap(),
        json!([
            { "name": "15.79./src/index.ts", "call": "14.215./src/index.ts" },
            { "name": "26.79./src/index.ts" },
            { "name": "38.79./src/index.ts" },
            { "name": "12.79./src/other.ts", "call": "11.215./src/other.ts" },
        ])
    );
}

#[test]
fn unnamed_signatures_string_literals_and_constructors() {
    let source =
        "const fn = () => 1;\nfn();\nconsole.log(\"x\");\nclass C { constructor() {} }\nnew C();\n";
    let (session, base) = checker(
        &[("/tsconfig.json", "{}"), ("/src/index.ts", source)],
        "/tsconfig.json",
    );
    let ask = |method: &str, fields: Value| call(&session, method, with(&base, fields)).unwrap();
    // An unnamed signature has no usages, and its node no references: tsgo
    // writes the empty lists.
    let arrow = "8.221./src/index.ts";
    assert_eq!(
        ask("getSignatureUsages", json!({ "signatureDecl": arrow })),
        json!([])
    );
    assert_eq!(
        ask(
            "getReferencedSymbolsForNode",
            json!({ "node": arrow, "position": 10 })
        ),
        json!([])
    );
    // A string literal's references: the same strings, without a symbol.
    assert_eq!(
        ask(
            "getReferencedSymbolsForNode",
            json!({ "node": "22.10./src/index.ts", "position": 38 })
        ),
        json!([{ "definition": "22.10./src/index.ts", "references": ["22.10./src/index.ts"] }])
    );
    // A class declaration at its constructor keyword.
    assert_eq!(
        ask(
            "getReferencedSymbolsForNode",
            json!({ "node": "23.265./src/index.ts", "position": 54 })
        ),
        json!([])
    );
}

#[test]
fn references_to_a_symbol_in_a_file() {
    let (session, base) = greet();
    let greet = symbol_in(&session, &base, "/src/index.ts", GREET, "greet");
    let references = |file: &str| {
        call(
            &session,
            "getReferencesToSymbolInFile",
            with(&base, json!({ "file": file, "symbol": handle(&greet) })),
        )
        .unwrap()
    };
    assert_eq!(
        references("/src/index.ts"),
        json!([
            "4.79./src/index.ts",
            "15.79./src/index.ts",
            "26.79./src/index.ts",
            "38.79./src/index.ts"
        ])
    );
    assert_eq!(references("/src/other.ts"), json!([]));
}

#[test]
fn js_doc_tags_and_documentation_comments() {
    let source = "\n/**\n * Adds two numbers together.\n * @param a the first number\n * @returns the sum\n * @throws {Error} when bad\n * @template T, U\n * @see other\n * @deprecated\n */\nexport function add(a: number, b: number): number { return a + b; }\n/** First. */\nexport function over(a: string): void;\n/** Second. */\nexport function over(a: number): void;\nexport function over(a: any) {}\n/** @typedef {string} Alias */\n/** Merged one. */\ninterface M { a: 1 }\n/** Merged two. */\ninterface M { b: 2 }\n";
    let js = "/**\n * @typedef {object} Opts\n * @property {string} name the name\n */\n/**\n * Does it. See {@link Opts} and {@linkcode run}.\n * @param {Opts} opts the options\n * @returns {void}\n */\nfunction run(opts) {}\n/** A value. @type {number} */\nconst value = 1;\nconst obj = {\n  /** The prop. @deprecated */\n  prop: 1,\n};\nmodule.exports = { run, value };\n";
    let (session, base) = checker(
        &[
            (
                "/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true, "allowJs": true, "checkJs": true } }"#,
            ),
            ("/src/main.ts", source),
            ("/src/a.js", js),
        ],
        "/tsconfig.json",
    );
    let documentation = |file: &str, text: &str, marker: &str| {
        let symbol = symbol_in(&session, &base, file, text, marker);
        let comment = call(
            &session,
            "getDocumentationComment",
            with(&base, json!({ "symbol": handle(&symbol) })),
        )
        .unwrap();
        let tags = call(
            &session,
            "getJsDocTags",
            with(&base, json!({ "symbol": handle(&symbol) })),
        )
        .unwrap();
        (comment, tags)
    };
    assert_eq!(
        documentation("/src/main.ts", source, "add(a"),
        (
            json!("Adds two numbers together."),
            json!([
                { "name": "param", "text": "a the first number" },
                { "name": "returns", "text": "the sum" },
                { "name": "throws", "text": "{Error} when bad" },
                { "name": "template", "text": "T, U" },
                { "name": "see", "text": "other" },
                { "name": "deprecated" },
            ])
        )
    );
    assert_eq!(
        documentation("/src/main.ts", source, "over(a: string"),
        (json!("First.\nSecond."), json!([]))
    );
    assert_eq!(
        documentation("/src/main.ts", source, "M { a"),
        (json!("Merged one.\nMerged two."), json!([]))
    );
    assert_eq!(
        documentation("/src/a.js", js, "run(opts"),
        (
            json!("Does it. See Opts and run."),
            json!([{ "name": "param", "text": "opts the options" }, { "name": "returns" }])
        )
    );
    // A variable's comment is its statement's.
    assert_eq!(
        documentation("/src/a.js", js, "value = 1"),
        (
            json!("A value."),
            json!([{ "name": "type", "text": "{number}" }])
        )
    );
    assert_eq!(
        documentation("/src/a.js", js, "prop: 1"),
        (json!("The prop."), json!([{ "name": "deprecated" }]))
    );
}

#[test]
fn import_adder_edits() {
    let source = "import { foo } from \"./foo\";\nconst value = foo + bar;\ntype T = Shape;\n";
    let foo = "export const foo = 1;\nexport const bar = 2;\nexport interface Shape {}\nconst local = 1;\n";
    let main = "const 名前 = 1;\n";
    let lib = "export function helper() {}\nexport default class Thing {}\n";
    let (session, base) = checker(
        &[
            ("/tsconfig.json", "{}"),
            ("/src/index.ts", source),
            ("/src/foo.ts", foo),
            ("/src/main.ts", main),
            ("/src/lib.ts", lib),
        ],
        "/tsconfig.json",
    );
    // tsgo's client `Symbol.getExportSymbol`: the export symbol of a local
    // one, or the symbol.
    let export_symbol = |file: &str, text: &str, marker: &str| {
        let symbol = symbol_in(&session, &base, file, text, marker);
        let export = call(
            &session,
            "getExportSymbolOfSymbol",
            with(&base, json!({ "symbol": handle(&symbol) })),
        )
        .unwrap();
        if export.is_null() {
            handle(&symbol)
        } else {
            handle(&export)
        }
    };
    let edits = |file: &str, actions: Value| {
        call(
            &session,
            "getImportAdderEdits",
            with(&base, json!({ "file": file, "actions": actions })),
        )
    };
    let bar = export_symbol("/src/foo.ts", foo, "bar");
    let shape = export_symbol("/src/foo.ts", foo, "Shape");
    let local = handle(&symbol_in(&session, &base, "/src/foo.ts", foo, "local"));
    assert_eq!(
        edits(
            "/src/index.ts",
            json!([{ "kind": "importSymbol", "symbol": bar }, { "kind": "importSymbol", "symbol": shape }])
        )
        .unwrap(),
        json!([{ "pos": 9, "end": 9, "newText": "bar, " }, { "pos": 12, "end": 12, "newText": ", Shape" }])
    );
    assert_eq!(
        edits(
            "/src/index.ts",
            json!([{ "kind": "importSymbol", "symbol": shape, "isValidTypeOnlyUseSite": false }])
        )
        .unwrap(),
        json!([{ "pos": 12, "end": 12, "newText": ", Shape" }])
    );
    // A symbol no module exports: no edits.
    assert_eq!(
        edits(
            "/src/index.ts",
            json!([{ "kind": "importSymbol", "symbol": local }])
        )
        .unwrap(),
        json!([])
    );
    // A new declaration, the default and named imports together, at the
    // start of a file with non-ASCII text.
    let helper = export_symbol("/src/lib.ts", lib, "helper");
    let thing = export_symbol("/src/lib.ts", lib, "Thing");
    assert_eq!(
        edits(
            "/src/main.ts",
            json!([{ "kind": "importSymbol", "symbol": helper }, { "kind": "importSymbol", "symbol": thing }])
        )
        .unwrap(),
        json!([{ "pos": 0, "end": 0, "newText": "import Thing, { helper } from \"./lib\";\n\n" }])
    );
    // The client's mistakes.
    let mut missing = helper.clone();
    missing["id"] = json!(999_999_999u64);
    assert_eq!(
        edits(
            "/src/main.ts",
            json!([{ "kind": "importSymbol", "symbol": missing }])
        )
        .unwrap_err(),
        "api: client error: symbol handle 999999999 not found in source file"
    );
    assert_eq!(
        edits(
            "/src/main.ts",
            json!([{ "kind": "unknown", "symbol": helper }])
        )
        .unwrap_err(),
        "api: client error: unknown import adder action kind \"unknown\""
    );
    assert_eq!(
        edits("/src/main.ts", json!([{ "kind": "importSymbol" }])).unwrap_err(),
        "api: client error: import adder action 0 missing symbol"
    );
}
