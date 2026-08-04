use crate::HarnessResult;

use super::{
    decode_source, error, sha256_hex, CompilerConfiguration, CompilerFixtureExpansion,
    CompilerLink, CompilerUnit, OrderedSetting, UnitContent, MAX_COMPILER_VARIATIONS,
};

// This order is derived from `CompilerTest.varyBy` in TypeScript 6.0.3's
// `src/testRunner/compilerRunner.ts`. It is observable: it determines both the
// dimensions and iteration order of the configuration Cartesian product.
const VARY_BY: [&str; 77] = [
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "sourceMap",
    "inlineSourceMap",
    "assumeChangesOnlyAffectDirectDependencies",
    "target",
    "module",
    "allowJs",
    "checkJs",
    "jsx",
    "composite",
    "removeComments",
    "importHelpers",
    "importsNotUsedAsValues",
    "downlevelIteration",
    "verbatimModuleSyntax",
    "isolatedDeclarations",
    "erasableSyntaxOnly",
    "libReplacement",
    "strict",
    "noImplicitAny",
    "strictNullChecks",
    "strictFunctionTypes",
    "strictBindCallApply",
    "strictPropertyInitialization",
    "strictBuiltinIteratorReturn",
    "stableTypeOrdering",
    "noImplicitThis",
    "useUnknownInCatchVariables",
    "alwaysStrict",
    "noUnusedLocals",
    "noUnusedParameters",
    "exactOptionalPropertyTypes",
    "noImplicitReturns",
    "noFallthroughCasesInSwitch",
    "noUncheckedIndexedAccess",
    "noImplicitOverride",
    "noPropertyAccessFromIndexSignature",
    "moduleResolution",
    "allowSyntheticDefaultImports",
    "esModuleInterop",
    "allowUmdGlobalAccess",
    "allowImportingTsExtensions",
    "rewriteRelativeImportExtensions",
    "resolvePackageJsonExports",
    "resolvePackageJsonImports",
    "noUncheckedSideEffectImports",
    "inlineSources",
    "experimentalDecorators",
    "emitDecoratorMetadata",
    "resolveJsonModule",
    "allowArbitraryExtensions",
    "skipDefaultLibCheck",
    "emitBOM",
    "newLine",
    "noErrorTruncation",
    "noLib",
    "noResolve",
    "stripInternal",
    "disableSizeLimit",
    "noImplicitUseStrict",
    "noEmitHelpers",
    "noEmitOnError",
    "preserveConstEnums",
    "skipLibCheck",
    "allowUnusedLabels",
    "allowUnreachableCode",
    "suppressExcessPropertyErrors",
    "suppressImplicitAnyIndexErrors",
    "forceConsistentCasingInFileNames",
    "noStrictGenericChecks",
    "useDefineForClassFields",
    "preserveValueImports",
    "moduleDetection",
    "noEmit",
    "isolatedModules",
];

const BOOLEAN_VALUES: [(&str, i32); 2] = [("true", 1), ("false", 0)];
const TARGET_VALUES: [(&str, i32); 15] = [
    ("es3", 0),
    ("es5", 1),
    ("es6", 2),
    ("es2015", 2),
    ("es2016", 3),
    ("es2017", 4),
    ("es2018", 5),
    ("es2019", 6),
    ("es2020", 7),
    ("es2021", 8),
    ("es2022", 9),
    ("es2023", 10),
    ("es2024", 11),
    ("es2025", 12),
    ("esnext", 99),
];
const MODULE_VALUES: [(&str, i32); 15] = [
    ("none", 0),
    ("commonjs", 1),
    ("amd", 2),
    ("system", 4),
    ("umd", 3),
    ("es6", 5),
    ("es2015", 5),
    ("es2020", 6),
    ("es2022", 7),
    ("esnext", 99),
    ("node16", 100),
    ("node18", 101),
    ("node20", 102),
    ("nodenext", 199),
    ("preserve", 200),
];
const JSX_VALUES: [(&str, i32); 5] = [
    ("preserve", 1),
    ("react-native", 3),
    ("react-jsx", 4),
    ("react-jsxdev", 5),
    ("react", 2),
];
const IMPORTS_NOT_USED_AS_VALUES: [(&str, i32); 3] = [("remove", 0), ("preserve", 1), ("error", 2)];
const MODULE_RESOLUTION_VALUES: [(&str, i32); 6] = [
    ("node10", 2),
    ("node", 2),
    ("classic", 1),
    ("node16", 3),
    ("nodenext", 99),
    ("bundler", 100),
];
const NEW_LINE_VALUES: [(&str, i32); 2] = [("crlf", 0), ("lf", 1)];
const MODULE_DETECTION_VALUES: [(&str, i32); 3] = [("auto", 2), ("legacy", 1), ("force", 3)];

