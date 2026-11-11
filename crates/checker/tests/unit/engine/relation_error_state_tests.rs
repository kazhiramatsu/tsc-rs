use super::{
    indexed_access_error_info_selection, variance_error_info_selection, ErrorChainEntry,
    RelationErrorState,
};

#[test]
fn relation_error_snapshots_keep_tsgo_nested_diagnostics() {
    // Go relater.go:3212-3222 snapshots immutable error-chain heads and
    // related-info slice headers. These are tsgo aa8149273's complete
    // diagnostic messages for work/fixtures/P3.ts, including folded
    // properties, a union fallback and an elided return-type marker.
    let text = "\
interface Target { p: { a: { x: number } }; q: string }
const badNested: Target = { p: { a: { x: \"wrong\" } }, q: \"ok\" };
type Choice = { kind: \"a\"; value: number } | { kind: \"b\"; value: string };
const badChoice: Choice = { kind: \"a\", value: \"wrong\" };
interface CallbackTarget { fn: (x: number) => { value: number } }
const badReturn: CallbackTarget = { fn: (x: number) => ({ value: \"wrong\" }) };
interface Missing { required: number }
const missing: Missing = {};
declare const deep: { p: { a: { x: string } }; q: string };
const badDeep: Target = deep;
";
    let options = tsc_types::CompilerOptions {
        strict: Some(true),
        ..tsc_types::CompilerOptions::default()
    };
    crate::state::test_support::with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let messages = state
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 2322 | 2741))
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    tsc_diagnostics::render::flattened_message(&diagnostic.message, "\n", None)
                        .as_str()
                        .expect("scalar diagnostic observation")
                        .to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                (2322, "Type 'string' is not assignable to type 'number'.".to_owned()),
                (2322, "Type '{ kind: \"a\"; value: string; }' is not assignable to type 'Choice'.\n  Types of property 'value' are incompatible.\n    Type 'string' is not assignable to type 'number'.".to_owned()),
                (2322, "Type '(x: number) => { value: string; }' is not assignable to type '(x: number) => { value: number; }'.\n  Type '{ value: string; }' is not assignable to type '{ value: number; }'.\n    Types of property 'value' are incompatible.\n      Type 'string' is not assignable to type 'number'.".to_owned()),
                (2741, "Property 'required' is missing in type '{}' but required in type 'Missing'.".to_owned()),
                (2322, "Type '{ p: { a: { x: string; }; }; q: string; }' is not assignable to type 'Target'.\n  The types of 'p.a.x' are incompatible between these types.\n    Type 'string' is not assignable to type 'number'.".to_owned()),
            ]
        );
    });
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
        error_chain: (0..depth.unwrap_or(0)).fold(None, |next, _| {
            Some(std::rc::Rc::new(ErrorChainEntry {
                next,
                message: &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
                args: Vec::new(),
            }))
        }),
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
