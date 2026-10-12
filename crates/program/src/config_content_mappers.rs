//! tsgo tsoptions' content mappers: a config's `contentMappers` entries
//! (parsinghelpers.go `parseContentMapper`), the checks of their extensions,
//! the `--runExternalCode` gate, each package's manifest (contentmappers.go
//! `resolveContentMapperManifest`), and the config syntax their diagnostics
//! point at (tsconfigparsing.go).

use std::collections::HashSet;
use std::sync::Arc;

use tsc_diagnostics::{gen, Diagnostic, DiagnosticMessage, JsStr, JsString};
use tsc_syntax::{NodeId, SourceFile, SyntaxKind};

use super::{
    config_array_elements, config_diagnostic, config_location, config_object_properties,
    config_property, ConfigLocation, ConfigParseError, ConfigParseHost,
};
use crate::content_mapper::host::OptionPathSegment;
use crate::content_mapper::{Definition, Manifest, Mapper};
use crate::go_json::GoJson;
use crate::js_path::{combine_paths, directory_name, file_name_key, normalize_slashes};
use crate::json_value::JsonValue as Value;

/// What reading a config's content mappers needs besides its raw value.
pub(super) struct ContentMapperParse<'a> {
    pub host: &'a dyn ConfigParseHost,
    /// The root config's syntax (tsgo `contentMapperSourceFile`); a JSON
    /// object config has none, and its diagnostics no location.
    pub source: Option<&'a SourceFile>,
    pub json_object: bool,
    /// The merged options' `runExternalCode`.
    pub run_external_code: bool,
    /// Where mapper packages are resolved from: the config file, or
    /// `tsconfig.json` in the base directory.
    pub containing_file: JsString,
}

/// tsgo `parseJsonConfigFileContentWorker`'s content mapper steps, between
/// the spec checks and the file listing: the entries of the raw config's
/// `contentMappers` (`value`), their extensions checked, all dropped without
/// `runExternalCode`, and each resolved to its package's manifest (a mapper
/// whose package does not resolve is dropped). Errors are pushed in tsgo's
/// order.
pub(super) fn parse_content_mappers(
    parse: &ContentMapperParse<'_>,
    value: Option<&Value>,
    errors: &mut Vec<Diagnostic>,
) -> Result<Vec<Arc<Mapper>>, ConfigParseError> {
    let elements = content_mappers_of_raw(value, parse.json_object, errors);
    if elements.is_empty() {
        return Ok(Vec::new());
    }
    let syntax = ContentMapperSyntax {
        source: parse.source,
    };
    let mut mappers = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let Some(object) = element.as_object() else {
            continue;
        };
        match parse_content_mapper(object) {
            Ok(mapper) => mappers.push((index, mapper)),
            Err(failures) => {
                for (option, value_type) in failures {
                    errors.push(config_diagnostic(
                        &gen::Compiler_option_0_requires_a_value_of_type_1,
                        &[option, value_type],
                        syntax.location(syntax.mapper(Some(index), None)),
                    ));
                }
            }
        }
    }
    check_extensions(&mut mappers, parse, &syntax, errors);
    if mappers.is_empty() {
        return Ok(Vec::new());
    }
    if !parse.run_external_code {
        // Without the flag the mappers are not trusted to run: their
        // extensions are not registered and their files not mapped.
        errors.push(config_diagnostic(
            &gen::Content_mappers_require_the_runExternalCode_command_line_flag_to_be_enabled,
            &[] as &[&str],
            syntax.location(syntax.key()),
        ));
        return Ok(Vec::new());
    }
    let mut resolved = Vec::with_capacity(mappers.len());
    for (index, mut mapper) in mappers {
        let resolution = resolve_content_mapper_manifest(
            parse.host,
            parse.containing_file.as_js(),
            &mapper.definition.package,
        )?;
        mapper.package_directory = resolution.package_directory;
        match resolution.manifest {
            Ok(manifest) => {
                mapper.manifest = manifest;
                resolved.push(Arc::new(mapper));
            }
            Err(message) => errors.push(config_diagnostic(
                message,
                &[mapper.definition.package.as_str()],
                syntax.location(syntax.mapper(Some(index), Some("package"))),
            )),
        }
    }
    Ok(resolved)
}

