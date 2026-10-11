//! File-owned symbols (microsoft/TypeScript aa8149273, #64518): a binder
//! symbol is named by its file's descriptor and its number in the file, and
//! answers its parent, export symbol, members and exports without a
//! checker; a checker's symbol is the snapshot's. Pinned to tsgo
//! aa8149273's raw responses for the same files; node ids and the numbers
//! of symbols and types are each implementation's own.

use super::*;

const MAIN: &str = "export const x = 42;\nexport class C<T> { value: T; m(): void {} }\nnamespace NS { export const inner = 1; }\nexport const f = () => 1;\n";

fn project() -> (Session, Value) {
    checker(
        &[
            (
                "/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true } }"#,
            ),
            ("/src/main.ts", MAIN),
            ("/src/view.tsx", "export const v = <div />;\n"),
        ],
        "/tsconfig.json",
    )
}

fn symbol_in(session: &Session, base: &Value, file: &str, position: usize) -> Value {
    call(
        session,
        "getSymbolAtPosition",
        with(base, json!({ "file": file, "position": position })),
    )
    .unwrap()
}

fn main_symbol(session: &Session, base: &Value, text: &str) -> Value {
    symbol_in(session, base, "/src/main.ts", MAIN.find(text).unwrap())
}

/// A symbol property request, which names its symbol alone.
fn property(session: &Session, method: &str, symbol: &Value) -> Value {
    call(session, method, json!({ "symbol": symbol["reference"] })).unwrap()
}

fn names(symbols: &Value) -> Vec<&str> {
    symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap())
        .collect()
}

#[test]
fn a_binder_symbol_is_named_by_its_files_descriptor() {
    let (session, base) = project();
    let x = main_symbol(&session, &base, "x = 42");
    let node_id = x["reference"]["file"]["nodeId"].as_str().unwrap();
    assert!(node_id.parse::<u64>().unwrap() > 0);
    assert_eq!(
        x["reference"],
        json!({
            "kind": 0,
            "file": {
                "fileName": "/src/main.ts", "path": "/src/main.ts",
                "contentHash": "8606d32f59215859672339ffa97c7763", "parseOptionsKey": "0",
                "scriptKind": 3, "nodeId": node_id,
            },
            "id": x["reference"]["id"],
        })
    );
    assert_eq!(
        shape(&x),
        json!({
            "reference": "file:/src/main.ts", "name": "x", "flags": 2, "checkFlags": 0,
            "declarations": ["8.262./src/main.ts"], "valueDeclaration": "8.262./src/main.ts",
            "parent": "*",
        })
    );
    assert_eq!(x["parent"]["file"], node_id);
    // Another file: its own hash, script kind and parse.
    let v = symbol_in(&session, &base, "/src/view.tsx", 13);
    let file = &v["reference"]["file"];
    assert_eq!(file["contentHash"], "d7b1d88b0bbf17b2035a610621debb69");
    assert_eq!(
        (file["parseOptionsKey"].clone(), file["scriptKind"].clone()),
        (json!("0"), json!(4))
    );
    assert_ne!(file["nodeId"], node_id);
}

#[test]
fn a_file_symbols_relations_need_no_checker() {
    let (session, base) = project();
    let x = main_symbol(&session, &base, "x = 42");
    let module = property(&session, "getParentOfSymbol", &x);
    assert_eq!(
        shape(&module),
        json!({
            "reference": "file:/src/main.ts", "name": "\"/src/main\"", "flags": 512,
            "checkFlags": 0, "declarations": ["1.308./src/main.ts"],
            "valueDeclaration": "1.308./src/main.ts",
        })
    );
    assert_eq!(compact(&module), x["parent"]);
    assert_eq!(
        property(&session, "getExportSymbolOfSymbol", &x),
        Value::Null
    );
    // Tables by declaration position.
    let class = main_symbol(&session, &base, "C<T>");
    let members = property(&session, "getMembersOfSymbol", &class);
    assert_eq!(names(&members), ["T", "value", "m"]);
    assert_eq!(
        (
            members[2]["flags"].clone(),
            members[2]["declarations"].clone()
        ),
        (json!(8192), json!(["23.176./src/main.ts"]))
    );
    let exports = property(&session, "getExportsOfSymbol", &class);
    assert_eq!(exports.as_array().unwrap().len(), 1);
    assert_eq!(
        shape(&exports[0]),
        json!({
            "reference": "file:/src/main.ts", "name": "prototype", "flags": 4_194_308,
            "checkFlags": 0, "parent": "*",
        })
    );
    // Go writes the nil slice of an empty table as `[]`.
    assert_eq!(
        property(&session, "getExportsOfSymbol", &members[1]),
        json!([])
    );
    assert_eq!(
        property(&session, "getMembersOfSymbol", &members[1]),
        json!([])
    );
    let namespace = main_symbol(&session, &base, "NS");
    let inner = property(&session, "getExportsOfSymbol", &namespace);
    assert_eq!(names(&inner), ["inner"]);
    assert_eq!(inner[0]["parent"], compact(&namespace));
}

