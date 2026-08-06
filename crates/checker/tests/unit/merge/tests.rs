use tsc_types::{CompilerOptions, SymbolFlags};

use crate::state::test_support::with_program_state;

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
fn script_file_declare_global_is_not_an_augmentation() {
    // m4-review A9 (tsc collectModuleReferences 124144): a
    // script-file `declare global` never merges into globals —
    // tsc-probed rows (vendored 6.0.3 noLib): 2669 only, no
    // duplicate against the sibling var.
    assert_eq!(
        checked_rows("declare global { var gv: number; }\nvar gv: string;\n"),
        [(2669, 8, 6)]
    );
}

#[test]
fn script_file_declare_global_does_not_pollute_globals() {
    // tsc-probed: the member never lands in globals, so the use
    // reports 2304 (pre-fix the port suppressed it).
    assert_eq!(
        checked_rows("declare global { var gv2: number; }\nconst use: number = gv2;\n"),
        [(2669, 8, 6), (2304, 56, 3)]
    );
}

#[test]
fn augmentation_conflicts_survive_to_the_post_pass_flush() {
    // m4-review A8: the flush runs AFTER the augmentation passes
    // (tsc 88882 follows 88874-88881) — a `declare global` class
    // colliding with a script-file global records DURING pass 1
    // and still reports. Pre-fix the map was already flushed and
    // the records died silently. tsc-probed rows (vendored 6.0.3
    // noLib): 2300 in both files.
    with_program_state(
        &[
            (
                "a.ts",
                "export {};\ndeclare global { class G { g(): void } }\n",
            ),
            ("b.ts", "class G { s(): void {} }\n"),
        ],
        &CompilerOptions::default(),
        |state| {
            let mut pins: Vec<(u32, Option<String>, u32)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.file_name.clone(), d.start.unwrap_or(u32::MAX)))
                .collect();
            pins.sort();
            assert_eq!(
                pins,
                [
                    (2300, Some("a.ts".to_owned()), 34),
                    (2300, Some("b.ts".to_owned()), 6),
                ]
            );
        },
    );
}

#[test]
fn cross_file_duplicate_classes_report_2300_on_both_files() {
    with_program_state(
        &[("a.ts", "class C {}\n"), ("b.ts", "class C {}\n")],
        &CompilerOptions::default(),
        |state| {
            let mut pins: Vec<(u32, Option<String>)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.file_name.clone()))
                .collect();
            pins.sort();
            assert_eq!(
                pins,
                [
                    (2300, Some("a.ts".to_owned())),
                    (2300, Some("b.ts".to_owned())),
                ]
            );
            // Each report carries the "was also declared here"
            // related info pointing at the OTHER file.
            for diagnostic in &state.diagnostics {
                assert_eq!(diagnostic.related.len(), 1);
                assert_ne!(diagnostic.related[0].file_name, diagnostic.file_name);
            }
        },
    );
}

#[test]
fn cross_file_let_redeclaration_reports_2451() {
    with_program_state(
        &[
            ("a.ts", "declare let x: number;\n"),
            ("b.ts", "declare let x: string;\n"),
        ],
        &CompilerOptions::default(),
        |state| {
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2451, 2451]);
        },
    );
}

#[test]
fn cross_file_interfaces_merge_declarations_and_members() {
    with_program_state(
        &[
            ("a.ts", "interface I { a: number }\n"),
            ("b.ts", "interface I { b: string }\n"),
        ],
        &CompilerOptions::default(),
        |state| {
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            let symbol = state
                .resolve_file_scope_name("I", SymbolFlags::TYPE)
                .expect("merged interface resolves");
            assert_eq!(state.binder.symbol(symbol).declarations.len(), 2);
            // The merged global is a checker-side clone (the file-a
            // original was not transient), and both originals chase
            // to it.
            assert!(state
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::TRANSIENT));
            let declared = state
                .get_declared_type_of_class_or_interface(symbol)
                .expect("thisless non-generic interface");
            let members = state
                .resolve_structured_type_members(declared)
                .expect("members resolve");
            let names: Vec<String> = state
                .members_of(members)
                .properties
                .iter()
                .map(|&p| state.binder.symbol(p).escaped_name.clone())
                .collect();
            assert_eq!(names, ["a", "b"]);
        },
    );
}

#[test]
fn global_this_declaration_conflicts_with_builtin() {
    for (name, options) in [
        ("a.ts", CompilerOptions::default()),
        (
            "a.js",
            CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        ),
    ] {
        with_program_state(&[(name, "var globalThis;\n")], &options, |state| {
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2397]);
        });
    }
}

#[test]
fn var_undefined_conflicts_with_builtin_but_type_undefined_does_not() {
    with_program_state(
        &[("a.ts", "var undefined: number;\n")],
        &CompilerOptions::default(),
        |state| {
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2397]);
        },
    );
    with_program_state(
        &[("a.ts", "interface undefined { a: number }\n")],
        &CompilerOptions::default(),
        |state| {
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn plain_js_omits_only_its_own_duplicate_location() {
    let options = CompilerOptions {
        allow_js: true,
        ..CompilerOptions::default()
    };
    with_program_state(
        &[
            ("a.d.ts", "declare class A {}\n"),
            ("b.js", "const A = {};\n"),
        ],
        &options,
        |state| {
            let pins = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.file_name.as_deref().unwrap_or_default(),
                        diagnostic.start.unwrap_or(u32::MAX),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(pins, [(2451, "a.d.ts", 14)]);
        },
    );
}

#[test]
fn checked_js_reports_cross_file_block_scoped_redeclarations() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("a.js", "class Bar {}\n"), ("b.js", "const Bar = 3;\n")],
        &options,
        |state| {
            let pins = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.file_name.as_deref().unwrap_or_default(),
                        diagnostic.start.unwrap_or(u32::MAX),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(pins, [(2451, "a.js", 6), (2451, "b.js", 6)]);
        },
    );
}
