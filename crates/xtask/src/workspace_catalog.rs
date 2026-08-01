use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

const ROLE_METADATA_POINTER: &str = "/tsc-rs/role";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceTarget {
    name: String,
    kinds: Vec<String>,
}

impl WorkspaceTarget {
    pub(crate) fn is_kind(&self, kind: &str) -> bool {
        self.kinds.iter().any(|candidate| candidate == kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspacePackage {
    role: String,
    package_name: String,
    manifest_path: PathBuf,
    default_run: Option<String>,
    dev_profile_opt_level: Option<u64>,
    targets: Vec<WorkspaceTarget>,
}

impl WorkspacePackage {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }

    pub(crate) fn package_name(&self) -> &str {
        &self.package_name
    }

    pub(crate) fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub(crate) fn dev_profile_opt_level(&self) -> Option<u64> {
        self.dev_profile_opt_level
    }

    pub(crate) fn require_default_run_target(
        &self,
    ) -> Result<&WorkspaceTarget, WorkspaceCatalogError> {
        let default_run = self.default_run.as_deref().ok_or_else(|| {
            WorkspaceCatalogError::MissingDefaultRun {
                package: self.package_name.clone(),
                manifest_path: self.manifest_path.clone(),
            }
        })?;
        self.targets
            .iter()
            .find(|target| target.name == default_run && target.is_kind("bin"))
            .ok_or_else(|| WorkspaceCatalogError::InvalidDefaultRunTarget {
                package: self.package_name.clone(),
                manifest_path: self.manifest_path.clone(),
                target: default_run.to_owned(),
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceCatalog {
    workspace_root: PathBuf,
    packages_by_role: BTreeMap<String, WorkspacePackage>,
}

impl WorkspaceCatalog {
    /// Loads the workspace inventory from Cargo itself, so package renames do not
    /// have to be duplicated in xtask.
    pub(crate) fn discover(workspace_root: &Path) -> Result<Self, WorkspaceCatalogError> {
        let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let output = Command::new(cargo)
            .current_dir(workspace_root)
            .arg("metadata")
            .arg("--no-deps")
            .arg("--format-version")
            .arg("1")
            .arg("--manifest-path")
            .arg(workspace_root.join("Cargo.toml"))
            .output()
            .map_err(WorkspaceCatalogError::SpawnCargoMetadata)?;

        if !output.status.success() {
            return Err(WorkspaceCatalogError::CargoMetadataFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        Self::from_metadata_json(&output.stdout)
    }

    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub(crate) fn packages(&self) -> impl Iterator<Item = &WorkspacePackage> {
        self.packages_by_role.values()
    }

    pub(crate) fn package_for_role(&self, role: &str) -> Option<&WorkspacePackage> {
        self.packages_by_role.get(role)
    }

    pub(crate) fn require_package(
        &self,
        role: &str,
    ) -> Result<&WorkspacePackage, WorkspaceCatalogError> {
        self.package_for_role(role)
            .ok_or_else(|| WorkspaceCatalogError::UnknownRole {
                role: role.to_owned(),
                available_roles: self.packages_by_role.keys().cloned().collect(),
            })
    }

    fn from_metadata_json(bytes: &[u8]) -> Result<Self, WorkspaceCatalogError> {
        let metadata: CargoMetadata =
            serde_json::from_slice(bytes).map_err(WorkspaceCatalogError::ParseCargoMetadata)?;
        let workspace_members = metadata
            .workspace_members
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut discovered_members = BTreeSet::new();
        let mut packages_by_role = BTreeMap::<String, WorkspacePackage>::new();

        for package in metadata
            .packages
            .into_iter()
            .filter(|package| workspace_members.contains(&package.id))
        {
            discovered_members.insert(package.id.clone());
            let role = package_role(&package)?;
            let dev_profile_opt_level = package_dev_profile_opt_level(&package)?;
            let workspace_package = WorkspacePackage {
                role: role.clone(),
                package_name: package.name,
                manifest_path: package.manifest_path,
                default_run: package.default_run,
                dev_profile_opt_level,
                targets: package
                    .targets
                    .into_iter()
                    .map(WorkspaceTarget::from)
                    .collect(),
            };

            if let Some(first) = packages_by_role.get(&role) {
                return Err(WorkspaceCatalogError::DuplicateRole {
                    role,
                    first_package: first.package_name.clone(),
                    second_package: workspace_package.package_name,
                });
            }
            packages_by_role.insert(role, workspace_package);
        }

        if discovered_members != workspace_members {
            return Err(WorkspaceCatalogError::MissingWorkspaceMembers {
                member_ids: workspace_members
                    .difference(&discovered_members)
                    .cloned()
                    .collect(),
            });
        }

        Ok(Self {
            workspace_root: metadata.workspace_root,
            packages_by_role,
        })
    }
}

fn package_role(package: &CargoPackage) -> Result<String, WorkspaceCatalogError> {
    let Some(value) = package.metadata.pointer(ROLE_METADATA_POINTER) else {
        return Err(WorkspaceCatalogError::MissingRole {
            package: package.name.clone(),
            manifest_path: package.manifest_path.clone(),
        });
    };
    let Value::String(role) = value else {
        return Err(WorkspaceCatalogError::NonStringRole {
            package: package.name.clone(),
            manifest_path: package.manifest_path.clone(),
        });
    };
    let role = role.trim();
    if role.is_empty() {
        return Err(WorkspaceCatalogError::EmptyRole {
            package: package.name.clone(),
            manifest_path: package.manifest_path.clone(),
        });
    }

    Ok(role.to_owned())
}

fn package_dev_profile_opt_level(
    package: &CargoPackage,
) -> Result<Option<u64>, WorkspaceCatalogError> {
    let Some(value) = package.metadata.pointer("/tsc-rs/dev-profile-opt-level") else {
        return Ok(None);
    };
    value
        .as_u64()
        .filter(|level| *level <= 3)
        .map(Some)
        .ok_or_else(|| WorkspaceCatalogError::InvalidDevProfileOptLevel {
            package: package.name.clone(),
            manifest_path: package.manifest_path.clone(),
        })
}

impl From<CargoTarget> for WorkspaceTarget {
    fn from(target: CargoTarget) -> Self {
        Self {
            name: target.name,
            kinds: target.kind,
        }
    }
}

#[derive(Debug)]
pub(crate) enum WorkspaceCatalogError {
    SpawnCargoMetadata(io::Error),
    CargoMetadataFailed {
        exit_code: Option<i32>,
        stderr: String,
    },
    ParseCargoMetadata(serde_json::Error),
    MissingWorkspaceMembers {
        member_ids: Vec<String>,
    },
    MissingRole {
        package: String,
        manifest_path: PathBuf,
    },
    NonStringRole {
        package: String,
        manifest_path: PathBuf,
    },
    EmptyRole {
        package: String,
        manifest_path: PathBuf,
    },
    InvalidDevProfileOptLevel {
        package: String,
        manifest_path: PathBuf,
    },
    DuplicateRole {
        role: String,
        first_package: String,
        second_package: String,
    },
    UnknownRole {
        role: String,
        available_roles: Vec<String>,
    },
    MissingDefaultRun {
        package: String,
        manifest_path: PathBuf,
    },
    InvalidDefaultRunTarget {
        package: String,
        manifest_path: PathBuf,
        target: String,
    },
}

impl fmt::Display for WorkspaceCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnCargoMetadata(error) => {
                write!(formatter, "failed to run `cargo metadata`: {error}")
            }
            Self::CargoMetadataFailed { exit_code, stderr } => {
                write!(
                    formatter,
                    "`cargo metadata` failed with exit code {}",
                    exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_owned())
                )?;
                if !stderr.is_empty() {
                    write!(formatter, ": {stderr}")?;
                }
                Ok(())
            }
            Self::ParseCargoMetadata(error) => {
                write!(formatter, "failed to parse `cargo metadata` output: {error}")
            }
            Self::MissingWorkspaceMembers { member_ids } => write!(
                formatter,
                "`cargo metadata` omitted workspace member(s): {}",
                member_ids.join(", ")
            ),
            Self::MissingRole {
                package,
                manifest_path,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) is missing package.metadata.tsc-rs.role",
                manifest_path.display()
            ),
            Self::NonStringRole {
                package,
                manifest_path,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) has a non-string package.metadata.tsc-rs.role",
                manifest_path.display()
            ),
            Self::EmptyRole {
                package,
                manifest_path,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) has an empty package.metadata.tsc-rs.role",
                manifest_path.display()
            ),
            Self::InvalidDevProfileOptLevel {
                package,
                manifest_path,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) has package.metadata.tsc-rs.dev-profile-opt-level outside the supported integer range 0..=3",
                manifest_path.display()
            ),
            Self::DuplicateRole {
                role,
                first_package,
                second_package,
            } => write!(
                formatter,
                "workspace role `{role}` is assigned to both `{first_package}` and `{second_package}`"
            ),
            Self::UnknownRole {
                role,
                available_roles,
            } => write!(
                formatter,
                "unknown workspace role `{role}` (available: {})",
                available_roles.join(", ")
            ),
            Self::MissingDefaultRun {
                package,
                manifest_path,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) does not define package.default-run",
                manifest_path.display()
            ),
            Self::InvalidDefaultRunTarget {
                package,
                manifest_path,
                target,
            } => write!(
                formatter,
                "workspace package `{package}` ({}) has package.default-run `{target}`, but no matching bin target",
                manifest_path.display()
            ),
        }
    }
}

