use super::*;

#[test]
fn defaults_to_report_only() {
    assert_eq!(
        parse_fuzz_preflight_args(std::iter::empty()).unwrap(),
        FuzzPreflightArgs {
            require_ready: false,
        }
    );
}

#[test]
fn accepts_require_ready() {
    assert_eq!(
        parse_fuzz_preflight_args(["--require-ready"].into_iter().map(str::to_owned)).unwrap(),
        FuzzPreflightArgs {
            require_ready: true,
        }
    );
}

#[test]
fn rejects_every_other_argument() {
    assert!(parse_fuzz_preflight_args(["--ready"].into_iter().map(str::to_owned)).is_err());
    assert!(parse_fuzz_preflight_args(["extra"].into_iter().map(str::to_owned)).is_err());
}
