//! The type structure queries (P5-4b): a type's parts by its kind of type,
//! base types, type arguments, properties, index infos, apparent, awaited,
//! widened and non-nullable types, assignability and the node queries
//! (contextual types, context sensitivity, type nodes). Pinned to tsgo's
//! responses at 19dadef8 for the same file; the handles are each
//! implementation's own, so the tests compare what they identify.

use super::*;

const SOURCE: &str = r#"export interface Box<T> { value: T; }
export interface Thisless { a: number; }
export class Base<T = string> { base!: T; }
export class Derived<U extends object> extends Base<U> { value!: U; }
export type Plain<T> = { [K in keyof T]: T[K] };
export type Cond<T> = T extends string ? "s" : "n";
export type IA<T, K extends keyof T> = T[K];
export type TL<T extends string> = `a-${T}`;
export type Pair = [first: string, second?: number];
export let union: string | number = 1;
export let maybe: string | undefined;
export const idx: { [k: string]: number; readonly [n: number]: 1 } = {};
export const promise: Promise<number> = Promise.resolve(1);
export const arr: number[] = [];
export const fresh = "lit";
export const ctx: (a: string) => void = a => {};
declare function f(x: number): void;
f(1);
"#;

/// The project of [`SOURCE`] with the session and the base of its requests.
fn types() -> (Session, Value) {
    checker(
        &[
            (
                "/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true, "target": "es2020", "lib": ["es2020"] } }"#,
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

/// The error a request answers with in a batch, where a panic is the
/// request's error.
fn batch_error(session: &Session, base: &Value, method: &str, fields: Value) -> String {
    let batch = ask(
        session,
        base,
        "batchRequests",
        json!({ "requests": [{ "method": method, "params": with(base, fields) }] }),
    );
    batch["responses"][0]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("{method} answered {batch}"))
        .to_owned()
}

/// The declared type (`declared`) or the type of the symbol whose name
/// starts at `text`.
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

/// A type response's kind: its flags, object flags and intrinsic name or
/// literal value.
fn kind(ty: &Value) -> Value {
    json!([
        ty["flags"],
        ty["objectFlags"],
        ty.get("intrinsicName")
            .or(ty.get("value"))
            .unwrap_or(&Value::Null)
    ])
}

const STRING: [u64; 2] = [32, 0];
const NUMBER: [u64; 2] = [64, 0];

fn is(ty: &Value, flags: [u64; 2], name: &str) -> bool {
    kind(ty) == json!([flags[0], flags[1], name])
}

#[test]
fn a_type_property_request_takes_its_kind_of_type() {
    let (session, base) = types();
    let union = type_of(&session, &base, "union:", false);
    assert_eq!(union["flags"], 134_217_728);
    let types = ask(
        &session,
        &base,
        "getTypesOfType",
        json!({ "objectId": union["id"] }),
    );
    assert!(is(&types[0], STRING, "string") && is(&types[1], NUMBER, "number"));
    // tsgo's accessors panic for another kind of type.
    let error = |method: &str| {
        batch_error(
            &session,
            &base,
            method,
            json!({ "objectId": union["id"], "type": union["id"] }),
        )
    };
    assert_eq!(
        error("getTargetOfType"),
        "panic: Unhandled case in Type.Target"
    );
    assert_eq!(
        error("getFreshTypeOfType"),
        "panic: interface conversion: checker.TypeData is *checker.UnionType, not *checker.LiteralType"
    );
    assert_eq!(
        error("getObjectTypeOfType"),
        "panic: interface conversion: checker.TypeData is *checker.UnionType, not *checker.IndexedAccessType"
    );
    let nil = "panic: runtime error: invalid memory address or nil pointer dereference";
    assert_eq!(error("getTypeParametersOfType"), nil);
    assert_eq!(error("getTypeArguments"), nil);
    // A union has no base types; it is its own reduced type.
    assert_eq!(
        ask(
            &session,
            &base,
            "getBaseTypes",
            json!({ "type": union["id"] })
        ),
        json!([])
    );
    assert_eq!(
        ask(
            &session,
            &base,
            "getReducedType",
            json!({ "objectId": union["id"] })
        ),
        union
    );
}

#[test]
fn a_literal_type_and_its_fresh_and_regular_types() {
    let (session, base) = types();
    let fresh = type_of(&session, &base, "fresh =", false);
    assert_eq!(kind(&fresh), json!([1024, 0, "lit"]));
    assert_eq!(fresh["freshType"], fresh["id"]);
    let property = |method: &str| {
        ask(
            &session,
            &base,
            method,
            json!({ "objectId": fresh["id"], "type": fresh["id"] }),
        )
    };
    assert_eq!(property("getFreshTypeOfType"), fresh);
    let regular = property("getRegularTypeOfType");
    assert_eq!(regular["id"], fresh["regularType"]);
    assert_eq!(regular["freshType"], fresh["id"]);
    assert!(is(&property("getBaseTypeOfLiteralType"), STRING, "string"));
    // A fresh literal type is its own widened type.
    assert_eq!(property("getWidenedType"), fresh);
}

#[test]
fn class_and_interface_type_parameters_and_base_types() {
    let (session, base) = types();
    let derived = type_of(&session, &base, "Derived<U", true);
    assert_eq!(derived["objectFlags"], 5);
    let property = |method: &str, ty: &Value| {
        ask(
            &session,
            &base,
            method,
            json!({ "objectId": ty["id"], "type": ty["id"] }),
        )
    };
    let parameters = property("getTypeParametersOfType", &derived);
    assert_eq!(parameters.as_array().unwrap().len(), 1);
    assert_eq!(parameters[0]["id"], derived["typeParameters"][0]);
    assert_eq!(
        property("getOuterTypeParametersOfType", &derived),
        json!([])
    );
    assert_eq!(
        property("getLocalTypeParametersOfType", &derived),
        parameters
    );
    let this = property("getThisTypeOfType", &derived);
    assert_eq!(this["id"], derived["thisType"]);
    assert_eq!(this["isThisType"], true);
    // The base type is a reference to Base, with Derived's type parameter.
    let bases = property("getBaseTypes", &derived);
    assert_eq!(bases[0]["objectFlags"], 536_870_916);
    let base_class = type_of(&session, &base, "Base<T", true);
    assert_eq!(property("getTargetOfType", &bases[0]), base_class);
    assert_eq!(
        property("getTypeArguments", &bases[0]),
        json!([parameters[0]])
    );
    assert!(is(
        &property("getConstraintOfTypeParameter", &parameters[0]),
        [131_072, 0],
        "object"
    ));
    assert_eq!(
        property("getDefaultFromTypeParameter", &parameters[0]),
        Value::Null
    );
    let base_parameter = &property("getTypeParametersOfType", &base_class)[0];
    assert!(is(
        &property("getDefaultFromTypeParameter", base_parameter),
        STRING,
        "string"
    ));
    // A thisless interface has neither type parameters nor a this type,
    // and is no reference.
    let thisless = type_of(&session, &base, "Thisless", true);
    assert_eq!(thisless["objectFlags"], 2);
    assert_eq!(property("getTypeParametersOfType", &thisless), json!([]));
    assert_eq!(property("getThisTypeOfType", &thisless), Value::Null);
    assert_eq!(
        batch_error(
            &session,
            &base,
            "getTypeArguments",
            json!({ "type": thisless["id"] })
        ),
        "panic: runtime error: invalid memory address or nil pointer dereference"
    );
}

#[test]
fn the_parts_of_structured_types() {
    let (session, base) = types();
    let property = |method: &str, ty: &Value| {
        ask(
            &session,
            &base,
            method,
            json!({ "objectId": ty["id"], "type": ty["id"] }),
        )
    };
    // An indexed access type.
    let access = type_of(&session, &base, "IA<T", true);
    assert_eq!(
        property("getObjectTypeOfType", &access)["id"],
        access["objectType"]
    );
    assert_eq!(
        property("getIndexTypeOfType", &access)["id"],
        access["indexType"]
    );
    // A conditional type and its branches.
    let conditional = type_of(&session, &base, "Cond<T>", true);
    assert_eq!(
        property("getCheckTypeOfType", &conditional)["id"],
        conditional["checkType"]
    );
    assert!(is(
        &property("getExtendsTypeOfType", &conditional),
        STRING,
        "string"
    ));
    assert_eq!(
        kind(&property("getTrueTypeOfConditionalType", &conditional)),
        json!([1024, 0, "s"])
    );
    assert_eq!(
        kind(&property("getFalseTypeOfConditionalType", &conditional)),
        json!([1024, 0, "n"])
    );
    // A mapped type's parts, which its response names.
    let mapped = type_of(&session, &base, "Plain<T>", true);
    assert_eq!(mapped["objectFlags"], 32);
    for (method, field) in [
        ("getTypeParameterOfMappedType", "typeParameter"),
        ("getConstraintTypeOfMappedType", "constraintType"),
        ("getTemplateTypeOfMappedType", "templateType"),
    ] {
        assert_eq!(property(method, &mapped)["id"], mapped[field]);
    }
    assert_eq!(property("getNameTypeOfMappedType", &mapped), Value::Null);
    assert_eq!(
        property("getConstraintTypeOfMappedType", &mapped)["flags"],
        2_097_152
    );
    // A template literal type's types.
    let template = type_of(&session, &base, "TL<T", true);
    assert_eq!(template["texts"], json!(["a-", ""]));
    assert_eq!(property("getTypesOfType", &template)[0]["flags"], 524_288);
    // A tuple: its target, its type arguments, the target's type
    // parameters and the labels' declarations.
    let pair = type_of(&session, &base, "Pair =", true);
    assert_eq!(pair["isTupleType"], true);
    let target = property("getTargetOfType", &pair);
    assert_eq!(target["id"], pair["target"]);
    assert_eq!(target["objectFlags"], 12);
    assert_eq!(target["elementFlags"], json!([1, 2]));
    assert_eq!(
        target["labeledElementDeclarations"],
        json!(["131.203./src/main.ts", "134.203./src/main.ts"])
    );
    let arguments = property("getTypeArguments", &pair);
    assert!(is(&arguments[0], STRING, "string"));
    assert_eq!(arguments[1]["flags"], 134_217_728);
    assert_eq!(
        property("getTypeParametersOfType", &target)
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(property("isArrayType", &pair), false);
    assert_eq!(property("isArrayLikeType", &pair), true);
}

#[test]
fn properties_and_index_infos() {
    let (session, base) = types();
    let object = type_of(&session, &base, "idx:", false);
    assert_eq!(
        ask(
            &session,
            &base,
            "getPropertiesOfType",
            json!({ "type": object["id"] })
        ),
        json!([])
    );
    let infos = ask(
        &session,
        &base,
        "getIndexInfosOfType",
        json!({ "type": object["id"] }),
    );
    assert!(is(&infos[0]["keyType"], STRING, "string"));
    assert!(is(&infos[0]["valueType"], NUMBER, "number"));
    assert_eq!(infos[0]["isReadonly"], false);
    assert_eq!(infos[0]["declaration"], "170.182./src/main.ts");
    assert!(is(&infos[1]["keyType"], NUMBER, "number"));
    assert_eq!(kind(&infos[1]["valueType"]), json!([2048, 0, 1]));
    assert_eq!(infos[1]["isReadonly"], true);
    assert_eq!(infos[1]["declaration"], "176.182./src/main.ts");
    let number_info = ask(
        &session,
        &base,
        "getIndexInfoOfType",
        json!({ "type": object["id"], "kind": 1 }),
    );
    assert_eq!(number_info, infos[1]);
    assert_eq!(
        call(
            &session,
            "getIndexInfoOfType",
            with(&base, json!({ "type": object["id"], "kind": 2 }))
        )
        .unwrap_err(),
        "api: client error: invalid index kind 2"
    );
    // A property by its name.
    let boxed = type_of(&session, &base, "Box<T>", true);
    let value = ask(
        &session,
        &base,
        "getPropertyOfType",
        json!({ "type": boxed["id"], "name": "value" }),
    );
    assert_eq!(
        shape(&value),
        json!({
            "id": "*", "project": "/tsconfig.json", "name": "value", "flags": 33_554_436,
            "checkFlags": 1, "declarations": ["11.172./src/main.ts"],
            "valueDeclaration": "11.172./src/main.ts", "parent": "*",
        })
    );
    assert_eq!(
        ask(
            &session,
            &base,
            "getTypeOfPropertyOfType",
            json!({ "type": boxed["id"], "name": "value" })
        )["id"],
        boxed["typeParameters"][0]
    );
    assert_eq!(
        ask(
            &session,
            &base,
            "getPropertyOfType",
            json!({ "type": boxed["id"], "name": "missing" })
        ),
        Value::Null
    );
    // A union's properties keep the order of its first type's: tsgo does
    // not sort them.
    let union = type_of(&session, &base, "union:", false);
    let properties = ask(
        &session,
        &base,
        "getPropertiesOfType",
        json!({ "type": union["id"] }),
    );
    let names = properties
        .as_array()
        .unwrap()
        .iter()
        .map(|property| property["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["toString", "valueOf", "toLocaleString"]);
}

#[test]
fn apparent_awaited_and_non_nullable_types() {
    let (session, base) = types();
    let string = ask(&session, &base, "getStringType", json!({}));
    let apparent = ask(
        &session,
        &base,
        "getApparentType",
        json!({ "objectId": string["id"] }),
    );
    assert_eq!(kind(&apparent), json!([1_048_576, 2, null]));
    // A function type's apparent properties are those of
    // CallableFunction (sorted by declaration).
    let function = type_of(&session, &base, "ctx:", false);
    let properties = ask(
        &session,
        &base,
        "getApparentPropertiesOfType",
        json!({ "objectId": function["id"] }),
    );
    let names = properties
        .as_array()
        .unwrap()
        .iter()
        .map(|property| property["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names[..9],
        [
            "toString",
            "prototype",
            "length",
            "arguments",
            "caller",
            "apply",
            "call",
            "bind",
            "name"
        ]
    );
    // The late-bound `[Symbol.hasInstance]` is written as tsgo writes it.
    assert!(names[9].starts_with("__@hasInstance@"), "{names:?}");
    let promise = type_of(&session, &base, "promise:", false);
    assert!(is(
        &ask(
            &session,
            &base,
            "getAwaitedType",
            json!({ "type": promise["id"] })
        ),
        NUMBER,
        "number"
    ));
    let maybe = type_of(&session, &base, "maybe:", false);
    assert!(is(
        &ask(
            &session,
            &base,
            "getNonNullableType",
            json!({ "objectId": maybe["id"] })
        ),
        STRING,
        "string"
    ));
}

#[test]
fn arrays_and_assignability() {
    let (session, base) = types();
    let array = type_of(&session, &base, "arr:", false);
    let union = type_of(&session, &base, "union:", false);
    let check =
        |method: &str, ty: &Value| ask(&session, &base, method, json!({ "type": ty["id"] }));
    assert_eq!(check("isArrayType", &array), true);
    assert_eq!(check("isArrayLikeType", &array), true);
    assert_eq!(check("isArrayType", &union), false);
    let number = ask(&session, &base, "getNumberType", json!({}));
    let assignable = |source: &Value, target: &Value| {
        ask(
            &session,
            &base,
            "isTypeAssignableTo",
            json!({ "source": source["id"], "target": target["id"] }),
        )
    };
    assert_eq!(assignable(&number, &union), true);
    assert_eq!(assignable(&union, &number), false);
    assert_eq!(
        call(
            &session,
            "isTypeAssignableTo",
            with(&base, json!({ "source": number["id"], "target": 999_999 }))
        )
        .unwrap_err(),
        "api: client error: type handle 999999 not found in project registry"
    );
}

#[test]
fn contextual_types_and_type_nodes() {
    let (session, base) = types();
    let node = |fields: Value, method: &str| ask(&session, &base, method, fields);
    // The arrow function `a => {}` takes the declared function type.
    let function = type_of(&session, &base, "ctx:", false);
    let arrow = json!({ "location": "236.220./src/main.ts" });
    assert_eq!(
        node(arrow.clone(), "getContextualType")["id"],
        function["id"]
    );
    assert_eq!(node(arrow, "isContextSensitive"), true);
    // `f(1)`'s argument.
    assert!(is(
        &node(
            json!({ "location": "253.214./src/main.ts", "index": 0 }),
            "getContextualTypeForArgument"
        ),
        NUMBER,
        "number"
    ));
    // The type node `string | number`.
    let union = type_of(&session, &base, "union:", false);
    assert_eq!(
        node(
            json!({ "location": "145.193./src/main.ts" }),
            "getTypeFromTypeNode"
        ),
        union
    );
}
