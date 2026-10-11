//! The checker's queries (P5-4a): the symbol at a position or node handle,
//! the types of symbols and locations, the symbol and type responses, the
//! symbol queries, `typeToString` and the intrinsic types. Pinned to tsgo's
//! responses at 19dadef8 for the same files; the numbers of symbols and
//! types are each implementation's own, so the tests compare what they
//! identify.

use super::*;

const MAIN: &str = r#"export const x = 42;
export let s = "hi";
export function add(a: number, b: number, ...rest: number[]): number { return a + b; }
export class MyClass<T> { value: T; constructor(v: T) { this.value = v; } method(): void {} }
export interface I { readonly p?: string; [k: string]: any }
export type U = string | number;
export const tup: [a: number, b?: string, ...c: boolean[]] = [1];
export enum E { A, B = 2 }
export const big = 10n;
export const neg = -1.5;
export const obj = { a: 1, b: "x" };
export type M = { [K in "a" | "b"]: K };
declare const sym: unique symbol;
export { sym };
export default class {}
namespace NS { export const inner = 1; }
export const f = () => 1;
"#;

const FILES: &[(&str, &str)] = &[
    (
        "/tsconfig.json",
        r#"{ "compilerOptions": { "strict": true, "target": "es2020", "lib": ["es2020"] } }"#,
    ),
    ("/src/main.ts", MAIN),
];

/// A session with the project open: the session and the base of the
/// requests (snapshot and project).
fn checker(files: &[(&str, &str)], config: &str) -> (Session, Value) {
    let (session, _) = session(files);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": [config] }),
    )
    .unwrap();
    let base = json!({
        "snapshot": response["snapshot"],
        "project": response["projects"][0]["id"],
    });
    (session, base)
}

/// The request `base` with `fields` added.
fn with(base: &Value, fields: Value) -> Value {
    let mut params = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        params[key] = value.clone();
    }
    params
}

fn position(text: &str) -> usize {
    MAIN.find(text)
        .unwrap_or_else(|| panic!("{text:?} in main.ts"))
}

fn symbol_at(session: &Session, base: &Value, at: usize) -> Value {
    call(
        session,
        "getSymbolAtPosition",
        with(base, json!({ "file": "/src/main.ts", "position": at })),
    )
    .unwrap()
}

/// The response with its handles replaced by `*` (non-zero) or `0`.
fn shape(response: &Value) -> Value {
    let mut response = response.clone();
    for key in [
        "id",
        "parent",
        "exportSymbol",
        "symbol",
        "aliasSymbol",
        "target",
        "freshType",
        "regularType",
        "thisType",
    ] {
        if let Some(value) = response.get_mut(key) {
            *value = json!(if value.as_u64().unwrap_or(0) == 0 {
                "0"
            } else {
                "*"
            });
        }
    }
    response
}

#[test]
fn the_symbol_at_a_position_is_its_response() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let x = symbol_at(&session, &base, position("x = 42"));
    assert_eq!(
        shape(&x),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "x", "flags": 2, "checkFlags": 0,
            "declarations": ["8.262./src/main.ts"], "valueDeclaration": "8.262./src/main.ts",
            "parent": "*",
        })
    );
    // The end of a name touches it; the `=` after it has no symbol.
    assert_eq!(
        symbol_at(&session, &base, position("x = 42") + 1)["id"],
        x["id"]
    );
    assert_eq!(symbol_at(&session, &base, position("= 42")), Value::Null);
    // The parent is the module, which the file's symbol is.
    let module = call(
        &session,
        "getSymbolOfSourceFile",
        with(&base, json!({ "file": "/src/main.ts" })),
    )
    .unwrap();
    assert_eq!(module["id"], x["parent"]);
    assert_eq!(
        shape(&module),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "\"/src/main\"", "flags": 512,
            "checkFlags": 0, "declarations": ["1.308./src/main.ts"],
            "valueDeclaration": "1.308./src/main.ts",
        })
    );
    let batch = call(
        &session,
        "getSymbolsAtPositions",
        with(
            &base,
            json!({
                "file": "/src/main.ts",
                "positions": [position("x = 42"), position("= 42"), position("add(")],
            }),
        ),
    )
    .unwrap();
    assert_eq!(batch[0], x);
    assert_eq!(batch[1], Value::Null);
    assert_eq!(batch[2]["name"], "add");
    assert_eq!(batch[2]["flags"], 16);
    assert_eq!(batch[2]["declarations"], json!(["19.264./src/main.ts"]));
}

