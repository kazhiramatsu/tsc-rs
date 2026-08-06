use tsc_diagnostics::DiagnosticCategory;
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

/// Class-band pins (oracle: tsc 6.0.3 noLib, scratchpad probe.sh
/// p2-p6, 2026-07-14).
fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    checked_rows_with_options("a.ts", text, &CompilerOptions::default())
}

fn checked_rows_with_options(
    file_name: &str,
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, u32, u32)> {
    with_program_state(&[(file_name, text)], options, |state| {
        state.check_source_file(0);
        rows(state)
    })
}

fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
    state
        .diagnostics
        .iter()
        .filter(|diag| diag.file_name.is_some() && diag.category() == DiagnosticCategory::Error)
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

#[test]
fn strict_property_initialization_constructor_face_reports_2564() {
    // Oracle: (2564, 10, 1) — the empty constructor never assigns
    // p; the flow probe (isPropertyInitializedInConstructor,
    // M5 post-close review) proves undefined survived. The
    // no-constructor face is pinned live in check.rs
    // (class_property_out_annotation_reports_2636).
    assert_eq!(
        checked_rows("class C { p: string; constructor() {} }\n"),
        [(2564, 10, 1)]
    );
    // Oracle: clean — a straight-line constructor assignment
    // proves initialization.
    assert_eq!(
        checked_rows("class C { p: string; constructor() { this.p = \"x\"; } }\n"),
        []
    );
    // Oracle: (2564, 10, 1) — a single-branch assignment is not
    // definite (the JOIN keeps undefined).
    assert_eq!(
        checked_rows(
            "class C { p: string; constructor(b: boolean) { if (b) { this.p = \"x\"; } } }\n"
        ),
        [(2564, 10, 1)]
    );
    // Oracle: clean — both branches assign.
    assert_eq!(
        checked_rows(
            "class C { p: string; constructor(b: boolean) { if (b) { this.p = \"x\"; } else { this.p = \"y\"; } } }\n"
        ),
        []
    );
    // Oracle: (2564, 10, 2) / clean — the private flavor grounds
    // on the `__#…@` description through the same synthetic
    // chain.
    assert_eq!(
        checked_rows("class C { #p: string; constructor() {} }\n"),
        [(2564, 10, 2)]
    );
    assert_eq!(
        checked_rows("class C { #p: string; constructor() { this.#p = \"x\"; } }\n"),
        []
    );
}

#[test]
fn static_property_conflict_uses_the_written_class_name() {
    let diagnostics = with_program_state(
        &[(
            "a.ts",
            "const Assigned = class { static prototype: number; };\n\
             namespace N { export default class DefaultWritten { static prototype: number; } }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2699)
                .map(|diagnostic| diagnostic.message_text().to_owned())
                .collect::<Vec<_>>()
        },
    );

    assert_eq!(
        diagnostics,
        [
            "Static property 'prototype' conflicts with built-in property 'Function.prototype' of constructor function 'Assigned'.",
            "Static property 'prototype' conflicts with built-in property 'Function.prototype' of constructor function 'DefaultWritten'.",
        ]
    );
}

#[test]
fn late_bound_prototype_merge_uses_the_written_computed_name() {
    let diagnostics = with_program_state(
        &[(
            "a.ts",
            "const names = { prototype: 'prototype' } as const;\n\
             class C { static [names.prototype](): void {} }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2300)
                .map(|diagnostic| diagnostic.message_text().to_owned())
                .collect::<Vec<_>>()
        },
    );

    assert_eq!(diagnostics, ["Duplicate identifier '[names.prototype]'."]);
}

#[test]
fn index_constraint_uses_the_written_property_name() {
    let diagnostics = with_program_state(
        &[(
            "a.ts",
            "interface I { [key: string]: number; [\"quoted\"]: string; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2411)
                .map(|diagnostic| diagnostic.message_text().to_owned())
                .collect::<Vec<_>>()
        },
    );

    assert_eq!(
        diagnostics,
        [
            "Property '[\"quoted\"]' of type 'string' is not assignable to 'string' index type 'number'."
        ]
    );
}

#[test]
fn checked_js_typed_property_initialization_row_is_published() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "export class C { field: string; }\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2564)
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.start.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2564, DiagnosticCategory::Error, 17)]
    );
}

