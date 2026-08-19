use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::{DocumentMut, Item, TableLike};
use yaml_rust2::{Yaml, YamlLoader};

use crate::workspace_catalog::{WorkspaceCatalog, WorkspacePackage};

const PROFILE_BLOCK_BEGIN: &str = "# BEGIN GENERATED: cargo xtask workspace sync";
const PROFILE_BLOCK_END: &str = "# END GENERATED: cargo xtask workspace sync";
const PACKAGE_SHORT_FLAG: &str = concat!("-", "p");
const PACKAGE_LONG_FLAG: &str = concat!("--", "package");
const BINARY_LONG_FLAG: &str = concat!("--", "bin");
const EXCLUDE_LONG_FLAG: &str = concat!("--", "exclude");

#[derive(Debug, PartialEq, Eq)]
enum AutomationYamlError {
    InvalidYaml(String),
    NonStringStepField {
        field: &'static str,
        value_kind: &'static str,
    },
}

impl std::fmt::Display for AutomationYamlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidYaml(error) => write!(formatter, "invalid YAML: {error}"),
            Self::NonStringStepField { field, value_kind } => write!(
                formatter,
                "step `{field}` value must be a YAML string, found {value_kind}"
            ),
        }
    }
}

impl Error for AutomationYamlError {}

#[derive(Debug, Default, PartialEq, Eq)]
struct AutomationYaml {
    run_scripts: Vec<String>,
    local_action_uses: Vec<String>,
}

pub(crate) fn run_workspace_command(
    mut args: impl Iterator<Item = String>,
    workspace: &Path,
) -> Result<(), Box<dyn Error>> {
    let command = args
        .next()
        .ok_or("missing workspace command (audit|sync)")?;
    if let Some(extra) = args.next() {
        return Err(format!("unexpected workspace {command} argument: {extra}").into());
    }

    match command.as_str() {
        "audit" => audit(workspace),
        "sync" => sync(workspace),
        _ => Err(format!("unknown workspace command: {command} (expected audit|sync)").into()),
    }
}

pub(crate) fn run_role_test(
    mut args: impl Iterator<Item = String>,
    workspace: &Path,
) -> Result<(), Box<dyn Error>> {
    let role = args.next().ok_or("test requires a workspace role")?;
    let catalog = WorkspaceCatalog::discover(workspace)?;
    let package = catalog.require_package(&role)?;

    let status = Command::new("cargo")
        .current_dir(workspace)
        .arg("test")
        .arg("--manifest-path")
        .arg(package.manifest_path())
        .args(args)
        .status()?;
    if !status.success() {
        return Err(
            format!("cargo test for workspace role `{role}` failed with {status:?}").into(),
        );
    }
    Ok(())
}

/// Verifies every name-sensitive workspace contract without changing files.
pub(crate) fn audit(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = WorkspaceCatalog::discover(workspace)?;
    if fs::canonicalize(catalog.workspace_root())? != fs::canonicalize(workspace)? {
        return Err(format!(
            "cargo metadata resolved workspace root {}, expected {}",
            catalog.workspace_root().display(),
            workspace.display()
        )
        .into());
    }
    let fuzz = catalog.require_package("fuzz")?;
    fuzz.require_default_run_target()?;

    audit_generated_profile(workspace, &catalog)?;
    let aliases = audit_workspace_dependencies(workspace, &catalog)?;
    audit_xtask_bootstrap_alias(workspace, &catalog)?;
    audit_automation_package_selectors(workspace)?;
    audit_xtask_cargo_selectors(workspace)?;
    audit_unit_test_layout(&catalog)?;

    println!(
        "workspace audit passed ({} roles, {} shared dependency aliases)",
        catalog.packages().count(),
        aliases.len()
    );
    Ok(())
}

