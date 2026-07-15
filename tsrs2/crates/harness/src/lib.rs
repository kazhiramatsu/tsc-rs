#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub use tsrs2_checker::{check_program, CheckResult, CompilerOptions, InputFile};

pub fn check_empty_program() -> CheckResult {
    check_program(&[], &CompilerOptions::default())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessError {
    message: String,
}

impl HarnessError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for HarnessError {}

pub type HarnessResult<T> = Result<T, HarnessError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramJson {
    pub schema: u32,
    pub cwd: String,
    pub options: BTreeMap<String, OptionValue>,
    pub libs: Vec<String>,
    pub files: Vec<ProgramFile>,
    pub matrix_key: String,
}

impl ProgramJson {
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str("  \"schema\": ");
        out.push_str(&self.schema.to_string());
        out.push_str(",\n");
        out.push_str("  \"cwd\": ");
        push_json_string(&mut out, &self.cwd);
        out.push_str(",\n");
        out.push_str("  \"options\": ");
        push_json_object(&mut out, &self.options, 2);
        out.push_str(",\n");
        out.push_str("  \"libs\": ");
        push_json_string_array(&mut out, &self.libs);
        out.push_str(",\n");
        out.push_str("  \"files\": [");
        if !self.files.is_empty() {
            out.push('\n');
            for (index, file) in self.files.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                out.push_str("    {\n");
                out.push_str("      \"name\": ");
                push_json_string(&mut out, &file.name);
                out.push_str(",\n");
                out.push_str("      \"textB64\": ");
                push_json_string(&mut out, &file.text_b64);
                out.push_str("\n    }");
            }
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("],\n");
        out.push_str("  \"matrixKey\": ");
        push_json_string(&mut out, &self.matrix_key);
        out.push_str("\n}\n");
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFile {
    pub name: String,
    pub text_b64: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionValue {
    Bool(bool),
    Number(i32),
    String(String),
    StringList(Vec<String>),
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceFileUnit {
    name: String,
    text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedFixture {
    cwd: String,
    directives: BTreeMap<String, DirectiveValue>,
    files: Vec<SourceFileUnit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectiveValue {
    raw_name: String,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectiveKind {
    Bool,
    Number,
    String,
    LowerString,
    StringList,
    MatrixString,
    CurrentDirectory,
    FileName,
    HarnessOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectiveSpec {
    canonical: &'static str,
    kind: DirectiveKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MatrixDimension {
    canonical: String,
    values: Vec<MatrixValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MatrixValue {
    key: String,
    value: OptionValue,
}

#[derive(Clone, Debug)]
struct LibResolver {
    lib_dir: PathBuf,
    lib_map: BTreeMap<String, String>,
    target_values: BTreeMap<String, i32>,
    module_values: BTreeMap<String, i32>,
    jsx_values: BTreeMap<String, i32>,
    module_resolution_values: BTreeMap<String, i32>,
    target_to_lib: BTreeMap<i32, String>,
    lib_priority: BTreeMap<String, usize>,
    fallback_priority: usize,
}

pub fn expand_fixture_file(
    fixture_path: &Path,
    vendor_lib_dir: &Path,
) -> HarnessResult<Vec<ProgramJson>> {
    let text = fs::read_to_string(fixture_path).map_err(|err| {
        HarnessError::new(format!(
            "failed to read fixture {}: {err}",
            fixture_path.display()
        ))
    })?;
    let default_file_name = fixture_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| {
            HarnessError::new(format!(
                "fixture path has no UTF-8 file name: {}",
                fixture_path.display()
            ))
        })?;
    expand_fixture_text(default_file_name, &text, vendor_lib_dir)
}

pub fn expand_fixture_text(
    default_file_name: &str,
    text: &str,
    vendor_lib_dir: &Path,
) -> HarnessResult<Vec<ProgramJson>> {
    let parsed = parse_fixture(default_file_name, text)?;
    let resolver = shared_lib_resolver(vendor_lib_dir)?;
    expand_parsed_fixture(&parsed, &resolver)
}

pub fn write_program_jsons(
    programs: &[ProgramJson],
    out_dir: &Path,
) -> HarnessResult<Vec<PathBuf>> {
    fs::create_dir_all(out_dir).map_err(|err| {
        HarnessError::new(format!(
            "failed to create output directory {}: {err}",
            out_dir.display()
        ))
    })?;

    let mut paths = Vec::with_capacity(programs.len());
    for program in programs {
        let file_name = if programs.len() == 1 {
            "program.json".to_owned()
        } else {
            format!("program-{}.json", sanitize_matrix_key(&program.matrix_key))
        };
        let path = out_dir.join(file_name);
        fs::write(&path, program.to_json()).map_err(|err| {
            HarnessError::new(format!("failed to write {}: {err}", path.display()))
        })?;
        paths.push(path);
    }
    Ok(paths)
}

fn expand_parsed_fixture(
    parsed: &ParsedFixture,
    resolver: &LibResolver,
) -> HarnessResult<Vec<ProgramJson>> {
    let (base_options, matrix_dimensions) = build_options(&parsed.directives, resolver)?;
    let matrix_points = matrix_points(&matrix_dimensions);
    let mut programs = Vec::with_capacity(matrix_points.len());

    for point in matrix_points {
        let mut options = base_options.clone();
        let mut key_parts = Vec::new();
        for (name, value) in point {
            options.insert(name.clone(), value.value.clone());
            key_parts.push(format!("{}={}", name, value.key));
        }
        let matrix_key = key_parts.join(",");
        let libs = resolve_program_libs(&options, resolver)?;
        let files = parsed
            .files
            .iter()
            .map(|file| ProgramFile {
                name: file.name.clone(),
                text_b64: base64_encode(file.text.as_bytes()),
            })
            .collect();

        programs.push(ProgramJson {
            schema: 1,
            cwd: parsed.cwd.clone(),
            options,
            libs,
            files,
            matrix_key,
        });
    }

    Ok(programs)
}

fn parse_fixture(default_file_name: &str, text: &str) -> HarnessResult<ParsedFixture> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines = split_lines_keep_endings(text);
    let mut directives = BTreeMap::<String, DirectiveValue>::new();
    let mut cwd = "/".to_owned();
    let mut body_start = 0;
    let mut saw_header = false;

    while body_start < lines.len() {
        let line = lines[body_start];
        if let Some((raw_name, value)) = parse_directive_line(line) {
            let normalized = normalize_directive_name(raw_name);
            let spec = directive_spec(&normalized).ok_or_else(|| {
                HarnessError::new(format!("unknown fixture directive @{raw_name}"))
            })?;
            if spec.kind == DirectiveKind::FileName {
                break;
            }
            if spec.kind == DirectiveKind::CurrentDirectory {
                cwd = normalize_cwd(value);
            } else if spec.kind != DirectiveKind::HarnessOnly {
                directives.insert(
                    spec.canonical.to_owned(),
                    DirectiveValue {
                        raw_name: raw_name.to_owned(),
                        value: value.to_owned(),
                    },
                );
            }
            saw_header = true;
            body_start += 1;
            continue;
        }

        if line.trim().is_empty() && saw_header {
            body_start += 1;
            continue;
        }

        break;
    }

    let files = split_fixture_files(default_file_name, &lines[body_start..])?;
    Ok(ParsedFixture {
        cwd,
        directives,
        files,
    })
}

fn split_fixture_files(
    default_file_name: &str,
    lines: &[&str],
) -> HarnessResult<Vec<SourceFileUnit>> {
    let mut files = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_text = String::new();

    for line in lines {
        if let Some((raw_name, value)) = parse_directive_line(line) {
            if directive_spec(&normalize_directive_name(raw_name))
                .is_some_and(|spec| spec.kind == DirectiveKind::FileName)
            {
                if let Some(name) = current_name.take() {
                    files.push(SourceFileUnit {
                        name,
                        text: std::mem::take(&mut current_text),
                    });
                } else if !current_text.is_empty() {
                    files.push(SourceFileUnit {
                        name: default_file_name.to_owned(),
                        text: std::mem::take(&mut current_text),
                    });
                }

                let file_name = value.trim();
                if file_name.is_empty() {
                    return Err(HarnessError::new("@filename directive has an empty value"));
                }
                current_name = Some(file_name.to_owned());
                continue;
            }
        }

        if current_name.is_none() {
            current_name = Some(default_file_name.to_owned());
        }
        current_text.push_str(line);
    }

    if let Some(name) = current_name {
        files.push(SourceFileUnit {
            name,
            text: current_text,
        });
    }

    if files.is_empty() {
        files.push(SourceFileUnit {
            name: default_file_name.to_owned(),
            text: String::new(),
        });
    }

    Ok(files)
}

fn build_options(
    directives: &BTreeMap<String, DirectiveValue>,
    resolver: &LibResolver,
) -> HarnessResult<(BTreeMap<String, OptionValue>, Vec<MatrixDimension>)> {
    let mut options = BTreeMap::new();
    let mut matrix_dimensions = Vec::new();

    for (canonical, directive) in directives {
        let normalized_name = normalize_directive_name(&directive.raw_name);
        let spec = directive_spec(&normalized_name).ok_or_else(|| {
            HarnessError::new(format!("unknown fixture directive @{}", directive.raw_name))
        })?;

        match spec.kind {
            DirectiveKind::Bool => {
                if is_bool_matrix_value(&directive.value) {
                    matrix_dimensions.push(MatrixDimension {
                        canonical: canonical.clone(),
                        values: expand_bool_matrix_values(&directive.value)?,
                    });
                } else {
                    options.insert(
                        canonical.clone(),
                        OptionValue::Bool(parse_bool(&directive.value)?),
                    );
                }
            }
            DirectiveKind::Number => {
                options.insert(
                    canonical.clone(),
                    OptionValue::Number(parse_number(&directive.value, canonical)?),
                );
            }
            DirectiveKind::String => {
                options.insert(canonical.clone(), parse_string_option(&directive.value));
            }
            DirectiveKind::LowerString => {
                options.insert(
                    canonical.clone(),
                    parse_lower_string_option(&directive.value),
                );
            }
            DirectiveKind::StringList => {
                options.insert(
                    canonical.clone(),
                    OptionValue::StringList(split_option_list(&directive.value)),
                );
            }
            DirectiveKind::MatrixString => {
                let values = expand_matrix_string_values(canonical, &directive.value, resolver)?;
                if values.len() > 1 {
                    matrix_dimensions.push(MatrixDimension {
                        canonical: canonical.clone(),
                        values,
                    });
                } else if let Some(value) = values.into_iter().next() {
                    options.insert(canonical.clone(), value.value);
                } else {
                    options.insert(canonical.clone(), OptionValue::Null);
                }
            }
            DirectiveKind::CurrentDirectory
            | DirectiveKind::FileName
            | DirectiveKind::HarnessOnly => {}
        }
    }

    Ok((options, matrix_dimensions))
}

fn is_bool_matrix_value(raw: &str) -> bool {
    let values = split_option_list(raw);
    values.len() > 1 || values.iter().any(|value| value == "*")
}

fn expand_bool_matrix_values(raw: &str) -> HarnessResult<Vec<MatrixValue>> {
    let values = split_option_list(raw);
    let bools = if values.iter().any(|value| value == "*") {
        let mut excluded = Vec::new();
        for value in values.iter().filter_map(|value| value.strip_prefix('-')) {
            excluded.push(parse_bool(value)?);
        }
        [false, true]
            .into_iter()
            .filter(|value| !excluded.contains(value))
            .collect::<Vec<_>>()
    } else {
        let mut bools = Vec::with_capacity(values.len());
        for value in values {
            let value = parse_bool(&value)?;
            if !bools.contains(&value) {
                bools.push(value);
            }
        }
        bools
    };

    if bools.is_empty() {
        return Err(HarnessError::new("boolean matrix expanded to no values"));
    }
    Ok(bools
        .into_iter()
        .map(|value| MatrixValue {
            key: value.to_string(),
            value: OptionValue::Bool(value),
        })
        .collect())
}

fn expand_matrix_string_values(
    canonical: &str,
    raw: &str,
    resolver: &LibResolver,
) -> HarnessResult<Vec<MatrixValue>> {
    let values = split_option_list(raw);
    if !values.iter().any(|value| value == "*") {
        return Ok(values
            .into_iter()
            .map(|value| MatrixValue {
                key: value.clone(),
                value: OptionValue::String(value),
            })
            .collect());
    }

    let all_values = resolver.matrix_string_values(canonical).ok_or_else(|| {
        HarnessError::new(format!("wildcard matrix is not supported for {canonical}"))
    })?;
    let excluded = values
        .iter()
        .filter_map(|value| value.strip_prefix('-').map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let expanded = all_values
        .into_iter()
        .filter(|value| !excluded.contains(value))
        .map(|value| MatrixValue {
            key: value.clone(),
            value: OptionValue::String(value),
        })
        .collect::<Vec<_>>();
    if expanded.is_empty() {
        return Err(HarnessError::new(format!(
            "wildcard matrix for {canonical} expanded to no values"
        )));
    }
    Ok(expanded)
}

fn matrix_points(dimensions: &[MatrixDimension]) -> Vec<Vec<(String, MatrixValue)>> {
    let mut points: Vec<Vec<(String, MatrixValue)>> = vec![Vec::new()];
    for dimension in dimensions {
        let mut next = Vec::with_capacity(points.len() * dimension.values.len());
        for point in &points {
            for value in &dimension.values {
                let mut point = point.clone();
                point.push((dimension.canonical.clone(), value.clone()));
                next.push(point);
            }
        }
        points = next;
    }
    points
}

fn resolve_program_libs(
    options: &BTreeMap<String, OptionValue>,
    resolver: &LibResolver,
) -> HarnessResult<Vec<String>> {
    if matches!(options.get("noLib"), Some(OptionValue::Bool(true))) {
        return Ok(Vec::new());
    }

    let roots = match options.get("lib") {
        Some(OptionValue::StringList(libs)) => libs
            .iter()
            .map(|lib| resolver.lib_file_name(lib))
            .collect::<HarnessResult<Vec<_>>>()?,
        Some(_) => {
            return Err(HarnessError::new(
                "lib option must be serialized as a string list",
            ));
        }
        None => vec![resolver.default_lib_file_name(options.get("target"))?],
    };

    resolver.expand_lib_files(&roots)
}

/// Process-wide resolver cache: the resolver is immutable once built,
/// and rebuilding it per fixture (a 6MB `_tsc.js` read plus several
/// full-text scans) dominated corpus walks.
fn shared_lib_resolver(lib_dir: &Path) -> HarnessResult<Arc<LibResolver>> {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, Arc<LibResolver>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(resolver) = cache.lock().expect("lib resolver cache").get(lib_dir) {
        return Ok(resolver.clone());
    }
    let resolver = Arc::new(LibResolver::from_vendor_lib_dir(lib_dir)?);
    cache
        .lock()
        .expect("lib resolver cache")
        .insert(lib_dir.to_owned(), resolver.clone());
    Ok(resolver)
}

impl LibResolver {
    fn from_vendor_lib_dir(lib_dir: &Path) -> HarnessResult<Self> {
        let tsc_path = lib_dir.join("_tsc.js");
        let tsc = fs::read_to_string(&tsc_path).map_err(|err| {
            HarnessError::new(format!("failed to read {}: {err}", tsc_path.display()))
        })?;
        let lib_entries = parse_lib_entries(&tsc)?;
        let mut lib_map = BTreeMap::new();
        let mut lib_priority = BTreeMap::new();
        for (index, (name, file_name)) in lib_entries.iter().enumerate() {
            lib_map.insert(name.clone(), file_name.clone());
            lib_priority.entry(name.clone()).or_insert(index + 1);
        }

        let fallback_priority = lib_entries.len() + 2;
        Ok(Self {
            lib_dir: lib_dir.to_owned(),
            lib_map,
            target_values: parse_option_number_map(&tsc, "var targetOptionDeclaration")?,
            module_values: parse_option_number_map(&tsc, "var moduleOptionDeclaration")?,
            jsx_values: parse_standalone_number_map(
                &tsc,
                "var jsxOptionMap = new Map(Object.entries({",
            )?,
            module_resolution_values: parse_option_number_map(&tsc, "name: \"moduleResolution\"")?,
            target_to_lib: parse_target_to_lib_map(&tsc)?,
            lib_priority,
            fallback_priority,
        })
    }

    fn lib_file_name(&self, lib: &str) -> HarnessResult<String> {
        let trimmed = lib.trim().to_ascii_lowercase();
        if trimmed.starts_with("lib.") && trimmed.ends_with(".d.ts") {
            return Ok(trimmed);
        }
        self.lib_map
            .get(&trimmed)
            .cloned()
            .ok_or_else(|| HarnessError::new(format!("unknown lib option value: {lib}")))
    }

    fn default_lib_file_name(&self, target: Option<&OptionValue>) -> HarnessResult<String> {
        let target_value = match target {
            Some(OptionValue::String(target)) => {
                let normalized = target.to_ascii_lowercase();
                let value = *self.target_values.get(&normalized).ok_or_else(|| {
                    HarnessError::new(format!("unknown target option value: {target}"))
                })?;
                if value == 0 {
                    12
                } else {
                    value
                }
            }
            Some(OptionValue::Null) | None => 12,
            Some(_) => {
                return Err(HarnessError::new(
                    "target option must be serialized as a string or null",
                ));
            }
        };

        Ok(self
            .target_to_lib
            .get(&target_value)
            .cloned()
            .unwrap_or_else(|| "lib.d.ts".to_owned()))
    }

    fn matrix_string_values(&self, canonical: &str) -> Option<Vec<String>> {
        let values = match canonical {
            "target" => &self.target_values,
            "module" => &self.module_values,
            "jsx" => &self.jsx_values,
            "moduleResolution" => &self.module_resolution_values,
            _ => return None,
        };
        let mut values = values
            .iter()
            .map(|(name, value)| (*value, name.clone()))
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        Some(values.into_iter().map(|(_, name)| name).collect())
    }

    fn expand_lib_files(&self, roots: &[String]) -> HarnessResult<Vec<String>> {
        // Closure expansion reads every lib file in the closure just to
        // discover `<reference lib>` edges; the corpus reuses a handful
        // of root sets across thousands of programs, so the result is
        // cached per (lib dir, roots).
        type ExpansionKey = (PathBuf, Vec<String>);
        static CACHE: OnceLock<Mutex<BTreeMap<ExpansionKey, Vec<String>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
        let key = (self.lib_dir.clone(), roots.to_vec());
        if let Some(files) = cache.lock().expect("lib expansion cache").get(&key) {
            return Ok(files.clone());
        }

        let mut files = Vec::<String>::new();
        let mut seen = BTreeSet::<String>::new();
        for root in roots {
            self.collect_lib_file(root, &mut seen, &mut files)?;
        }

        let indexed: BTreeMap<String, usize> = files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.clone(), index))
            .collect();
        files.sort_by(|left, right| {
            self.lib_file_priority(left)
                .cmp(&self.lib_file_priority(right))
                .then_with(|| indexed[left].cmp(&indexed[right]))
        });
        cache
            .lock()
            .expect("lib expansion cache")
            .insert(key, files.clone());
        Ok(files)
    }

    fn collect_lib_file(
        &self,
        file_name: &str,
        seen: &mut BTreeSet<String>,
        files: &mut Vec<String>,
    ) -> HarnessResult<()> {
        let normalized = file_name.to_ascii_lowercase();
        if !seen.insert(normalized.clone()) {
            return Ok(());
        }

        let path = self.lib_dir.join(&normalized);
        let text = fs::read_to_string(&path).map_err(|err| {
            HarnessError::new(format!("failed to read {}: {err}", path.display()))
        })?;
        files.push(normalized);
        for reference in parse_lib_references(&text) {
            let referenced_file = self.lib_file_name(&reference)?;
            self.collect_lib_file(&referenced_file, seen, files)?;
        }
        Ok(())
    }

    fn lib_file_priority(&self, file_name: &str) -> usize {
        let basename = Path::new(file_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(file_name);
        if basename == "lib.d.ts" || basename == "lib.es6.d.ts" {
            return 0;
        }
        let lib_name = basename
            .strip_prefix("lib.")
            .and_then(|name| name.strip_suffix(".d.ts"))
            .unwrap_or(basename);
        self.lib_priority
            .get(lib_name)
            .copied()
            .unwrap_or(self.fallback_priority)
    }
}

fn parse_directive_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let after_slashes = trimmed.strip_prefix("//")?.trim_start();
    let after_at = after_slashes.strip_prefix('@')?;
    let colon = after_at.find(':')?;
    let name = after_at[..colon].trim();
    if name.is_empty() {
        return None;
    }
    Some((name, after_at[colon + 1..].trim()))
}

fn directive_spec(normalized_name: &str) -> Option<DirectiveSpec> {
    let spec = match normalized_name {
        "filename" => DirectiveSpec {
            canonical: "filename",
            kind: DirectiveKind::FileName,
        },
        "currentdirectory" => DirectiveSpec {
            canonical: "currentDirectory",
            kind: DirectiveKind::CurrentDirectory,
        },
        "allowarbitraryextensions" => bool_option("allowArbitraryExtensions"),
        "allowimportingtsextensions" => bool_option("allowImportingTsExtensions"),
        "allowjs" => bool_option("allowJs"),
        "allowunreachablecode" => bool_option("allowUnreachableCode"),
        "allowumdglobalaccess" => bool_option("allowUmdGlobalAccess"),
        "allowsyntheticdefaultimports" => bool_option("allowSyntheticDefaultImports"),
        "allowunusedlabels" => bool_option("allowUnusedLabels"),
        "alwaysstrict" => bool_option("alwaysStrict"),
        "checkjs" => bool_option("checkJs"),
        "declaration" => bool_option("declaration"),
        "declarationmap" => bool_option("declarationMap"),
        "downleveliteration" => bool_option("downlevelIteration"),
        "emitdeclarationonly" => bool_option("emitDeclarationOnly"),
        "emitdecoratormetadata" => bool_option("emitDecoratorMetadata"),
        "esmoduleinterop" => bool_option("esModuleInterop"),
        "exactoptionalpropertytypes" => bool_option("exactOptionalPropertyTypes"),
        "experimentaldecorators" => bool_option("experimentalDecorators"),
        "importhelpers" => bool_option("importHelpers"),
        "isolatedmodules" => bool_option("isolatedModules"),
        "libreplacement" => bool_option("libReplacement"),
        "noemit" => bool_option("noEmit"),
        "noemithelpers" => bool_option("noEmitHelpers"),
        "noemitonerror" => bool_option("noEmitOnError"),
        "noimplicitany" => bool_option("noImplicitAny"),
        "noimplicitoverride" => bool_option("noImplicitOverride"),
        "noimplicitreturns" => bool_option("noImplicitReturns"),
        "noimplicitthis" => bool_option("noImplicitThis"),
        "nolib" => bool_option("noLib"),
        "nopropertyaccessfromindexsignature" => bool_option("noPropertyAccessFromIndexSignature"),
        "nouncheckedindexedaccess" => bool_option("noUncheckedIndexedAccess"),
        "nouncheckedsideeffectimports" => bool_option("noUncheckedSideEffectImports"),
        "nounusedlocals" => bool_option("noUnusedLocals"),
        "nounusedparameters" => bool_option("noUnusedParameters"),
        "preserveconstenums" => bool_option("preserveConstEnums"),
        "preservevalueimports" => bool_option("preserveValueImports"),
        "pretty" => bool_option("pretty"),
        "removecomments" => bool_option("removeComments"),
        "resolvejsonmodule" => bool_option("resolveJsonModule"),
        "resolvepackagejsonexports" => bool_option("resolvePackageJsonExports"),
        "resolvepackagejsonimports" => bool_option("resolvePackageJsonImports"),
        "rewrite relative import extensions" => bool_option("rewriteRelativeImportExtensions"),
        "rewriterelativeimportextensions" => bool_option("rewriteRelativeImportExtensions"),
        "skipdefaultlibcheck" => bool_option("skipDefaultLibCheck"),
        "skiplibcheck" => bool_option("skipLibCheck"),
        "sourcemap" => bool_option("sourceMap"),
        "stripinternal" => bool_option("stripInternal"),
        "strict" => bool_option("strict"),
        "strictbindcallapply" => bool_option("strictBindCallApply"),
        "strictbuiltiniteratorreturn" => bool_option("strictBuiltinIteratorReturn"),
        "strictfunctiontypes" => bool_option("strictFunctionTypes"),
        "strictnullchecks" => bool_option("strictNullChecks"),
        "strictpropertyinitialization" => bool_option("strictPropertyInitialization"),
        "suppressimplicitanyindexerrors" => bool_option("suppressImplicitAnyIndexErrors"),
        "suppressoutputpathcheck" => bool_option("suppressOutputPathCheck"),
        "trace resolution" => bool_option("traceResolution"),
        "traceresolution" => bool_option("traceResolution"),
        "usedefineforclassfields" => bool_option("useDefineForClassFields"),
        "useunknownincatchvariables" => bool_option("useUnknownInCatchVariables"),
        "verbatimmodulesyntax" => bool_option("verbatimModuleSyntax"),
        "maxnodemodulejsdepth" => DirectiveSpec {
            canonical: "maxNodeModuleJsDepth",
            kind: DirectiveKind::Number,
        },
        "baseurl" => string_option("baseUrl"),
        "ignoredeprecations" => string_option("ignoreDeprecations"),
        "importsnotusedasvalues" => lower_string_option("importsNotUsedAsValues"),
        "jsxfactory" => string_option("jsxFactory"),
        "jsxfragmentfactory" => string_option("jsxFragmentFactory"),
        "jsximportsource" => string_option("jsxImportSource"),
        "moduledetection" => lower_string_option("moduleDetection"),
        "outdir" => string_option("outDir"),
        "outfile" => string_option("outFile"),
        "rootdir" => string_option("rootDir"),
        "jsx" => DirectiveSpec {
            canonical: "jsx",
            kind: DirectiveKind::MatrixString,
        },
        "target" => DirectiveSpec {
            canonical: "target",
            kind: DirectiveKind::MatrixString,
        },
        "module" => DirectiveSpec {
            canonical: "module",
            kind: DirectiveKind::MatrixString,
        },
        "moduleresolution" => DirectiveSpec {
            canonical: "moduleResolution",
            kind: DirectiveKind::MatrixString,
        },
        "customconditions" => list_option("customConditions"),
        "lib" => list_option("lib"),
        "typeroots" => list_option("typeRoots"),
        "types" => list_option("types"),
        "notypesandsymbols"
        | "noimplicitreferences"
        | "tsexpecterror"
        | "tsignore"
        | "tsnocheck" => DirectiveSpec {
            canonical: "harnessOnly",
            kind: DirectiveKind::HarnessOnly,
        },
        _ => return None,
    };
    Some(spec)
}

fn bool_option(canonical: &'static str) -> DirectiveSpec {
    DirectiveSpec {
        canonical,
        kind: DirectiveKind::Bool,
    }
}

fn string_option(canonical: &'static str) -> DirectiveSpec {
    DirectiveSpec {
        canonical,
        kind: DirectiveKind::String,
    }
}

fn lower_string_option(canonical: &'static str) -> DirectiveSpec {
    DirectiveSpec {
        canonical,
        kind: DirectiveKind::LowerString,
    }
}

fn list_option(canonical: &'static str) -> DirectiveSpec {
    DirectiveSpec {
        canonical,
        kind: DirectiveKind::StringList,
    }
}

fn normalize_directive_name(name: &str) -> String {
    name.chars()
        .filter(|ch| *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_cwd(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "/".to_owned()
    } else {
        value.to_owned()
    }
}

fn parse_bool(value: &str) -> HarnessResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(HarnessError::new(format!(
            "expected boolean directive value, got {other:?}"
        ))),
    }
}

fn parse_number(value: &str, name: &str) -> HarnessResult<i32> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|err| HarnessError::new(format!("expected numeric value for {name}: {err}")))
}

fn parse_string_option(value: &str) -> OptionValue {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("undefined") || trimmed.eq_ignore_ascii_case("null") {
        OptionValue::Null
    } else {
        OptionValue::String(trimmed.to_owned())
    }
}

fn parse_lower_string_option(value: &str) -> OptionValue {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("undefined") || trimmed.eq_ignore_ascii_case("null") {
        OptionValue::Null
    } else {
        OptionValue::String(trimmed.to_ascii_lowercase())
    }
}

fn split_option_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn split_lines_keep_endings(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                index += 1;
                lines.push(&text[start..index]);
                start = index;
            }
            b'\r' => {
                index += 1;
                if bytes.get(index) == Some(&b'\n') {
                    index += 1;
                }
                lines.push(&text[start..index]);
                start = index;
            }
            _ => {
                index += 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn parse_lib_entries(tsc: &str) -> HarnessResult<Vec<(String, String)>> {
    let body = extract_array_body(tsc, "var libEntries = [")?;
    let mut entries = Vec::new();
    for line in body.lines() {
        let Some(first_quote) = line.find('"') else {
            continue;
        };
        let rest = &line[first_quote + 1..];
        let Some(second_quote) = rest.find('"') else {
            continue;
        };
        let name = &rest[..second_quote];
        let rest = &rest[second_quote + 1..];
        let Some(third_quote) = rest.find('"') else {
            continue;
        };
        let rest = &rest[third_quote + 1..];
        let Some(fourth_quote) = rest.find('"') else {
            continue;
        };
        entries.push((name.to_owned(), rest[..fourth_quote].to_owned()));
    }
    if entries.is_empty() {
        return Err(HarnessError::new("failed to parse libEntries from _tsc.js"));
    }
    Ok(entries)
}

fn parse_target_to_lib_map(tsc: &str) -> HarnessResult<BTreeMap<i32, String>> {
    let body = extract_array_body(tsc, "var targetToLibMap = /* @__PURE__ */ new Map([")?;
    let mut map = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        let Some(comma) = line.find(',') else {
            continue;
        };
        let value = line[1..comma]
            .split_whitespace()
            .next()
            .ok_or_else(|| HarnessError::new("targetToLibMap entry missing target value"))?
            .parse::<i32>()
            .map_err(|err| HarnessError::new(format!("invalid targetToLibMap value: {err}")))?;
        let Some(first_quote) = line[comma + 1..].find('"') else {
            continue;
        };
        let rest = &line[comma + 1 + first_quote + 1..];
        let Some(second_quote) = rest.find('"') else {
            continue;
        };
        map.insert(value, rest[..second_quote].to_owned());
    }
    if map.is_empty() {
        return Err(HarnessError::new(
            "failed to parse targetToLibMap from _tsc.js",
        ));
    }
    Ok(map)
}

fn parse_option_number_map(tsc: &str, marker: &str) -> HarnessResult<BTreeMap<String, i32>> {
    let start = tsc
        .find(marker)
        .ok_or_else(|| HarnessError::new(format!("missing {marker} in _tsc.js")))?;
    let map_start = tsc[start..]
        .find("type: new Map(Object.entries({")
        .ok_or_else(|| HarnessError::new(format!("missing option map for {marker}")))?
        + start
        + "type: new Map(Object.entries({".len();
    let map_end = tsc[map_start..]
        .find("}))")
        .ok_or_else(|| HarnessError::new(format!("unterminated option map for {marker}")))?
        + map_start;
    let body = &tsc[map_start..map_end];
    let mut values = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim().trim_matches('"').to_owned();
        let value = line[colon + 1..]
            .split_whitespace()
            .next()
            .ok_or_else(|| HarnessError::new(format!("missing numeric value for option {key}")))?
            .parse::<i32>()
            .map_err(|err| {
                HarnessError::new(format!("invalid numeric value for option {key}: {err}"))
            })?;
        values.insert(key, value);
    }
    if values.is_empty() {
        return Err(HarnessError::new(format!(
            "failed to parse option map for {marker}"
        )));
    }
    Ok(values)
}

fn parse_standalone_number_map(tsc: &str, marker: &str) -> HarnessResult<BTreeMap<String, i32>> {
    let map_start = tsc
        .find(marker)
        .ok_or_else(|| HarnessError::new(format!("missing {marker} in _tsc.js")))?
        + marker.len();
    let map_end = tsc[map_start..]
        .find("}))")
        .ok_or_else(|| HarnessError::new(format!("unterminated option map for {marker}")))?
        + map_start;
    let body = &tsc[map_start..map_end];
    let mut values = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim().trim_matches('"').to_owned();
        let value = line[colon + 1..]
            .split_whitespace()
            .next()
            .ok_or_else(|| HarnessError::new(format!("missing numeric value for option {key}")))?
            .parse::<i32>()
            .map_err(|err| {
                HarnessError::new(format!("invalid numeric value for option {key}: {err}"))
            })?;
        values.insert(key, value);
    }
    if values.is_empty() {
        return Err(HarnessError::new(format!(
            "failed to parse option map for {marker}"
        )));
    }
    Ok(values)
}

fn extract_array_body<'a>(text: &'a str, marker: &str) -> HarnessResult<&'a str> {
    let start = text
        .find(marker)
        .ok_or_else(|| HarnessError::new(format!("missing marker in _tsc.js: {marker}")))?
        + marker.len();
    let end = text[start..]
        .find("];")
        .ok_or_else(|| HarnessError::new(format!("unterminated array after marker: {marker}")))?
        + start;
    Ok(&text[start..end])
}

