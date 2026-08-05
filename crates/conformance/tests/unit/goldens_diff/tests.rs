use super::*;
use crate::GoldenMessageChain;

fn diag(code: u32, start: u32, pass: &str, text: &str) -> GoldenDiag {
    GoldenDiag {
        file: Some("a.ts".to_owned()),
        start: Some(start),
        length: Some(1),
        line: Some(1),
        col: Some(start),
        code,
        pass: Some(pass.to_owned()),
        category: "error".to_owned(),
        chain: GoldenMessageChain {
            text: text.to_owned(),
            code,
            category: "error".to_owned(),
            next: Vec::new(),
        },
        related: Vec::new(),
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    }
}

#[test]
fn case_record_diff_is_occurrence_exact() {
    // old: two identical 2322s and one 1005; new: ONE 2322 (one
    // occurrence removed), the same 1005, and a new 2451.
    let old = vec![
        diag(2322, 4, "semantic", "not assignable"),
        diag(2322, 4, "semantic", "not assignable"),
        diag(1005, 9, "syntactic", "expected"),
    ];
    let new = vec![
        diag(2322, 4, "semantic", "not assignable"),
        diag(1005, 9, "syntactic", "expected"),
        diag(2451, 12, "semantic", "redeclare"),
    ];
    let (added, removed) = diff_case_records(&old, &new).unwrap();
    assert_eq!(added, vec![2]);
    assert_eq!(removed, vec![1]);

    // A span move is a remove+add pair, never a silent match.
    let moved = vec![diag(2322, 6, "semantic", "not assignable")];
    let (added, removed) =
        diff_case_records(&[diag(2322, 4, "semantic", "not assignable")], &moved).unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(removed.len(), 1);
}
