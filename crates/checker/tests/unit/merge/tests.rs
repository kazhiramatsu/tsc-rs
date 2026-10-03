use tsc_types::{CompilerOptions, ScriptTarget, SymbolFlags};

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
                .map(|d| {
                    (
                        d.code(),
                        d.file_name.as_ref().map(|value| {
                            value
                                .as_str()
                                .expect("scalar filename observation")
                                .to_owned()
                        }),
                        d.start.unwrap_or(u32::MAX),
                    )
                })
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
fn global_namespace_merge_resolves_alias_meaning_before_conflict_detection() {
    // checkerInitializationCrash.ts: the exported import-equals symbol is
    // syntactically an Alias, but resolveSymbol exposes its TypeAlias target
    // before mergeSymbol applies the incoming type alias's exclusions. The
    // conflict is therefore owned by the augmentation merge (2300 on both
    // names), not by the later checkAliasSymbol fallback (2440).
    with_program_state(
        &[
            (
                "a.d.ts",
                "declare namespace react { type ReactNode = string; }\n\
                 declare global { namespace FullCalendarVDom { export import VNode = react.ReactNode; } }\n\
                 export {};\n",
            ),
            (
                "b.d.ts",
                "declare global { namespace FullCalendarVDom { type VNode = number; } }\n\
                 export {};\n",
            ),
        ],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state.check_source_file(1);
            let mut pins = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.file_name.as_ref().map(|value| value.as_js().as_str().expect("scalar name observation")).unwrap_or_default(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                    )
                })
                .collect::<Vec<_>>();
            pins.sort_unstable();
            assert_eq!(
                pins,
                [(2300, "a.d.ts", 113, 5), (2300, "b.d.ts", 51, 5)]
            );
        },
    );
}

#[test]
fn global_augmentation_conflicts_with_an_earlier_umd_global_export() {
    with_program_state(
        &[
            (
                "global.d.ts",
                "declare global {\n    const React: typeof import(\"./module\");\n}\nexport {};\n",
            ),
            (
                "module.d.ts",
                "export as namespace React;\nexport function foo(): string;\n",
            ),
            ("some_module.ts", "export {}\nReact.foo;\n"),
            ("emits.ts", "console.log(\"hello\");\nReact.foo;\n"),
        ],
        &CompilerOptions {
            strict: Some(true),
            module: Some(99),
            target: Some(ScriptTarget::ES2018.bits()),
            ..CompilerOptions::default()
        },
        |state| {
            let pins = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic
                            .file_name
                            .as_ref()
                            .map(|value| value.as_js().as_str().expect("scalar name observation"))
                            .unwrap_or_default(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                pins,
                [(2451, "global.d.ts", 27, 5), (2451, "module.d.ts", 20, 5),]
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
                .map(|d| {
                    (
                        d.code(),
                        d.file_name.as_ref().map(|value| {
                            value
                                .as_str()
                                .expect("scalar filename observation")
                                .to_owned()
                        }),
                    )
                })
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
                .map(|&p| {
                    state
                        .binder
                        .symbol(p)
                        .escaped_name
                        .as_js()
                        .as_str()
                        .expect("scalar name observation")
                        .to_owned()
                })
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
                        diagnostic
                            .file_name
                            .as_ref()
                            .map(|value| value.as_js().as_str().expect("scalar name observation"))
                            .unwrap_or_default(),
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
            let mut pins = state
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic
                            .file_name
                            .as_ref()
                            .map(|value| value.as_js().as_str().expect("scalar name observation"))
                            .unwrap_or_default(),
                        diagnostic.start.unwrap_or(u32::MAX),
                    )
                })
                .collect::<Vec<_>>();
            // The program sorts; the sink holds the merge's report order.
            pins.sort();
            assert_eq!(pins, [(2451, "a.js", 6), (2451, "b.js", 6)]);
        },
    );
}