#[test]
fn overwrite_base_property_fifth_face_reports_2612() {
    // Oracle: (2564, 38, 1) + (2612, 38, 1) — constructor present
    // but the property is NOT assigned in it: the fifth 2612
    // disjunct (85370, !isPropertyInitializedInConstructor) fires
    // alongside the 2564 face. The probe's declared type is the
    // DERIVED CLASS type (tsc quirk, preserved). Raw emission
    // order here (override checks run before property
    // initialization); the program layer's sort restores tsc's
    // 2564-first order at equal spans.
    assert_eq!(
        checked_rows(
            "class B { p = 1 }\nclass D extends B { p: number; constructor() { super(); } }\n"
        ),
        [(2612, 38, 1), (2564, 38, 1)]
    );
    // Oracle: clean — the constructor assignment clears BOTH
    // faces.
    assert_eq!(
        checked_rows(
            "class B { p = 1 }\nclass D extends B { p: number; constructor() { super(); this.p = 2; } }\n"
        ),
        []
    );
}

#[test]
fn override_without_base_class_reports_4112() {
    // Oracle: (4112, 19, 1).
    assert_eq!(
        checked_rows("class C { override m(): void {} }\n"),
        [(4112, 19, 1)]
    );
}

#[test]
fn no_implicit_override_requires_modifier_for_concrete_base_member() {
    let options = CompilerOptions {
        no_implicit_override: Some(true),
        ..CompilerOptions::default()
    };
    // Oracle: (4114, 39, 1).
    assert_eq!(
        checked_rows_with_options(
            "a.ts",
            "class B { m() {} }\nclass D extends B { m() {} }\n",
            &options,
        ),
        [(4114, 39, 1)]
    );
    assert_eq!(
        checked_rows_with_options(
            "a.ts",
            "class B { m() {} }\nclass D extends B { override m() {} }\n",
            &options,
        ),
        []
    );
}

#[test]
fn checked_js_override_tag_is_attached_to_the_member() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_override: Some(true),
        ..CompilerOptions::default()
    };
    let diagnostics = checked_rows_with_options(
        "a.js",
        "class A { m() {} }\nclass B extends A {\n/** @override */ m() {}\n/** @override */ n() {}\n}\n",
        &options,
    );
    assert_eq!(
        diagnostics
            .iter()
            .map(|&(code, _, _)| code)
            .collect::<Vec<_>>(),
        [4122]
    );
}

#[test]
fn incompatible_derived_property_reports_member_specific_2416() {
    // Oracle: (2416, 63, 1) — the member row's chain root IS the
    // reported code; the broad 2415 suppresses.
    assert_eq!(
        checked_rows(
            "class B2 { p: { x: number } = { x: 1 } }\nclass D2 extends B2 { p: { x: string } = { x: \"s\" } }\n"
        ),
        [(2416, 63, 1)]
    );
}

#[test]
fn interface_multi_extends_mismatch_reports_2320_at_name() {
    // Oracle: (2320, 64, 2) with the Named_property 2319 detail in
    // the chain tail.
    let text =
        "interface I1 { a: number }\ninterface I2 { a: string }\ninterface I3 extends I1, I2 {}\n";
    assert_eq!(checked_rows(text), [(2320, 64, 2)]);
}

#[test]
fn empty_string_class_members_do_not_conflict() {
    assert_eq!(
        checked_rows("class C { \"\": number; \"\": string; }\n"),
        [(2717, 22, 2)]
    );
}

#[test]
fn empty_heritage_list_position_is_utf16() {
    assert_eq!(
        checked_rows("const é = 0; class C implements {}\n"),
        [(1097, 31, 0)]
    );
}

#[test]
fn unimplemented_inherited_abstract_member_reports_2515() {
    // Oracle: (2515, 48, 2).
    assert_eq!(
        checked_rows("abstract class AB { abstract m(): void; }\nclass CC extends AB {}\n"),
        [(2515, 48, 2)]
    );
}

#[test]
fn class_modifier_error_suppresses_heritage_grammar() {
    // m4-review S7 (oracle: vendored tsc 6.0.3, noLib, strict,
    // 2026-07-19): tsc reports 1042 ONLY — checkGrammarModifiers'
    // async verdict suppresses the duplicate-extends walk (1172).
    // The live 1042 producer now reports the owning row while the
    // duplicate-extends follower stays suppressed.
    assert_eq!(
        checked_rows("declare const A: any, B: any;\nasync class C extends A extends B {}\n"),
        [(1042, 30, 5)]
    );
}