/// tsgo `getPropFromRaw("contentMappers", isObject, "object")`: the array's
/// elements. A JSON object config's are as given, and a value that is not
/// an array of objects is reported once; a source file's lose their nulls
/// in its conversion (whose own diagnostics report a value that is not an
/// array).
fn content_mappers_of_raw(
    value: Option<&Value>,
    json_object: bool,
    errors: &mut Vec<Diagnostic>,
) -> Vec<Value> {
    let Some(value) = value else {
        return Vec::new();
    };
    let value_type = match value {
        Value::Null => return Vec::new(),
        Value::Array(values) if !json_object => {
            return values
                .iter()
                .filter(|value| !value.is_null())
                .map(converted_json)
                .collect()
        }
        Value::Array(values) => {
            if !values.iter().all(Value::is_object) {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &["contentMappers", "object"],
                    None,
                ));
            }
            return values.clone();
        }
        _ if json_object => "Array",
        _ => return Vec::new(),
    };
    errors.push(config_diagnostic(
        &gen::Compiler_option_0_requires_a_value_of_type_1,
        &["contentMappers", value_type],
        None,
    ));
    Vec::new()
}

/// tsgo applyExtendedConfig: a base config with `contentMappers` gives its
/// array, as its conversion left it (any other value, none). Kept out of
/// line: the extends recursion's frames stay small.
#[inline(never)]
pub(super) fn inherited(value: &Value) -> Option<Value> {
    value.is_array().then(|| converted_json(value))
}

/// The config's `contentMappers` after the extends merge, which sets the
/// inherited array on a config that does not write the key: its own value
/// as written, else the inherited one.
#[inline(never)]
pub(super) fn merged(
    raw: &mut crate::json_value::JsonObject,
    raw_property_names: &mut std::collections::BTreeSet<JsString>,
    inherited: Option<Value>,
) -> Option<Value> {
    if !raw_property_names.contains("contentMappers".as_bytes()) {
        let inherited = inherited?;
        raw.insert("contentMappers".to_owned(), inherited);
        raw_property_names.insert("contentMappers".into());
    }
    raw.get("contentMappers")
        .cloned()
        .map(super::config_raw_projection)
}

/// A value as tsgo's conversion of config syntax leaves it: arrays at every
/// depth without their nulls, object properties kept.
fn converted_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .filter(|value| !value.is_null())
                .map(converted_json)
                .collect(),
        ),
        Value::Object(object) => {
            let mut converted = crate::json_value::JsonObject::new();
            for (name, value) in object.iter() {
                converted.insert(name.clone(), converted_json(value));
            }
            Value::Object(converted)
        }
        value => value.clone(),
    }
}

/// tsgo `parseContentMapper`: a non-empty string `package`, an `extensions`
/// array of strings and an optional object `options` (kept as tsgo's JSON
/// text), or the `(option, type)` of each one that is not.
fn parse_content_mapper(
    object: &crate::json_value::JsonObject,
) -> Result<Mapper, Vec<(&'static str, &'static str)>> {
    let mut failures = Vec::new();
    let mut definition = Definition::default();
    match object.get("package") {
        Some(Value::String(package)) if !package.is_empty() => {
            definition.package = package.to_string_lossy().into_owned();
        }
        _ => failures.push(("contentMapper.package", "string")),
    }
    match object.get("extensions") {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
            definition.extensions = Some(
                values
                    .iter()
                    .filter_map(Value::as_js)
                    .map(|extension| extension.to_string_lossy().into_owned())
                    .collect(),
            );
        }
        _ => failures.push(("contentMapper.extensions", "string[]")),
    }
    match object.get("options") {
        None => {}
        Some(options @ Value::Object(_)) => {
            definition.options = Some(GoJson::from_json(options).compact());
        }
        Some(_) => failures.push(("contentMapper.options", "object")),
    }
    if failures.is_empty() {
        Ok(Mapper {
            definition,
            ..Mapper::default()
        })
    } else {
        Err(failures)
    }
}

