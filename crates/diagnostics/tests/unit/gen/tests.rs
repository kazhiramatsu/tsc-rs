use super::*;

#[test]
fn generated_diagnostic_pins_match_typescript_7_1() {
    assert_eq!(Unterminated_string_literal.code, 1002);
    assert_eq!(_0_expected.code, 1005);
    assert_eq!(ALL_BY_CODE.len(), 2215);
    assert!(ALL_BY_CODE.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(
        A_label_is_not_allowed_here.text,
        "A label is not allowed here."
    );
}

/// The tsc 6.0.3 entries TypeScript 7.1 reworded (nine) or removed (TS1463
/// and TS1464) stay reachable for the 6.0.3 reference profile.
#[test]
fn legacy_module_holds_the_reworded_and_removed_tsc_6_0_3_entries() {
    assert_eq!(
        typescript_6_0_3::ALL_BY_CODE
            .iter()
            .map(|(code, _)| *code)
            .collect::<Vec<_>>(),
        [1344, 1463, 1464, 5074, 5090, 6048, 6353, 6401, 6420, 8030, 9019]
    );
    assert_eq!(
        typescript_6_0_3::A_label_is_not_allowed_here.text,
        "'A label is not allowed here."
    );
    for (code, legacy) in typescript_6_0_3::ALL_BY_CODE {
        assert_eq!(legacy.code, *code);
        assert!(super::super::by_code(*code).is_none_or(|current| current.text != legacy.text));
    }
}
