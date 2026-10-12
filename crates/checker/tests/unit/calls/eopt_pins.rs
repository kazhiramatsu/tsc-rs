use crate::state::test_support::with_program_state;
use tsc_types::CompilerOptions;

#[test]
fn exact_optional_overload_selection_matches_single_signature_face() {
    // 7.3-era carry-in re-probed at 7.4 (scratchpad probe74j.mjs,
    // vendored 6.0.3 noLib + exactOptionalPropertyTypes): the
    // declared `{ a: number; c?: undefined }` argument fails
    // overload #1's `{ a; c?: string }` under eOPT exactly like
    // the single-signature path, so selection falls to
    // `unknown -> string` and the `n: number = r` follow-up
    // reports 2322. The pre-7.4 port picked #1 (an applicability-
    // relation eOPT gap in the old selection loop) — RESOLVED by
    // the 7.4b chooseOverload rebuild.
    let options = CompilerOptions {
        exact_optional_property_types: Some(true),
        ..CompilerOptions::default()
    };
    let rows = with_program_state(
        &[("a.ts", "declare function pick(o: { a: number; c?: string }): number;\ndeclare function pick(o: unknown): string;\ndeclare const arg: { a: number; c?: undefined };\nconst r = pick(arg);\nconst n: number = r;\n")],
        &options,
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(rows, [(2322, 180, 1)]);
}

#[test]
fn an_optional_parameter_s_argument_head_names_the_option() {
    // tsgo 7.1 (tsc-aa814927, strict + exactOptionalPropertyTypes):
    // `a.ts(4,3): error TS2379`. reportRelationError swaps the argument
    // head on the reported target, `O` without the optional
    // parameter's `| undefined` (Astro's `Code.astro`).
    let options = CompilerOptions {
        strict: Some(true),
        exact_optional_property_types: Some(true),
        ..CompilerOptions::default()
    };
    let rows = with_program_state(
        &[("a.ts", "interface O { meta?: string; a?: string }\ndeclare function f(o?: O): void;\ndeclare const meta: string | undefined;\nf({ meta, a: \"\" });\n")],
        &options,
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(rows, [(2379, 117, 15)]);
}