#[derive(Clone, Debug)]
pub(super) struct ParsedUnit {
    pub(super) name: String,
    pub(super) file_options: Vec<OrderedSetting>,
    pub(super) content: Option<String>,
}

#[derive(Clone, Debug)]
struct Variation {
    key: String,
    value: Option<i32>,
}

pub(super) fn expand_compiler_fixture(
    source: u32,
    fixture_path: &str,
    raw: &[u8],
) -> HarnessResult<CompilerFixtureExpansion> {
    let (encoding, decoded) = decode_source(raw);
    let settings = extract_compiler_settings(&decoded);
    let configurations = expand_configurations(fixture_path, &settings)?;
    let (parsed_units, links) = make_units_from_test(&decoded, fixture_path)?;
    let mut units = parsed_units
        .into_iter()
        .map(compiler_unit)
        .collect::<Vec<_>>();
    let virtual_config = units
        .iter()
        .position(|unit| is_config_file_name(&unit.name))
        .map(|index| units.remove(index));

    Ok(CompilerFixtureExpansion {
        source,
        encoding,
        decoded_utf8_bytes: decoded.len() as u64,
        decoded_sha256: sha256_hex(decoded.as_bytes()),
        settings,
        normal_units: units,
        virtual_config,
        links,
        configurations,
    })
}

pub(super) fn extract_compiler_settings(content: &str) -> Vec<OrderedSetting> {
    let mut settings = Vec::new();
    for start in multiline_starts(content) {
        if let Some((name, value)) = parse_option_at(&content[start..]) {
            set_ordered(&mut settings, name, value);
        }
    }
    settings
}

fn multiline_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, ch) in content.char_indices() {
        if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            let next = index + ch.len_utf8();
            if next < content.len() {
                starts.push(next);
            }
        }
    }
    starts
}

fn parse_option_at(text: &str) -> Option<(String, String)> {
    let mut offset = 0;
    consume_exact(text, &mut offset, "//")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, "@")?;
    let name_start = offset;
    while let Some((ch, width)) = next_char(text, offset) {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            break;
        }
        offset += width;
    }
    if offset == name_start {
        return None;
    }
    let name = text[name_start..offset].to_owned();
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, ":")?;
    skip_js_whitespace(text, &mut offset);
    let value_end = find_cr_or_lf(&text[offset..]).map_or(text.len(), |relative| offset + relative);
    Some((name, js_trim(&text[offset..value_end]).to_owned()))
}

fn parse_link_at(text: &str) -> Option<CompilerLink> {
    let mut offset = 0;
    consume_exact(text, &mut offset, "//")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, "@link")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, ":")?;
    skip_js_whitespace(text, &mut offset);
    let value_end = find_cr_or_lf(&text[offset..]).map_or(text.len(), |relative| offset + relative);
    let value = &text[offset..value_end];
    let arrow = value.rfind("->")?;
    Some(CompilerLink {
        target: js_trim(&value[..arrow]).to_owned(),
        link_path: js_trim(&value[arrow + 2..]).to_owned(),
    })
}

fn parse_first_option(text: &str) -> Option<(String, String)> {
    multiline_starts(text)
        .into_iter()
        .find_map(|start| parse_option_at(&text[start..]))
}

fn parse_first_link(text: &str) -> Option<CompilerLink> {
    multiline_starts(text)
        .into_iter()
        .find_map(|start| parse_link_at(&text[start..]))
}

fn find_cr_or_lf(text: &str) -> Option<usize> {
    text.char_indices()
        .find_map(|(index, ch)| matches!(ch, '\r' | '\n').then_some(index))
}

pub(super) fn make_units_from_test(
    code: &str,
    fixture_path: &str,
) -> HarnessResult<(Vec<ParsedUnit>, Vec<CompilerLink>)> {
    let mut units = Vec::new();
    let mut links = Vec::new();
    let mut current_content: Option<String> = None;
    let mut current_options = Vec::new();
    let mut current_name: Option<String> = None;

    for line in split_content_by_newlines(code) {
        if let Some(link) = parse_first_link(line) {
            links.push(link);
            continue;
        }
        if let Some((name, value)) = parse_first_option(line) {
            set_ordered(&mut current_options, name.clone(), value.clone());
            if !name.eq_ignore_ascii_case("filename") {
                continue;
            }

            if current_name.as_deref().is_some_and(|name| !name.is_empty()) {
                units.push(ParsedUnit {
                    name: current_name
                        .take()
                        .expect("truthy file name must be present"),
                    file_options: std::mem::take(&mut current_options),
                    content: current_content.take(),
                });
                current_name = Some(value);
            } else {
                current_name = Some(value);
                if current_content
                    .as_deref()
                    .is_some_and(|content| !content.is_empty() && !only_trivia(content))
                {
                    return Err(error(format!(
                        "compiler fixture {fixture_path:?} contains non-comment content before its first @filename directive"
                    )));
                }
                current_content = Some(String::new());
            }
            continue;
        }
        append_content_line(&mut current_content, line);
    }

    let name = if !units.is_empty()
        || current_name
            .as_deref()
            .is_some_and(|current_name| !current_name.is_empty())
    {
        current_name.unwrap_or_default()
    } else {
        base_file_name(fixture_path).to_owned()
    };
    units.push(ParsedUnit {
        name,
        file_options: current_options,
        // The final unit uses `currentFileContent || ""` upstream. Only
        // intermediate units can retain JavaScript `undefined` content.
        content: Some(current_content.unwrap_or_default()),
    });

    Ok((units, links))
}