#[test]
fn the_symbols_of_members_and_namespaces() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let class = symbol_at(&session, &base, position("MyClass"));
    let value = symbol_at(&session, &base, position("value: T"));
    assert_eq!(
        shape(&value),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "value", "flags": 4, "checkFlags": 0,
            "declarations": ["51.174./src/main.ts"], "valueDeclaration": "51.174./src/main.ts",
            "parent": "*",
        })
    );
    assert_eq!(value["parent"], class["id"]);
    // An optional property, an enum member, a local namespace.
    let p = symbol_at(&session, &base, position("p?"));
    assert_eq!(
        (p["name"].clone(), p["flags"].clone()),
        (json!("p"), json!(16_777_220))
    );
    let member = symbol_at(&session, &base, position("B = 2"));
    assert_eq!(member["flags"], 8);
    assert_eq!(member["declarations"], json!(["132.307./src/main.ts"]));
    let namespace = symbol_at(&session, &base, position("NS"));
    assert_eq!(
        shape(&namespace),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "NS", "flags": 512, "checkFlags": 0,
            "declarations": ["202.269./src/main.ts"], "valueDeclaration": "202.269./src/main.ts",
        })
    );
    let inner = symbol_at(&session, &base, position("inner"));
    assert_eq!(inner["parent"], namespace["id"]);
}

#[test]
fn tokens_the_tree_does_not_keep_have_their_symbols() {
    // tsgo creates a token for the scanner's keyword or punctuation: a
    // class or function keyword and `=>` stand for their declaration, a
    // constructor keyword for its class.
    let (session, base) = checker(FILES, "/tsconfig.json");
    let default = symbol_at(&session, &base, position("class {}"));
    assert_eq!(
        (default["name"].clone(), default["flags"].clone()),
        (json!("default"), json!(32))
    );
    assert_eq!(
        symbol_at(&session, &base, position("function add"))["name"],
        "add"
    );
    let arrow = symbol_at(&session, &base, position("=> 1"));
    assert_eq!(
        shape(&arrow),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "__function", "flags": 16,
            "checkFlags": 0, "declarations": ["221.221./src/main.ts"],
            "valueDeclaration": "221.221./src/main.ts",
        })
    );
    assert_eq!(
        symbol_at(&session, &base, position("constructor"))["name"],
        "MyClass"
    );
    let ty = call(
        &session,
        "getTypeAtPosition",
        with(
            &base,
            json!({ "file": "/src/main.ts", "position": position("= 42") }),
        ),
    )
    .unwrap();
    assert_eq!(ty["intrinsicName"], "error");
}

#[test]
fn the_type_of_a_symbol_is_its_response() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let type_of = |at: &str| {
        let symbol = symbol_at(&session, &base, position(at));
        call(
            &session,
            "getTypeOfSymbol",
            with(&base, json!({ "symbol": symbol["id"] })),
        )
        .unwrap()
    };
    // A fresh literal type: its own fresh type, a regular type beside it.
    let x = type_of("x = 42");
    assert_eq!(
        shape(&x),
        json!({
            "id": "*", "flags": 2048, "objectFlags": 0, "isTupleType": false, "value": 42,
            "freshType": "*", "regularType": "*", "isThisType": false,
        })
    );
    assert_eq!(x["freshType"], x["id"]);
    assert_ne!(x["regularType"], x["id"]);
    assert_eq!(
        shape(&type_of("s = ")),
        json!({
            "id": "*", "flags": 32, "objectFlags": 0, "isTupleType": false, "value": null,
            "isThisType": false, "intrinsicName": "string",
        })
    );
    let add = type_of("add(");
    assert_eq!(
        shape(&add),
        json!({
            "id": "*", "flags": 1_048_576, "objectFlags": 16, "isTupleType": false,
            "value": null, "isThisType": false, "symbol": "*",
        })
    );
    assert_eq!(
        add["symbol"],
        symbol_at(&session, &base, position("add("))["id"]
    );
    // A tuple reference made from a type node.
    assert_eq!(
        shape(&type_of("tup")),
        json!({
            "id": "*", "flags": 1_048_576, "objectFlags": 536_870_916, "isTupleType": true,
            "value": null, "target": "*", "isThisType": false,
        })
    );
    assert_eq!(type_of("big")["value"], "10");
    assert_eq!(type_of("neg")["value"], -1.5);
    // An object literal's widened type has its members resolved.
    assert_eq!(type_of("obj")["objectFlags"], 2_097_168);
    let member = type_of("A, B");
    assert_eq!(
        (member["flags"].clone(), member["value"].clone()),
        (json!(34_816), json!(0))
    );
    assert_eq!(
        member["symbol"],
        symbol_at(&session, &base, position("A, B"))["id"]
    );
    assert_eq!(type_of("sym")["flags"], 16_384);
    // An interface has no value type.
    assert_eq!(type_of("I {")["intrinsicName"], "error");
}

