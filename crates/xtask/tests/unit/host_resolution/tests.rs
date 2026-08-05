use super::*;

fn strings(values: &[&str]) -> impl Iterator<Item = String> {
    values
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>()
        .into_iter()
}

fn draft_error(values: &[&str]) -> String {
    parse_draft_args(strings(values)).unwrap_err().to_string()
}

fn check_error(values: &[&str]) -> String {
    parse_check_args(strings(values)).unwrap_err().to_string()
}

#[test]
fn parses_default_and_explicit_arguments() {
    assert_eq!(
        parse_draft_args(strings(&[])).unwrap(),
        PathBuf::from(tsc_conformance::HOST_RESOLUTION_REL_PATH)
    );
    assert_eq!(
        parse_draft_args(strings(&["--out", "target/host-resolution.json"])).unwrap(),
        PathBuf::from("target/host-resolution.json")
    );
    assert_eq!(parse_check_args(strings(&[])).unwrap(), None);
    assert_eq!(
        parse_check_args(strings(&["--baseline", "origin/main"])).unwrap(),
        Some("origin/main".to_owned())
    );
}

#[test]
fn rejects_duplicate_options() {
    assert_eq!(
        draft_error(&["--out", "first.json", "--out", "second.json"]),
        "duplicate --out"
    );
    assert_eq!(
        check_error(&["--baseline", "first", "--baseline", "second"]),
        "duplicate --baseline"
    );
}

#[test]
fn rejects_missing_or_option_shaped_values() {
    for args in [vec!["--out"], vec!["--out", ""], vec!["--out", "--other"]] {
        assert_eq!(draft_error(&args), "missing value after --out");
    }
    for args in [
        vec!["--baseline"],
        vec!["--baseline", "   "],
        vec!["--baseline", "--other"],
    ] {
        assert_eq!(check_error(&args), "missing value after --baseline");
    }
}

#[test]
fn rejects_unknown_flags_and_positional_arguments() {
    assert_eq!(
        draft_error(&["--baseline", "HEAD"]),
        "unexpected host-resolution draft argument: --baseline"
    );
    assert_eq!(
        draft_error(&["extra"]),
        "unexpected host-resolution draft argument: extra"
    );
    assert_eq!(
        check_error(&["--out", "registry.json"]),
        "unexpected host-resolution check argument: --out"
    );
    assert_eq!(
        check_error(&["extra"]),
        "unexpected host-resolution check argument: extra"
    );
}
