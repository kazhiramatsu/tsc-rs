use tsc_diagnostics::DiagnosticCategory;
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::{check_program, InputFile};

// m4-review A6 (oracle: vendored tsc 6.0.3, noLib, strict,
// 2026-07-19): the auto-accessor arms of the getTypeOfAccessors
// ladder — annotation, widened initializer, implicit-any — plus
// the B21 isPrivateWithinAmbient guards and the circular-getter
// tail. Pre-fix the PropertyDeclaration arms were missing and an
// auto-accessor was silently `any`.

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
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

#[test]
fn auto_accessor_widens_its_initializer() {
    assert_eq!(
        checked_rows("class C { accessor x = 1; }\ndeclare const c: C;\nconst s: string = c.x;\n"),
        [(2322, 54, 1)]
    );
}

#[test]
fn auto_accessor_annotation_checks_its_initializer() {
    assert_eq!(
        checked_rows("class C { accessor x: number = \"s\"; }\n"),
        [(2322, 19, 1)]
    );
}

#[test]
fn any_initialized_auto_accessor_stays_clean() {
    assert_eq!(
        checked_rows(
            "declare const d: any;\nclass C { accessor x = d; }\ndeclare const c2: C;\nc2.x = \"ok\";\nconst n2: number = c2.x;\n"
        ),
        []
    );
}

#[test]
fn auto_accessor_write_type_reads_its_annotation() {
    assert_eq!(
        checked_rows("class C { accessor x: number = 1; }\ndeclare const c: C;\nc.x = \"s\";\n"),
        [(2322, 56, 3)]
    );
}

#[test]
fn bare_auto_accessor_reports_7008_member_implicit_any() {
    assert_eq!(checked_rows("class C { accessor x; }\n"), [(7008, 19, 1)]);
}

#[test]
fn ambient_private_setter_suppresses_implicit_any() {
    // m4-review B21: tsc's isPrivateWithinAmbient guard — no 7032.
    assert_eq!(checked_rows("declare class A { private set x(v); }\n"), []);
}

#[test]
fn accessor_implicit_any_is_an_error_or_suggestion_with_the_same_identity() {
    // Oracle: vendored tsc 6.0.3, noLib. Under strict:false the
    // accessor heads are suggestion-pass/category; noImplicitAny
    // true changes only the pass/category. The annotated and
    // ambient-private controls stay absent in both modes.
    let text = "class C { set x(value) {} }\n\
                abstract class G { abstract get y(); }\n\
                class P { set #p(value) {} }\n\
                class L { set \"a\"(value) {} }\n\
                declare class A { private set z(value); }\n\
                class T { set typed(value: number) {} }\n";
    let rows = |options: CompilerOptions| {
        check_program(
            &[InputFile::new("a.ts".to_owned(), text.to_owned())],
            &options,
        )
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 7032 | 7033))
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message.text.clone(),
                diagnostic.related.len(),
            )
        })
        .collect::<Vec<_>>()
    };

    let expected = [
        (
            7032,
            Some(text.find("x(value)").expect("setter name") as u32),
            "Property 'x' implicitly has type 'any', because its set accessor lacks a parameter type annotation.",
            1,
        ),
        (
            7033,
            Some(text.find("y();").expect("getter name") as u32),
            "Property 'y' implicitly has type 'any', because its get accessor lacks a return type annotation.",
            1,
        ),
        (
            7032,
            Some(text.find("#p(value)").expect("private setter name") as u32),
            "Property '#p' implicitly has type 'any', because its set accessor lacks a parameter type annotation.",
            2,
        ),
        (
            7032,
            Some(text.find("\"a\"(value)").expect("literal setter name") as u32),
            "Property '\"a\"' implicitly has type 'any', because its set accessor lacks a parameter type annotation.",
            3,
        ),
    ];
    for (options, category) in [
        (
            CompilerOptions {
                strict: Some(false),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            rows(options),
            expected
                .iter()
                .map(|(code, start, message, length)| (
                    *code,
                    category,
                    *start,
                    Some(*length),
                    (*message).to_owned(),
                    0,
                ))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn circular_unannotated_getter_reports_7023() {
    assert_eq!(
        checked_rows("class C { get x() { return this.x; } }\n"),
        [(7023, 14, 1)]
    );
}

#[test]
fn setter_this_parameter_is_not_the_value_parameter() {
    // The A2-exposed FP root: getSetAccessorValueParameter skips
    // a leading `this` in the two-parameter shape, so the paired
    // getter's inferred type comes from the VALUE parameter (tsc
    // 16677-16682; thisTypeInAccessors corpus face). tsc 6.0.3
    // reports only the accessor-this 2784 here — no 2322.
    assert_eq!(
        checked_rows(
            "const copied = {\n    n: 15,\n    get x() { return this.n },\n    set x(this: { n: number }, m: number) { this.n = m; }\n};\n"
        ),
        [(2784, 69, 19)]
    );
}

#[test]
fn annotated_bare_auto_accessor_reports_2564() {
    // The M5 strictPropertyInitialization face sees the
    // annotation through the A6 ladder.
    assert_eq!(
        checked_rows("class C { accessor x: number; }\n"),
        [(2564, 19, 1)]
    );
}
