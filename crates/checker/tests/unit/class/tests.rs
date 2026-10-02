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
                .map(|diagnostic| {
                    diagnostic
                        .message_text()
                        .as_str()
                        .expect("scalar diagnostic observation")
                        .to_owned()
                })
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
                .map(|diagnostic| {
                    diagnostic
                        .message_text()
                        .as_str()
                        .expect("scalar diagnostic observation")
                        .to_owned()
                })
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
                .map(|diagnostic| {
                    diagnostic
                        .message_text()
                        .as_str()
                        .expect("scalar diagnostic observation")
                        .to_owned()
                })
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
fn empty_string_class_members_report_duplicates_like_tsgo() {
    // tsgo (tsc-19dadef8) reports both `""` properties as TS2300 and the
    // second one's type as TS2717; tsc 6.0's class check skipped them.
    assert_eq!(
        checked_rows("class C { \"\": number; \"\": string; }\n"),
        [(2300, 10, 2), (2300, 22, 2), (2717, 22, 2)]
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

/// tsgo (TypeScript 7.1 at 19dadef8, noLib probe) reports only the
/// implicit-any rows: getDeclaredTypeOfClassOrInterface merges no
/// `Point.prototype = { ... }` object literal into the class (tsc 6.0's
/// getAssignedClassSymbol is gone), and the binder declares no expando
/// over the class's synthetic `prototype`, so `add` is not duplicated.
#[test]
fn prototype_object_assignment_is_not_merged_into_a_function_merged_class() {
    assert_eq!(
        checked_rows(
            "declare class Point {\n  constructor(x: number, y: number);\n  public x: number;\n  public add(dx: number, dy: number): Point;\n}\nfunction Point(x, y) {\n  this.x = x;\n}\nPoint.prototype = {\n  x: 0,\n  add: function(dx, dy) { return new Point(this.x + dx, 0); }\n};\n",
        ),
        [(7006, 141, 1), (7006, 144, 1), (2683, 151, 4)]
    );
}

/// tsgo bindDeferredExpandoAssignment declares an expando only when no
/// non-expando declaration has the name: `C.x = "s"` is a plain assignment
/// to the static property (TS2322 at the target), while the two `C.y`
/// assignments declare one expando whose type admits both values (tsgo
/// noLib probe at 19dadef8).
#[test]
fn checked_js_expando_does_not_redeclare_a_static_member() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with_options(
            "a.js",
            "class C {\n  static x = 1;\n}\nC.x = \"s\";\nC.y = 2;\nC.y = \"t\";\nlet n = C.x;\nlet m = C.y;\n",
            &options,
        ),
        [(2322, 28, 3)]
    );
}

/// tsgo (TypeScript 7.1 at 19dadef8, noLib probe): an expando member's
/// type is the union of its assignments (read here after the `"s"`
/// assignment narrows it); a `const` arrow function takes expandos, a
/// `let` one does not.
#[test]
fn expando_members_follow_tsgo_initializer_targets() {
    assert_eq!(
        checked_rows(
            "function F() {}\nF.x = 1;\nF.x = \"s\";\nconst n: number = F.x;\nconst g = () => 0;\ng.y = true;\nconst b: string = g.y;\nlet h = () => 0;\nh.z = 1;\n",
        ),
        [(2322, 42, 1), (2322, 96, 1), (2339, 132, 1)]
    );
}

/// tsgo (TypeScript 7.1 at 19dadef8, noLib probe): a JSDoc `@type` on an
/// expando assignment decides the member's type; the deferred binding finds
/// a class declared later; no namespace is created for a missing entity or
/// a missing nested member.
#[test]
fn checked_js_expandos_bind_after_the_file_without_namespaces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with_options(
            "a.js",
            "const o = {};\n/** @type {number} */\no.p = 1;\no.p = \"s\";\nC.b = 2;\nclass C {}\nmissing.c = 3;\nconst f = function () {};\nf.nested.d = 4;\n",
            &options,
        ),
        [(2322, 45, 3), (2449, 56, 1), (2304, 76, 7), (2339, 119, 6)]
    );
}

/// tsgo isClassInstanceProperty (TypeScript 7.1 at 19dadef8, noLib probe):
/// a JavaScript assignment declaration is judged by its left side, so the
/// static expando `C.blah2 = 456` is no instance field (only TS2565 for its
/// read before the assignment), while a base constructor's `this.f = 1` is
/// one (TS2855 through `super`).
#[test]
fn checked_js_super_access_judges_assignment_declarations_by_their_left_side() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        target: Some(99),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with_options(
            "index.js",
            "class C {\n  static blah1 = 123;\n}\nC.blah2 = 456;\nclass D extends C {\n  static {\n    super.blah1;\n    super.blah2;\n  }\n}\nclass E {\n  constructor() { this.f = 1; }\n}\nclass G extends E {\n  m() { return super.f; }\n}\n",
            &options,
        ),
        [(2565, 107, 5), (2855, 205, 1)]
    );
}
