//! The signature queries, name lookups and node builder (P5-4c): signatures
//! of types, calls and declarations with their parts, type predicates,
//! `resolveName` and `getSymbolsInScope` at nodes and positions, the
//! well-known symbols and signature, and the encoded nodes of
//! `typeToTypeNode` and `signatureToSignatureDeclaration`. Pinned to tsgo's
//! responses at 19dadef8 for the same file; the handles are each
//! implementation's own, so the tests compare what they identify.

use super::*;

const SOURCE: &str = r#"export function over(x: string): string;
export function over(x: number): number;
export function over(x: any) { return x; }
export function rest(a: number, ...more: string[]): void {}
export function generic<T extends object>(value: T): T { return value; }
export function isText(v: unknown): v is string { return typeof v === "string"; }
export function assertsThis(this: Fluent): asserts this {}
export class Fluent { name = ""; isFluent(): this is Fluent { return true; } key(k: keyof this) { return k; } }
export abstract class Shape { constructor(public readonly id: number) {} }
export const arrow = (a: string, b?: number) => a;
export interface Indexed { [key: string]: number; }
const local = 1;
function scope(param: number) {
  const inner = param + local;
  return inner;
}
over("a");
import("./main.js");
"#;

/// The project of [`SOURCE`] with the session and the base of its requests.
fn signatures() -> (Session, Value) {
    checker(
        &[
            (
                "/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true, "target": "es2020", "lib": ["es2020"], "module": "esnext" } }"#,
            ),
            ("/src/main.ts", SOURCE),
        ],
        "/tsconfig.json",
    )
}

/// A request's response.
fn ask(session: &Session, base: &Value, method: &str, fields: Value) -> Value {
    call(session, method, with(base, fields)).unwrap_or_else(|error| panic!("{method}: {error}"))
}

/// The type of the symbol whose name starts at `text` (its declared type
/// with `declared`).
fn type_of(session: &Session, base: &Value, text: &str, declared: bool) -> Value {
    let at = SOURCE.find(text).unwrap_or_else(|| panic!("{text:?}"));
    let symbol = ask(
        session,
        base,
        "getSymbolAtPosition",
        json!({ "file": "/src/main.ts", "position": at }),
    );
    let method = if declared {
        "getDeclaredTypeOfSymbol"
    } else {
        "getTypeOfSymbol"
    };
    ask(session, base, method, json!({ "symbol": symbol["id"] }))
}

/// The call (`kind` 0) or construct signatures of the type at `text`.
fn signatures_of(session: &Session, base: &Value, text: &str, kind: i32) -> Vec<Value> {
    let ty = type_of(session, base, text, false);
    let signatures = ask(
        session,
        base,
        "getSignaturesOfType",
        json!({ "type": ty["id"], "kind": kind }),
    );
    signatures.as_array().unwrap().clone()
}

fn type_text(session: &Session, base: &Value, ty: &Value) -> String {
    ask(session, base, "typeToString", json!({ "type": ty["id"] }))
        .as_str()
        .unwrap()
        .to_owned()
}

/// The names of the symbols of a response.
fn names(symbols: &Value) -> Vec<&str> {
    symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap())
        .collect()
}