fn audit_unit_test_layout(catalog: &WorkspaceCatalog) -> Result<(), Box<dyn Error>> {
    for package in catalog.packages() {
        let crate_root = package.manifest_path().parent().ok_or_else(|| {
            format!(
                "manifest has no parent: {}",
                package.manifest_path().display()
            )
        })?;
        let tests = crate_root.join("tests");
        if !tests.is_dir() {
            return Err(format!(
                "workspace crate `{}` must own a tests/ directory",
                package.package_name()
            )
            .into());
        }
        let integration_targets = fs::read_dir(&tests)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|path| {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            })
            .collect::<Vec<_>>();
        if integration_targets.len() > 2 {
            return Err(format!(
                "workspace crate `{}` has {} direct integration targets; consolidate broad contracts behind one tests/*.rs module tree and keep at most one focused hosted-canary target",
                package.package_name(),
                integration_targets.len()
            )
            .into());
        }

        let mut pending = VecDeque::from([crate_root.join("src")]);
        while let Some(directory) = pending.pop_front() {
            for entry in fs::read_dir(&directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push_back(path);
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                    continue;
                }
                let text = fs::read_to_string(&path)?;
                if let Some(line) = first_inline_test_module_line(&text) {
                    return Err(format!(
                        "{}:{line} defines tests inline; move the module body below the crate's tests/unit/ tree and retain only a #[path] declaration in src",
                        path.display()
                    )
                    .into());
                }
                if package.package_name() == "tsc-rs-emitter" {
                    if let Some((line, identifier)) = first_retired_comment_scope_identifier(&text)
                    {
                        return Err(format!(
                            "{}:{line} references the retired contextless identifier `{identifier}`; every emission route threads EmitContext (h2-5h-a CS-5 deletion, pinned by CS-6)",
                            path.display()
                        )
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}

/// The comment-scope ladder retired the printer's contextless emission
/// surface (h2-5h-a CS-5) and CS-6 pins the deletion permanently: the
/// detached constructor may not reappear anywhere in the emitter, and the
/// five shim names may not be redefined — the `_with_context` family IS
/// the emission API.
fn first_retired_comment_scope_identifier(text: &str) -> Option<(usize, &'static str)> {
    const RETIRED_TOKEN: &str = "detached_transitional";
    const RETIRED_DEFINITIONS: [&str; 5] = [
        "fn emit_required_node(",
        "fn emit_node_id(",
        "fn emit_identifier_name(",
        "fn emit_required_identifier_name(",
        "fn emit_child_after_token(",
    ];
    for (index, line) in text.lines().enumerate() {
        if line.contains(RETIRED_TOKEN) {
            return Some((index + 1, RETIRED_TOKEN));
        }
        for definition in RETIRED_DEFINITIONS {
            if line.contains(definition) {
                return Some((index + 1, definition));
            }
        }
    }
    None
}

fn first_inline_test_module_line(text: &str) -> Option<usize> {
    let lines = text.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        for candidate in &lines[index + 1..] {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            if candidate.starts_with("#[path") {
                break;
            }
            if candidate.starts_with("#[") {
                continue;
            }
            let module = candidate.strip_prefix("mod ").or_else(|| {
                candidate
                    .strip_prefix("pub ")
                    .and_then(|candidate| candidate.split_once("mod ").map(|(_, module)| module))
                    .or_else(|| {
                        candidate.strip_prefix("pub(").and_then(|candidate| {
                            candidate.split_once("mod ").map(|(_, module)| module)
                        })
                    })
            });
            if module.is_some_and(|module| module.contains('{')) {
                return Some(index + 1);
            }
            break;
        }
    }
    None
}

/// Rewrites only the explicitly marked profile block from package role metadata.
pub(crate) fn sync(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = WorkspaceCatalog::discover(workspace)?;
    let cargo_toml = workspace.join("Cargo.toml");
    let current = fs::read_to_string(&cargo_toml)?;
    let expected = render_profile_block(&catalog)?;
    let updated = replace_profile_block(&current, &expected)?;

    if updated == current {
        println!("workspace profile is already synchronized");
    } else {
        fs::write(&cargo_toml, updated)?;
        println!("synchronized workspace profile in {}", cargo_toml.display());
    }
    Ok(())
}

fn audit_generated_profile(
    workspace: &Path,
    catalog: &WorkspaceCatalog,
) -> Result<(), Box<dyn Error>> {
    let cargo_toml = workspace.join("Cargo.toml");
    let current = fs::read_to_string(&cargo_toml)?;
    let range = profile_block_range(&current)?;
    let expected = render_profile_block(catalog)?;
    if current[range] != expected {
        return Err("generated dev profile is stale; run `cargo xtask workspace sync`".into());
    }
    Ok(())
}

fn render_profile_block(catalog: &WorkspaceCatalog) -> Result<String, Box<dyn Error>> {
    let mut rendered = String::new();
    writeln!(rendered, "{PROFILE_BLOCK_BEGIN}")?;
    for package in catalog
        .packages()
        .filter(|package| package.dev_profile_opt_level().is_some())
    {
        let level = package
            .dev_profile_opt_level()
            .expect("filtered packages have an opt level");
        writeln!(rendered, "[profile.dev.package.{}]", package.package_name())?;
        writeln!(rendered, "opt-level = {level}")?;
    }
    write!(rendered, "{PROFILE_BLOCK_END}")?;
    Ok(rendered)
}

fn replace_profile_block(current: &str, expected: &str) -> Result<String, Box<dyn Error>> {
    let range = profile_block_range(current)?;
    let mut updated = String::with_capacity(current.len() - range.len() + expected.len());
    updated.push_str(&current[..range.start]);
    updated.push_str(expected);
    updated.push_str(&current[range.end..]);
    Ok(updated)
}

fn profile_block_range(text: &str) -> Result<std::ops::Range<usize>, Box<dyn Error>> {
    let begins = text.match_indices(PROFILE_BLOCK_BEGIN).collect::<Vec<_>>();
    let ends = text.match_indices(PROFILE_BLOCK_END).collect::<Vec<_>>();
    if begins.len() != 1 || ends.len() != 1 {
        return Err(format!(
            "Cargo.toml must contain exactly one generated profile marker pair (found {} begin, {} end)",
            begins.len(),
            ends.len()
        )
        .into());
    }
    let start = begins[0].0;
    let end = ends[0].0 + PROFILE_BLOCK_END.len();
    if start >= ends[0].0 {
        return Err("generated profile end marker appears before its begin marker".into());
    }
    Ok(start..end)
}

fn audit_workspace_dependencies(
    workspace: &Path,
    catalog: &WorkspaceCatalog,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let root_manifest = workspace.join("Cargo.toml");
    let root_document = read_toml(&root_manifest)?;
    let workspace_table = root_document
        .get("workspace")
        .and_then(Item::as_table_like)
        .ok_or("root Cargo.toml is missing [workspace]")?;
    let dependencies = workspace_table
        .get("dependencies")
        .and_then(Item::as_table_like)
        .ok_or("root Cargo.toml is missing [workspace.dependencies]")?;

    let packages_by_name = catalog
        .packages()
        .map(|package| (package.package_name(), package))
        .collect::<BTreeMap<_, _>>();
    let mut aliases_by_package = BTreeMap::<String, String>::new();

    for (alias, item) in dependencies.iter() {
        let Some(fields) = item.as_table_like() else {
            continue;
        };
        let package_name = table_string(fields, "package").unwrap_or(alias);
        let Some(package) = packages_by_name.get(package_name) else {
            continue;
        };
        let path = table_string(fields, "path").ok_or_else(|| {
            format!("workspace dependency alias `{alias}` must declare its package path")
        })?;
        let declared_dir = fs::canonicalize(workspace.join(path))?;
        let package_dir = fs::canonicalize(
            package
                .manifest_path()
                .parent()
                .ok_or("workspace package manifest has no parent")?,
        )?;
        if declared_dir != package_dir {
            return Err(format!(
                "workspace dependency alias `{alias}` points to {}, but package `{package_name}` is at {}",
                declared_dir.display(),
                package_dir.display()
            )
            .into());
        }
        if let Some(first) = aliases_by_package.insert(package_name.to_owned(), alias.to_owned()) {
            return Err(format!(
                "workspace package `{package_name}` has duplicate dependency aliases `{first}` and `{alias}`"
            )
            .into());
        }
    }

    for package in catalog
        .packages()
        .filter(|package| package.role() != "xtask")
    {
        if !aliases_by_package.contains_key(package.package_name()) {
            return Err(format!(
                "workspace package `{}` is missing a root workspace dependency alias",
                package.package_name()
            )
            .into());
        }
    }

    let aliases = aliases_by_package
        .values()
        .cloned()
        .collect::<BTreeSet<_>>();
    let package_dirs = catalog
        .packages()
        .map(|package| {
            package
                .manifest_path()
                .parent()
                .ok_or_else(|| "workspace package manifest has no parent".into())
                .and_then(|path| fs::canonicalize(path).map_err(Into::into))
        })
        .collect::<Result<BTreeSet<PathBuf>, Box<dyn Error>>>()?;

    for package in catalog.packages() {
        audit_member_dependencies(package, &aliases, &package_dirs)?;
    }

    Ok(aliases)
}

fn audit_member_dependencies(
    package: &WorkspacePackage,
    workspace_aliases: &BTreeSet<String>,
    package_dirs: &BTreeSet<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let document = read_toml(package.manifest_path())?;
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(dependencies) = document.get(section).and_then(Item::as_table_like) else {
            continue;
        };
        audit_dependency_table(
            package,
            section,
            dependencies,
            workspace_aliases,
            package_dirs,
        )?;
    }

    if let Some(targets) = document.get("target").and_then(Item::as_table_like) {
        for (target, item) in targets.iter() {
            let Some(target_table) = item.as_table_like() else {
                continue;
            };
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                let Some(dependencies) = target_table.get(section).and_then(Item::as_table_like)
                else {
                    continue;
                };
                audit_dependency_table(
                    package,
                    &format!("target.{target}.{section}"),
                    dependencies,
                    workspace_aliases,
                    package_dirs,
                )?;
            }
        }
    }
    Ok(())
}

fn audit_dependency_table(
    package: &WorkspacePackage,
    section: &str,
    dependencies: &dyn TableLike,
    workspace_aliases: &BTreeSet<String>,
    package_dirs: &BTreeSet<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for (alias, item) in dependencies.iter() {
        if workspace_aliases.contains(alias) {
            let inherited = item
                .as_table_like()
                .and_then(|fields| fields.get("workspace"))
                .and_then(Item::as_bool)
                == Some(true);
            if !inherited {
                return Err(format!(
                    "{} [{section}] dependency `{alias}` must use `.workspace = true`",
                    package.manifest_path().display()
                )
                .into());
            }
        }

        let Some(path) = item
            .as_table_like()
            .and_then(|fields| table_string(fields, "path"))
        else {
            continue;
        };
        let manifest_dir = package
            .manifest_path()
            .parent()
            .ok_or("workspace package manifest has no parent")?;
        let dependency_dir = fs::canonicalize(manifest_dir.join(path))?;
        if package_dirs.contains(&dependency_dir) {
            return Err(format!(
                "{} [{section}] dependency `{alias}` uses a direct workspace path; inherit the root alias instead",
                package.manifest_path().display()
            )
            .into());
        }
    }
    Ok(())
}

fn table_string<'a>(table: &'a dyn TableLike, key: &str) -> Option<&'a str> {
    table.get(key).and_then(Item::as_str)
}