#[test]
fn declared_and_non_missing_types() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let ask = |method: &str, at: &str| {
        let symbol = symbol_at(&session, &base, position(at));
        call(
            &session,
            method,
            with(&base, json!({ "symbol": symbol["id"] })),
        )
        .unwrap()
    };
    let class = ask("getDeclaredTypeOfSymbol", "MyClass");
    assert_eq!(
        shape(&class),
        json!({
            "id": "*", "flags": 1_048_576, "objectFlags": 5, "isTupleType": false,
            "value": null, "target": "*", "typeParameters": class["typeParameters"],
            "localTypeParameters": class["typeParameters"], "isThisType": false,
            "thisType": "*", "symbol": "*",
        })
    );
    assert_eq!(class["target"], class["id"]);
    assert_eq!(class["typeParameters"].as_array().unwrap().len(), 1);
    let alias = ask("getDeclaredTypeOfSymbol", "U =");
    assert_eq!(alias["flags"], 134_217_728);
    assert_eq!(
        alias["aliasSymbol"],
        symbol_at(&session, &base, position("U ="))["id"]
    );
    let enum_type = ask("getDeclaredTypeOfSymbol", "E {");
    assert_eq!(enum_type["flags"], 134_250_496);
    assert_eq!(enum_type["aliasSymbol"], enum_type["symbol"]);
    assert_eq!(ask("getDeclaredTypeOfSymbol", "I {")["objectFlags"], 2);
    // Without exactOptionalPropertyTypes the optional property's type has
    // no missing type to remove.
    assert_eq!(
        ask("getNonMissingTypeOfSymbol", "p?")["id"],
        ask("getTypeOfSymbol", "p?")["id"]
    );
}

#[test]
fn the_type_at_a_position_or_node() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let at = |offset: usize| {
        call(
            &session,
            "getTypeAtPosition",
            with(&base, json!({ "file": "/src/main.ts", "position": offset })),
        )
        .unwrap()
    };
    // An expression's type is its regular type.
    let literal = at(position("42"));
    assert_eq!(literal["regularType"], literal["id"]);
    assert_ne!(literal["freshType"], literal["id"]);
    let this = at(position("this.value"));
    assert_eq!(
        (this["flags"].clone(), this["isThisType"].clone()),
        (json!(524_288), json!(true))
    );
    assert_eq!(
        this["symbol"],
        symbol_at(&session, &base, position("MyClass"))["id"]
    );
    // The `export` modifier is a node without a type.
    assert_eq!(at(0)["intrinsicName"], "error");
    assert_eq!(at(position("MyClass"))["objectFlags"], 5);
    // A node handle: the variable declaration's type is its symbol's.
    let declaration = call(
        &session,
        "getTypeAtLocation",
        with(&base, json!({ "location": "8.262./src/main.ts" })),
    )
    .unwrap();
    assert_eq!(declaration["value"], 42);
    assert_eq!(declaration["freshType"], declaration["id"]);
    let module = call(
        &session,
        "getSymbolAtLocation",
        with(&base, json!({ "location": "1.308./src/main.ts" })),
    )
    .unwrap();
    assert_eq!(module["name"], "\"/src/main\"");
}

