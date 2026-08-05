use super::FlowType;
use crate::state::test_support::with_program_state;
use crate::CompilerOptions;

#[test]
fn flow_type_accessors() {
    let ty = tsc_types::TypeId(7);
    assert_eq!(FlowType::Type(ty).get_type(), ty);
    assert_eq!(FlowType::Incomplete(ty).get_type(), ty);
    assert!(!FlowType::Type(ty).is_incomplete());
    assert!(FlowType::Incomplete(ty).is_incomplete());
}

#[test]
fn class_property_implicit_any_prints_the_declaration_name() {
    // symbolToString prints the declaration name (`#p`), never
    // the `__#<id>@` mangling (56202/56218; 6.6 review C1;
    // oracle-pinned vs vendored tsc 6.0.3 noLib — the
    // no-assignment auto face; the `= []` autoArray flavor needs
    // the Array global and rides the lib-backed band).
    let text = "class C { #p; constructor() { } }\n";
    assert_eq!(checked_rows(text), [(7008, 10, 2)]);
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let message = &state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error)
            .expect("7008 row")
            .message;
        assert!(
            message.text.contains("'#p'"),
            "declaration-name display: {}",
            message.text
        );
    });
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

// ---- class-property flow init (6.6e; rows oracle-pinned vs
// vendored tsc 6.0.3 noLib per shape, 2026-07-19) ----

#[test]
fn constructor_assignments_type_auto_properties() {
    // getFlowTypeInConstructor: the this-rooted synthetic query
    // over returnFlowNode infers `x: number` — the misuse row is
    // the proof the inference (not any) landed.
    assert_eq!(
        checked_rows(
            "class C { x; constructor() { this.x = 1; } }\ndeclare const c: C;\nconst s: string = c.x;\n"
        ),
        [(2322, 71, 1)]
    );
    // No assignment reaches the return flow — all-nullable answer
    // falls through to the widening tail's 7008.
    assert_eq!(
        checked_rows("class C { x; constructor() { } }\n"),
        [(7008, 10, 1)]
    );
    // Conditional assignments union (number | string here — the
    // misuse row proves the JOIN, not an any/单-type collapse).
    assert_eq!(
        checked_rows(
            "class C { x; constructor(b: boolean) { if (b) this.x = 1; else this.x = \"s\"; const v = this.x; const n: boolean = v; } }\n"
        ),
        [(2322, 101, 1)]
    );
}

#[test]
fn in_constructor_this_property_reads_flow_narrow() {
    // The access.rs face (75319→75347): a this-property READ in
    // the constructor routes through getFlowTypeOfProperty with
    // the REAL access node as reference.
    assert_eq!(
        checked_rows("class C { x; constructor() { this.x = 1; const y: string = this.x; } }\n"),
        [(2322, 47, 1)]
    );
}

#[test]
fn ungated_faces_stay_clean_on_oracle_clean_shapes() {
    // Two faces the 6.6f gate retirement UNMASKED in canaries —
    // pinned oracle-clean (vendored tsc noLib, 2026-07-19):
    // late-bound unique-literal computed key indexing…
    assert_eq!(
        checked_rows("const a = \"a\";\ntype A = { [a]: number };\ndeclare const c: A;\nc[a];\n"),
        []
    );
    // …and for-in over an optional chain narrowing the chain's
    // links in the body (tsc #51941).
    assert_eq!(
        checked_rows(
            "type R = { [k: string]: T5 };\ntype T5 = { main?: { childs: R } };\nfunction f50(obj: T5) {\n  for (const key in obj.main?.childs) {\n    if (obj.main.childs[key] === obj) { return obj; }\n  }\n  return null;\n}\n"
        ),
        []
    );
}

#[test]
fn in_operator_fixture_body_is_clean_without_lib() {
    // Without a global Record, missing-key synthesis is skipped
    // and the fixture body still checks clean.
    let text = "const a = 'a';\nconst b = 'b';\nconst d = 'd';\n\ntype A = { [a]: number; };\ntype B = { [b]: string; };\n\ndeclare const c: A | B;\n\nif ('a' in c) {\n    c;\n    c['a'];\n}\n\nif ('d' in c) {\n    c;\n}\n\nif (a in c) {\n    c;\n    c[a];\n}\n\nif (d in c) {\n    c;\n}\n";
    assert_eq!(checked_rows(text), []);
}

#[test]
fn static_blocks_and_privates_flow_type_auto_properties() {
    // getFlowTypeInStaticBlocks (this = the class in a static
    // block).
    assert_eq!(
        checked_rows(
            "class C { static x; static { this.x = 1; } }\ndeclare const n: string;\nconst z: string = C.x;\n"
        ),
        [(2322, 76, 1)]
    );
    // Private identifiers: accessName = the `__#…@` description
    // (\"#x\") matching the real `this.#x` access name.
    assert_eq!(
        checked_rows(
            "class C { #x; constructor() { this.#x = \"s\"; const n: number = this.#x; } }\n"
        ),
        [(2322, 51, 1)]
    );
}