fn read_toml(path: &Path) -> Result<DocumentMut, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    text.parse::<DocumentMut>()
        .map_err(|error| format!("invalid TOML at {}: {error}", path.display()).into())
}

fn audit_automation_package_selectors(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let mut workflow_files = Vec::new();
    collect_files(&workspace.join(".github/workflows"), &mut workflow_files)?;
    workflow_files.retain(|path| is_yaml_manifest(path));
    workflow_files.sort();

    let mut pending_local_actions = VecDeque::new();
    for path in workflow_files {
        for reference in audit_yaml_automation_file(&path)? {
            pending_local_actions.push_back((path.clone(), reference));
        }
    }
    audit_referenced_local_actions(workspace, pending_local_actions)?;

    let mut script_files = Vec::new();
    collect_files(&workspace.join("scripts"), &mut script_files)?;
    for path in script_files {
        let text = fs::read_to_string(&path)?;
        if let Some(selector) = automation_cargo_selector(&text) {
            return Err(format!(
                "automation file {} contains direct Cargo selector `{selector}`; use workspace gates, a manifest path/default-run, or an xtask role",
                path.display(),
            )
            .into());
        }
    }
    Ok(())
}

fn audit_yaml_automation_file(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let analysis = match analyze_automation_yaml(&text) {
        Ok(analysis) => analysis,
        Err(error) => {
            return Err(format!(
                "automation file {} cannot be audited safely: {error}",
                path.display()
            )
            .into());
        }
    };
    if let Some(selector) = analysis
        .run_scripts
        .iter()
        .find_map(|script| automation_cargo_selector(script))
    {
        return Err(format!(
            "automation file {} contains direct Cargo selector `{selector}`; use workspace gates, a manifest path/default-run, or an xtask role",
            path.display(),
        )
        .into());
    }
    Ok(analysis.local_action_uses)
}