#[test]
fn a_checkers_symbol_is_the_snapshots() {
    let (session, base) = project();
    let class = main_symbol(&session, &base, "C<T>");
    let declared = call(
        &session,
        "getDeclaredTypeOfSymbol",
        with(&base, json!({ "symbol": class["reference"] })),
    )
    .unwrap();
    assert_eq!(declared["symbol"], compact(&class));
    // The declared type's property is instantiated: a transient symbol.
    let value = call(
        &session,
        "getPropertyOfType",
        with(&base, json!({ "type": declared["id"], "name": "value" })),
    )
    .unwrap();
    assert_eq!(
        (
            value["reference"]["kind"].clone(),
            value["reference"]["snapshot"].clone()
        ),
        (json!(1), base["snapshot"].clone())
    );
    assert!(value["reference"].get("file").is_none());
    assert_eq!(
        shape(&value),
        json!({
            "reference": "snapshot:/tsconfig.json", "name": "value", "flags": 33_554_436,
            "checkFlags": 1, "declarations": ["19.174./src/main.ts"],
            "valueDeclaration": "19.174./src/main.ts", "parent": "*",
        })
    );
    // Its parent is the class, answered as the file's.
    assert_eq!(value["parent"], compact(&class));
    assert_eq!(property(&session, "getParentOfSymbol", &value), class);
    assert_eq!(property(&session, "getMembersOfSymbol", &value), json!([]));
    let ty = call(
        &session,
        "getTypeOfSymbol",
        with(&base, json!({ "symbol": value["reference"] })),
    )
    .unwrap();
    let parameter = &property(&session, "getMembersOfSymbol", &class)[0];
    assert_eq!(ty["symbol"], compact(parameter));
}

#[test]
fn the_checkers_symbol_of_a_node_and_its_parent_and_merged_symbol() {
    let (session, base) = project();
    let ask = |method: &str, fields: Value| call(&session, method, with(&base, fields)).unwrap();
    let x = main_symbol(&session, &base, "x = 42");
    let declaration = &x["declarations"][0];
    assert_eq!(
        ask("getSymbolOfNode", json!({ "location": declaration })),
        x
    );
    assert_eq!(
        ask(
            "getSymbolOfDeclarationForChecker",
            json!({ "location": declaration })
        ),
        x
    );
    assert_eq!(
        ask("getMergedSymbol", json!({ "symbol": x["reference"] })),
        x
    );
    let module = property(&session, "getParentOfSymbol", &x);
    assert_eq!(
        ask(
            "getParentOfSymbolForChecker",
            json!({ "symbol": x["reference"] })
        ),
        module
    );
    // The source file's own symbol is its module's.
    for method in ["getSymbolOfNode", "getSymbolOfDeclarationForChecker"] {
        assert_eq!(
            ask(method, json!({ "location": "1.308./src/main.ts" })),
            module
        );
    }
}