#[test]
fn signatures_of_types_and_their_parts() {
    let (session, base) = signatures();
    let ask = |method: &str, fields: Value| ask(&session, &base, method, fields);
    let text = |ty: &Value| type_text(&session, &base, ty);
    // The overloads, each with its declaration and parameters.
    let over = signatures_of(&session, &base, "over(x: any", 0);
    assert_eq!(over.len(), 2);
    assert_eq!(over[0]["flags"], 0);
    assert_eq!(over[0]["declaration"], "3.263./src/main.ts");
    assert_eq!(over[1]["declaration"], "12.263./src/main.ts");
    let returns: Vec<String> = over
        .iter()
        .map(|signature| {
            let parameters = ask(
                "getParametersOfSignature",
                json!({ "objectId": signature["id"] }),
            );
            assert_eq!(names(&parameters), ["x"]);
            text(&ask(
                "getReturnTypeOfSignature",
                json!({ "objectId": signature["id"] }),
            ))
        })
        .collect();
    assert_eq!(returns, ["string", "number"]);
    assert_eq!(
        ask("getTargetOfSignature", json!({ "objectId": over[0]["id"] })),
        Value::Null
    );
    // A rest parameter: HasRestParameter, its element type and the types at
    // positions past it.
    let rest = &signatures_of(&session, &base, "rest(", 0)[0];
    assert_eq!(rest["flags"], 1);
    let signature = json!({ "signature": rest["id"] });
    assert_eq!(
        text(&ask("getRestTypeOfSignature", signature.clone())),
        "string"
    );
    assert_eq!(
        text(&ask(
            "getRestTypeOfSignature",
            json!({ "signature": over[0]["id"] })
        )),
        "any"
    );
    let at = |index: i32| {
        text(&ask(
            "getParameterType",
            json!({ "signature": rest["id"], "index": index }),
        ))
    };
    assert_eq!([at(0), at(1), at(5)], ["number", "string", "string"]);
    assert_eq!(
        call(
            &session,
            "getParameterType",
            with(&base, json!({ "signature": rest["id"], "index": -1 }))
        )
        .unwrap_err(),
        "api: client error: invalid parameter index"
    );
    // Type parameters; an abstract class's construct signature (tsgo's
    // Construct | Abstract) and no call signature.
    let generic = &signatures_of(&session, &base, "generic<", 0)[0];
    let parameters = ask(
        "getTypeParametersOfSignature",
        json!({ "objectId": generic["id"] }),
    );
    assert_eq!(text(&parameters[0]), "T");
    assert_eq!(generic["typeParameters"][0], parameters[0]["id"]);
    let shape = &signatures_of(&session, &base, "Shape {", 1)[0];
    assert_eq!(shape["flags"], 12);
    assert_eq!(shape["declaration"], "137.177./src/main.ts");
    assert_eq!(
        names(&ask(
            "getParametersOfSignature",
            json!({ "objectId": shape["id"] })
        )),
        ["id"]
    );
    assert!(signatures_of(&session, &base, "Shape {", 0).is_empty());
    // The `this` parameter.
    let arrow = &signatures_of(&session, &base, "arrow =", 0)[0];
    assert_eq!(
        ask(
            "getThisParameterOfSignature",
            json!({ "objectId": arrow["id"] })
        ),
        Value::Null
    );
    let asserts = &signatures_of(&session, &base, "assertsThis(", 0)[0];
    let this = ask(
        "getThisParameterOfSignature",
        json!({ "objectId": asserts["id"] }),
    );
    assert_eq!(this["name"], "this");
    assert_eq!(this["declarations"], json!(["92.170./src/main.ts"]));
    assert_eq!(asserts["thisParameter"], this["id"]);
}