fn audit_referenced_local_actions(
    workspace: &Path,
    mut pending: VecDeque<(PathBuf, String)>,
) -> Result<(), Box<dyn Error>> {
    let workspace_root = fs::canonicalize(workspace)?;
    let mut visited = BTreeSet::new();
    while let Some((source, reference)) = pending.pop_front() {
        let manifest = resolve_local_action_manifest(&workspace_root, &reference).map_err(
            |error| {
                format!(
                    "automation file {} references local action `{reference}` that cannot be audited safely: {error}",
                    source.display()
                )
            },
        )?;
        if !visited.insert(manifest.clone()) {
            continue;
        }
        for nested_reference in audit_yaml_automation_file(&manifest)? {
            pending.push_back((manifest.clone(), nested_reference));
        }
    }
    Ok(())
}

fn resolve_local_action_manifest(
    workspace_root: &Path,
    reference: &str,
) -> Result<PathBuf, String> {
    let relative = local_action_relative_directory(reference)?;
    let declared_directory = workspace_root.join(relative);
    let action_directory = fs::canonicalize(&declared_directory).map_err(|error| {
        format!(
            "directory {} does not exist or cannot be resolved: {error}",
            declared_directory.display()
        )
    })?;
    if !action_directory.starts_with(workspace_root) {
        return Err(format!(
            "directory {} resolves outside workspace {}",
            action_directory.display(),
            workspace_root.display()
        ));
    }
    if !action_directory.is_dir() {
        return Err(format!(
            "{} is not an action directory",
            action_directory.display()
        ));
    }

    let entries = fs::read_dir(&action_directory)
        .map_err(|error| format!("cannot read {}: {error}", action_directory.display()))?;
    let mut manifests = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot read {}: {error}", action_directory.display()))?
            .path();
        if is_action_manifest(&path) {
            manifests.push(path);
        }
    }
    manifests.sort();
    let manifest = match manifests.as_slice() {
        [] => {
            return Err(format!(
                "directory {} has no action.yml or action.yaml",
                action_directory.display()
            ));
        }
        [manifest] => manifest,
        _ => {
            return Err(format!(
                "directory {} contains both action.yml and action.yaml",
                action_directory.display()
            ));
        }
    };
    if !manifest.is_file() {
        return Err(format!("{} is not a regular file", manifest.display()));
    }
    let manifest = fs::canonicalize(manifest)
        .map_err(|error| format!("cannot resolve {}: {error}", manifest.display()))?;
    if !manifest.starts_with(workspace_root) {
        return Err(format!(
            "manifest {} resolves outside workspace {}",
            manifest.display(),
            workspace_root.display()
        ));
    }
    Ok(manifest)
}