#[test]
fn intrinsic_types_are_the_checkers() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    for (method, flags, name) in [
        ("getAnyType", 1, "any"),
        ("getStringType", 32, "string"),
        ("getNumberType", 64, "number"),
        ("getVoidType", 16, "void"),
        ("getUndefinedType", 4, "undefined"),
        ("getNullType", 8, "null"),
        ("getNeverType", 262_144, "never"),
        ("getUnknownType", 2, "unknown"),
        ("getBigIntType", 128, "bigint"),
        ("getESSymbolType", 512, "symbol"),
        ("getNonPrimitiveType", 131_072, "object"),
    ] {
        let ty = call(&session, method, base.clone()).unwrap();
        assert_eq!(
            shape(&ty),
            json!({
                "id": "*", "flags": flags, "objectFlags": 0, "isTupleType": false,
                "value": null, "isThisType": false, "intrinsicName": name,
            }),
            "{method}"
        );
    }
    // boolean is the union of the two literal types.
    assert_eq!(
        shape(&call(&session, "getBooleanType", base.clone()).unwrap()),
        json!({
            "id": "*", "flags": 134_217_984, "objectFlags": 0, "isTupleType": false,
            "value": null, "isThisType": false,
        })
    );
    let any = call(&session, "getAnyType", base.clone()).unwrap();
    assert_eq!(
        call(
            &session,
            "typeToString",
            with(&base, json!({ "type": any["id"] }))
        )
        .unwrap(),
        "any"
    );
}

#[test]
fn members_exports_and_parents_of_symbols() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let property = |method: &str, symbol: &Value| {
        call(
            &session,
            method,
            with(&base, json!({ "objectId": symbol["id"] })),
        )
        .unwrap()
    };
    let class = symbol_at(&session, &base, position("MyClass"));
    let members = property("getMembersOfSymbol", &class);
    let names = members
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    // By declaration position.
    assert_eq!(names, ["T", "value", "__constructor", "method"]);
    let value = symbol_at(&session, &base, position("value: T"));
    assert_eq!(property("getParentOfSymbol", &value), class);
    assert_eq!(property("getExportSymbolOfSymbol", &value), Value::Null);
    // Go writes the nil slice of a symbol without exports as `[]`.
    assert_eq!(property("getExportsOfSymbol", &value), json!([]));
    let namespace = symbol_at(&session, &base, position("NS"));
    let exports = property("getExportsOfSymbol", &namespace);
    assert_eq!(exports[0]["name"], "inner");
}

#[test]
fn a_shared_files_symbol_has_one_handle_in_every_project() {
    // tsgo's client test "getSymbolAtPosition returns same Symbol instance
    // across projects".
    let files = [
        (
            "/a/tsconfig.json",
            r#"{ "files": ["../shared/shared.ts"] }"#,
        ),
        (
            "/b/tsconfig.json",
            r#"{ "files": ["../shared/shared.ts"] }"#,
        ),
        ("/shared/shared.ts", "export const sharedVar = 42;"),
    ];
    let (session, _) = session(&files);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/a/tsconfig.json", "/b/tsconfig.json"] }),
    )
    .unwrap();
    let snapshot = response["snapshot"].clone();
    let symbol = |project: &str| {
        call(
            &session,
            "getSymbolAtPosition",
            json!({ "snapshot": snapshot, "project": project, "file": "/shared/shared.ts", "position": 13 }),
        )
        .unwrap()
    };
    let a = symbol("/a/tsconfig.json");
    let b = symbol("/b/tsconfig.json");
    assert_eq!(a, b);
    // The project is the one it was first handed out in.
    assert_eq!(b["project"], "/a/tsconfig.json");
    // The other project's checker answers for it, with its own types.
    let ty = call(
        &session,
        "getTypeOfSymbol",
        json!({ "snapshot": snapshot, "project": "/b/tsconfig.json", "symbol": a["id"] }),
    )
    .unwrap();
    assert_eq!(ty["value"], 42);
    for project in ["/a/tsconfig.json", "/b/tsconfig.json"] {
        let number = call(
            &session,
            "getNumberType",
            json!({ "snapshot": snapshot, "project": project }),
        )
        .unwrap();
        assert_eq!(number["intrinsicName"], "number");
    }
}

const LIB: &str = "export function helper(n: number) { return n; }\nexport const enum CE { X = 1, Y = X << 1 }\nexport enum E { A = \"a\" }\n";

const USES: &str = r#"import { helper, CE, E } from "./lib";
import * as lib from "./lib";
export { helper as again } from "./lib";
let n: string | number = Math.random() > 0.5 ? "a" : 1;
if (typeof n === "string") { n.length; }
const shorthand = { helper };
const c = CE.Y;
class K { readonly r = 1; constructor() {} }
const arrow = (x: number) => x;
const cb = [1].map(x => x);
/** @param value the value */
function documented(value: number) { return value; }
const big = { a: 1, b: "two", c: [true], d: { e: null } } as const;
"#;