fn split_content_by_newlines(content: &str) -> impl Iterator<Item = &str> {
    content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn append_content_line(content: &mut Option<String>, line: &str) {
    let content = content.get_or_insert_with(String::new);
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(line);
}

fn compiler_unit(unit: ParsedUnit) -> CompilerUnit {
    let document_symlinks = unit
        .file_options
        .iter()
        .find(|setting| setting.name == "symlink")
        .filter(|setting| !setting.value.is_empty())
        .map(|setting| {
            setting
                .value
                .split(',')
                .map(|path| js_trim(path).to_owned())
                .collect()
        })
        .unwrap_or_default();
    let content = match unit.content {
        Some(content) => UnitContent::Present {
            utf8_bytes: content.len() as u64,
            sha256: sha256_hex(content.as_bytes()),
        },
        None => UnitContent::Missing,
    };
    CompilerUnit {
        name: unit.name,
        file_options: unit.file_options,
        content,
        document_symlinks,
    }
}

pub(super) fn is_config_file_name(path: &str) -> bool {
    matches!(
        base_file_name(path).to_ascii_lowercase().as_str(),
        "tsconfig.json" | "jsconfig.json"
    )
}

pub(super) fn expand_configurations(
    fixture_path: &str,
    settings: &[OrderedSetting],
) -> HarnessResult<Vec<CompilerConfiguration>> {
    let mut dimensions: Vec<(&'static str, Vec<String>)> = Vec::new();
    let mut variation_count = 1usize;
    for vary_by in VARY_BY {
        let Some(setting) = settings.iter().find(|setting| setting.name == vary_by) else {
            continue;
        };
        let Some(entries) = split_vary_by_setting_value(&setting.value, vary_by)? else {
            continue;
        };
        variation_count = variation_count
            .checked_mul(entries.len())
            .unwrap_or(MAX_COMPILER_VARIATIONS + 1);
        if variation_count > MAX_COMPILER_VARIATIONS {
            return Err(error(format!(
                "compiler fixture {fixture_path:?} exceeds TypeScript's {MAX_COMPILER_VARIATIONS}-configuration maximum"
            )));
        }
        dimensions.push((vary_by, entries));
    }

    if dimensions.is_empty() {
        return Ok(vec![CompilerConfiguration {
            variant: "default".to_owned(),
            description: String::new(),
            upstream_name: base_file_name(fixture_path).to_owned(),
            settings: Vec::new(),
        }]);
    }

    let mut states = Vec::with_capacity(variation_count);
    compute_configuration_product(&dimensions, 0, &mut Vec::new(), &mut states);
    Ok(states
        .into_iter()
        .map(|settings| compiler_configuration(fixture_path, settings))
        .collect())
}

fn compute_configuration_product(
    dimensions: &[(&'static str, Vec<String>)],
    offset: usize,
    state: &mut Vec<OrderedSetting>,
    output: &mut Vec<Vec<OrderedSetting>>,
) {
    if offset == dimensions.len() {
        output.push(state.clone());
        return;
    }
    let (name, entries) = &dimensions[offset];
    for entry in entries {
        state.push(OrderedSetting {
            name: (*name).to_owned(),
            value: entry.clone(),
        });
        compute_configuration_product(dimensions, offset + 1, state, output);
        state.pop();
    }
}

fn compiler_configuration(
    fixture_path: &str,
    settings: Vec<OrderedSetting>,
) -> CompilerConfiguration {
    let mut sorted = settings.iter().collect::<Vec<_>>();
    // All option names are ASCII, matching JavaScript's default string sort.
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let variant = sorted
        .iter()
        .map(|setting| {
            format!(
                "{}={}",
                setting.name.to_lowercase(),
                setting.value.to_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let description = sorted
        .iter()
        .map(|setting| format!("@{}: {}", setting.name, setting.value))
        .collect::<Vec<_>>()
        .join(", ");
    let just_name = base_file_name(fixture_path);
    let (stem, extension) = split_extension(just_name);
    CompilerConfiguration {
        upstream_name: format!("{stem}({variant}){extension}"),
        variant,
        description,
        settings,
    }
}

fn split_vary_by_setting_value(text: &str, vary_by: &str) -> HarnessResult<Option<Vec<String>>> {
    if text.is_empty() {
        return Ok(None);
    }
    let mut star = false;
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    for entry in text.split(',') {
        let entry = js_trim(entry).to_lowercase();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            star = true;
        } else if entry.starts_with('-') || entry.starts_with('!') {
            excludes.push(entry[1..].to_owned());
        } else {
            includes.push(entry);
        }
    }
    if includes.len() <= 1 && !star && excludes.is_empty() {
        return Ok(None);
    }

    let values = vary_by_values(vary_by);
    let mut variations: Vec<Variation> = Vec::new();
    for include in includes {
        let value = option_value(values, &include);
        if !variations
            .iter()
            .any(|variation| equivalent_variation(variation, &include, value))
        {
            variations.push(Variation {
                key: include,
                value,
            });
        }
    }
    if star {
        for &(key, value) in values {
            if !variations
                .iter()
                .any(|variation| variation.key == key || variation.value == Some(value))
            {
                variations.push(Variation {
                    key: key.to_owned(),
                    value: Some(value),
                });
            }
        }
    }
    for exclude in excludes {
        let value = option_value(values, &exclude);
        variations.retain(|variation| !equivalent_variation(variation, &exclude, value));
    }
    if variations.is_empty() {
        return Err(error(format!(
            "variations in compiler option '@{vary_by}' resulted in an empty set"
        )));
    }
    Ok(Some(
        variations
            .into_iter()
            .map(|variation| variation.key)
            .collect(),
    ))
}

fn equivalent_variation(variation: &Variation, key: &str, value: Option<i32>) -> bool {
    variation.key == key || value.is_some() && variation.value == value
}

fn option_value(values: &[(&str, i32)], key: &str) -> Option<i32> {
    values
        .iter()
        .find_map(|(candidate, value)| (*candidate == key).then_some(*value))
}

fn vary_by_values(vary_by: &str) -> &'static [(&'static str, i32)] {
    match vary_by {
        "target" => &TARGET_VALUES,
        "module" => &MODULE_VALUES,
        "jsx" => &JSX_VALUES,
        "importsNotUsedAsValues" => &IMPORTS_NOT_USED_AS_VALUES,
        "moduleResolution" => &MODULE_RESOLUTION_VALUES,
        "newLine" => &NEW_LINE_VALUES,
        "moduleDetection" => &MODULE_DETECTION_VALUES,
        _ => &BOOLEAN_VALUES,
    }
}

fn set_ordered(settings: &mut Vec<OrderedSetting>, name: String, value: String) {
    if let Some(existing) = settings.iter_mut().find(|setting| setting.name == name) {
        existing.value = value;
    } else {
        settings.push(OrderedSetting { name, value });
    }
}

fn base_file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn split_extension(file_name: &str) -> (&str, &str) {
    match file_name.rfind('.') {
        Some(index) => (&file_name[..index], &file_name[index..]),
        None => (file_name, ""),
    }
}

fn consume_exact(text: &str, offset: &mut usize, expected: &str) -> Option<()> {
    text.get(*offset..)?.starts_with(expected).then(|| {
        *offset += expected.len();
    })
}

fn next_char(text: &str, offset: usize) -> Option<(char, usize)> {
    text.get(offset..)?
        .chars()
        .next()
        .map(|ch| (ch, ch.len_utf8()))
}

fn skip_js_whitespace(text: &str, offset: &mut usize) {
    while let Some((ch, width)) = next_char(text, *offset) {
        if !is_js_whitespace(ch) {
            break;
        }
        *offset += width;
    }
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn only_trivia(text: &str) -> bool {
    let mut offset = 0;
    while offset < text.len() {
        skip_js_whitespace(text, &mut offset);
        if offset == text.len() {
            return true;
        }
        let rest = &text[offset..];
        if rest.starts_with("//") || offset == 0 && rest.starts_with("#!") {
            offset += find_cr_or_lf(rest).unwrap_or(rest.len());
            continue;
        }
        if let Some(comment) = rest.strip_prefix("/*") {
            let Some(end) = comment.find("*/") else {
                return true;
            };
            offset += 2 + end + 2;
            continue;
        }
        return false;
    }
    true
}
