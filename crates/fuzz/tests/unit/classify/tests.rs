use super::*;

#[test]
fn class_rows_sort_by_side_numeric_code_and_utf8_bytes() {
    let mut rows = [
        ClassRow {
            side: DifferenceSide::Tsrs,
            code: 9,
            normalized_message_head: "z".to_owned(),
        },
        ClassRow {
            side: DifferenceSide::Oracle,
            code: 10,
            normalized_message_head: "\u{e000}".to_owned(),
        },
        ClassRow {
            side: DifferenceSide::Oracle,
            code: 9,
            normalized_message_head: "z".to_owned(),
        },
        ClassRow {
            side: DifferenceSide::Oracle,
            code: 10,
            normalized_message_head: "\u{10000}".to_owned(),
        },
    ];
    rows.sort();
    assert_eq!(rows[0].code, 9);
    assert_eq!(rows[1].normalized_message_head, "\u{e000}");
    assert_eq!(rows[2].normalized_message_head, "\u{10000}");
    assert_eq!(rows[3].side, DifferenceSide::Tsrs);
}

#[test]
fn duplicate_rows_remain_distinct_multiset_occurrences() {
    let row = ClassRow {
        side: DifferenceSide::Oracle,
        code: 2322,
        normalized_message_head: "head".to_owned(),
    };
    let class = CanonicalClass {
        schema: CANONICAL_CLASS_SCHEMA,
        failure: ClassFailure::Tier {
            tier: ComparisonTier::T1,
        },
        pass: ClassPass::Semantic,
        outcome: ClassOutcome {
            side: OutcomeSide::Oracle,
            kind: "diagnostic".to_owned(),
        },
        rows: vec![row.clone(), row],
        renderer: None,
    };
    class.validate().expect("duplicate multiset rows are valid");
    let bytes = class.canonical_bytes().expect("class is serializable");
    assert_eq!(
        String::from_utf8(bytes)
            .expect("JSON is UTF-8")
            .matches("\"code\":2322")
            .count(),
        2
    );
}
