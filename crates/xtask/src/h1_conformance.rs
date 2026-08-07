use std::error::Error;

use tsc_harness::upstream_suites::h1_conformance::{
    check_recorded_manifest, generate_manifest, render_manifest, validate_manifest,
    ConformanceExpansionSummary, MANIFEST_RELATIVE_PATH,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManifestMode {
    Check,
    Write,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<ManifestMode, Box<dyn Error>> {
    let mut args = args;
    match args.next().as_deref() {
        Some("manifest") => {}
        Some(other) => return Err(format!("unknown h1-conformance command: {other}").into()),
        None => return Err("missing h1-conformance command (manifest)".into()),
    }

    let mut mode = None;
    for argument in args {
        let next = match argument.as_str() {
            "--check" => ManifestMode::Check,
            "--write" => ManifestMode::Write,
            other => {
                return Err(format!("unknown h1-conformance manifest argument: {other}").into())
            }
        };
        match mode {
            Some(previous) if previous == next => {
                return Err(
                    format!("duplicate h1-conformance manifest argument: {argument}").into(),
                )
            }
            Some(_) => {
                return Err(
                    "h1-conformance manifest accepts exactly one of --check or --write".into(),
                )
            }
            None => mode = Some(next),
        }
    }
    mode.ok_or_else(|| "missing h1-conformance manifest mode (--check|--write)".into())
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mode = parse_args(args)?;
    let workspace = super::find_workspace_root()?;
    match mode {
        ManifestMode::Check => {
            let summary = check_recorded_manifest(&workspace)?;
            print_summary("H1 conformance expansion manifest is current", &summary);
        }
        ManifestMode::Write => {
            let manifest = generate_manifest(&workspace)?;
            validate_manifest(&manifest)?;
            let rendered = render_manifest(&manifest)?;
            let path = super::upstream_suites::atomic_write_manifest(
                &workspace,
                MANIFEST_RELATIVE_PATH,
                &rendered,
                "H1 conformance expansion manifest",
            )?;
            let summary = check_recorded_manifest(&workspace)?;
            print_summary("wrote H1 conformance expansion manifest", &summary);
            println!("path: {}", path.display());
        }
    }
    Ok(())
}

fn print_summary(label: &str, summary: &ConformanceExpansionSummary) {
    println!("{label}");
    println!(
        "sources: files={} bytes={} unique_blobs={} enumerated_fixtures={} not_enumerated={}",
        summary.source_files,
        summary.source_bytes,
        summary.unique_blobs,
        summary.enumerated_fixtures,
        summary.not_enumerated_sources,
    );
    println!(
        "expansion: default_fixtures={} matrix_fixtures={} cases={} normal_units={} virtual_configs={}",
        summary.default_fixtures,
        summary.matrix_fixtures,
        summary.cases,
        summary.normal_units,
        summary.virtual_configs,
    );
    println!(
        "observations: runner={} case_observations={} not_run_cases={} not_run_observations={} results={} baselines_compared={}",
        summary.runner_observations,
        summary.case_observations,
        summary.not_run_cases,
        summary.not_run_case_observations,
        summary.execution_results_recorded,
        summary.reference_baselines_compared,
    );
}

#[cfg(test)]
#[path = "../tests/unit/h1_conformance/tests.rs"]
mod tests;