fn parse_lib_references(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let marker = "/// <reference lib=\"";
            let start = line.find(marker)? + marker.len();
            let end = line[start..].find('"')? + start;
            Some(line[start..end].to_owned())
        })
        .collect()
}

fn push_json_object(out: &mut String, options: &BTreeMap<String, OptionValue>, indent: usize) {
    if options.is_empty() {
        out.push_str("{}");
        return;
    }

    out.push_str("{\n");
    for (index, (key, value)) in options.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        push_indent(out, indent + 2);
        push_json_string(out, key);
        out.push_str(": ");
        push_option_value(out, value);
    }
    out.push('\n');
    push_indent(out, indent);
    out.push('}');
}

fn push_option_value(out: &mut String, value: &OptionValue) {
    match value {
        OptionValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        OptionValue::Number(value) => out.push_str(&value.to_string()),
        OptionValue::String(value) => push_json_string(out, value),
        OptionValue::StringList(values) => push_json_string_array(out, values),
        OptionValue::Null => out.push_str("null"),
    }
}

fn push_json_string_array(out: &mut String, values: &[String]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        push_json_string(out, value);
    }
    out.push(']');
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < ' ' => {
                out.push_str("\\u");
                out.push_str(&format!("{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push(' ');
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn sanitize_matrix_key(key: &str) -> String {
    let mut out = String::new();
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '=' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "default".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn vendor_lib_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/typescript-6.0.3/lib")
            .canonicalize()
            .expect("vendored TypeScript lib exists")
    }

    #[test]
    fn harness_reaches_checker_api() {
        assert!(check_empty_program().diagnostics.is_empty());
    }

    #[test]
    fn expands_single_file_snapshot() {
        let programs = expand_fixture_text(
            "plain.ts",
            "// @noLib: true\n\nlet x = 1;\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");

        assert_eq!(programs.len(), 1);
        assert_eq!(
            programs[0].to_json(),
            "{\n  \"schema\": 1,\n  \"cwd\": \"/\",\n  \"options\": {\n    \"noLib\": true\n  },\n  \"libs\": [],\n  \"files\": [\n    {\n      \"name\": \"plain.ts\",\n      \"textB64\": \"bGV0IHggPSAxOwo=\"\n    }\n  ],\n  \"matrixKey\": \"\"\n}\n"
        );
    }

    #[test]
    fn strips_bom_and_preserves_crlf_snapshot() {
        let programs = expand_fixture_text(
            "bom.ts",
            "\u{feff}// @noLib: true\r\n\r\nlet x = 1;\r\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");

        assert_eq!(programs[0].files[0].text_b64, "bGV0IHggPSAxOw0K");
    }

    #[test]
    fn splits_multi_file_snapshot() {
        let programs = expand_fixture_text(
            "multi.ts",
            "// @noLib: true\n// @filename: a.ts\nexport const a = 1;\n// @filename: b.ts\nimport { a } from \"./a\";\na;\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");

        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].files.len(), 2);
        assert_eq!(programs[0].files[0].name, "a.ts");
        assert_eq!(
            programs[0].files[0].text_b64,
            "ZXhwb3J0IGNvbnN0IGEgPSAxOwo="
        );
        assert_eq!(programs[0].files[1].name, "b.ts");
        assert_eq!(
            programs[0].files[1].text_b64,
            "aW1wb3J0IHsgYSB9IGZyb20gIi4vYSI7CmE7Cg=="
        );
    }

    #[test]
    fn expands_target_matrix_snapshot() {
        let programs = expand_fixture_text(
            "matrix.ts",
            "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");

        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].matrix_key, "target=es5");
        assert_eq!(
            programs[0].options.get("target"),
            Some(&OptionValue::String("es5".to_owned()))
        );
        assert_eq!(programs[1].matrix_key, "target=es2015");
        assert_eq!(
            programs[1].options.get("target"),
            Some(&OptionValue::String("es2015".to_owned()))
        );
    }

    #[test]
    fn resolves_default_and_explicit_libs() {
        let default_programs = expand_fixture_text(
            "default.ts",
            "// @target: es2015\nlet x = new Promise(() => {});\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");
        assert!(default_programs[0]
            .libs
            .contains(&"lib.es6.d.ts".to_owned()));
        assert!(default_programs[0]
            .libs
            .contains(&"lib.es5.d.ts".to_owned()));
        assert!(default_programs[0]
            .libs
            .contains(&"lib.es2015.promise.d.ts".to_owned()));

        let explicit_programs = expand_fixture_text(
            "lib.ts",
            "// @lib: es5,dom\nlet documentTitle = document.title;\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");
        assert_eq!(
            explicit_programs[0].options.get("lib"),
            Some(&OptionValue::StringList(vec![
                "es5".to_owned(),
                "dom".to_owned(),
            ]))
        );
        assert!(explicit_programs[0]
            .libs
            .contains(&"lib.es5.d.ts".to_owned()));
        assert!(explicit_programs[0]
            .libs
            .contains(&"lib.dom.d.ts".to_owned()));
    }

    #[test]
    fn rejects_unknown_directives() {
        let err = expand_fixture_text(
            "bad.ts",
            "// @definitelyUnknown: true\nlet x = 1;\n",
            &vendor_lib_dir(),
        )
        .expect_err("unknown directives are hard errors");
        assert!(err.to_string().contains("unknown fixture directive"));
    }

    #[test]
    fn hand_picked_fixture_set_expands() {
        let fixtures = [
            ("plain.ts", "// @noLib: true\nlet x = 1;\n", 1),
            ("strict.ts", "// @noLib: true\n// @strict: true\nlet x = 1;\n", 1),
            ("target.ts", "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n", 2),
            ("module.ts", "// @noLib: true\n// @module: commonjs, esnext\nexport {};\n", 2),
            ("both.ts", "// @noLib: true\n// @target: es5, es2015\n// @module: commonjs, esnext\nexport {};\n", 4),
            ("crlf.ts", "// @noLib: true\r\nlet x = 1;\r\n", 1),
            ("bom.ts", "\u{feff}// @noLib: true\nlet x = 1;\n", 1),
            ("multi.ts", "// @noLib: true\n// @filename: a.ts\nlet a = 1;\n// @filename: b.ts\nlet b = 2;\n", 1),
            ("lib.ts", "// @lib: es5\nlet x: string;\n", 1),
            ("jsx.tsx", "// @noLib: true\n// @jsx: react-jsx\n<div />;\n", 1),
            ("allowjs.ts", "// @noLib: true\n// @allowJs: true\nlet x = 1;\n", 1),
            ("checkjs.ts", "// @noLib: true\n// @checkJs: true\nlet x = 1;\n", 1),
            ("decl.ts", "// @noLib: true\n// @declaration: true\nlet x = 1;\n", 1),
            ("unused.ts", "// @noLib: true\n// @noUnusedLocals: true\nlet x = 1;\n", 1),
            ("moduleResolution.ts", "// @noLib: true\n// @moduleResolution: node16\nexport {};\n", 1),
            ("outdir.ts", "// @noLib: true\n// @outdir: built\nlet x = 1;\n", 1),
            ("types.ts", "// @noLib: true\n// @types: node,jest\nlet x = 1;\n", 1),
            ("cwd.ts", "// @noLib: true\n// @currentDirectory: /src\nlet x = 1;\n", 1),
            ("filename-case.ts", "// @noLib: true\n// @Filename: main.ts\nlet x = 1;\n", 1),
            ("null-module.ts", "// @noLib: true\n// @module: undefined\nexport {};\n", 1),
        ];
        assert_eq!(fixtures.len(), 20);

        for (name, text, expected_count) in fixtures {
            let programs = expand_fixture_text(name, text, &vendor_lib_dir())
                .unwrap_or_else(|err| panic!("{name} should expand: {err}"));
            assert_eq!(programs.len(), expected_count, "{name}");
        }
    }

    #[test]
    fn writes_program_json_files() {
        let programs = expand_fixture_text(
            "matrix.ts",
            "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n",
            &vendor_lib_dir(),
        )
        .expect("fixture expands");
        let temp = std::env::temp_dir().join(format!(
            "tsrs2-harness-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));

        let paths = write_program_jsons(&programs, &temp).expect("programs write");
        assert_eq!(paths.len(), 2);
        assert!(paths[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("target=es5"));
        assert!(paths[1]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("target=es2015"));

        fs::remove_dir_all(temp).expect("remove temp dir");
    }
}