/// tsgo `AllSupportedExtensionsWithJson`, flat.
const NATIVE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".d.ts", ".cts", ".d.cts", ".mts", ".d.mts", ".js", ".jsx", ".cjs", ".mjs",
    ".json",
];

/// Each mapper's extensions as tsgo registers them: one that does not begin
/// with `.`, a built-in one (whatever its case) or one an earlier mapper
/// registered (by the host's case sensitivity) is reported and dropped.
fn check_extensions(
    mappers: &mut [(usize, Mapper)],
    parse: &ContentMapperParse<'_>,
    syntax: &ContentMapperSyntax<'_>,
    errors: &mut Vec<Diagnostic>,
) {
    let case_sensitive = parse.host.use_case_sensitive_file_names();
    let mut seen = HashSet::new();
    for (index, mapper) in mappers.iter_mut() {
        let extensions = mapper.definition.extensions.take().unwrap_or_default();
        let mut valid = Vec::with_capacity(extensions.len());
        for extension in extensions {
            let message = if !extension.starts_with('.') {
                &gen::Content_mapper_file_extension_0_must_begin_with_a
            } else if NATIVE_EXTENSIONS
                .iter()
                .any(|native| equal_fold(native, &extension))
            {
                &gen::Content_mapper_file_extension_0_is_a_built_in_extension_and_cannot_be_registered_by_a_content_mapper
            } else if !seen.insert(file_name_key(
                JsStr::from(extension.as_str()),
                case_sensitive,
            )) {
                &gen::Content_mapper_file_extension_0_is_registered_by_more_than_one_content_mapper
            } else {
                valid.push(extension);
                continue;
            };
            errors.push(config_diagnostic(
                message,
                &[extension.as_str()],
                syntax.location(syntax.extension(*index, &extension)),
            ));
        }
        mapper.definition.extensions = Some(valid);
    }
}

/// Go `strings.EqualFold`: character by character under simple case
/// folding (`ſ` is `s`, the Kelvin sign `k`), approximated by comparing
/// lower and upper cases.
pub(super) fn equal_fold(left: &str, right: &str) -> bool {
    let mut right = right.chars();
    left.chars().all(|left| {
        right.next().is_some_and(|right| {
            left == right
                || left.to_lowercase().eq(right.to_lowercase())
                || left.to_uppercase().eq(right.to_uppercase())
        })
    }) && right.next().is_none()
}

/// A mapper package's directory, and its manifest or why there is none.
pub(super) struct ManifestResolution {
    pub package_directory: String,
    pub manifest: Result<Manifest, &'static DiagnosticMessage>,
}

/// tsgo `resolveContentMapperManifest`: the package found in `node_modules`
/// from the containing file's directory, and the manifest its package.json
/// declares. Nothing is run.
pub(super) fn resolve_content_mapper_manifest(
    host: &dyn ConfigParseHost,
    containing_file: JsStr<'_>,
    package: &str,
) -> Result<ManifestResolution, ConfigParseError> {
    let unresolved = |package_directory: String, message| ManifestResolution {
        package_directory,
        manifest: Err(message),
    };
    let Some(package_directory) =
        resolve_package_directory(host, directory_name(containing_file).as_js(), package)?
    else {
        return Ok(unresolved(
            String::new(),
            &gen::The_content_mapper_package_0_could_not_be_resolved,
        ));
    };
    let directory = package_directory.to_string_lossy().into_owned();
    let package_json = combine_paths(package_directory.as_js(), JsStr::from("package.json"));
    let Some(text) = host.read_file(package_json.as_js())? else {
        return Ok(unresolved(
            directory,
            &gen::The_content_mapper_package_0_could_not_be_resolved,
        ));
    };
    let Some(fields) = PackageJsonFields::parse(&text) else {
        return Ok(unresolved(
            directory,
            &gen::The_package_json_of_the_content_mapper_package_0_could_not_be_parsed,
        ));
    };
    if fields.name.is_empty() {
        return Ok(unresolved(
            directory,
            &gen::The_package_json_of_the_content_mapper_package_0_does_not_specify_a_name,
        ));
    }
    let Some(content_mapper) = fields.content_mapper else {
        return Ok(unresolved(
            directory,
            &gen::The_package_json_of_the_content_mapper_package_0_does_not_declare_a_typescript_contentMapper_object,
        ));
    };
    let Some(exec) = content_mapper.exec.filter(|exec| !exec.is_empty()) else {
        return Ok(unresolved(
            directory,
            &gen::The_typescript_contentMapper_exec_of_the_content_mapper_package_0_must_be_a_non_empty_array_of_strings,
        ));
    };
    Ok(ManifestResolution {
        package_directory: directory,
        manifest: Ok(Manifest {
            name: fields.name,
            version: fields.version,
            exec: Some(exec),
            compiler_options: content_mapper.compiler_options,
            dynamic_config: content_mapper.dynamic_config,
        }),
    })
}