fn uses() -> (Session, Value) {
    checker(
        &[
            (
                "/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true } }"#,
            ),
            ("/lib.ts", LIB),
            ("/main.ts", USES),
        ],
        "/tsconfig.json",
    )
}

fn uses_symbol(session: &Session, base: &Value, text: &str, delta: usize) -> Value {
    let at = USES.find(text).unwrap() + delta;
    call(
        session,
        "getSymbolAtPosition",
        with(base, json!({ "file": "/main.ts", "position": at })),
    )
    .unwrap()
}

#[test]
fn aliases_resolve_to_their_targets() {
    let (session, base) = uses();
    let ask = |method: &str, symbol: &Value| {
        call(
            &session,
            method,
            with(&base, json!({ "symbol": symbol["id"] })),
        )
        .unwrap()
    };
    let alias = uses_symbol(&session, &base, "helper", 0);
    assert_eq!(
        shape(&alias),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "helper", "flags": 2_097_152,
            "checkFlags": 0, "declarations": ["7.278./main.ts"],
        })
    );
    let target = ask("getAliasedSymbol", &alias);
    assert_eq!(
        shape(&target),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "helper", "flags": 16,
            "checkFlags": 0, "declarations": ["3.264./lib.ts"],
            "valueDeclaration": "3.264./lib.ts", "parent": "*",
        })
    );
    assert_eq!(ask("getImmediateAliasedSymbol", &alias), target);
    assert_eq!(ask("getFullyQualifiedName", &alias), "helper");
    assert_eq!(ask("getFullyQualifiedName", &target), "\"/lib\".helper");
    // A namespace import's module: its exports by declaration, and one by
    // name.
    let namespace = uses_symbol(&session, &base, "lib ", 0);
    let module = ask("getAliasedSymbol", &namespace);
    assert_eq!(module["name"], "\"/lib\"");
    let exports = ask("getExportsOfModule", &module);
    let names = exports
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["helper", "CE", "E"]);
    let member = |name: &str| {
        call(
            &session,
            "getMemberInModuleExports",
            with(&base, json!({ "symbol": module["id"], "name": name })),
        )
        .unwrap()
    };
    assert_eq!(member("helper"), target);
    assert_eq!(member("nope"), Value::Null);
    // The re-export's local target.
    let again = uses_symbol(&session, &base, "again", 0);
    let local = call(
        &session,
        "getExportSpecifierLocalTargetSymbol",
        with(&base, json!({ "location": again["declarations"][0] })),
    )
    .unwrap();
    assert_eq!(local, target);
    // tsgo panics for a node that is neither an export specifier nor an
    // identifier; the connection (here the batch) answers the panic.
    let batch = call(
        &session,
        "batchRequests",
        json!({ "requests": [{
            "method": "getExportSpecifierLocalTargetSymbol",
            "params": with(&base, json!({ "location": alias["declarations"][0] })),
        }] }),
    )
    .unwrap();
    assert_eq!(
        batch["responses"][0]["error"],
        "panic: Unhandled case in getExportSpecifierLocalTargetSymbol, node should be ExportSpecifier | Identifier"
    );
    let shorthand = call(
        &session,
        "getShorthandAssignmentValueSymbol",
        with(&base, json!({ "location": target["declarations"][0] })),
    )
    .unwrap();
    assert_eq!(shorthand, Value::Null);
}