impl Error for WorkspaceCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SpawnCargoMetadata(error) => Some(error),
            Self::ParseCargoMetadata(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    metadata: Value,
    #[serde(default)]
    default_run: Option<String>,
    targets: Vec<CargoTarget>,
}

#[derive(Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_workspace_packages_by_stable_role() {
        let catalog = WorkspaceCatalog::from_metadata_json(
            metadata_json(
                r#"{"tsc-rs":{"role":"checker","dev-profile-opt-level":3}}"#,
                r#"{"tsc-rs":{"role":"fuzz"}}"#,
            )
            .as_bytes(),
        )
        .expect("valid metadata");

        assert_eq!(catalog.workspace_root(), Path::new("/workspace"));
        let checker = catalog.require_package("checker").expect("checker role");
        assert_eq!(checker.role(), "checker");
        assert_eq!(checker.package_name(), "renamed-checker");
        assert_eq!(checker.default_run, None);
        assert_eq!(checker.dev_profile_opt_level(), Some(3));
        assert_eq!(
            checker.manifest_path(),
            Path::new("/workspace/crates/checker/Cargo.toml")
        );
        assert_eq!(checker.targets.len(), 1);
        assert_eq!(checker.targets[0].name, "checker_core");
        assert_eq!(checker.targets[0].kinds, &["lib"]);

        let fuzz = catalog.require_package("fuzz").expect("fuzz role");
        assert_eq!(fuzz.default_run.as_deref(), Some("fuzz-producer"));
        assert_eq!(fuzz.dev_profile_opt_level(), None);
        assert_eq!(
            fuzz.require_default_run_target()
                .expect("valid default-run")
                .name,
            "fuzz-producer"
        );
        assert_eq!(
            fuzz.targets
                .iter()
                .filter(|target| target.is_kind("bin"))
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>(),
            ["fuzz-producer"]
        );
        assert_eq!(catalog.packages().count(), 2);
        assert!(catalog.package_for_role("dependency").is_none());
    }

    #[test]
    fn rejects_a_workspace_package_without_a_role() {
        let error = WorkspaceCatalog::from_metadata_json(
            metadata_json("{}", r#"{"tsc-rs":{"role":"fuzz"}}"#).as_bytes(),
        )
        .expect_err("role is required");

        assert!(error
            .to_string()
            .contains("`renamed-checker` (/workspace/crates/checker/Cargo.toml) is missing"));
    }

    #[test]
    fn rejects_empty_and_non_string_roles() {
        let empty = WorkspaceCatalog::from_metadata_json(
            metadata_json(
                r#"{"tsc-rs":{"role":"  "}}"#,
                r#"{"tsc-rs":{"role":"fuzz"}}"#,
            )
            .as_bytes(),
        )
        .expect_err("empty role is invalid");
        assert!(empty.to_string().contains("has an empty"));

        let non_string = WorkspaceCatalog::from_metadata_json(
            metadata_json(r#"{"tsc-rs":{"role":42}}"#, r#"{"tsc-rs":{"role":"fuzz"}}"#).as_bytes(),
        )
        .expect_err("role must be a string");
        assert!(non_string.to_string().contains("has a non-string"));
    }

    #[test]
    fn rejects_duplicate_roles() {
        let error = WorkspaceCatalog::from_metadata_json(
            metadata_json(
                r#"{"tsc-rs":{"role":"compiler"}}"#,
                r#"{"tsc-rs":{"role":"compiler"}}"#,
            )
            .as_bytes(),
        )
        .expect_err("roles must be unique");

        assert_eq!(
            error.to_string(),
            "workspace role `compiler` is assigned to both `renamed-checker` and `renamed-fuzz`"
        );
    }

    #[test]
    fn rejects_invalid_dev_profile_opt_level() {
        let error = WorkspaceCatalog::from_metadata_json(
            metadata_json(
                r#"{"tsc-rs":{"role":"checker","dev-profile-opt-level":99}}"#,
                r#"{"tsc-rs":{"role":"fuzz"}}"#,
            )
            .as_bytes(),
        )
        .expect_err("opt level must be supported by Cargo");

        assert!(error.to_string().contains("supported integer range 0..=3"));
    }

    #[test]
    fn default_run_must_name_a_binary_target() {
        let metadata = metadata_json(
            r#"{"tsc-rs":{"role":"checker"}}"#,
            r#"{"tsc-rs":{"role":"fuzz"}}"#,
        )
        .replace(
            r#""default_run": "fuzz-producer""#,
            r#""default_run": "missing-producer""#,
        );
        let catalog =
            WorkspaceCatalog::from_metadata_json(metadata.as_bytes()).expect("valid catalog");

        assert!(catalog
            .require_package("fuzz")
            .expect("fuzz role")
            .require_default_run_target()
            .unwrap_err()
            .to_string()
            .contains("no matching bin target"));
        assert!(catalog
            .require_package("checker")
            .expect("checker role")
            .require_default_run_target()
            .unwrap_err()
            .to_string()
            .contains("does not define package.default-run"));
    }

    #[test]
    fn reports_unknown_roles_with_available_choices() {
        let catalog = WorkspaceCatalog::from_metadata_json(
            metadata_json(
                r#"{"tsc-rs":{"role":"checker"}}"#,
                r#"{"tsc-rs":{"role":"fuzz"}}"#,
            )
            .as_bytes(),
        )
        .expect("valid metadata");

        assert_eq!(
            catalog.require_package("parser").unwrap_err().to_string(),
            "unknown workspace role `parser` (available: checker, fuzz)"
        );
    }

    fn metadata_json(checker_metadata: &str, fuzz_metadata: &str) -> String {
        format!(
            r#"{{
                "workspace_root": "/workspace",
                "workspace_members": ["checker-id", "fuzz-id"],
                "packages": [
                    {{
                        "id": "checker-id",
                        "name": "renamed-checker",
                        "manifest_path": "/workspace/crates/checker/Cargo.toml",
                        "metadata": {checker_metadata},
                        "default_run": null,
                        "targets": [{{
                            "name": "checker_core",
                            "kind": ["lib"],
                            "crate_types": ["lib"],
                            "src_path": "/workspace/crates/checker/src/lib.rs"
                        }}]
                    }},
                    {{
                        "id": "fuzz-id",
                        "name": "renamed-fuzz",
                        "manifest_path": "/workspace/crates/fuzz/Cargo.toml",
                        "metadata": {fuzz_metadata},
                        "default_run": "fuzz-producer",
                        "targets": [
                            {{
                                "name": "fuzz_core",
                                "kind": ["lib"],
                                "crate_types": ["lib"],
                                "src_path": "/workspace/crates/fuzz/src/lib.rs"
                            }},
                            {{
                                "name": "fuzz-producer",
                                "kind": ["bin"],
                                "crate_types": ["bin"],
                                "src_path": "/workspace/crates/fuzz/src/bin/producer.rs"
                            }}
                        ]
                    }},
                    {{
                        "id": "external-id",
                        "name": "external-dependency",
                        "manifest_path": "/registry/external/Cargo.toml",
                        "metadata": {{}},
                        "default_run": null,
                        "targets": []
                    }}
                ]
            }}"#
        )
    }
}
