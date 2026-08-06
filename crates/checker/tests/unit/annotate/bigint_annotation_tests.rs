use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;

// m4-review A14: non-decimal bigint literal types resolve through
// the full parsePseudoBigInt port (oracle: vendored tsc 6.0.3,
// noLib, strict, 2026-07-19). The "radix is M6" escape reason was
// false — `type A = 0x2n` is legal tsc and the parser was already
// live for expressions.

fn rows_and_partials(text: &str) -> (Vec<(u32, u32, u32)>, usize) {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let rows = state
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
            .collect();
        (rows, state.partial_check_records.len())
    })
}

#[test]
fn hex_bigint_literal_type_resolves_and_relates() {
    assert_eq!(
        rows_and_partials("type A = 0x2n;\ndeclare const v: A;\nconst w: 2n = v;\n"),
        (vec![], 0)
    );
}

#[test]
fn negative_binary_bigint_literal_type_resolves() {
    assert_eq!(
        rows_and_partials("type N = -0b101n;\ndeclare const q: N;\nconst r: -5n = q;\n"),
        (vec![], 0)
    );
}