/// tsgo (TypeScript 7.1 at 19dadef8, noLib probe): an assignment to a
/// function's namespace export is not an expando declaration (the name
/// already has a non-expando declaration), and a namespace is no expando
/// target, so both assignments are checked against `number`.
#[test]
fn expando_assignment_does_not_redeclare_a_namespace_export() {
    assert_eq!(
        checked_rows(
            "namespace N {\n  export var x = 1;\n}\nfunction F() {}\nnamespace F {\n  export var x = 1;\n}\nF.x = \"s\";\nN.x = \"s\";\n",
        ),
        [(2322, 88, 3), (2322, 99, 3)]
    );
}

/// A related row as (file, start, code).
type RelatedRow = (String, u32, u32);

/// Each cross-file conflict's rows as (file, start, code, related rows),
/// after the program's sort and dedup.
fn merged_conflict_rows(files: &[(&str, &str)]) -> Vec<(String, u32, u32, Vec<RelatedRow>)> {
    with_program_state(files, &CompilerOptions::default(), |state| {
        let mut diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file_name.is_some())
            .cloned()
            .collect::<Vec<_>>();
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
        let name = |file: &Option<tsc_types::JsString>| {
            file.as_ref()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    name(&diagnostic.file_name),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.code(),
                    diagnostic
                        .related
                        .iter()
                        .map(|related| {
                            (
                                name(&related.file_name),
                                related.start.unwrap_or(u32::MAX),
                                related.message.code,
                            )
                        })
                        .collect(),
                )
            })
            .collect()
    })
}

#[test]
fn cross_file_conflicts_report_every_declaration_like_tsgo() {
    // tsgo (tsc-19dadef8) has no amalgamatedDuplicates: eight conflicting
    // names across two files report TS2451 at every declaration, each with
    // its "was also declared here" row, instead of tsc 6.0's TS6200 pair.
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let one = names
        .iter()
        .map(|name| format!("declare var {name}: number;\n"))
        .collect::<String>();
    let two = names
        .iter()
        .map(|name| format!("declare let {name}: number;\n"))
        .collect::<String>();
    let rows = merged_conflict_rows(&[("one.ts", &one), ("two.ts", &two)]);
    let expected = ["one.ts", "two.ts"]
        .iter()
        .flat_map(|file| {
            let other = if *file == "one.ts" {
                "two.ts"
            } else {
                "one.ts"
            };
            // `declare var x: number;` and `declare let x: number;` put
            // every name at the same offset.
            (0..names.len() as u32).map(move |line| {
                let start = line * 23 + 12;
                (
                    (*file).to_owned(),
                    start,
                    2451,
                    vec![(other.to_owned(), start, 6203)],
                )
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);
}

#[test]
fn augmentation_conflicts_merge_their_related_rows_like_tsgo() {
    // tsgo (tsc-19dadef8): each augmentation merges, and reports,
    // separately; the two a.ts diagnostics differ only by related
    // information and merge into one with two leading rows (TS6203).
    let b = "export {};\n\ndeclare module \"./a\" {\n    export const x = 0;\n}\n\ndeclare module \"../dir/a\" {\n    export const x = 0;\n}\n";
    let rows = merged_conflict_rows(&[("/dir/a.ts", "export const x = 0;\n"), ("/dir/b.ts", b)]);
    let first = b.find("x = 0").expect("first augmentation") as u32;
    let second = b.rfind("x = 0").expect("second augmentation") as u32;
    assert_eq!(
        rows,
        [
            (
                "/dir/a.ts".to_owned(),
                13,
                2451,
                vec![
                    ("/dir/b.ts".to_owned(), first, 6203),
                    ("/dir/b.ts".to_owned(), second, 6203),
                ],
            ),
            (
                "/dir/b.ts".to_owned(),
                first,
                2451,
                vec![("/dir/a.ts".to_owned(), 13, 6203)],
            ),
            (
                "/dir/b.ts".to_owned(),
                second,
                2451,
                vec![("/dir/a.ts".to_owned(), 13, 6203)],
            ),
        ]
    );
}
