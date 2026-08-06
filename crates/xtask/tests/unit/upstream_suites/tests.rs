use super::*;

fn parse(values: &[&str]) -> Result<ManifestMode, Box<dyn Error>> {
    parse_args(values.iter().map(|value| (*value).to_owned()))
}

#[test]
fn accepts_only_the_fixed_manifest_modes() {
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
fn rejects_missing_or_unknown_commands_and_modes() {
    assert!(parse(&[]).is_err());
    assert!(parse(&["manifest"]).is_err());
    assert!(parse(&["expand", "--check"]).is_err());
    assert!(parse(&["manifest", "check"]).is_err());
}

#[test]
fn rejects_duplicate_or_conflicting_modes() {
    assert!(parse(&["manifest", "--check", "--check"]).is_err());
    assert!(parse(&["manifest", "--write", "--write"]).is_err());
    assert!(parse(&["manifest", "--check", "--write"]).is_err());
    assert!(parse(&["manifest", "--write", "--check"]).is_err());
}

#[test]
fn rejects_every_configurable_corpus_argument() {
    for argument in ["--suite", "--filter", "--limit", "--out"] {
        assert!(
            parse(&["manifest", "--check", argument]).is_err(),
            "accepted forbidden argument {argument}"
        );
    }
}
