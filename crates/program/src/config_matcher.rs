//! Compiled matching for TypeScript config `files` wildcard specifications.
//!
//! The matcher deliberately does not build a regular expression. Both the
//! path-component walk and each wildcard-component walk are iterative dynamic
//! programs, so adversarial runs of `*` cannot recurse or backtrack
//! exponentially. A compiled pattern is reusable across directory entries.

const COMMON_PACKAGE_FOLDERS: &[&str] = &["node_modules", "bower_components", "jspm_packages"];

/// A compiled TypeScript config-file include pattern.
///
/// This is the `files` usage of TypeScript's wildcard machinery, not the
/// subtly different `directories` or `exclude` usages.
///
/// tsc-port: filesMatcher @6.0.3
/// tsc-hash: 5895ac907a3cdc42307d65af29e1c20bf90924b13cb92f16dd7a3b6fdbc84a92
/// tsc-span: _tsc.js:18401-18430
/// tsc-port: getRegularExpressionsForWildcards/getSubPatternFromSpec @6.0.3
/// tsc-hash: 63f8b014e6d05ad08fa7f2f36526b14c450cadc58e8c830ab9a13b4ace58424c
/// tsc-span: _tsc.js:18457-18503
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigFilePattern {
    root: String,
    components: Vec<PatternComponent>,
    case_sensitive: bool,
}

impl ConfigFilePattern {
    /// Compile one config `include` specification relative to `base`.
    ///
    /// An empty specification and a specification ending in a whole-component
    /// `**` produce `None`, as `getSubPatternFromSpec(..., "files")` does.
    /// POSIX and rooted drive paths are supported. Path normalization is
    /// lexical and therefore keeps wildcard-bearing components intact.
    pub fn new(spec: &str, base: &str, case_sensitive: bool) -> Result<Option<Self>, String> {
        if spec.is_empty() {
            return Ok(None);
        }

        let normalized = normalize_spec(spec, base)?;
        if normalized
            .components
            .last()
            .is_some_and(|part| part == "**")
        {
            return Ok(None);
        }

        let implicit_glob = normalized
            .components
            .last()
            .is_none_or(|part| !part.contains(['.', '*', '?']));
        let mut components = normalized
            .components
            .into_iter()
            .map(PatternComponent::compile)
            .collect::<Vec<_>>();
        if implicit_glob {
            components.push(PatternComponent::Recursive);
            components.push(PatternComponent::compile("*".to_owned()));
        }

        Ok(Some(Self {
            root: normalized.root,
            components,
            case_sensitive,
        }))
    }

