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
#[path = "../tests/unit/host_resolution/tests.rs"]
mod tests;