fn local_action_relative_directory(reference: &str) -> Result<PathBuf, String> {
    if !reference.starts_with("./") {
        return Err("local action reference must start with `./`".to_owned());
    }
    let mut relative = PathBuf::new();
    for component in Path::new(reference).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(component) => relative.push(component),
            std::path::Component::ParentDir => {
                return Err(
                    "local action reference may not contain `..` because it could escape the workspace"
                        .to_owned(),
                );
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err("local action reference must be workspace-relative".to_owned());
            }
        }
    }
    Ok(relative)
}

fn is_yaml_manifest(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yml" | "yaml")
    )
}

fn is_action_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("action.yml" | "action.yaml")
    )
}

fn audit_xtask_bootstrap_alias(
    workspace: &Path,
    catalog: &WorkspaceCatalog,
) -> Result<(), Box<dyn Error>> {
    let xtask = catalog.require_package("xtask")?;
    let config_path = workspace.join(".cargo/config.toml");
    let document = read_toml(&config_path)?;
    let configured = document
        .get("alias")
        .and_then(Item::as_table_like)
        .and_then(|aliases| aliases.get("xtask"))
        .and_then(Item::as_str)
        .ok_or(".cargo/config.toml must define the xtask bootstrap alias")?;
    let expected = format!("run -p {} --", xtask.package_name());
    if configured != expected {
        return Err(format!(
            "xtask bootstrap alias must be `{expected}` so it works from every workspace directory"
        )
        .into());
    }
    Ok(())
}

