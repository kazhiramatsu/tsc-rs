use super::parse_semantic_history_args;

fn parse(values: &[&str]) -> Result<String, String> {
    parse_semantic_history_args(values.iter().map(|value| (*value).to_owned()))
        .map_err(|error| error.to_string())
}

#[test]
fn requires_one_explicit_baseline() {
    assert_eq!(parse(&["--baseline", "base-sha"]).unwrap(), "base-sha");
    assert!(parse(&[]).unwrap_err().contains("requires --baseline"));
    assert!(parse(&["--baseline"])
        .unwrap_err()
        .contains("missing value"));
    assert!(parse(&["--baseline", "--unknown"])
        .unwrap_err()
        .contains("missing value"));
    assert!(parse(&["--baseline", "a", "--baseline", "b"])
        .unwrap_err()
        .contains("duplicate"));
    assert!(parse(&["base-sha"])
        .unwrap_err()
        .contains("unexpected semantic-history argument"));
}
