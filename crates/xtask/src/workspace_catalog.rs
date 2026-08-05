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
#[path = "../tests/unit/workspace_catalog/tests.rs"]
mod tests;