    /// Return whether `absolute_path` is selected by this compiled pattern.
    ///
    /// Unsupported or relative candidate paths simply do not match. The path
    /// component DP uses linear scratch space; `**` only has recursive meaning
    /// when it is an entire pattern component.
    pub fn matches(&self, absolute_path: &str) -> bool {
        let Ok(path) = normalize_absolute(absolute_path) else {
            return false;
        };
        if !regex_text_eq(&self.root, &path.root, self.case_sensitive) {
            return false;
        }

        let inputs = path
            .components
            .iter()
            .map(|text| InputComponent::new(text, self.case_sensitive))
            .collect::<Vec<_>>();
        let input_count = inputs.len();
        let mut previous = vec![false; input_count + 1];
        let mut current = vec![false; input_count + 1];
        previous[0] = true;

        for component in &self.components {
            current.fill(false);
            match component {
                PatternComponent::Recursive => {
                    current[0] = previous[0];
                    for input_index in 1..=input_count {
                        current[input_index] = previous[input_index]
                            || (current[input_index - 1]
                                && inputs[input_index - 1].recursive_wildcard_allowed());
                    }
                }
                PatternComponent::Glob(glob) => {
                    for input_index in 1..=input_count {
                        current[input_index] = previous[input_index - 1]
                            && glob.matches(
                                &inputs[input_index - 1],
                                input_index == input_count,
                                self.case_sensitive,
                            );
                    }
                }
            }
            std::mem::swap(&mut previous, &mut current);
        }

        previous[input_count]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternComponent {
    Recursive,
    Glob(GlobComponent),
}

impl PatternComponent {
    fn compile(text: String) -> Self {
        if text == "**" {
            return Self::Recursive;
        }

        let mut has_wildcard = false;
        let tokens = text
            .encode_utf16()
            .map(|unit| match unit {
                unit if unit == u16::from(b'*') => {
                    has_wildcard = true;
                    GlobToken::Star
                }
                unit if unit == u16::from(b'?') => {
                    has_wildcard = true;
                    GlobToken::Question
                }
                literal => GlobToken::Literal(literal),
            })
            .collect();
        Self::Glob(GlobComponent {
            tokens,
            has_wildcard,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GlobComponent {
    tokens: Vec<GlobToken>,
    has_wildcard: bool,
}

impl GlobComponent {
    fn matches(
        &self,
        input: &InputComponent<'_>,
        is_last_path_component: bool,
        case_sensitive: bool,
    ) -> bool {
        if self.has_wildcard && input.common_package_folder {
            return false;
        }

        let input_count = input.characters.len();
        let min_js_dot = is_last_path_component
            .then(|| min_js_dot_index(&input.characters, case_sensitive))
            .flatten();
        let mut previous = vec![false; input_count + 1];
        let mut current = vec![false; input_count + 1];
        previous[0] = true;

        for (token_index, token) in self.tokens.iter().enumerate() {
            current.fill(false);
            match token {
                GlobToken::Literal(literal) => {
                    for input_index in 1..=input_count {
                        current[input_index] = previous[input_index - 1]
                            && regex_code_unit_eq(
                                *literal,
                                input.characters[input_index - 1],
                                case_sensitive,
                            );
                    }
                }
                GlobToken::Question => {
                    for input_index in 1..=input_count {
                        let character = input.characters[input_index - 1];
                        current[input_index] = previous[input_index - 1]
                            && !(token_index == 0 && character == u16::from(b'.'));
                    }
                }
                GlobToken::Star => {
                    current[0] = previous[0];
                    for input_index in 1..=input_count {
                        let character_index = input_index - 1;
                        let may_consume = !(token_index == 0
                            && character_index == 0
                            && input.characters[character_index] == u16::from(b'.'))
                            && min_js_dot != Some(character_index);
                        current[input_index] =
                            previous[input_index] || (current[input_index - 1] && may_consume);
                    }
                }
            }
            std::mem::swap(&mut previous, &mut current);
        }

        previous[input_count]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobToken {
    Literal(u16),
    Star,
    Question,
}

struct InputComponent<'a> {
    text: &'a str,
    characters: Vec<u16>,
    common_package_folder: bool,
}

impl<'a> InputComponent<'a> {
    fn new(text: &'a str, case_sensitive: bool) -> Self {
        Self {
            text,
            characters: text.encode_utf16().collect(),
            common_package_folder: COMMON_PACKAGE_FOLDERS
                .iter()
                .any(|folder| regex_text_eq(text, folder, case_sensitive)),
        }
    }

    fn recursive_wildcard_allowed(&self) -> bool {
        !self.text.starts_with('.') && !self.common_package_folder
    }
}

#[derive(Debug, Eq, PartialEq)]
struct NormalizedPath {
    root: String,
    components: Vec<String>,
}

fn normalize_spec(spec: &str, base: &str) -> Result<NormalizedPath, String> {
    reject_nul(spec, "config file pattern")?;
    let slashed_spec = spec.replace('\\', "/");
    if split_root(&slashed_spec)?.is_some() {
        return normalize_absolute_slashed(&slashed_spec);
    }

    let mut normalized = normalize_absolute(base)
        .map_err(|detail| format!("invalid config pattern base {base:?}: {detail}"))?;
    reduce_components(&mut normalized.components, &slashed_spec);
    Ok(normalized)
}

fn normalize_absolute(path: &str) -> Result<NormalizedPath, String> {
    reject_nul(path, "path")?;
    normalize_absolute_slashed(&path.replace('\\', "/"))
}

fn normalize_absolute_slashed(path: &str) -> Result<NormalizedPath, String> {
    let Some((root, tail)) = split_root(path)? else {
        return Err(format!("path {path:?} is not absolute"));
    };
    let mut components = Vec::new();
    reduce_components(&mut components, tail);
    Ok(NormalizedPath { root, components })
}

fn split_root(path: &str) -> Result<Option<(String, &str)>, String> {
    if path.starts_with("//") {
        return Err(format!(
            "UNC path {path:?} is outside the supported root profile"
        ));
    }
    if let Some(tail) = path.strip_prefix('/') {
        return Ok(Some(("/".to_owned(), tail)));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return Ok(Some((path[..3].to_owned(), &path[3..])));
    }
    Ok(None)
}

fn reduce_components(components: &mut Vec<String>, tail: &str) {
    for component in tail.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            component => components.push(component.to_owned()),
        }
    }
}

fn min_js_dot_index(characters: &[u16], case_sensitive: bool) -> Option<usize> {
    const SUFFIX: [u16; 7] = [
        b'.' as u16,
        b'm' as u16,
        b'i' as u16,
        b'n' as u16,
        b'.' as u16,
        b'j' as u16,
        b's' as u16,
    ];
    let start = characters.len().checked_sub(SUFFIX.len())?;
    characters[start..]
        .iter()
        .copied()
        .zip(SUFFIX)
        .all(|(left, right)| regex_code_unit_eq(left, right, case_sensitive))
        .then_some(start)
}

fn reject_nul(text: &str, role: &str) -> Result<(), String> {
    if text.contains('\0') {
        Err(format!("{role} contains a NUL byte"))
    } else {
        Ok(())
    }
}

fn regex_text_eq(left: &str, right: &str, case_sensitive: bool) -> bool {
    let mut left = left.encode_utf16();
    let mut right = right.encode_utf16();
    loop {
        match (left.next(), right.next()) {
            (Some(left), Some(right)) if regex_code_unit_eq(left, right, case_sensitive) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn regex_code_unit_eq(left: u16, right: u16, case_sensitive: bool) -> bool {
    left == right
        || (!case_sensitive
            && regex_canonicalize_code_unit(left) == regex_canonicalize_code_unit(right))
}

/// ECMAScript `Canonicalize` for a non-Unicode, ignore-case RegExp. TypeScript
/// creates wildcard regexes without the `u` flag, so Unicode case folding and
/// Rust scalar-value lowercasing are observably different here.
fn regex_canonicalize_code_unit(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut uppercase = character.to_uppercase();
    let Some(first) = uppercase.next() else {
        return unit;
    };
    if uppercase.next().is_some() || first.len_utf16() != 1 {
        return unit;
    }
    let uppercase = first as u32 as u16;
    if unit >= 0x80 && uppercase < 0x80 {
        unit
    } else {
        uppercase
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigFilePattern;

    fn pattern(spec: &str) -> ConfigFilePattern {
        ConfigFilePattern::new(spec, "/work", true)
            .expect("valid pattern")
            .expect("usable files pattern")
    }

    #[test]
    fn normalizes_paths_and_expands_implicit_directory_globs() {
        let pattern = pattern("./src/../src");
        assert!(pattern.matches("/work/src/index.ts"));
        assert!(pattern.matches("/work/src/nested/index.ts"));
        assert!(!pattern.matches("/work/index.ts"));

        let drive = ConfigFilePattern::new("../src/*.TS", "C:/Project/config", false)
            .expect("valid drive pattern")
            .expect("usable drive pattern");
        assert!(drive.matches("c:\\project\\SRC\\main.ts"));
    }

    #[test]
    fn recursive_wildcard_excludes_implicit_directories() {
        let selection = pattern("src/**/*.ts");
        assert!(selection.matches("/work/src/nested/index.ts"));
        assert!(!selection.matches("/work/src/.cache/index.ts"));
        assert!(!selection.matches("/work/src/node_modules/pkg/index.ts"));

        let explicit = pattern("src/node_modules/**/*.ts");
        assert!(explicit.matches("/work/src/node_modules/pkg/index.ts"));
    }

    #[test]
    fn component_wildcards_preserve_dot_package_and_min_js_rules() {
        let javascript = pattern("src/*.js");
        assert!(javascript.matches("/work/src/main.js"));
        assert!(!javascript.matches("/work/src/.hidden.js"));
        assert!(!javascript.matches("/work/src/main.min.js"));

        assert!(pattern("src/.*.js").matches("/work/src/.hidden.js"));
        assert!(pattern("src/*.min.js").matches("/work/src/main.min.js"));
        assert!(pattern("src/*.*").matches("/work/src/.hidden.js"));
        assert!(pattern("src/*.*").matches("/work/src/main.min.js"));
        assert!(!pattern("src/*/*.ts").matches("/work/src/node_modules/index.ts"));

        let implicit = pattern("src");
        assert!(!implicit.matches("/work/src/.hidden.ts"));
        assert!(!implicit.matches("/work/src/main.min.js"));
        assert!(pattern(".dir/**/*.ts").matches("/work/.dir/main.ts"));
    }

    #[test]
    fn only_a_whole_component_double_star_is_recursive() {
        assert!(ConfigFilePattern::new("src/**", "/work", true)
            .expect("valid pattern")
            .is_none());

        let ordinary = pattern("src/**name.ts");
        assert!(ordinary.matches("/work/src/long-name.ts"));
        assert!(!ordinary.matches("/work/src/nested/long-name.ts"));
    }

    #[test]
    fn question_mark_matches_one_character_with_the_host_case_profile() {
        let sensitive = pattern("src/file?.ts");
        assert!(sensitive.matches("/work/src/file1.ts"));
        assert!(!sensitive.matches("/work/src/file10.ts"));
        assert!(!sensitive.matches("/WORK/src/file1.ts"));

        let insensitive = ConfigFilePattern::new("src/file?.TS", "/work", false)
            .expect("valid pattern")
            .expect("usable files pattern");
        assert!(insensitive.matches("/WORK/SRC/FILE1.ts"));

        let protected = ConfigFilePattern::new("İ/*.TS", "/work", false)
            .expect("valid protected-case pattern")
            .expect("usable protected-case pattern");
        assert!(protected.matches("/WORK/İ/FILE.ts"));
        assert!(!protected.matches("/work/i/file.ts"));

        let insensitive_component = |component: &str| {
            ConfigFilePattern::new(&format!("{component}/*.ts"), "/work", false)
                .expect("valid Unicode case pattern")
                .expect("usable Unicode case pattern")
        };
        assert!(!insensitive_component("K").matches("/work/k/file.ts"));
        assert!(insensitive_component("Σ").matches("/work/ς/file.ts"));
        assert!(!insensitive_component("ẞ").matches("/work/ß/file.ts"));

        assert!(!pattern("src/?.ts").matches("/work/src/💩.ts"));
        assert!(pattern("src/??.ts").matches("/work/src/💩.ts"));
        assert!(pattern("src/💩.ts").matches("/work/src/💩.ts"));
    }

    #[test]
    fn relative_patterns_can_select_files_outside_the_config_base() {
        let outside = pattern("../shared/**/*.ts");
        assert!(outside.matches("/shared/nested/main.ts"));
        assert!(!outside.matches("/work/shared/main.ts"));
    }
}
