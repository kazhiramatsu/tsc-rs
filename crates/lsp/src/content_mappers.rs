//! tsgo `parseContentMapperContributions` (lsp/server.go): an editor's
//! content mapper contributions (`custom/setContentMapperContributions`)
//! as the session's mappers for inferred projects.

use std::collections::HashSet;
use std::sync::Arc;

use tsc_lsproto::ContentMapperContribution;
use tsc_program::content_mapper::{Definition, Manifest, Mapper};
use tsc_project::ContentMapperContributions;

use crate::gofmt::quote;

/// tsgo `parseContentMapperContributions`: each contribution's extensions
/// checked, and an inferred project contribution as a mapper named after
/// its contributor and index (its package and contribution ID); the
/// extensions such mappers claim, each by one contribution, sorted.
pub fn parse_content_mapper_contributions(
    values: &[ContentMapperContribution],
) -> Result<ContentMapperContributions, String> {
    let mut result = ContentMapperContributions::default();
    let mut claimed_extensions = HashSet::new();
    for (index, value) in values.iter().enumerate() {
        if value.contributor_id.is_empty() {
            return Err("content mapper contribution requires a contributorId".to_owned());
        }
        let identity = format!("{}[{index}]", value.contributor_id);
        let mut valid_extensions = Vec::with_capacity(value.extensions.len());
        for extension in &value.extensions {
            if !is_valid_contributed_content_mapper_extension(extension) {
                return Err(format!(
                    "content mapper contribution {} has invalid extension {}",
                    quote(&identity),
                    quote(extension)
                ));
            }
            valid_extensions.push(extension.clone());
        }
        let Some(inferred_project) = &value.inferred_project_contribution else {
            continue;
        };
        let manifest = &inferred_project.manifest;
        if manifest.name.is_empty() || manifest.exec.is_empty() {
            return Err(format!(
                "content mapper contribution {} requires a manifest name and exec",
                quote(&identity)
            ));
        }
        for option in manifest.compiler_options.iter().flatten() {
            if tsc_program::compiler_option_declaration_ignore_case(option).is_none() {
                return Err(format!(
                    "content mapper contribution {} requests unknown compiler option {}",
                    quote(&identity),
                    quote(option)
                ));
            }
        }
        for extension in &valid_extensions {
            if !claimed_extensions.insert(extension.to_lowercase()) {
                return Err(format!(
                    "content mapper contributions both claim extension {}",
                    quote(extension)
                ));
            }
            result.extensions.push(extension.clone());
        }
        let options = match &inferred_project.options {
            Some(options) => serde_json::to_string(options).map_err(|_| {
                format!(
                    "content mapper contribution {} has invalid options",
                    quote(&identity)
                )
            })?,
            None => "{}".to_owned(),
        };
        let mut mapper = Mapper {
            definition: Definition {
                package: identity.clone(),
                extensions: Some(valid_extensions),
                options: Some(options),
            },
            manifest: Manifest {
                name: manifest.name.clone(),
                version: manifest.version.clone().unwrap_or_default(),
                exec: Some(manifest.exec.clone()),
                compiler_options: Some(manifest.compiler_options.clone().unwrap_or_default()),
                dynamic_config: manifest.dynamic_config.unwrap_or_default(),
            },
            package_directory: String::new(),
            contribution_id: identity.clone(),
        };
        if let Some(cwd) = &manifest.cwd {
            if !tsc_program::path_is_absolute(cwd.as_str()) {
                return Err(format!(
                    "content mapper contribution {} has non-absolute cwd",
                    quote(&identity)
                ));
            }
            mapper.package_directory = cwd.clone();
        }
        result.mappers.push(Arc::new(mapper));
    }
    result.extensions.sort();
    Ok(result)
}

/// tsgo `isValidContributedContentMapperExtension`: one dotted extension
/// that is none of the compiler's own (in any case).
fn is_valid_contributed_content_mapper_extension(extension: &str) -> bool {
    const NATIVE_EXTENSIONS: [&str; 12] = [
        ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".cts", ".d.cts", ".cjs", ".mts", ".d.mts", ".mjs",
        ".json",
    ];
    // tsgo `GetAnyExtensionFromPath("file" + extension)`: from the last dot of
    // the base name.
    let file_name = format!("file{extension}");
    let base = file_name.rsplit('/').next().unwrap_or(&file_name);
    let any_extension = base.rfind('.').map_or("", |index| &base[index..]);
    if extension.len() <= 1 || !extension.starts_with('.') || any_extension != extension {
        return false;
    }
    !NATIVE_EXTENSIONS
        .iter()
        .any(|native| native.eq_ignore_ascii_case(extension))
}
