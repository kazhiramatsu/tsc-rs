use crate::state::test_support::with_program_state;
use crate::{check_program, CompilerOptions, InputFile};
use tsc_diagnostics::DiagnosticCategory;

fn unused_rows(
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, DiagnosticCategory, u32, u32, String)> {
    unused_rows_for_files(&[("a.ts", text)], options)
}

fn unused_rows_for_files(
    files: &[(&str, &str)],
    options: &CompilerOptions,
) -> Vec<(u32, DiagnosticCategory, u32, u32, String)> {
    let files = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect::<Vec<_>>();
    check_program(&files, options)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code(),
                6133 | 6138 | 6192 | 6196 | 6198 | 6199 | 6205
            )
        })
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text().to_owned(),
            )
        })
        .collect()
}

fn unused_rows_with_file_for_files(
    files: &[(&str, &str)],
    options: &CompilerOptions,
) -> Vec<(String, u32, DiagnosticCategory, u32, u32, String)> {
    let files = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect::<Vec<_>>();
    check_program(&files, options)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code(),
                6133 | 6138 | 6192 | 6196 | 6198 | 6199 | 6205
            )
        })
        .map(|diagnostic| {
            let code = diagnostic.code();
            let category = diagnostic.category();
            let start = diagnostic.start.unwrap_or(u32::MAX);
            let length = diagnostic.length.unwrap_or(u32::MAX);
            let message = diagnostic.message_text().to_owned();
            (
                diagnostic.file_name.unwrap_or_default(),
                code,
                category,
                start,
                length,
                message,
            )
        })
        .collect()
}

#[test]
fn unused_identifier_drain_preserves_bound_siblings_of_parse_errors() {
    let text = "export {}; const unused = 1; const used = 2; used; const broken = ;";
    let rows = unused_rows(text, &CompilerOptions::default());
    let start = text.find("unused").expect("unused declaration") as u32;
    assert_eq!(
        rows,
        vec![(
            6133,
            DiagnosticCategory::Suggestion,
            start,
            "unused".len() as u32,
            "'unused' is declared but its value is never read.".to_owned(),
        )]
    );

    assert!(unused_rows(
        "const globalUnused = 1; const broken = ;",
        &CompilerOptions::default(),
    )
    .is_empty());
}

#[test]
fn parse_error_suppresses_the_nearest_unused_declaration_group() {
    assert!(unused_rows(
        "export {}; const hidden = 1, broken = ;",
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn unused_identifier_recovery_preserves_unicode_escape_spans() {
    let text = r"var \u0061wait = 12;
var \u0079ield = 12;
type typ\u0065 = 12;
export {};
";
    let rows = unused_rows(text, &CompilerOptions::default());
    assert_eq!(
        rows,
        vec![
            (
                6133,
                DiagnosticCategory::Suggestion,
                text.find(r"\u0061wait").expect("escaped await") as u32,
                r"\u0061wait".len() as u32,
                "'await' is declared but its value is never read.".to_owned(),
            ),
            (
                6133,
                DiagnosticCategory::Suggestion,
                text.find(r"\u0079ield").expect("escaped yield") as u32,
                r"\u0079ield".len() as u32,
                "'yield' is declared but its value is never read.".to_owned(),
            ),
            (
                6196,
                DiagnosticCategory::Suggestion,
                text.find(r"typ\u0065").expect("escaped type") as u32,
                r"typ\u0065".len() as u32,
                "'type' is declared but never used.".to_owned(),
            ),
        ]
    );
}

#[test]
fn merged_type_and_value_parameter_references_keep_their_meaning_masks() {
    let options = CompilerOptions {
        no_unused_locals: Some(true),
        no_unused_parameters: Some(true),
        ..CompilerOptions::default()
    };

    let value_only = "export function f<T>(T: number) { return T; }";
    assert_eq!(
        unused_rows(value_only, &options),
        [(
            6133,
            DiagnosticCategory::Error,
            value_only.find("<T>").expect("type parameter range") as u32,
            3,
            "'T' is declared but its value is never read.".to_owned(),
        )],
        "a value-meaning read must not mark the merged type-parameter face"
    );

    let type_only = "export function g<T>(T: number): T { throw 0; }";
    assert_eq!(
        unused_rows(type_only, &options),
        [(
            6133,
            DiagnosticCategory::Error,
            type_only.find("(T:").expect("value parameter") as u32 + 1,
            1,
            "'T' is declared but its value is never read.".to_owned(),
        )],
        "a type-meaning read must not mark the merged value-parameter face"
    );

    assert!(
        unused_rows(
            "export function h<T>(value: T): T { return value; }",
            &options,
        )
        .is_empty(),
        "the non-colliding sibling keeps both reference meanings"
    );
}

#[test]
fn unused_module_instantiation_alias_does_not_reference_assignment_target() {
    let text = "declare namespace pack1 {
  const test1: string;
  export { test1 };
}
declare namespace pack2 {
  import test1 = pack1.test1;
  export { test1 };
}
export import test1 = pack2.test1;
declare namespace mod1 {
  type test1 = string;
  export { test1 };
}
declare namespace mod2 {
  import test1 = mod1.test1;
  export { test1 };
}
const test2 = mod2;
";
    let rows = unused_rows(text, &CompilerOptions::default());
    let start = text.find("test2 =").expect("test2 declaration") as u32;
    assert_eq!(
        rows,
        vec![(
            6133,
            DiagnosticCategory::Suggestion,
            start,
            "test2".len() as u32,
            "'test2' is declared but its value is never read.".to_owned(),
        )]
    );
}

#[test]
fn unused_invalid_declare_accessor_uses_member_ambient_flag() {
    let text = "class C {
    declare get #pair()
    declare set #pair(value: number)
}
";
    let rows = unused_rows(text, &CompilerOptions::default());
    let start = text.find("#pair").expect("private getter name") as u32;
    assert_eq!(
        rows,
        vec![(
            6133,
            DiagnosticCategory::Suggestion,
            start,
            "#pair".len() as u32,
            "'#pair' is declared but its value is never read.".to_owned(),
        )]
    );
}

const CLASS_PROBE: &str = "class C {
  #used = 0;
  #unused = 0;
  private oldUsed = 0;
  private oldUnused = 0;
  get #pair() { return 0; }
  set #pair(_alue: number) {}
  get #dead() { return 0; }
  set #dead(_alue: number) {}
  constructor(private live: number, private dead: number) {
    this.#used;
    this.oldUsed;
    this.#pair;
    this.live;
  }
}
";

