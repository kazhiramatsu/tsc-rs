//! User preferences against tsgo's: `fixtures/preferences.json` pairs the
//! client's configuration sections with what tsgo `aa8149273` makes of them
//! (`ParseUserPreferences`, then `json.Marshal` of the preferences), recorded
//! with a Go probe in tsgo's `ls/lsutil` package.

use serde_json::Value;
use tsc_ls::lsutil::parse_user_preferences;

#[test]
fn preferences_match_tsgo() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/preferences.json")).unwrap();
    for (index, case) in fixture.as_array().unwrap().iter().enumerate() {
        let sections = case["sections"].as_object().unwrap();
        assert_eq!(
            parse_user_preferences(sections).to_config(),
            case["tsgo"],
            "case {index}: {}",
            case["sections"]
        );
    }
}
