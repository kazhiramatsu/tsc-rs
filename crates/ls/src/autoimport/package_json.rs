//! tsgo `packagejson.InfoCacheEntry` as the auto-import registry reads it:
//! whether a directory has a package.json, and the dependencies it names.

use std::collections::BTreeSet;

use tsc_host::CompilerHost;
use tsc_types::JsStr;

/// tsgo `packagejson.DependencyFields`' fields, in `RangeDependencies`
/// order.
const DEPENDENCY_FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// tsgo `packagejson.InfoCacheEntry`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageJsonInfo {
    /// tsgo `PackageDirectory`.
    pub directory: String,
    /// tsgo `DirectoryExists`.
    pub directory_exists: bool,
    /// tsgo `Contents`: none when the directory has no package.json.
    pub contents: Option<PackageJsonContents>,
}

/// tsgo `packagejson.PackageJson`'s parts the registry reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageJsonContents {
    /// tsgo `Parseable`: the file is a JSON object.
    pub parseable: bool,
    /// Every dependency with its field (tsgo `RangeDependencies`); a field
    /// that is not an object of strings is absent.
    pub dependencies: Vec<(String, &'static str)>,
}

impl PackageJsonInfo {
    /// tsgo `Exists`.
    pub fn exists(&self) -> bool {
        self.contents.is_some()
    }

    /// tsgo `autoImportRegistryCloneHost.GetPackageJson`: the package.json
    /// at `file_name`, read through `host`.
    pub fn read(host: &dyn CompilerHost, file_name: &str) -> Self {
        let directory = tsc_program::get_directory_path(JsStr::from_str(file_name))
            .to_string_lossy()
            .into_owned();
        let text = host
            .read_file_js(JsStr::from_str(file_name))
            .ok()
            .flatten()
            .and_then(|bytes| tsc_program::decode_host_text(bytes).ok());
        let Some(text) = text else {
            let directory_exists = host
                .directory_exists_js(JsStr::from_str(&directory))
                .unwrap_or(false);
            return Self {
                directory,
                directory_exists,
                contents: None,
            };
        };
        Self {
            directory,
            directory_exists: true,
            contents: Some(PackageJsonContents::parse(&text)),
        }
    }
}

impl PackageJsonContents {
    /// tsgo `packagejson.Parse`: unparseable unless the text is a JSON
    /// object.
    pub fn parse(text: &str) -> Self {
        let Ok(serde_json::Value::Object(object)) = serde_json::from_str::<serde_json::Value>(text)
        else {
            return Self::default();
        };
        let mut dependencies = Vec::new();
        for field in DEPENDENCY_FIELDS {
            // tsgo reads a field as a map of strings.
            let Some(serde_json::Value::Object(names)) = object.get(field) else {
                continue;
            };
            if !names.values().all(serde_json::Value::is_string) {
                continue;
            }
            dependencies.extend(names.keys().map(|name| (name.clone(), field)));
        }
        Self {
            parseable: true,
            dependencies,
        }
    }
}

/// tsgo `addPackageJsonDependencies`: the `dependencies` and
/// `peerDependencies` of a package.json, an `@types` package as the package
/// it types.
pub(crate) fn add_package_json_dependencies(
    contents: &PackageJsonContents,
    dependencies: &mut BTreeSet<String>,
) {
    for (name, field) in &contents.dependencies {
        if name.is_empty() || name == "@types/" || name.starts_with('.') {
            // Edge cases that could make us blow up probably
            continue;
        }
        if matches!(*field, "dependencies" | "peerDependencies") {
            dependencies.insert(tsc_program::get_package_name_from_types_package_name(name));
        }
    }
}
