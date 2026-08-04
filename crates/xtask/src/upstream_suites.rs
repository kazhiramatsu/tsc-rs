use std::error::Error;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tsc_harness::upstream_suites::{
    check_recorded_manifest, generate_manifest, render_manifest, validate_manifest,
    ExpansionSummary, MANIFEST_RELATIVE_PATH,
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
        Some(other) => return Err(format!("unknown upstream-suites command: {other}").into()),
        None => return Err("missing upstream-suites command (manifest)".into()),
    }

    let mut mode = None;
    for arg in args {
        let next = match arg.as_str() {
            "--check" => ManifestMode::Check,
            "--write" => ManifestMode::Write,
            other => {
                return Err(format!("unknown upstream-suites manifest argument: {other}").into())
            }
        };
        match mode {
            Some(previous) if previous == next => {
                return Err(format!("duplicate upstream-suites manifest argument: {arg}").into())
            }
            Some(_) => {
                return Err(
                    "upstream-suites manifest accepts exactly one of --check or --write".into(),
                )
            }
            None => mode = Some(next),
        }
    }

    mode.ok_or_else(|| "missing upstream-suites manifest mode (--check|--write)".into())
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mode = parse_args(args)?;
    let workspace = super::find_workspace_root()?;
    match mode {
        ManifestMode::Check => {
            let summary = check_recorded_manifest(&workspace)?;
            print_summary("upstream suite expansion manifest is current", &summary);
        }
        ManifestMode::Write => {
            let manifest = generate_manifest(&workspace)?;
            // Reject an invalid generated value before publishing it. The
            // recorded-file check below then reparses and validates the exact
            // bytes that reached the fixed workspace path.
            validate_manifest(&manifest)?;
            let rendered = render_manifest(&manifest)?;
            let path = atomic_write_manifest(&workspace, &rendered)?;
            let summary = check_recorded_manifest(&workspace)?;
            print_summary("wrote upstream suite expansion manifest", &summary);
            println!("path: {}", path.display());
        }
    }
    Ok(())
}

fn print_summary(label: &str, summary: &ExpansionSummary) {
    println!("{label}");
    println!(
        "corpus: files={} bytes={}",
        summary.corpus_files, summary.corpus_bytes
    );
    println!(
        "compiler: sources={} default_fixtures={} matrix_fixtures={} cases={} normal_units={} virtual_configs={} present_empty_units={} missing_content_units={} link_directives={} document_symlink_directives={} document_symlink_paths={}",
        summary.compiler_sources,
        summary.compiler_default_fixtures,
        summary.compiler_matrix_fixtures,
        summary.compiler_cases,
        summary.compiler_normal_units,
        summary.compiler_virtual_configs,
        summary.compiler_present_empty_units,
        summary.compiler_missing_content_units,
        summary.compiler_link_directives,
        summary.compiler_document_symlink_directives,
        summary.compiler_document_symlink_paths,
    );
    println!(
        "projects: descriptors={} backing_files={} cases={} declared_inputs={} missing_backing_inputs={}",
        summary.project_descriptors,
        summary.project_backing_files,
        summary.project_cases,
        summary.project_declared_inputs,
        summary.project_missing_backing_inputs,
    );
    println!(
        "total: cases={} not_run_cases={}",
        summary.total_cases, summary.not_run_cases
    );
}

fn atomic_write_manifest(workspace: &Path, bytes: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let path = workspace.join(MANIFEST_RELATIVE_PATH);
    let relative = path.strip_prefix(workspace).map_err(|_| {
        format!(
            "upstream suite manifest path escaped the workspace: {}",
            path.display()
        )
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "upstream suite manifest path is not a normalized workspace path: {}",
            path.display()
        )
        .into());
    }

    let canonical_workspace = workspace.canonicalize()?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("manifest path has no parent: {}", path.display()))?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(&canonical_workspace) {
        return Err(format!(
            "upstream suite manifest parent escaped the workspace: {}",
            parent.display()
        )
        .into());
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("manifest path has no file name: {}", path.display()))?;
    let target = canonical_parent.join(file_name);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(format!(
                "upstream suite manifest target is not a regular file: {}",
                target.display()
            )
            .into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = canonical_parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        sequence
    ));
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &target)?;
        let metadata = fs::symlink_metadata(&target)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("published upstream suite manifest is not a regular file".into());
        }
        if fs::read(&target)? != bytes {
            return Err("upstream suite manifest changed during atomic publication".into());
        }
        fs::File::open(&canonical_parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(path)
}

#[cfg(test)]
mod tests {
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
}