#[test]
fn unused_type_parameters_cover_every_ts_owner_and_exact_spans() {
    let text = "export class ClassSingle<T> {}\n\
                    export class ClassMultiple<T, U> {}\n\
                    export class ClassPartial<T, U> { value!: T; }\n\
                    export const ClassExpression = class<T, U> { value!: T; };\n\
                    export interface InterfaceSingle<T> {}\n\
                    export interface InterfaceMultiple<T, U> {}\n\
                    export interface InterfacePartial<T, U> { value: T; }\n\
                    export type AliasSingle<T> = number;\n\
                    export type AliasMultiple<T, U> = number;\n\
                    export type AliasPartial<T, U> = T;\n\
                    export function declaration<T, U>(value: T): T { return value; }\n\
                    export const expression = function<T, U>(value: T): T { return value; };\n\
                    export const arrow = <T, U>(value: T): T => value;\n\
                    export class Members { method<T, U>(value: T): T { return value; } }\n\
                    export type FunctionShape = <T, U>(value: T) => T;\n\
                    export type ConstructorShape = new <T, U>(value: T) => T;\n\
                    export interface Signatures {\n\
                        <T, U>(value: T): T;\n\
                        new<T, U>(value: T): T;\n\
                        method<T, U>(value: T): T;\n\
                    }\n\
                    export const Underscore = <_T>(value: number): number => value;\n";

    let rows = unused_rows(text, &CompilerOptions::default());
    assert_eq!(rows.len(), 19);
    assert_eq!(rows.iter().filter(|row| row.0 == 6205).count(), 3);
    assert_eq!(rows.iter().filter(|row| row.0 == 6133).count(), 16);
    assert!(rows
        .iter()
        .all(|row| row.1 == DiagnosticCategory::Suggestion));

    let single_start = text.find("<T> {}").expect("single class list") as u32;
    assert!(rows.iter().any(|row| {
        row.0 == 6133
            && row.2 == single_start
            && row.3 == 3
            && row.4 == "'T' is declared but its value is never read."
    }));
    let multiple_start = text.find("<T, U> {}").expect("multiple class list") as u32;
    assert!(rows.iter().any(|row| {
        row.0 == 6205
            && row.2 == multiple_start
            && row.3 == 6
            && row.4 == "All type parameters are unused."
    }));
    let partial_start =
        text.find("ClassPartial<T, U>").expect("partial class") + "ClassPartial<T, ".len();
    assert!(rows.iter().any(|row| {
        row.0 == 6133
            && row.2 == partial_start as u32
            && row.3 == 1
            && row.4 == "'U' is declared but its value is never read."
    }));

    let local_mode_rows = unused_rows(
        text,
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(local_mode_rows
        .iter()
        .all(|row| row.1 == DiagnosticCategory::Suggestion));

    let parameter_mode_rows = unused_rows(
        text,
        &CompilerOptions {
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(parameter_mode_rows.len(), 19);
    assert!(parameter_mode_rows
        .iter()
        .all(|row| row.1 == DiagnosticCategory::Error));
}

#[test]
fn unused_type_parameters_honor_trivia_underscores_and_last_merged_declaration() {
    let text = "export class Trivia<T /* kept in aggregate span */> {}\n\
                    export interface LastUnused<T> { value: T; }\n\
                    export interface LastUnused<T> { other: number; }\n\
                    export interface LastUsed<T> { other: number; }\n\
                    export interface LastUsed<T> { value: T; }\n\
                    export function OverloadLastUnused<T>(value: T): T;\n\
                    export function OverloadLastUnused<T>(value: number): number { return value; }\n\
                    export function OverloadLastUsed<T>(value: number): number;\n\
                    export function OverloadLastUsed<T>(value: T): T { return value; }\n\
                    export type Ignored<_T, _U> = number;\n";
    let rows = unused_rows(text, &CompilerOptions::default());
    assert_eq!(rows.len(), 2);

    let trivia = "<T /* kept in aggregate span */>";
    let trivia_start = text.find(trivia).expect("trivia type parameter list") as u32;
    assert_eq!(
        (&rows[0].0, &rows[0].2, &rows[0].3, rows[0].4.as_str()),
        (
            &6133,
            &trivia_start,
            &(trivia.len() as u32),
            "'T' is declared but its value is never read.",
        )
    );

    let last_unused = "OverloadLastUnused<T>(value: number)";
    let last_unused_start = text
        .find(last_unused)
        .expect("last merged overload declaration")
        + "OverloadLastUnused".len();
    assert_eq!(
        (&rows[1].0, &rows[1].2, &rows[1].3, rows[1].4.as_str()),
        (
            &6133,
            &(last_unused_start as u32),
            &3,
            "'T' is declared but its value is never read.",
        )
    );
}

#[test]
fn unused_infer_type_parameters_follow_node_spans_and_parameter_mode() {
    let text = "export type Used<T> = T extends infer U ? U : never;\n\
                    export type Unused<T> = T extends infer U ? string : never;\n\
                    export type Underscore<T> = T extends infer _U ? string : never;\n\
                    export type Repeated<T> = T extends { left: infer U; right: infer U } ? string : never;\n\
                    export type Outside = infer U;\n";
    let expected_starts = [
        text.find("infer U ? string").expect("single unused infer"),
        text.find("infer U; right").expect("first repeated infer"),
        text.find("infer U } ? string")
            .expect("second repeated infer"),
        text.rfind("infer U").expect("outside infer"),
    ];
    for (options, category) in [
        (CompilerOptions::default(), DiagnosticCategory::Suggestion),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
        ),
    ] {
        let rows = unused_rows(text, &options);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter()
                .map(|row| (row.0, row.1, row.2, row.3, row.4.as_str()))
                .collect::<Vec<_>>(),
            expected_starts
                .iter()
                .map(|start| (
                    6133,
                    category,
                    *start as u32,
                    7,
                    "'U' is declared but its value is never read.",
                ))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn unused_private_class_members_follow_reference_and_accessor_anchors() {
    let rows = unused_rows(
        CLASS_PROBE,
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows.iter()
            .map(|(code, category, _, _, message)| (*code, *category, message.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                6133,
                DiagnosticCategory::Error,
                "'#unused' is declared but its value is never read."
            ),
            (
                6133,
                DiagnosticCategory::Error,
                "'oldUnused' is declared but its value is never read."
            ),
            (
                6133,
                DiagnosticCategory::Error,
                "'#dead' is declared but its value is never read."
            ),
            (
                6138,
                DiagnosticCategory::Error,
                "Property 'dead' is declared but its value is never read."
            ),
        ]
    );
    assert_eq!(
        rows.iter()
            .map(|(_, _, start, length, _)| (*start, *length))
            .collect::<Vec<_>>(),
        [(25, 7), (71, 9), (150, 5), (246, 4)]
    );
}

#[test]
fn unused_class_members_are_suggestions_without_no_unused_locals() {
    for options in [
        CompilerOptions::default(),
        CompilerOptions {
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    ] {
        let rows = unused_rows(CLASS_PROBE, &options);
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .all(|(_, category, _, _, _)| *category == DiagnosticCategory::Suggestion));
    }
}

#[test]
fn private_brand_in_expression_counts_as_a_read() {
    let rows = unused_rows(
        "class C { #unused: undefined; #brand: undefined; has(v: any) { return #brand in v; } }\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, 6133);
    assert_eq!(
        rows[0].4,
        "'#unused' is declared but its value is never read."
    );
}

#[test]
fn source_file_locals_group_imports_and_publish_checked_js() {
    let options = CompilerOptions {
        no_unused_locals: Some(true),
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let rows = unused_rows_for_files(
        &[
            ("dep.ts", "export class A {}\nexport class B {}\n"),
            ("imports.ts", "import { A, B } from './dep';\n"),
            (
                "locals.ts",
                "function deadTs() {}\nexport function keptTs() {}\n",
            ),
            (
                "locals.js",
                "function deadJs() {}\nexport function keptJs() {}\n",
            ),
        ],
        &options,
    );
    assert_eq!(
        rows,
        [
            (
                6192,
                DiagnosticCategory::Error,
                0,
                29,
                "All imports in import declaration are unused.".to_owned(),
            ),
            (
                6133,
                DiagnosticCategory::Error,
                9,
                6,
                "'deadJs' is declared but its value is never read.".to_owned(),
            ),
            (
                6133,
                DiagnosticCategory::Error,
                9,
                6,
                "'deadTs' is declared but its value is never read.".to_owned(),
            ),
        ]
    );
}

#[test]
fn source_file_locals_are_suggestions_by_default() {
    let rows = unused_rows("export {};\nconst dead = 1;\n", &CompilerOptions::default());
    assert_eq!(
        rows,
        [(
            6133,
            DiagnosticCategory::Suggestion,
            17,
            4,
            "'dead' is declared but its value is never read.".to_owned(),
        )]
    );
}

#[test]
fn block_locals_follow_suggestion_and_error_modes() {
    let text = "export {};\nif (true) {\n    const dead = 1;\n}\n";
    for (options, category) in [
        (CompilerOptions::default(), DiagnosticCategory::Suggestion),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options),
            [(
                6133,
                category,
                33,
                4,
                "'dead' is declared but its value is never read.".to_owned(),
            )]
        );
    }
}

#[test]
fn block_locals_preserve_reads_and_group_unused_variables() {
    assert!(unused_rows(
        "export {};\nif (true) {\n    const used = 1;\n    void used;\n}\n",
        &CompilerOptions::default(),
    )
    .is_empty());
    assert_eq!(
        unused_rows(
            "export {};\nif (true) {\n    const first = 1, second = 2;\n}\n",
            &CompilerOptions::default(),
        ),
        [(
            6199,
            DiagnosticCategory::Suggestion,
            27,
            28,
            "All variables are unused.".to_owned(),
        )]
    );
}

#[test]
fn module_locals_follow_suggestion_and_error_modes() {
    let text = "export namespace N {\n    const dead = 1;\n}\n";
    for (options, category) in [
        (CompilerOptions::default(), DiagnosticCategory::Suggestion),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options),
            [(
                6133,
                category,
                31,
                4,
                "'dead' is declared but its value is never read.".to_owned(),
            )]
        );
    }
}

#[test]
fn module_registration_preserves_exports_reads_and_global_augmentations() {
    assert!(unused_rows(
            "export namespace N {\n    const used = 1;\n    void used;\n    export const publicValue = 2;\n}\n",
            &CompilerOptions::default(),
        )
        .is_empty());
    assert!(unused_rows(
        "export {};\ndeclare global {\n    const ambientGlobal: number;\n}\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn loop_and_case_locals_follow_suggestion_and_error_modes() {
    let text = "export {};\nfor (let deadFor = 0; false;) {}\nfor (const deadOf of [1]) {}\nfor (const deadIn in { key: 1 }) {}\nswitch (0) { case 0: const deadCase = 1; break; }\n";
    for (options, category) in [
        (CompilerOptions::default(), DiagnosticCategory::Suggestion),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, row_category, _, _, message)| {
                    (*code, *row_category, message.as_str())
                })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    category,
                    "'deadFor' is declared but its value is never read.",
                ),
                (
                    6133,
                    category,
                    "'deadOf' is declared but its value is never read.",
                ),
                (
                    6133,
                    category,
                    "'deadIn' is declared but its value is never read.",
                ),
                (
                    6133,
                    category,
                    "'deadCase' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn loop_and_case_registration_preserves_reads_and_iteration_underscores() {
    assert!(unused_rows(
            "export {};\nfor (let usedFor = 0; usedFor < 1; usedFor++) {}\nfor (const usedOf of [1]) { void usedOf; }\nfor (const usedIn in { key: 1 }) { void usedIn; }\nfor (const _ignored of [1]) {}\nswitch (0) { case 0: const usedCase = 1; void usedCase; }\n",
            &CompilerOptions::default(),
        )
        .is_empty());
}

#[test]
fn function_declaration_locals_and_parameters_use_independent_modes() {
    let text = "export function mixed(deadParameter: number, usedParameter: number) {\n    const deadLocal = 1;\n    return usedParameter;\n}\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn function_declaration_registration_preserves_body_and_parameter_exemptions() {
    assert!(unused_rows(
        "export declare function declared(deadParameter: number): void;\n\
             export function implemented(_ignoredParameter: number, usedParameter: number) {\n\
                 const usedLocal = 1;\n\
                 return usedParameter + usedLocal;\n\
             }\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn function_declaration_shadowed_array_bindings_keep_tsc_spans() {
    let rows = unused_rows(
        "export declare const y: any;\n\
             export function first(x: any) {\n    var [x] = y;\n}\n\
             export function initialized(x: any) {\n    var [x = y] = y;\n}\n\
             export function rest(x: any) {\n    var [...x] = y;\n}\n\
             export function nested(x: any) {\n    var [[x]] = y;\n}\n\
             export function nestedInitialized(x: any) {\n    var [[x] = y] = y;\n}\n\
             export function parameter([x]: [any]) {\n}\n",
        &CompilerOptions::default(),
    );
    assert_eq!(
        rows.iter()
            .map(|(code, category, start, length, _)| { (*code, *category, *start, *length) })
            .collect::<Vec<_>>(),
        [
            (6133, DiagnosticCategory::Suggestion, 51, 1),
            (6133, DiagnosticCategory::Suggestion, 69, 3),
            (6133, DiagnosticCategory::Suggestion, 108, 1),
            (6133, DiagnosticCategory::Suggestion, 126, 7),
            (6133, DiagnosticCategory::Suggestion, 162, 1),
            (6133, DiagnosticCategory::Suggestion, 180, 6),
            (6133, DiagnosticCategory::Suggestion, 217, 1),
            (6133, DiagnosticCategory::Suggestion, 236, 3),
            (6133, DiagnosticCategory::Suggestion, 282, 1),
            (6133, DiagnosticCategory::Suggestion, 301, 3),
            (6133, DiagnosticCategory::Suggestion, 343, 3),
        ]
    );
}

#[test]
fn function_expression_locals_and_parameters_use_independent_modes() {
    let text = "export const assigned = function (deadParameter: number) {\n    const deadLocal = 1;\n};\n\
                    (function (deadIifeParameter: number) {\n    const deadIifeLocal = 1;\n})();\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    parameter_category,
                    "'deadIifeParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadIifeLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn function_expression_registration_preserves_names_reads_and_parameter_exemptions() {
    assert!(unused_rows(
        "export const assigned = function named(\n\
                 _ignoredParameter: number,\n\
                 usedParameter: number,\n\
             ) {\n\
                 const usedLocal = usedParameter;\n\
                 return named && usedLocal;\n\
             };\n\
             (function (_ignoredIifeParameter: number) {})();\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn function_expression_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
        unused_rows(
            "export const shadowed = function (value: number) {\n    var [value] = [1];\n};\n",
            &CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .iter()
        .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
        .collect::<Vec<_>>(),
        [
            (
                6133,
                DiagnosticCategory::Error,
                "'value' is declared but its value is never read.",
            ),
            (
                6133,
                DiagnosticCategory::Suggestion,
                "'value' is declared but its value is never read.",
            ),
        ]
    );
}

#[test]
fn function_expression_nested_class_uses_the_local_mode() {
    assert_eq!(
        unused_rows(
            "export const nested = function () {\n    class DeadClass {}\n};\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        )
        .iter()
        .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
        .collect::<Vec<_>>(),
        [(
            6196,
            DiagnosticCategory::Error,
            "'DeadClass' is declared but never used.",
        )]
    );
}

#[test]
fn arrow_function_locals_and_parameters_use_independent_modes() {
    let text = "export const mixed = (deadParameter: number, usedParameter: number) => {\n    const deadLocal = 1;\n    return usedParameter;\n};\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn arrow_function_registration_preserves_expression_bodies_and_parameter_exemptions() {
    assert!(unused_rows(
            "export const expression = (_ignoredParameter: number, usedParameter: number) => usedParameter;\n\
             export const block = (usedParameter: number) => {\n\
                 const usedLocal = 1;\n\
                 return usedParameter + usedLocal;\n\
             };\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
}

#[test]
fn arrow_function_checked_js_assignment_local_uses_property_name_anchor() {
    let text = "class D {}\nD.prototype.foo = () => {\n    this.n = 1;\n};\n";
    let expected_start = text.find("n = 1").expect("property name") as u32;
    assert_eq!(
        unused_rows_for_files(
            &[("a.js", text)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [(
            6133,
            DiagnosticCategory::Suggestion,
            expected_start,
            1,
            "'n' is declared but its value is never read.".to_owned(),
        )]
    );
}

#[test]
fn arrow_function_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
            unused_rows(
                "export const shadowed = (value: number) => {\n    var [value] = [1];\n    return 0;\n};\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
}

#[test]
fn method_declaration_locals_and_parameters_use_independent_modes() {
    let text = "export class Container {\n    method(deadParameter: number, usedParameter: number) {\n        const deadLocal = 1;\n        return usedParameter;\n    }\n}\n\
                    export const object = {\n    method(deadObjectParameter: number) {\n        const deadObjectLocal = 1;\n        return 0;\n    },\n};\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    parameter_category,
                    "'deadObjectParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadObjectLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn method_declaration_registration_preserves_overloads_reads_and_parameter_exemptions() {
    assert!(unused_rows(
        "export class Container {\n\
                 overload(deadSignatureParameter: number): void;\n\
                 overload(_ignoredImplementationParameter: number): void {}\n\
                 used(_ignoredParameter: number, usedParameter: number) {\n\
                     const usedLocal = 1;\n\
                     return usedParameter + usedLocal;\n\
                 }\n\
             }\n\
             export const object = {\n\
                 used(_ignoredParameter: number, usedParameter: number) {\n\
                     return usedParameter;\n\
                 },\n\
             };\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn method_declaration_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
            unused_rows(
                "export class Container {\n    shadowed(value: number) {\n        var [value] = [1];\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
}

#[test]
fn get_accessor_locals_and_parameters_use_independent_modes() {
    let text = "export class Container {\n    get value() {\n        const deadLocal = 1;\n        return 0;\n    }\n}\n\
                    export const Expression = class {\n    get value() {\n        const deadExpressionLocal = 1;\n        return 0;\n    }\n};\n\
                    export const object = {\n    get value() {\n        const deadObjectLocal = 1;\n        return 0;\n    },\n};\n\
                    export class Invalid {\n    get value(deadParameter: number) {\n        return 0;\n    }\n}\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadExpressionLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadObjectLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn get_accessor_registration_preserves_reads_underscores_and_ambient_declarations() {
    assert!(unused_rows(
        "export class Container {\n\
                 get used() {\n\
                     const usedLocal = 1;\n\
                     return usedLocal;\n\
                 }\n\
                 get ignored(_ignoredParameter: number) {\n\
                     return 0;\n\
                 }\n\
             }\n\
             export const object = {\n\
                 get used() {\n\
                     const usedLocal = 1;\n\
                     return usedLocal;\n\
                 },\n\
             };\n\
             export declare class Ambient {\n\
                 get value(): number;\n\
             }\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn get_accessor_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
            unused_rows(
                "export class Container {\n    get value(value: number) {\n        var [value] = [1];\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
}

#[test]
fn get_accessor_nested_class_uses_the_local_mode() {
    assert_eq!(
            unused_rows(
                "export class Container {\n    get value() {\n        class DeadClass {}\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [(
                6196,
                DiagnosticCategory::Error,
                "'DeadClass' is declared but never used.",
            )]
        );
}

#[test]
fn set_accessor_locals_and_parameters_use_independent_modes() {
    let text = "export class Container {\n    set value(deadParameter: number) {\n        const deadLocal = 1;\n    }\n}\n\
                    export const object = {\n    set value(deadObjectParameter: number) {\n        const deadObjectLocal = 1;\n    },\n};\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    parameter_category,
                    "'deadObjectParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadObjectLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn set_accessor_registration_preserves_reads_underscores_and_ambient_declarations() {
    assert!(unused_rows(
        "export class Container {\n\
                 set used(usedParameter: number) {\n\
                     const usedLocal = usedParameter;\n\
                     usedLocal;\n\
                 }\n\
                 set ignored(_ignoredParameter: number) {}\n\
             }\n\
             export const object = {\n\
                 set used(usedParameter: number) {\n\
                     usedParameter;\n\
                 },\n\
             };\n\
             export declare class Ambient {\n\
                 set value(deadSignatureParameter: number);\n\
             }\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn set_accessor_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
            unused_rows(
                "export class Container {\n    set value(value: number) {\n        var [value] = [1];\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
}

#[test]
fn constructor_locals_and_parameters_use_independent_modes() {
    let text = "export class Container {\n    constructor(deadParameter: number) {\n        const deadLocal = 1;\n    }\n}\n\
                    export const Expression = class {\n    constructor(deadExpressionParameter: number) {\n        const deadExpressionLocal = 1;\n    }\n};\n";
    for (options, parameter_category, local_category) in [
        (
            CompilerOptions::default(),
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Suggestion,
            DiagnosticCategory::Error,
        ),
        (
            CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
            DiagnosticCategory::Error,
            DiagnosticCategory::Error,
        ),
    ] {
        assert_eq!(
            unused_rows(text, &options)
                .iter()
                .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    parameter_category,
                    "'deadParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadLocal' is declared but its value is never read.",
                ),
                (
                    6133,
                    parameter_category,
                    "'deadExpressionParameter' is declared but its value is never read.",
                ),
                (
                    6133,
                    local_category,
                    "'deadExpressionLocal' is declared but its value is never read.",
                ),
            ]
        );
    }
}

#[test]
fn constructor_registration_preserves_overloads_reads_and_parameter_properties() {
    assert!(unused_rows(
        "export class Container {\n\
                 constructor(\n\
                     public publicProperty: number,\n\
                     _ignoredParameter: number,\n\
                     usedParameter: number,\n\
                 ) {\n\
                     const usedLocal = usedParameter;\n\
                     usedLocal;\n\
                 }\n\
             }\n\
             export class Overloaded {\n\
                 constructor(deadSignatureParameter: number);\n\
                 constructor(_ignoredImplementationParameter: number) {}\n\
             }\n\
             export const Expression = class {\n\
                 constructor(usedParameter: number) {\n\
                     usedParameter;\n\
                 }\n\
             };\n",
        &CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn destructured_parameter_property_uses_synthetic_symbol_name() {
    let rows = unused_rows(
        "class Container {\n\
                 constructor(private [a, b, c]: [number, string, boolean]) {}\n\
             }\n",
        &CompilerOptions::default(),
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.0 == 6138)
            .map(|row| (row.1, row.4.as_str()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCategory::Suggestion,
            "Property '__missing' is declared but its value is never read.",
        )]
    );
}

#[test]
fn constructor_shadowed_parameter_and_local_keep_distinct_kinds() {
    assert_eq!(
            unused_rows(
                "export class Container {\n    constructor(value: number) {\n        var [value] = [1];\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
}

#[test]
fn constructor_nested_class_uses_the_local_mode() {
    assert_eq!(
        unused_rows(
            "export class Container {\n    constructor() {\n        class DeadClass {}\n    }\n}\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        )
        .iter()
        .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
        .collect::<Vec<_>>(),
        [(
            6196,
            DiagnosticCategory::Error,
            "'DeadClass' is declared but never used.",
        )]
    );
}

#[test]
fn declaration_file_unused_locals_are_ambient_suggestions() {
    let rows = unused_rows_for_files(
        &[("a.d.ts", "export {};\ndeclare const dead: number;\n")],
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows,
        [(
            6133,
            DiagnosticCategory::Suggestion,
            25,
            4,
            "'dead' is declared but its value is never read.".to_owned(),
        )]
    );
}

#[test]
fn ambient_declaration_in_source_file_is_an_unused_suggestion() {
    let text = "export {};\ndeclare const dead: number;\n";
    let rows = unused_rows(
        text,
        &CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows,
        [(
            6133,
            DiagnosticCategory::Suggestion,
            text.find("dead").expect("ambient declaration") as u32,
            4,
            "'dead' is declared but its value is never read.".to_owned(),
        )]
    );
}

#[test]
fn jsdoc_links_mark_direct_and_qualified_import_roots_as_referenced() {
    let rows = unused_rows_for_files(
            &[
                ("dep.ts", "export interface A {}\n"),
                (
                    "direct.ts",
                    "import type { A } from './dep';\n/** {@link A} */\nexport interface B {}\n",
                ),
                (
                    "qualified.ts",
                    "import * as ns from './dep';\n/** {@linkplain ns.A details} */\nexport function documented() {}\n",
                ),
            ],
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert!(rows.is_empty());
}

#[test]
fn checked_js_type_queries_and_typedef_merges_mark_source_locals() {
    let rows = unused_rows_for_files(
        &[(
            "a.js",
            "const exemplar = () => 1;\n\
                 /** @param {typeof exemplar} value */\n\
                 export function consume(value) { void value; }\n\
                 /** @typedef {number} Local */\n\
                 var Local = 1;\n",
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_property_assignment_function_parameters_remain_unused() {
    let usage = "/** @constructor */
Outer.Pos = function (line, ch) {};
Outer.Used = function (used) { return used; };
/** @type {number} */
Outer.Pos.prototype.line;
var pos = new Outer.Pos(1, 'x');
pos.line;
";
    let rows = unused_rows_for_files(
        &[
            ("module.js", "var Outer = function(element, config) {};\n"),
            ("usage.js", usage),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows.iter()
            .filter(|row| {
                row.4.starts_with("'line'")
                    || row.4.starts_with("'ch'")
                    || row.4.starts_with("'used'")
            })
            .map(|row| (row.0, row.1, row.2, row.3, row.4.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                6133,
                DiagnosticCategory::Suggestion,
                usage.find("line,").expect("line parameter") as u32,
                4,
                "'line' is declared but its value is never read.",
            ),
            (
                6133,
                DiagnosticCategory::Suggestion,
                usage.find("ch)").expect("ch parameter") as u32,
                2,
                "'ch' is declared but its value is never read.",
            ),
        ]
    );
}

#[test]
fn checked_js_require_alias_reads_mark_the_source_local() {
    let rows = unused_rows_for_files(
        &[
            (
                "dep.js",
                "function Exported() {}\nmodule.exports = Exported;\n",
            ),
            (
                "index.js",
                "const Exported = require('./dep');\nExported.member;\nnew Exported;\n",
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_jsdoc_types_and_destructured_alias_exports_mark_source_locals() {
    let rows = unused_rows_for_files(
        &[
            (
                "lib.js",
                "class SomeClass {}\nmodule.exports = { SomeClass };\n",
            ),
            (
                "main.js",
                "const { SomeClass, SomeClass: Another } = require('./lib');\n\
                     /** @param {SomeClass} value */\n\
                     export function consume(value) { void value; }\n\
                     module.exports = { SomeClass, Another };\n",
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_contained_reads_mark_nested_and_commonjs_locals() {
    let rows = unused_rows_for_files(
        &[
            (
                "node.d.ts",
                "declare var exports: any;\n\
                     declare var module: { exports: any };\n",
            ),
            (
                "main.js",
                "/// <reference path='node.d.ts' />\n\
                     exports = module.exports = C;\n\
                     function C() {\n\
                       var x = {};\n\
                       return x;\n\
                     }\n\
                     function build() {\n\
                       const obj = {};\n\
                       return obj;\n\
                     }\n\
                     build();\n",
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_reference_reconciliation_preserves_non_reads() {
    let text = "export {};\n\
                    function recursive() { recursive(); }\n\
                    function write() { let assigned; assigned = 1; }\n\
                    function labels() { let marker = 1; marker: { break marker; } }\n\
                    write();\n\
                    labels();\n";
    let rows = unused_rows_for_files(
        &[("main.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows.iter()
            .map(|row| (row.0, row.4.as_str()))
            .collect::<Vec<_>>(),
        [
            (6133, "'recursive' is declared but its value is never read."),
            (6133, "'assigned' is declared but its value is never read."),
            (6133, "'marker' is declared but its value is never read."),
        ]
    );
}

#[test]
fn unused_jsdoc_import_tag_does_not_become_a_source_local_diagnostic() {
    let rows = unused_rows_for_files(
        &[
            ("types.ts", "export interface Foo { a: number; }\n"),
            ("foo.js", "/** @import x = require(\"types\") */\n"),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_conformance_reference_reads_mark_source_locals() {
    let cases = [
        (
            "jsDeclarationsReferenceToClassInstanceCrossFile",
            vec![
                (
                    "/rectangle.js",
                    r#"class Rectangle {
    constructor() {
        console.log("I'm a rectangle!");
    }
}

module.exports = { Rectangle };
"#,
                ),
                (
                    "/index.js",
                    r#"const {Rectangle} = require('./rectangle');

class Render {
    constructor() {
        /**
         * Object list
         * @type {Rectangle[]}
         */
        this.objects = [];
    }
    /**
     * Adds a rectangle
     *
     * @returns {Rectangle} the rect
     */
    addRectangle() {
        const obj = new Rectangle();
        this.objects.push(obj);
        return obj;
    }
}

module.exports = { Render };
"#,
                ),
                (
                    "/test.js",
                    r#"const {Render} = require("./index");
let render = new Render();

render.addRectangle();
console.log("Objects", render.objects);
"#,
                ),
            ],
        ),
        (
            "importTag13",
            vec![
                ("/types.ts", "export interface Foo {\n    a: number;\n}\n"),
                ("/foo.js", "/** @import x = require(\"types\") */\n"),
            ],
        ),
        (
            "jsdocTypeReferenceToImportOfFunctionExpression",
            vec![
                (
                    "/MW.js",
                    r#"/** @typedef {import("./MC")} MC */

class MW {
  /**
   * @param {MC} compiler the compiler
   */
  constructor(compiler) {
    this.compiler = compiler;
  }
}

module.exports = MW;
"#,
                ),
                (
                    "/MC.js",
                    r#"const MW = require("./MW");

/** @typedef {number} Meyerhauser */

/** @class */
module.exports = function MC() {
    /** @type {any} */
    var x = {}
    return new MW(x);
};
"#,
                ),
            ],
        ),
        (
            "moduleExportAlias2",
            vec![
                (
                    "/node.d.ts",
                    "declare function require(name: string): any;\n\
declare var exports: any;\n\
declare var module: { exports: any };\n",
                ),
                (
                    "/semver.js",
                    r#"/// <reference path='node.d.ts' />
exports = module.exports = C
exports.f = n => n + 1
function C() {
    this.p = 1
}
"#,
                ),
                (
                    "/index.js",
                    r#"/// <reference path='node.d.ts' />
const C = require("./semver")
var two = C.f(1)
var c = new C
"#,
                ),
            ],
        ),
        (
            "moduleExportAlias5",
            vec![(
                "/bug24754.js",
                r#"// #24754
const webpack = function (){
}
exports = module.exports = webpack;
exports.version = 1001;

webpack.WebpackOptionsDefaulter = 1111;
"#,
            )],
        ),
        (
            "typeFromPropertyAssignment19",
            vec![
                (
                    "/types.d.ts",
                    "declare var require: any;\ndeclare var module: any;\n",
                ),
                (
                    "/semver.js",
                    r#"/// <reference path='./types.d.ts'/>
exports = module.exports = C
C.f = n => n + 1
function C() {
    this.p = 1
}
"#,
                ),
                (
                    "/index.js",
                    r#"/// <reference path='./types.d.ts'/>
const C = require("./semver")
var two = C.f(1)
"#,
                ),
            ],
        ),
    ];
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let failures = cases
        .into_iter()
        .filter_map(|(name, files)| {
            let rows = unused_rows_with_file_for_files(&files, &options);
            let expected = match name {
                "moduleExportAlias2" => vec![
                    (
                        "/index.js".to_owned(),
                        6133,
                        DiagnosticCategory::Suggestion,
                        69,
                        3,
                        "'two' is declared but its value is never read.".to_owned(),
                    ),
                    (
                        "/index.js".to_owned(),
                        6133,
                        DiagnosticCategory::Suggestion,
                        86,
                        1,
                        "'c' is declared but its value is never read.".to_owned(),
                    ),
                ],
                "typeFromPropertyAssignment19" => vec![(
                    "/index.js".to_owned(),
                    6133,
                    DiagnosticCategory::Suggestion,
                    71,
                    3,
                    "'two' is declared but its value is never read.".to_owned(),
                )],
                _ => Vec::new(),
            };
            (rows != expected).then_some((name, expected, rows))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn checked_js_cross_file_unused_registrations_are_source_owned_and_deduplicated() {
    let mw = r#"/** @typedef {import("./MC")} MC */
class MW {
    /** @param {MC} compiler */
    constructor(compiler) {
        this.compiler = compiler;
    }
}
module.exports = MW;
"#;
    let mc = r#"const MW = require("./MW");
/** @class */
module.exports = function MC() {
    var y = 0;
    var x = {};
    return new MW(x);
};
"#;
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    with_program_state(&[("/MW.js", mw), ("/MC.js", mc)], &options, |state| {
        state.check_source_file(0);
        let mc_root = state.binder.source(1).root;
        assert_eq!(
            state
                .potentially_unused_identifiers
                .get(&mc_root)
                .map(Vec::len),
            Some(1),
            "a cross-file forced registration belongs to MC.js"
        );

        state.check_source_file(1);
        assert!(
            !state.potentially_unused_identifiers.contains_key(&mc_root),
            "the owning source-file drain removes its entry"
        );
        let y_start = mc.find("y = 0").expect("fixture y") as u32;
        let rows = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 6133)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.as_deref(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [(
                Some("/MC.js"),
                y_start,
                1,
                "'y' is declared but its value is never read.",
            )],
            "x is read and the duplicated function registration emits y only once"
        );

        let diagnostic_count = state.diagnostics.len();
        state.check_source_file(1);
        assert_eq!(
            state.diagnostics.len(),
            diagnostic_count,
            "rechecking a type-checked source file is idempotent"
        );
    });
}