/// tsgo `ResolvePackageDirectory` (bundler resolution, a package directory
/// only): from each ancestor of `directory` that is not itself a
/// `node_modules`, `node_modules/<package>` and then
/// `node_modules/@types/<mangled package>`, whichever directory exists
/// first, at its real path (a symlinked package resolves to its target).
fn resolve_package_directory(
    host: &dyn ConfigParseHost,
    directory: JsStr<'_>,
    package: &str,
) -> Result<Option<JsString>, ConfigParseError> {
    let (package_name, _) = parse_package_name(package);
    let mangled = mangle_scoped_package_name(package);
    let (types_name, _) = parse_package_name(&mangled);
    let mut current = directory.to_owned();
    loop {
        if base_name(current.as_js()) != "node_modules" {
            let node_modules = combine_paths(current.as_js(), JsStr::from("node_modules"));
            if host.directory_exists(node_modules.as_js())? {
                let candidate = combine_paths(node_modules.as_js(), JsStr::from(package_name));
                if host.directory_exists(candidate.as_js())? {
                    return package_real_path(host, candidate, package).map(Some);
                }
                let at_types = combine_paths(node_modules.as_js(), JsStr::from("@types"));
                if host.directory_exists(at_types.as_js())? {
                    let candidate = combine_paths(at_types.as_js(), JsStr::from(types_name));
                    if host.directory_exists(candidate.as_js())? {
                        return package_real_path(host, candidate, package).map(Some);
                    }
                }
            }
        }
        let parent = directory_name(current.as_js());
        if parent == current {
            return Ok(None);
        }
        current = parent;
    }
}

/// tsgo `createResolvedModuleHandlingSymlink`: a package name that is not
/// relative resolves to the real path, unless it differs only in case on a
/// case-insensitive host.
fn package_real_path(
    host: &dyn ConfigParseHost,
    candidate: JsString,
    package: &str,
) -> Result<JsString, ConfigParseError> {
    if is_external_module_name_relative(package) {
        return Ok(candidate);
    }
    let Some(real) = host.realpath(candidate.as_js())? else {
        return Ok(candidate);
    };
    let real = normalize_slashes(real.as_js());
    let case_sensitive = host.use_case_sensitive_file_names();
    if file_name_key(real.as_js(), case_sensitive)
        == file_name_key(candidate.as_js(), case_sensitive)
    {
        return Ok(candidate);
    }
    Ok(real)
}