fn audit_xtask_cargo_selectors(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files(&workspace.join("crates/xtask/src"), &mut files)?;
    for path in files {
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        if let Some(selector) = rust_source_cargo_selector(&text) {
            return Err(format!(
                "xtask source {} contains Cargo selector `{selector}`; resolve packages by role and use a manifest path/default-run instead",
                path.display(),
            )
            .into());
        }
    }
    Ok(())
}

fn collect_files(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        output.push(path.to_owned());
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_files(&entry_path, output)?;
        } else if entry_path.is_file() {
            output.push(entry_path);
        }
    }
    Ok(())
}

fn is_cargo_selector(token: &str) -> bool {
    token == PACKAGE_SHORT_FLAG
        || token
            .strip_prefix(PACKAGE_SHORT_FLAG)
            .is_some_and(|value| !value.is_empty())
        || token == PACKAGE_LONG_FLAG
        || token.starts_with(&format!("{PACKAGE_LONG_FLAG}="))
        || token == BINARY_LONG_FLAG
        || token.starts_with(&format!("{BINARY_LONG_FLAG}="))
        || token == EXCLUDE_LONG_FLAG
        || token.starts_with(&format!("{EXCLUDE_LONG_FLAG}="))
}

#[cfg(test)]
fn workflow_cargo_selector(text: &str) -> Result<Option<String>, AutomationYamlError> {
    Ok(analyze_automation_yaml(text)?
        .run_scripts
        .into_iter()
        .find_map(|script| automation_cargo_selector(&script)))
}

#[cfg(test)]
fn workflow_run_scripts(text: &str) -> Result<Vec<String>, AutomationYamlError> {
    Ok(analyze_automation_yaml(text)?.run_scripts)
}

fn analyze_automation_yaml(text: &str) -> Result<AutomationYaml, AutomationYamlError> {
    let documents = YamlLoader::load_from_str(text)
        .map_err(|error| AutomationYamlError::InvalidYaml(error.to_string()))?;
    let mut analysis = AutomationYaml::default();
    for document in &documents {
        collect_automation_steps(document, &mut analysis)?;
    }
    Ok(analysis)
}

