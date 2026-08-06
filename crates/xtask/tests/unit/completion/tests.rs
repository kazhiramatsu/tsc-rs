use super::*;

fn probe(ready: bool, detail: &str) -> CompletionProbe {
    CompletionProbe::new(ready, detail)
}

fn inputs(ready: bool) -> CompletionInputs {
    CompletionInputs {
        all_corpus_fp_zero: probe(ready, "fp"),
        exact_scope: probe(ready, "scope"),
        supported_t0_t3: probe(ready, "tiers"),
        supported_t4: probe(ready, "render"),
        syntactic_in_scope: probe(ready, "syntactic"),
        zero_escapes: probe(ready, "escapes"),
        rust_ledger: probe(ready, "ledger"),
        declaration_converse: probe(ready, "d2"),
        b1_b4_evidence: probe(ready, "evidence"),
        full_corpus_invariants: probe(ready, "invariants"),
        m9_steady_state: probe(ready, "m9"),
    }
}

#[test]
fn report_enumerates_the_normative_rows_in_order() {
    let report = build_report(inputs(false));
    assert_eq!(report.schema, COMPLETION_SCHEMA);
    assert_eq!(report.ready_rows, 0);
    assert_eq!(report.total_rows, 11);
    assert!(!report.complete);
    assert_eq!(
        report
            .rows
            .iter()
            .map(|row| (row.number, row.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (1, "all-corpus-fp-zero"),
            (2, "exact-scope"),
            (3, "supported-t0-t3"),
            (4, "supported-t4"),
            (5, "syntactic-in-scope"),
            (6, "zero-escapes"),
            (7, "rust-function-ledger"),
            (8, "declaration-converse"),
            (9, "b1-b4-evidence"),
            (10, "full-corpus-invariants"),
            (11, "m9-steady-state"),
        ]
    );
}

#[test]
fn report_only_succeeds_while_strict_mode_names_every_pending_row() {
    let report = build_report(inputs(false));
    enforce(&report, false).unwrap();
    let error = enforce(&report, true).unwrap_err().to_string();
    assert!(error.contains("1:all-corpus-fp-zero"), "{error}");
    assert!(error.contains("11:m9-steady-state"), "{error}");
}

#[test]
fn sampled_or_failed_invariants_cannot_satisfy_strict_completion() {
    let mut observation = inputs(true);
    observation.full_corpus_invariants =
        probe(false, "sampled invariant evidence is not full-corpus");
    let report = build_report(observation);
    assert_eq!(report.ready_rows, 10);
    let error = enforce(&report, true).unwrap_err().to_string();
    assert!(error.contains("10:full-corpus-invariants"), "{error}");
    assert!(!error.contains("9:b1-b4-evidence"), "{error}");
    assert!(!error.contains("11:m9-steady-state"), "{error}");
}

#[test]
fn all_eleven_rows_are_required_for_completion() {
    let report = build_report(inputs(true));
    assert!(report.complete);
    assert_eq!(report.ready_rows, 11);
    enforce(&report, true).unwrap();
}

#[test]
fn argument_parser_accepts_only_the_strict_flag() {
    assert!(
        !parse_args(std::iter::empty::<String>())
            .unwrap()
            .require_done
    );
    assert!(
        parse_args(["--require-done"].into_iter().map(str::to_owned))
            .unwrap()
            .require_done
    );
    assert!(parse_args(["--strict"].into_iter().map(str::to_owned)).is_err());
}