#[test]
fn symbol_queries_of_the_checker() {
    let (session, base) = uses();
    let ask = |method: &str, symbol: &Value| {
        call(
            &session,
            method,
            with(&base, json!({ "symbol": symbol["id"] })),
        )
        .unwrap()
    };
    let readonly = uses_symbol(&session, &base, "readonly r", 9);
    assert_eq!(ask("isReadonlySymbol", &readonly), true);
    assert_eq!(ask("getTargetSymbol", &readonly), readonly);
    assert_eq!(
        ask("getExportSymbolOfSymbolForChecker", &readonly),
        readonly
    );
    // A function's name: its variable's, or an anonymous function's.
    let arrow = uses_symbol(&session, &base, "=> x;", 0);
    assert_eq!(ask("getFullyQualifiedName", &arrow), "arrow");
    let callback = uses_symbol(&session, &base, "x => x", 2);
    assert_eq!(
        ask("getFullyQualifiedName", &callback),
        "(Anonymous function)"
    );
    // A JSDoc parameter tag's name is the parameter.
    let parameter = uses_symbol(&session, &base, "value the", 0);
    assert_eq!(parameter["name"], "value");
    assert_eq!(parameter["flags"], 1);
    // A const enum member's value at its declaration.
    let member = uses_symbol(&session, &base, "Y;", 0);
    let value = call(
        &session,
        "getConstantValue",
        with(&base, json!({ "location": member["declarations"][0] })),
    )
    .unwrap();
    assert_eq!(value, json!({ "isNumber": true, "value": 2 }));
}

#[test]
fn narrowing_at_a_reference() {
    let (session, base) = uses();
    let reference = USES.find("n.length").unwrap();
    let symbol = uses_symbol(&session, &base, "n.length", 0);
    let at_reference = call(
        &session,
        "getTypeAtPosition",
        with(&base, json!({ "file": "/main.ts", "position": reference })),
    )
    .unwrap();
    assert_eq!(at_reference["intrinsicName"], "string");
    // At the declaration (not a reference) the declared type.
    let declared = call(
        &session,
        "getTypeOfSymbolAtLocation",
        with(
            &base,
            json!({ "symbol": symbol["id"], "location": symbol["declarations"][0] }),
        ),
    )
    .unwrap();
    assert_eq!(declared["flags"], 134_217_728);
}

#[test]
fn type_to_string_takes_tsgos_format_flags() {
    let (session, base) = uses();
    let big = uses_symbol(&session, &base, "big", 0);
    let ty = call(
        &session,
        "getTypeOfSymbol",
        with(&base, json!({ "symbol": big["id"] })),
    )
    .unwrap();
    let text = |flags: u32| {
        call(
            &session,
            "typeToString",
            with(&base, json!({ "type": ty["id"], "flags": flags })),
        )
        .unwrap()
    };
    let one_line = "{ readonly a: 1; readonly b: \"two\"; readonly c: readonly [true]; readonly d: { readonly e: null; }; }";
    assert_eq!(text(0), one_line);
    assert_eq!(text(1), one_line);
    // MultilineObjectLiterals.
    assert_eq!(
        text(1 << 10),
        "{\n    readonly a: 1;\n    readonly b: \"two\";\n    readonly c: readonly [true];\n    readonly d: {\n        readonly e: null;\n    };\n}"
    );
}

#[test]
fn handles_the_snapshot_does_not_know_are_the_clients_errors() {
    let (session, base) = checker(FILES, "/tsconfig.json");
    let error =
        |method: &str, fields: Value| call(&session, method, with(&base, fields)).unwrap_err();
    assert_eq!(
        error("getTypeOfSymbol", json!({ "symbol": 0 })),
        "api: client error: empty symbol handle"
    );
    assert_eq!(
        error("getTypeOfSymbol", json!({ "symbol": 999_999 })),
        "api: client error: symbol handle 999999 not found in snapshot registry"
    );
    assert_eq!(
        error("getSymbolAtLocation", json!({ "location": "bad" })),
        "api: client error: invalid node handle \"bad\""
    );
    assert_eq!(
        error("typeToString", json!({ "type": 0 })),
        "api: client error: empty type handle"
    );
    assert_eq!(
        error("typeToString", json!({ "type": 999_999 })),
        "api: client error: type handle 999999 not found (no registry for project /tsconfig.json)"
    );
    call(&session, "getAnyType", base.clone()).unwrap();
    assert_eq!(
        error("typeToString", json!({ "type": 999_999 })),
        "api: client error: type handle 999999 not found in project registry"
    );
    assert_eq!(
        error("getSymbolOfSourceFile", json!({ "file": "src/nope.ts" })),
        "api: client error: source file not found: src/nope.ts"
    );
    assert_eq!(
        call(
            &session,
            "getAnyType",
            json!({ "snapshot": 99, "project": "/tsconfig.json" })
        )
        .unwrap_err(),
        "api: client error: snapshot 99 not found"
    );
}

#[path = "session_types.rs"]
mod types;

#[path = "session_signatures.rs"]
mod signatures;