fn collect_automation_steps(
    node: &Yaml,
    analysis: &mut AutomationYaml,
) -> Result<(), AutomationYamlError> {
    match node {
        Yaml::Hash(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some("steps") {
                    if let Yaml::Array(steps) = value {
                        collect_step_automation(steps, analysis)?;
                    }
                }
                collect_automation_steps(value, analysis)?;
            }
        }
        Yaml::Array(items) => {
            for item in items {
                collect_automation_steps(item, analysis)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_step_automation(
    steps: &[Yaml],
    analysis: &mut AutomationYaml,
) -> Result<(), AutomationYamlError> {
    for step in steps {
        let Yaml::Hash(fields) = step else {
            continue;
        };
        for (key, value) in fields {
            match key.as_str() {
                Some("run") => {
                    let script = value
                        .as_str()
                        .ok_or(AutomationYamlError::NonStringStepField {
                            field: "run",
                            value_kind: yaml_value_kind(value),
                        })?;
                    analysis.run_scripts.push(script.to_owned());
                }
                Some("uses") => {
                    let reference =
                        value
                            .as_str()
                            .ok_or(AutomationYamlError::NonStringStepField {
                                field: "uses",
                                value_kind: yaml_value_kind(value),
                            })?;
                    if reference.starts_with("./") {
                        analysis.local_action_uses.push(reference.to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn yaml_value_kind(value: &Yaml) -> &'static str {
    match value {
        Yaml::Array(_) => "sequence",
        Yaml::Hash(_) => "mapping",
        Yaml::String(_) => "string",
        Yaml::Boolean(_) => "boolean",
        Yaml::Integer(_) | Yaml::Real(_) => "number",
        Yaml::Alias(_) => "unresolved alias",
        Yaml::Null => "null",
        Yaml::BadValue => "invalid value",
    }
}

fn automation_cargo_selector(text: &str) -> Option<String> {
    for command in shell_commands(text) {
        let Some(cargo_index) = cargo_command_index(&command) else {
            continue;
        };
        let arguments = &command[cargo_index + 1..];
        let cargo_arguments = arguments
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .collect::<Vec<_>>();
        if let Some((selector_index, selector)) = cargo_arguments
            .iter()
            .enumerate()
            .find(|(_, argument)| is_cargo_selector(argument))
        {
            let is_forwarded_xtask_argument = cargo_subcommand(&cargo_arguments)
                .is_some_and(|(index, command)| command == "xtask" && index < selector_index);
            if !is_forwarded_xtask_argument {
                return Some(selector.to_string());
            }
        }
    }
    None
}

fn cargo_command_index(command: &[String]) -> Option<usize> {
    let mut index = 0;
    while let Some(word) = command.get(index) {
        if is_cargo_executable(word) {
            return Some(index);
        }
        if matches!(
            word.as_str(),
            "-" | "run:"
                | "if"
                | "elif"
                | "while"
                | "until"
                | "then"
                | "do"
                | "!"
                | "command"
                | "exec"
                | "env"
                | "sudo"
        ) || is_shell_assignment(word)
        {
            index += 1;
            continue;
        }
        return None;
    }
    None
}

fn is_cargo_executable(word: &str) -> bool {
    matches!(word, "cargo" | "$CARGO" | "${CARGO}")
        || Path::new(word).file_name().and_then(|name| name.to_str()) == Some("cargo")
}

fn is_shell_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn cargo_subcommand<'a>(arguments: &'a [&String]) -> Option<(usize, &'a str)> {
    let mut index = 0;
    while let Some(argument) = arguments.get(index).map(|argument| argument.as_str()) {
        if argument.starts_with('+') {
            index += 1;
            continue;
        }
        if matches!(argument, "--color" | "--config" | "-Z") {
            index += 2;
            continue;
        }
        if argument.starts_with('-') {
            index += 1;
            continue;
        }
        return Some((index, argument));
    }
    None
}

fn shell_commands(text: &str) -> Vec<Vec<String>> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    fn finish_word(word: &mut String, command: &mut Vec<String>) {
        if !word.is_empty() {
            command.push(std::mem::take(word));
        }
    }

    fn finish_command(
        word: &mut String,
        command: &mut Vec<String>,
        commands: &mut Vec<Vec<String>>,
    ) {
        finish_word(word, command);
        if !command.is_empty() {
            commands.push(std::mem::take(command));
        }
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut commands = Vec::new();
    let mut command = Vec::new();
    let mut word = String::new();
    let mut quote = Quote::None;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    word.push(character);
                }
                index += 1;
            }
            Quote::Double => {
                if character == '"' {
                    quote = Quote::None;
                    index += 1;
                } else if character == '\\' {
                    if chars.get(index + 1) == Some(&'\r') && chars.get(index + 2) == Some(&'\n') {
                        index += 3;
                    } else if chars.get(index + 1) == Some(&'\n') {
                        index += 2;
                    } else if let Some(escaped) = chars.get(index + 1) {
                        word.push(*escaped);
                        index += 2;
                    } else {
                        index += 1;
                    }
                } else {
                    word.push(character);
                    index += 1;
                }
            }
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    index += 1;
                }
                '"' => {
                    quote = Quote::Double;
                    index += 1;
                }
                '\\' => {
                    if chars.get(index + 1) == Some(&'\r') && chars.get(index + 2) == Some(&'\n') {
                        index += 3;
                    } else if chars.get(index + 1) == Some(&'\n') {
                        index += 2;
                    } else if let Some(escaped) = chars.get(index + 1) {
                        word.push(*escaped);
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
                '#' if word.is_empty() => {
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                }
                ' ' | '\t' | '\r' => {
                    finish_word(&mut word, &mut command);
                    index += 1;
                }
                '\n' | ';' | '|' | '&' => {
                    finish_command(&mut word, &mut command, &mut commands);
                    index += 1;
                    if matches!(character, '|' | '&') && chars.get(index) == Some(&character) {
                        index += 1;
                    }
                }
                _ => {
                    word.push(character);
                    index += 1;
                }
            },
        }
    }
    finish_command(&mut word, &mut command, &mut commands);
    commands
}

fn rust_source_cargo_selector(text: &str) -> Option<String> {
    for literal in rust_string_literals(text) {
        if is_cargo_selector(&literal) {
            return Some(literal);
        }
        if let Some(selector) = automation_cargo_selector(&literal) {
            return Some(selector);
        }
    }
    None
}

fn rust_string_literals(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            index = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| index + offset + 1);
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index = skip_rust_block_comment(bytes, index);
            continue;
        }
        if let Some((content_start, hash_count)) = rust_raw_string_start(bytes, index) {
            let (literal, next) = parse_rust_raw_string(text, content_start, hash_count);
            literals.push(literal);
            index = next;
            continue;
        }
        let quote_index = if bytes[index] == b'"' {
            Some(index)
        } else if matches!(bytes.get(index), Some(b'b' | b'c'))
            && bytes.get(index + 1) == Some(&b'"')
        {
            Some(index + 1)
        } else {
            None
        };
        if let Some(quote_index) = quote_index {
            let (literal, next) = parse_rust_cooked_string(text, quote_index);
            literals.push(literal);
            index = next;
            continue;
        }
        if bytes[index] == b'\'' {
            index = skip_rust_char_or_lifetime(bytes, index);
            continue;
        }
        index += 1;
    }
    literals
}

fn skip_rust_block_comment(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 0usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth -= 1;
            index += 2;
            if depth == 0 {
                break;
            }
        } else {
            index += 1;
        }
    }
    index
}

