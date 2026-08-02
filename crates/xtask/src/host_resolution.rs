use std::error::Error;
use std::path::PathBuf;

use super::find_workspace_root;

pub(crate) fn draft(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let out = parse_draft_args(args)?;
    let workspace = find_workspace_root()?;
    let out = if out.is_absolute() {
        out
    } else {
        workspace.join(out)
    };
    tsc_conformance::draft_host_resolution_registry(&workspace, &out)
}

pub(crate) fn check(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let baseline = parse_check_args(args)?;
    let workspace = find_workspace_root()?;
    tsc_conformance::check_host_resolution_registry(&workspace, baseline.as_deref())
}

fn parse_draft_args(args: impl Iterator<Item = String>) -> Result<PathBuf, Box<dyn Error>> {
    let mut out = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                if out.is_some() {
                    return Err("duplicate --out".into());
                }
                out = Some(PathBuf::from(option_value(&mut args, "--out")?));
            }
            _ => return Err(format!("unexpected host-resolution draft argument: {arg}").into()),
        }
    }
    Ok(out.unwrap_or_else(|| PathBuf::from(tsc_conformance::HOST_RESOLUTION_REL_PATH)))
}

fn parse_check_args(args: impl Iterator<Item = String>) -> Result<Option<String>, Box<dyn Error>> {
    let mut baseline = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                if baseline.is_some() {
                    return Err("duplicate --baseline".into());
                }
                baseline = Some(option_value(&mut args, "--baseline")?);
            }
            _ => return Err(format!("unexpected host-resolution check argument: {arg}").into()),
        }
    }
    Ok(baseline)
}

fn option_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, Box<dyn Error>> {
    let value = args
        .next()
        .ok_or_else(|| format!("missing value after {option}"))?;
    if value.trim().is_empty() || value.starts_with('-') {
        return Err(format!("missing value after {option}").into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
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
}
