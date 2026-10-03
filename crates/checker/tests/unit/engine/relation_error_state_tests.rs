use super::{
    indexed_access_error_info_selection, variance_error_info_selection, ErrorChainEntry,
    RelationErrorState,
};

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
        error_chain: (0..depth.unwrap_or(0))
            .map(|_| ErrorChainEntry {
                message: &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
                args: Vec::new(),
            })
            .collect(),
        error_info_revision: revision,
        ..RelationErrorState::default()
    }
}

#[test]
fn relation_error_state_selectors_follow_tsgo_priority_and_depth() {
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
                .expect("equal depth favors original"),
            &original_short,
        ),
        "tsgo's <= tie break keeps originalErrorChain"
    );
    assert!(
        indexed_access_error_info_selection(&empty, &current_short).is_none(),
        "a nil originalErrorChain does not trigger retry selection"
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