fn rust_raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') || bytes.get(cursor) == Some(&b'c') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hash_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some((cursor + 1, cursor - hash_start))
}

fn parse_rust_raw_string(text: &str, content_start: usize, hash_count: usize) -> (String, usize) {
    let closing = format!("\"{}", "#".repeat(hash_count));
    let Some(offset) = text[content_start..].find(&closing) else {
        return (text[content_start..].to_owned(), text.len());
    };
    let content_end = content_start + offset;
    (
        text[content_start..content_end].to_owned(),
        content_end + closing.len(),
    )
}

fn parse_rust_cooked_string(text: &str, quote_index: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut literal = String::new();
    let mut index = quote_index + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return (literal, index + 1),
            b'\\' => {
                index += 1;
                if index >= bytes.len() {
                    break;
                }
                match bytes[index] {
                    b'\n' => {
                        index += 1;
                        while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                            index += 1;
                        }
                    }
                    b'x' if index + 2 < bytes.len() => {
                        let hex = &text[index + 1..index + 3];
                        if let Ok(value) = u8::from_str_radix(hex, 16) {
                            literal.push(char::from(value));
                            index += 3;
                        } else {
                            index += 1;
                        }
                    }
                    b'u' if bytes.get(index + 1) == Some(&b'{') => {
                        let digits_start = index + 2;
                        if let Some(end_offset) = text[digits_start..].find('}') {
                            let digits_end = digits_start + end_offset;
                            let digits = text[digits_start..digits_end].replace('_', "");
                            if let Ok(value) = u32::from_str_radix(&digits, 16) {
                                if let Some(character) = char::from_u32(value) {
                                    literal.push(character);
                                }
                            }
                            index = digits_end + 1;
                        } else {
                            index += 1;
                        }
                    }
                    escaped => {
                        literal.push(match escaped {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'0' => '\0',
                            other => char::from(other),
                        });
                        index += 1;
                    }
                }
            }
            _ => {
                let Some(character) = text[index..].chars().next() else {
                    break;
                };
                literal.push(character);
                index += character.len_utf8();
            }
        }
    }
    (literal, text.len())
}

fn skip_rust_char_or_lifetime(bytes: &[u8], index: usize) -> usize {
    let mut cursor = index + 1;
    if bytes.get(cursor) == Some(&b'\\') {
        cursor += 2;
    } else if let Some(character) = std::str::from_utf8(&bytes[cursor..])
        .ok()
        .and_then(|text| text.chars().next())
    {
        cursor += character.len_utf8();
    }
    if bytes.get(cursor) == Some(&b'\'') {
        cursor + 1
    } else {
        index + 1
    }
}

#[cfg(test)]
#[path = "../tests/unit/workspace_maintenance/tests.rs"]
mod tests;
