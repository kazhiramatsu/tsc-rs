use super::{
    indexed_access_error_info_selection, variance_error_info_selection, RelationErrorState,
};
use tsc_diagnostics::{DiagnosticCategory, MessageChain};

fn chain(depth: usize, label: &str) -> MessageChain {
    MessageChain {
        code: depth as u32,
        category: DiagnosticCategory::Error,
        text: format!("{label}-{depth}").into(),
        next_present: depth > 1,
        next: (depth > 1)
            .then(|| chain(depth - 1, label))
            .into_iter()
            .collect(),
    }
}

#[test]
fn enum_value_diagnostic_text_keeps_lone_units_after_escape_string() {
    use crate::evaluate::EvalValue;
    use tsc_types::JsString;

    // _tsc.js:64709–64722 applies escapeString, whose regexp does not
    // match surrogate units; rendering that value to UTF-8 is a later step.
    for unit in [0xd800, 0xd801, 0xdc00, 0xfffd] {
        let value = EvalValue::Str(JsString::from_code_units(&[unit, 0x22, 0x0a]));
        assert_eq!(
            super::enum_relation_value_text(&value).to_utf16(),
            [0x22, unit, 0x5c, 0x22, 0x5c, 0x6e, 0x22]
        );
    }
}

fn state(depth: Option<usize>, revision: u64) -> RelationErrorState {
    RelationErrorState {
        error_info: depth.map(|depth| chain(depth, "chain")),
        error_info_revision: revision,
        ..RelationErrorState::default()
    }
}

#[test]
fn relation_error_state_selectors_follow_tsc_priority_and_breadth() {
    let original_short = state(Some(1), 1);
    let original_long = state(Some(3), 2);
    let current_short = state(Some(1), 3);
    let current_long = state(Some(3), 4);
    let empty = state(None, 5);
    let saved = state(Some(2), 6);

    assert!(std::ptr::eq(
        indexed_access_error_info_selection(&original_short, &current_long)
            .expect("both chains exist"),
        &original_short,
    ));
    assert!(std::ptr::eq(
        indexed_access_error_info_selection(&original_long, &current_short)
            .expect("both chains exist"),
        &current_short,
    ));
    assert!(
        std::ptr::eq(
            indexed_access_error_info_selection(&original_short, &current_short)
                .expect("equal breadth favors original"),
            &original_short,
        ),
        "tsc's <= tie break keeps originalErrorInfo"
    );
    assert!(
        indexed_access_error_info_selection(&empty, &current_short).is_none(),
        "a falsy originalErrorInfo does not trigger retry selection"
    );

    assert!(std::ptr::eq(
        variance_error_info_selection(Some(&original_short), &current_short, &saved),
        &original_short,
    ));
    assert!(std::ptr::eq(
        variance_error_info_selection(Some(&empty), &current_short, &saved),
        &current_short,
    ));
    assert!(std::ptr::eq(
        variance_error_info_selection(Some(&empty), &empty, &saved),
        &saved,
    ));
    assert!(
        std::ptr::eq(
            variance_error_info_selection(Some(&empty), &empty, &empty),
            &empty,
        ),
        "all-falsy selection still restores the saved identity token"
    );
}