#[test]
fn type_predicates_of_signatures() {
    let (session, base) = signatures();
    let predicate = |signature: &Value| {
        ask(
            &session,
            &base,
            "getTypePredicateOfSignature",
            json!({ "signature": signature["id"] }),
        )
    };
    let is_text = predicate(&signatures_of(&session, &base, "isText(", 0)[0]);
    assert_eq!(
        (
            &is_text["kind"],
            &is_text["parameterIndex"],
            &is_text["parameterName"]
        ),
        (&json!(1), &json!(0), &json!("v"))
    );
    assert_eq!(type_text(&session, &base, &is_text["type"]), "string");
    // tsgo numbers a `this` predicate's parameter 0.
    let asserts = predicate(&signatures_of(&session, &base, "assertsThis(", 0)[0]);
    assert_eq!(asserts, json!({ "kind": 2, "parameterIndex": 0 }));
    let fluent = type_of(&session, &base, "Fluent {", true);
    let properties = ask(
        &session,
        &base,
        "getPropertiesOfType",
        json!({ "type": fluent["id"] }),
    );
    let method = |name: &str| {
        let property = properties
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["name"] == name)
            .unwrap();
        let ty = ask(
            &session,
            &base,
            "getTypeOfSymbol",
            json!({ "symbol": property["id"] }),
        );
        ask(
            &session,
            &base,
            "getSignaturesOfType",
            json!({ "type": ty["id"], "kind": 0 }),
        )[0]
        .clone()
    };
    let is_fluent = predicate(&method("isFluent"));
    assert_eq!(
        (&is_fluent["kind"], &is_fluent["parameterIndex"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(type_text(&session, &base, &is_fluent["type"]), "Fluent");
    // `keyof this` at a position.
    let key = method("key");
    for request in ["getParameterType", "getTypeParameterAtPosition"] {
        let ty = ask(
            &session,
            &base,
            request,
            json!({ "signature": key["id"], "index": 0 }),
        );
        assert_eq!(type_text(&session, &base, &ty), "keyof Fluent");
    }
    let over = &signatures_of(&session, &base, "over(x: any", 0)[0];
    assert_eq!(predicate(over), Value::Null);
}

#[test]
fn resolved_and_declaration_signatures() {
    let (session, base) = signatures();
    let node = |method: &str, location: &str| {
        ask(&session, &base, method, json!({ "location": location }))
    };
    // `over("a")` resolves to the first overload.
    let resolved = node("getResolvedSignature", "202.214./src/main.ts");
    assert_eq!(resolved["declaration"], "3.263./src/main.ts");
    let first = &signatures_of(&session, &base, "over(x: any", 0)[0];
    assert_eq!(resolved["id"], first["id"]);
    // An import call is untyped: the any signature, which has no declaration.
    let untyped = node("getResolvedSignature", "207.214./src/main.ts");
    assert_eq!(untyped["flags"], 0);
    assert!(untyped.get("declaration").is_none() && untyped.get("parameters").is_none());
    // An index signature's declaration has a signature too.
    let index = node("getSignatureFromDeclaration", "170.182./src/main.ts");
    assert_eq!(index["declaration"], "170.182./src/main.ts");
    let parameters = ask(
        &session,
        &base,
        "getParametersOfSignature",
        json!({ "objectId": index["id"] }),
    );
    assert_eq!(names(&parameters), ["key"]);
    let returns = ask(
        &session,
        &base,
        "getReturnTypeOfSignature",
        json!({ "objectId": index["id"] }),
    );
    assert_eq!(type_text(&session, &base, &returns), "number");
    let constructor = node("getSignatureFromDeclaration", "137.177./src/main.ts");
    assert_eq!(constructor["flags"], 12);
    // Other nodes panic as tsgo's do.
    let batch_error = |method: &str, location: &str| {
        let batch = ask(
            &session,
            &base,
            "batchRequests",
            json!({ "requests": [{
                "method": method,
                "params": with(&base, json!({ "location": location })),
            }] }),
        );
        batch["responses"][0]["error"].clone()
    };
    assert_eq!(
        batch_error("getSignatureFromDeclaration", "200.79./src/main.ts"),
        "panic: runtime error: invalid memory address or nil pointer dereference"
    );
    assert_eq!(
        batch_error("getResolvedSignature", "193.261./src/main.ts"),
        "panic: Unhandled case in resolveSignature"
    );
}

#[test]
fn names_resolve_at_nodes_and_positions() {
    let (session, base) = signatures();
    let resolve = |fields: Value| {
        let symbol = ask(&session, &base, "resolveName", fields);
        (!symbol.is_null()).then(|| (symbol["name"].clone(), symbol["declarations"][0].clone()))
    };
    // At `return inner`'s identifier.
    let at_inner = |name: &str, meaning: u32| {
        resolve(json!({ "name": name, "location": "200.79./src/main.ts", "meaning": meaning }))
    };
    assert_eq!(
        at_inner("inner", VALUE),
        Some((json!("inner"), json!("193.261./src/main.ts")))
    );
    assert_eq!(
        at_inner("param", VALUE),
        Some((json!("param"), json!("185.170./src/main.ts")))
    );
    assert_eq!(
        at_inner("local", VALUE),
        Some((json!("local"), json!("179.261./src/main.ts")))
    );
    assert!(at_inner("Array", TYPE).is_some());
    assert_eq!(at_inner("missing", VALUE), None);
    assert_eq!(at_inner("__proto__", VALUE), None);
    // By position: at `(` the lookup starts at the token, which is none of
    // the function's parameters, so they are out of scope there.
    let paren = SOURCE.find("(param: number)").unwrap();
    let at = |name: &str, position: usize| {
        resolve(
            json!({ "name": name, "file": "/src/main.ts", "position": position, "meaning": VALUE }),
        )
    };
    assert_eq!(at("param", paren), None);
    assert!(at("local", paren).is_some());
    assert_eq!(
        at("param", paren + 1),
        Some((json!("param"), json!("185.170./src/main.ts")))
    );
    // Without a location only the globals are in scope.
    let global = |name: &str, exclude: bool| {
        resolve(json!({ "name": name, "meaning": VALUE, "excludeGlobals": exclude }))
    };
    assert!(global("Array", false).is_some());
    assert_eq!(global("Array", true), None);
    assert_eq!(global("local", false), None);
}

/// tsgo's `SymbolFlags.Value` and `SymbolFlags.Type`.
const VALUE: u32 = 111_551;
const TYPE: u32 = 788_968;

#[test]
fn the_symbols_in_scope() {
    let (session, base) = signatures();
    let scope = |fields: Value| ask(&session, &base, "getSymbolsInScope", fields);
    let values = scope(json!({ "location": "200.79./src/main.ts", "meaning": VALUE }));
    let value_names = names(&values);
    // tsgo's order is its map's; the set is the same.
    assert_eq!(value_names.len(), 70);
    for name in [
        "inner",
        "param",
        "local",
        "scope",
        "over",
        "Fluent",
        "arguments",
        "Array",
        "globalThis",
        "undefined",
    ] {
        assert!(value_names.contains(&name), "{name}");
    }
    assert!(!value_names.contains(&"this") && !value_names.contains(&"Indexed"));
    let types = scope(json!({ "location": "200.79./src/main.ts", "meaning": TYPE }));
    assert_eq!(types.as_array().unwrap().len(), 172);
    // A function's type parameters, by position.
    let at = SOURCE.find("value: T)").unwrap();
    let generic = scope(json!({ "file": "/src/main.ts", "position": at, "meaning": TYPE }));
    let generic_names = names(&generic);
    assert_eq!(generic_names.len(), 173);
    assert!(generic_names.contains(&"T"));
    assert_eq!(
        call(
            &session,
            "getSymbolsInScope",
            with(&base, json!({ "meaning": VALUE }))
        )
        .unwrap_err(),
        "api: client error: getSymbolsInScope requires a location"
    );
}

#[test]
fn well_known_symbols_and_signature() {
    let (session, base) = signatures();
    let symbols = ask(&session, &base, "getWellKnownSymbols", json!({}));
    let ids: Vec<&Value> = ["unknown", "undefined", "arguments"]
        .iter()
        .map(|name| &symbols[name])
        .collect();
    assert!(ids.iter().all(|id| id.as_u64().is_some_and(|id| id > 0)));
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    let unknown = ask(&session, &base, "getWellKnownSignatures", json!({}))["unknown"].clone();
    let returns = ask(
        &session,
        &base,
        "getReturnTypeOfSignature",
        json!({ "objectId": unknown }),
    );
    assert_eq!(type_text(&session, &base, &returns), "any");
    assert_eq!(
        ask(
            &session,
            &base,
            "getParametersOfSignature",
            json!({ "objectId": unknown })
        ),
        json!([])
    );
    let error =
        |method: &str, fields: Value| call(&session, method, with(&base, fields)).unwrap_err();
    assert_eq!(
        error("getReturnTypeOfSignature", json!({ "objectId": 0 })),
        "api: client error: empty signature handle"
    );
    assert_eq!(
        error("getRestTypeOfSignature", json!({ "signature": 99999 })),
        "api: client error: signature handle 99999 not found in project registry"
    );
}

#[test]
fn node_builder_results_are_tsgos_encodings() {
    let (session, base) = signatures();
    let data = |method: &str, fields: Value| {
        ask(&session, &base, method, fields)["data"]
            .as_str()
            .unwrap_or_else(|| panic!("{method} encodes a node"))
            .to_owned()
    };
    // `<T extends object>(value: T) => T`.
    let generic = type_of(&session, &base, "generic<", false);
    assert_eq!(
        data("typeToTypeNode", json!({ "type": generic["id"] })),
        "AAAACQAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAGAAAABoAAAAaAAAAGgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAQAAAAYAAAAGAAAABwAAAAcAAAAIAAAAVHZhbHVlVFQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAuQAAAP//////////AAAAAAAAAAAHAAAAEAAAAP///////////////wYAAAABAAAAAQAAAAAAAACpAAAA//////////8AAAAAAgAAAAYAAAAQAAAATwAAAP//////////BQAAAAMAAAAAAABAEAAAAJcAAAD//////////wAAAAADAAAAAAAAAAAAAAD///////////////8LAAAAAQAAAAEAAAAAAAAAqgAAAP//////////AAAAAAYAAAAUAAAAEAAAAE8AAAD//////////wkAAAAHAAAAAgAAQAAAAAC4AAAA//////////8AAAAABwAAAAEAAAAQAAAATwAAAP//////////AAAAAAkAAAAEAABAEAAAALgAAAD//////////wAAAAABAAAAAQAAABAAAABPAAAA//////////8AAAAACwAAAAYAAEAQAAAA"
    );
    // `(a: string, b?: number | undefined) => string`, without truncation.
    let arrow = type_of(&session, &base, "arrow =", false);
    assert_eq!(
        data("typeToTypeNode", json!({ "type": arrow["id"], "flags": 1 })),
        "AAAACQAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAFAAAABSAAAAUgAAAFIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAQAAAAIAAABhYgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAC5AAAA//////////8AAAAAAAAAAAYAAAAQAAAA////////////////DQAAAAEAAAACAAAAAAAAAKoAAAD//////////wYAAAACAAAAFAAAABAAAABPAAAA//////////8FAAAAAwAAAAAAAEAAAAAAmgAAAP//////////AAAAAAMAAAAAAAAAEAAAAKoAAAD//////////wAAAAACAAAAHAAAABAAAABPAAAA//////////8IAAAABgAAAAIAAEAAAAAAOQAAAP//////////CQAAAAYAAAAAAAAAEAAAAMEAAAD//////////wAAAAAGAAAAAQAAABAAAAD///////////////8AAAAACQAAAAIAAAAAAAAAlgAAAP//////////DAAAAAoAAAAAAAAAEAAAAJ0AAAD//////////wAAAAAKAAAAAAAAABAAAACaAAAA//////////8AAAAAAQAAAAAAAAAQAAAA"
    );
    // A class's declared type is its name.
    let fluent = type_of(&session, &base, "Fluent {", true);
    let fluent_node = "AAAACQAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAEgAAABOAAAATgAAAE4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGAAAARmx1ZW50AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAALgAAAD//////////wAAAAAAAAAAAQAAABAAAABPAAAA//////////8AAAAAAQAAAAAAAEAQAAAA";
    assert_eq!(
        data("typeToTypeNode", json!({ "type": fluent["id"] })),
        fluent_node
    );
    // An arrow function declaration has no `=>` token (tsgo's factory).
    let over = &signatures_of(&session, &base, "over(x: any", 0)[0];
    assert_eq!(
        data(
            "signatureToSignatureDeclaration",
            json!({ "signature": over["id"], "kind": 220 })
        ),
        "AAAACQAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAEgAAABJAAAASQAAAEkAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAeAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADcAAAA//////////8AAAAAAAAAACwAAAAQAAAA////////////////BgAAAAEAAAABAAAAAAAAAKoAAAD//////////wAAAAACAAAAFAAAABAAAABPAAAA//////////8FAAAAAwAAAAAAAEAAAAAAmgAAAP//////////AAAAAAMAAAAAAAAAEAAAAJoAAAD//////////wcAAAABAAAAAAAAABAAAADyAAAA//////////8AAAAAAQAAAAEAAAAQAAAA////////////////AAAAAAcAAAAAAAAAAAAAAA=="
    );
    // A method signature with an empty name and a rest parameter.
    let rest = &signatures_of(&session, &base, "rest(", 0)[0];
    assert_eq!(
        data(
            "signatureToSignatureDeclaration",
            json!({ "signature": rest["id"], "kind": 174 })
        ),
        "AAAACQAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAFgAAABdAAAAXQAAAF0AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAABAAAABQAAAGFtb3JlAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAK4AAAD//////////wAAAAAAAAAAMgAAABAAAABPAAAA//////////8DAAAAAQAAAAAAAEAQAAAA////////////////DAAAAAEAAAACAAAAAAAAAKoAAAD//////////wcAAAADAAAAFAAAABAAAABPAAAA//////////8GAAAABAAAAAIAAEAAAAAAlgAAAP//////////AAAAAAQAAAAAAAAAEAAAAKoAAAD//////////wAAAAADAAAAFgAAABAAAAAZAAAA//////////8JAAAABwAAAAAAAAAQAAAATwAAAP//////////CgAAAAcAAAAEAABAAAAAAL0AAAD//////////wAAAAAHAAAAAQAAABAAAACaAAAA//////////8AAAAACgAAAAAAAAAQAAAAcwAAAP//////////AAAAAAEAAAAAAAAAEAAAAA=="
    );
    // A kind tsgo's helper does not handle panics, as tsgo's API answers.
    let batch = ask(
        &session,
        &base,
        "batchRequests",
        json!({ "requests": [{
            "method": "signatureToSignatureDeclaration",
            "params": with(&base, json!({ "signature": rest["id"], "kind": 79 })),
        }] }),
    );
    assert_eq!(
        batch["responses"][0]["error"],
        "panic: Unhandled kind in signatureToSignatureDeclarationHelper"
    );
    // The checker still answers after the panic.
    assert_eq!(
        data("typeToTypeNode", json!({ "type": fluent["id"] })),
        fluent_node
    );
}