#[test]
fn a_bad_symbol_reference_is_the_clients_error() {
    let (session, base) = project();
    let x = main_symbol(&session, &base, "x = 42");
    let file = |fields: Value| {
        let mut reference = x["reference"].clone();
        for (key, value) in fields.as_object().unwrap() {
            if key == "id" || key == "project" {
                reference[key] = value.clone();
            } else {
                reference["file"][key] = value.clone();
            }
        }
        json!({ "symbol": reference })
    };
    let snapshot = |fields: Value| {
        let mut reference = json!({ "kind": 1, "snapshot": base["snapshot"], "project": "/tsconfig.json", "id": 1 });
        for (key, value) in fields.as_object().unwrap() {
            reference[key] = value.clone();
        }
        json!({ "symbol": reference })
    };
    let checker_error = |params: Value| {
        call(&session, "getTypeOfSymbol", with(&base, params))
            .unwrap_err()
            .trim_start_matches("api: client error: ")
            .to_owned()
    };
    for (params, message) in [
        (
            json!({ "symbol": { "kind": 7, "id": 1 } }),
            "invalid symbol reference kind 7",
        ),
        (json!({ "symbol": {} }), "invalid file symbol reference"),
        (
            snapshot(json!({ "snapshot": 99 })),
            "snapshot symbol reference does not match the requested checker",
        ),
        (snapshot(json!({ "id": 0 })), "empty symbol handle"),
        (
            snapshot(json!({ "id": 999_999 })),
            "symbol handle 999999 not found in snapshot registry",
        ),
        (
            file(json!({ "contentHash": "0".repeat(32) })),
            "source file is not part of the requested program",
        ),
        (
            file(json!({ "id": 999_999 })),
            "symbol handle 999999 not found in source file",
        ),
        (
            file(json!({ "project": "/tsconfig.json" })),
            "invalid file symbol reference",
        ),
    ] {
        assert_eq!(checker_error(params.clone()), message, "{params}");
    }
    let property_error = |params: Value| {
        call(&session, "getParentOfSymbol", params)
            .unwrap_err()
            .trim_start_matches("api: client error: ")
            .to_owned()
    };
    for (params, message) in [
        (
            file(json!({ "id": 999_999 })),
            "symbol 999999 not found in source file",
        ),
        (
            json!({ "symbol": { "kind": 1, "snapshot": base["snapshot"], "id": 1 } }),
            "invalid snapshot symbol reference",
        ),
        (
            file(json!({ "contentHash": "0".repeat(32) })),
            "source file is not available",
        ),
        (
            file(json!({ "contentHash": "00" })),
            "invalid source file descriptor: content hash must contain 32 hexadecimal digits",
        ),
        (
            file(json!({ "nodeId": "1" })),
            "source file descriptor no longer identifies the cached source file",
        ),
    ] {
        assert_eq!(property_error(params.clone()), message, "{params}");
    }
}

#[test]
fn a_cached_source_files_declarations_by_node_index() {
    let (session, base) = project();
    let x = main_symbol(&session, &base, "x = 42");
    let descriptor = &x["reference"]["file"];
    assert_eq!(
        call(&session, "retainSourceFile", json!({ "file": descriptor })).unwrap(),
        json!({ "lease": 1 })
    );
    let declaration = |index: u32| {
        call(
            &session,
            "getSymbolOfDeclaration",
            json!({ "file": descriptor, "index": index }),
        )
        .map_err(|error| error.trim_start_matches("api: client error: ").to_owned())
    };
    assert_eq!(declaration(8).unwrap(), x);
    // The source file is a declaration (its module's); index 2 is the
    // statement list.
    assert_eq!(
        declaration(1).unwrap(),
        property(&session, "getParentOfSymbol", &x)
    );
    assert_eq!(
        declaration(2).unwrap_err(),
        "node index 2 is not a declaration"
    );
    for index in [0, 99_999] {
        assert_eq!(
            declaration(index).unwrap_err(),
            format!("declaration node index {index} is out of range")
        );
    }
    assert_eq!(
        call(&session, "releaseSourceFile", json!({ "lease": 1 })).unwrap(),
        true
    );
    assert_eq!(
        call(&session, "releaseSourceFile", json!({ "lease": 1 })).unwrap_err(),
        "api: client error: source file lease 1 not found"
    );
    let mut unknown = descriptor.clone();
    unknown["fileName"] = json!("/nope.ts");
    unknown["path"] = json!("/nope.ts");
    assert_eq!(
        call(&session, "retainSourceFile", json!({ "file": unknown })).unwrap_err(),
        "api: client error: source file is not available"
    );
}

#[test]
fn a_released_snapshots_descriptors_are_forgotten() {
    // The descriptors cache the hash tsgo keeps on its source file; they go
    // with the last snapshot or lease holding the parse.
    let (session, base) = project();
    let x = main_symbol(&session, &base, "x = 42");
    let cached = || {
        session
            .descriptors
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    let node_id = x["reference"]["file"]["nodeId"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(cached(), [node_id]);
    let descriptor = &x["reference"]["file"];
    call(&session, "retainSourceFile", json!({ "file": descriptor })).unwrap();
    call(&session, "release", json!({ "snapshot": base["snapshot"] })).unwrap();
    assert_eq!(cached(), [node_id]);
    call(&session, "releaseSourceFile", json!({ "lease": 1 })).unwrap();
    assert_eq!(cached(), Vec::<u64>::new());
}
