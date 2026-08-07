use super::*;

fn parse(values: &[&str]) -> Result<ManifestMode, Box<dyn Error>> {
    parse_args(values.iter().map(|value| (*value).to_owned()))
}

#[test]
fn accepts_only_fixed_manifest_modes() {
    assert_eq!(
        parse(&["manifest", "--check"]).unwrap(),
        ManifestMode::Check
    );
    assert_eq!(
        parse(&["manifest", "--write"]).unwrap(),
        ManifestMode::Write
    );
}

#[test]
fn rejects_missing_unknown_duplicate_and_configurable_arguments() {
    assert!(parse(&[]).is_err());
    assert!(parse(&["manifest"]).is_err());
    assert!(parse(&["expand", "--check"]).is_err());
    assert!(parse(&["manifest", "--check", "--check"]).is_err());
    assert!(parse(&["manifest", "--check", "--write"]).is_err());
    for argument in ["--suite", "--filter", "--limit", "--out"] {
        assert!(
            parse(&["manifest", "--check", argument]).is_err(),
            "accepted forbidden argument {argument}"
        );
    }
}