fn base_name(path: JsStr<'_>) -> JsStr<'_> {
    path.rsplit_once("/").map_or(path, |(_, name)| name)
}

/// tsgo `IsExternalModuleNameRelative`.
fn is_external_module_name_relative(name: &str) -> bool {
    name == "."
        || name == ".."
        || name.starts_with("./")
        || name.starts_with("../")
        || name.starts_with(".\\")
        || name.starts_with("..\\")
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.as_bytes().get(1) == Some(&b':')
}

/// tsgo `ParsePackageName`: the package (`@scope/name` or `name`) and the
/// path inside it.
fn parse_package_name(name: &str) -> (&str, &str) {
    let mut separator = name.find('/');
    if name.starts_with('@') {
        let offset = separator.map_or(0, |separator| separator + 1);
        separator = name[offset..].find('/').map(|index| index + offset);
    }
    match separator {
        Some(index) => (&name[..index], &name[index + 1..]),
        None => (name, ""),
    }
}

/// tsgo `MangleScopedPackageName`: `@scope/name` is `scope__name` under
/// `@types`.
fn mangle_scoped_package_name(name: &str) -> String {
    match name.strip_prefix('@').and_then(|rest| rest.split_once('/')) {
        Some((scope, rest)) => format!("{scope}__{rest}"),
        None => name.to_owned(),
    }
}

/// The package.json fields a mapper's manifest reads (tsgo
/// `packagejson.Parse`): a field of another JSON type than tsgo's
/// `Expected` field is as if absent, and a null element of a string array is
/// an empty string.
struct PackageJsonFields {
    name: String,
    version: String,
    content_mapper: Option<ContentMapperFields>,
}

/// tsgo `packagejson.ContentMapperFields`.
struct ContentMapperFields {
    exec: Option<Vec<String>>,
    compiler_options: Option<Vec<String>>,
    dynamic_config: bool,
}

impl PackageJsonFields {
    /// `None` when the text is not JSON or not an object (or null).
    fn parse(text: &str) -> Option<Self> {
        let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
        let empty = serde_json::Map::new();
        let object = match &value {
            serde_json::Value::Object(object) => object,
            serde_json::Value::Null => &empty,
            _ => return None,
        };
        let string = |name: &str| {
            object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let content_mapper = object
            .get("typescript")
            .and_then(serde_json::Value::as_object)
            .and_then(|typescript| typescript.get("contentMapper"))
            .and_then(serde_json::Value::as_object)
            .map(|content_mapper| ContentMapperFields {
                exec: string_array(content_mapper.get("exec")),
                compiler_options: string_array(content_mapper.get("compilerOptions")),
                dynamic_config: content_mapper
                    .get("dynamicConfig")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            });
        Some(Self {
            name: string("name"),
            version: string("version"),
            content_mapper,
        })
    }
}

fn string_array(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|element| match element {
            serde_json::Value::String(text) => Some(text.clone()),
            serde_json::Value::Null => Some(String::new()),
            _ => None,
        })
        .collect()
}

/// The root config's `contentMappers` syntax (tsgo `getContentMapperSyntax`
/// and its siblings); nothing without a source file.
pub(super) struct ContentMapperSyntax<'s> {
    pub source: Option<&'s SourceFile>,
}

impl ContentMapperSyntax<'_> {
    /// tsgo `getContentMappersKeySyntax`: the property's name.
    fn key(&self) -> Option<NodeId> {
        let source = self.source?;
        config_property(source, "contentMappers").map(|property| property.name_node)
    }

    /// tsgo `getContentMapperSyntax`: the value of `sub_key` in the mapper at
    /// `index`, else the mapper's element, else (an index outside the
    /// array, or a value that is not one) the property's value.
    pub(super) fn mapper(&self, index: Option<usize>, sub_key: Option<&str>) -> Option<NodeId> {
        let source = self.source?;
        let initializer = config_property(source, "contentMappers")?.initializer;
        if source.arena.node(initializer).kind != SyntaxKind::ArrayLiteralExpression {
            return Some(initializer);
        }
        let elements = config_array_elements(source, initializer);
        let Some(&element) = index.and_then(|index| elements.get(index)) else {
            return Some(initializer);
        };
        if let Some(sub_key) = sub_key {
            if source.arena.node(element).kind == SyntaxKind::ObjectLiteralExpression {
                if let Some(property) = config_object_properties(source, element)
                    .into_iter()
                    .find(|property| property.name == sub_key)
                {
                    return Some(property.initializer);
                }
            }
        }
        Some(element)
    }

    /// tsgo `getContentMapperExtensionSyntax`: the extension's string in the
    /// mapper's `extensions`, else that value or the mapper.
    fn extension(&self, index: usize, extension: &str) -> Option<NodeId> {
        let source = self.source?;
        let node = self.mapper(Some(index), Some("extensions"))?;
        if source.arena.node(node).kind == SyntaxKind::ArrayLiteralExpression {
            if let Some(element) = config_array_elements(source, node)
                .into_iter()
                .find(|element| {
                    source
                        .arena
                        .node(*element)
                        .data
                        .as_string_literal()
                        .is_some_and(|literal| literal.text == extension)
                })
            {
                return Some(element);
            }
        }
        Some(node)
    }

    /// tsgo `GetContentMapperOptionDiagnosticLocation`'s node: from the
    /// mapper's `options` (else the mapper), each segment of `path` that
    /// names an array element or an object property in turn.
    pub(super) fn option(
        &self,
        index: Option<usize>,
        path: &[OptionPathSegment],
    ) -> Option<NodeId> {
        let source = self.source?;
        let mut node = self
            .mapper(index, Some("options"))
            .or_else(|| self.mapper(index, None))?;
        for segment in path {
            let kind = source.arena.node(node).kind;
            let next = match segment {
                OptionPathSegment::Index(index) if kind == SyntaxKind::ArrayLiteralExpression => {
                    config_array_elements(source, node).get(*index).copied()
                }
                OptionPathSegment::Property(name)
                    if kind == SyntaxKind::ObjectLiteralExpression =>
                {
                    config_object_properties(source, node)
                        .into_iter()
                        .find(|property| property.name == name.as_str())
                        .map(|property| property.initializer)
                }
                _ => None,
            };
            let Some(next) = next else {
                break;
            };
            node = next;
        }
        Some(node)
    }

    pub(super) fn location(&self, node: Option<NodeId>) -> Option<ConfigLocation> {
        config_location(self.source?, node?)
    }
}

/// tsgo `GetContentMapperForFileName`: the first mapper registering the
/// longest of the mappers' extensions the file name ends with (in the
/// host's case sensitivity).
pub fn content_mapper_for_file_name<'m>(
    mappers: &'m [Arc<Mapper>],
    file_name: JsStr<'_>,
    case_sensitive: bool,
) -> Option<&'m Arc<Mapper>> {
    let extensions = mappers
        .iter()
        .flat_map(|mapper| mapper.definition.extensions.iter().flatten())
        .map(String::as_str);
    let extension = longest_extension(file_name, extensions, case_sensitive)?;
    mappers.iter().find(|mapper| {
        mapper
            .definition
            .extensions
            .iter()
            .flatten()
            .any(|candidate| {
                *candidate == extension || (!case_sensitive && equal_fold(&extension, candidate))
            })
    })
}

/// tsgo `GetLongestExtensionFromPath`: the longest of `extensions` the file
/// name ends with (after a `.`), as the file name spells it.
pub fn longest_extension<'f>(
    file_name: JsStr<'f>,
    extensions: impl Iterator<Item = &'f str> + Clone,
    case_sensitive: bool,
) -> Option<String> {
    let file_name = file_name.to_string_lossy();
    let file_name = file_name.trim_end_matches('/');
    let mut longest: Option<&str> = None;
    for extension in extensions {
        if extension.len() <= longest.map_or(0, str::len) {
            continue;
        }
        let Some(start) = file_name.len().checked_sub(extension.len()) else {
            continue;
        };
        let Some(suffix) = file_name.get(start..) else {
            continue;
        };
        let matched = if case_sensitive {
            suffix == extension
        } else {
            equal_fold(suffix, extension)
        };
        if suffix.starts_with('.') && matched {
            longest = Some(suffix);
        }
    }
    longest.map(str::to_owned)
}

#[cfg(test)]
#[path = "../tests/unit/config_content_mappers_tests.rs"]
mod config_content_mappers_tests;
