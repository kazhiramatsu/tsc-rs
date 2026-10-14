//! tsgo `vfs/vfsmatch`'s glob specs: a spec (`src/**/*.ts`) compiled into
//! path components and matched against a path without regular expressions,
//! with the rules of its usage (files, directories or exclude).

use tsc_diagnostics::JsStr;

use crate::js_path::{combine_paths, root_parts};

/// tsgo `vfsmatch.Usage`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Usage {
    Files,
    Directories,
    Exclude,
}

/// tsgo `SpecMatcher`: whether a path matches any of the specs.
#[derive(Clone, Debug)]
pub struct SpecMatcher {
    patterns: Vec<GlobPattern>,
}

impl SpecMatcher {
    /// tsgo `NewSpecMatcher`: the specs relative to `base_path`, none when
    /// no spec compiles.
    pub fn new<S: AsRef<str>>(
        specs: &[S],
        base_path: &str,
        usage: Usage,
        case_sensitive: bool,
    ) -> Option<Self> {
        let patterns: Vec<GlobPattern> = specs
            .iter()
            .filter_map(|spec| {
                GlobPattern::compile(spec.as_ref(), base_path, usage, case_sensitive)
            })
            .collect();
        (!patterns.is_empty()).then_some(Self { patterns })
    }

    /// tsgo `MatchString` (and `MatchFileName`).
    pub fn match_string(&self, path: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.matches(path))
    }

    /// tsgo `MatchIndex`: the first spec the path matches.
    pub fn match_index(&self, path: &str) -> Option<usize> {
        self.patterns
            .iter()
            .position(|pattern| pattern.matches(path))
    }
}

/// tsgo `globPattern`.
#[derive(Clone, Debug)]
struct GlobPattern {
    /// The path's segments to match (`["src", "**", "*.ts"]`).
    components: Vec<Component>,
    /// Exclude patterns have different matching rules.
    is_exclude: bool,
    case_sensitive: bool,
    /// For "files" patterns, exclude `.min.js` by default.
    exclude_min_js: bool,
}

/// tsgo `component`: one path segment of a pattern.
#[derive(Clone, Debug)]
enum Component {
    /// An exact match (`src`).
    Literal(String),
    /// A segment with `*` or `?` (`*.ts`). Include patterns' wildcards skip
    /// the common package folders.
    Wildcard {
        segments: Vec<Segment>,
        skip_package_folders: bool,
    },
    /// `**`: zero or more directories.
    DoubleAsterisk,
}

/// tsgo `segment`: a piece of a wildcard component (`*.ts` is a star and
/// the literal `.ts`).
#[derive(Clone, Debug)]
enum Segment {
    Literal(String),
    /// Any characters but `/`.
    Star,
    /// One character but `/`.
    Question,
}

impl GlobPattern {
    /// tsgo `compileGlobPattern`: none when the pattern would match nothing.
    fn compile(spec: &str, base_path: &str, usage: Usage, case_sensitive: bool) -> Option<Self> {
        let mut parts = get_normalized_path_components(spec, base_path);
        let mut case_sensitive = case_sensitive;
        if tsc_host::is_encoded_dynamic_file_name(&parts[0]) {
            case_sensitive = true;
        }
        // "src/**" without a filename matches nothing (for include patterns)
        if usage != Usage::Exclude && parts.last().is_some_and(|last| last == "**") {
            return None;
        }
        // Normalize root: "/home/" -> "/home"
        parts[0] = remove_trailing_directory_separator(&parts[0]).to_owned();
        if tsc_host::is_encoded_dynamic_file_name(&parts[0]) {
            let root_parts: Vec<String> = parts[0].split('/').map(str::to_owned).collect();
            parts.splice(0..1, root_parts);
        }
        // Directories implicitly match all files: "src" -> "src/**/*"
        if parts.last().is_some_and(|last| is_implicit_glob(last)) {
            parts.push("**".to_owned());
            parts.push("*".to_owned());
        }
        let is_include = usage != Usage::Exclude;
        Some(Self {
            components: parts
                .iter()
                .map(|part| parse_component(part, is_include))
                .collect(),
            is_exclude: usage == Usage::Exclude,
            case_sensitive,
            exclude_min_js: usage == Usage::Files,
        })
    }

    /// tsgo `matches`.
    fn matches(&self, path: &str) -> bool {
        self.match_path(path, 0, 0)
    }

    /// tsgo `matchPathParts` of a single path.
    fn match_path(&self, path: &str, mut offset: usize, mut component_index: usize) -> bool {
        loop {
            let Some((part, next_offset)) = next_path_part(path, offset) else {
                return self.pattern_satisfied(component_index);
            };
            let Some(component) = self.components.get(component_index) else {
                return self.is_exclude;
            };
            match component {
                Component::DoubleAsterisk => {
                    if self.match_path(path, offset, component_index + 1) {
                        return true;
                    }
                    if !self.is_exclude && (is_hidden_path(part) || is_package_folder(part)) {
                        return false;
                    }
                    offset = next_offset;
                    continue;
                }
                Component::Literal(literal) => {
                    if !self.strings_equal(literal, part) {
                        return false;
                    }
                }
                Component::Wildcard {
                    segments,
                    skip_package_folders,
                } => {
                    if *skip_package_folders && is_package_folder(part) {
                        return false;
                    }
                    if !self.match_wildcard(segments, part) {
                        return false;
                    }
                }
            }
            offset = next_offset;
            component_index += 1;
        }
    }

    /// tsgo `patternSatisfied`: only trailing `**` components may match
    /// nothing.
    fn pattern_satisfied(&self, component_index: usize) -> bool {
        self.components[component_index..]
            .iter()
            .all(|component| matches!(component, Component::DoubleAsterisk))
    }

    /// tsgo `matchWildcard`: a path component against wildcard segments.
    fn match_wildcard(&self, segments: &[Segment], text: &str) -> bool {
        // Include patterns: wildcards at start cannot match hidden files
        if !self.is_exclude
            && is_hidden_path(text)
            && matches!(segments.first(), Some(Segment::Star | Segment::Question))
        {
            return false;
        }
        // Fast path: single * followed by literal suffix (e.g., "*.ts")
        if let [Segment::Star, Segment::Literal(suffix)] = segments {
            if text.len() < suffix.len()
                || !text.is_char_boundary(text.len() - suffix.len())
                || !self.strings_equal(suffix, &text[text.len() - suffix.len()..])
            {
                return false;
            }
            return self.should_include_min_js(text, segments);
        }
        self.match_segments(segments, text) && self.should_include_min_js(text, segments)
    }

    /// tsgo `matchSegments`: iterative, backtracking only to the last star.
    fn match_segments(&self, segments: &[Segment], text: &str) -> bool {
        let (mut segment_index, mut text_index) = (0, 0);
        let mut star: Option<(usize, usize)> = None;
        while text_index < text.len() {
            if let Some(segment) = segments.get(segment_index) {
                match segment {
                    Segment::Literal(literal) => {
                        let end = text_index + literal.len();
                        if end <= text.len()
                            && text.is_char_boundary(end)
                            && self.strings_equal(literal, &text[text_index..end])
                        {
                            text_index = end;
                            segment_index += 1;
                            continue;
                        }
                    }
                    Segment::Question => {
                        let ch = text[text_index..].chars().next().expect("a character");
                        if ch != '/' {
                            text_index += ch.len_utf8();
                            segment_index += 1;
                            continue;
                        }
                    }
                    Segment::Star => {
                        // Record star position for backtracking, then try matching zero chars.
                        star = Some((segment_index, text_index));
                        segment_index += 1;
                        continue;
                    }
                }
            }
            // Current segment didn't match. Backtrack to last star if possible.
            if let Some((star_segment, star_text)) = star {
                if star_text < text.len() && !text[star_text..].starts_with('/') {
                    // Star consumes one more character, retry from segment after star.
                    let ch = text[star_text..].chars().next().expect("a character");
                    let star_text = star_text + ch.len_utf8();
                    star = Some((star_segment, star_text));
                    text_index = star_text;
                    segment_index = star_segment + 1;
                    continue;
                }
            }
            return false;
        }
        // Consume any trailing stars.
        segments[segment_index.min(segments.len())..]
            .iter()
            .all(|segment| matches!(segment, Segment::Star))
    }

    /// tsgo `shouldIncludeMinJs`: a files pattern takes a `.min.js` file
    /// only when it mentions the `.min.` suffix.
    fn should_include_min_js(&self, file_name: &str, segments: &[Segment]) -> bool {
        if !self.exclude_min_js || !self.has_min_js_suffix(file_name) {
            return true;
        }
        // Allow when the user's pattern explicitly references the .min. suffix.
        segments.iter().any(|segment| {
            let Segment::Literal(literal) = segment else {
                return false;
            };
            let literal = if self.case_sensitive {
                literal.clone()
            } else {
                literal.to_lowercase()
            };
            literal.contains(".min.js") || literal.contains(".min.")
        })
    }

    /// tsgo `hasMinJsSuffix`.
    fn has_min_js_suffix(&self, file_name: &str) -> bool {
        const MIN_JS: &str = ".min.js";
        if self.case_sensitive {
            return file_name.ends_with(MIN_JS);
        }
        file_name.len() >= MIN_JS.len()
            && file_name.is_char_boundary(file_name.len() - MIN_JS.len())
            && equal_fold(&file_name[file_name.len() - MIN_JS.len()..], MIN_JS)
    }

    /// tsgo `stringsEqual`.
    fn strings_equal(&self, a: &str, b: &str) -> bool {
        if self.case_sensitive {
            return a == b;
        }
        equal_fold(a, b)
    }
}

/// tsgo `parseComponent`.
fn parse_component(text: &str, is_include: bool) -> Component {
    if text == "**" {
        return Component::DoubleAsterisk;
    }
    if !text.contains(['*', '?']) {
        return Component::Literal(text.to_owned());
    }
    Component::Wildcard {
        segments: parse_segments(text),
        skip_package_folders: is_include,
    }
}

/// tsgo `parseSegments`: `*.ts` as a star and the literal `.ts`.
fn parse_segments(text: &str) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '*' || ch == '?' {
            if index > start {
                result.push(Segment::Literal(text[start..index].to_owned()));
            }
            result.push(if ch == '*' {
                Segment::Star
            } else {
                Segment::Question
            });
            start = index + 1;
        }
    }
    if start < text.len() {
        result.push(Segment::Literal(text[start..].to_owned()));
    }
    result
}

/// tsgo `nextPathPartSingle`: the path's component at `offset` (the root
/// of an absolute path first, as an empty component) and the offset after
/// it.
fn next_path_part(path: &str, mut offset: usize) -> Option<(&str, usize)> {
    let bytes = path.as_bytes();
    if offset >= bytes.len() {
        return None;
    }
    if offset == 0 && bytes[0] == b'/' {
        return Some(("", 1));
    }
    while offset < bytes.len() && bytes[offset] == b'/' {
        offset += 1;
    }
    if offset >= bytes.len() {
        return None;
    }
    let rest = &path[offset..];
    Some(match rest.find('/') {
        Some(index) => (&rest[..index], offset + index),
        None => (rest, path.len()),
    })
}

/// tsgo `isHiddenPath`.
fn is_hidden_path(name: &str) -> bool {
    name.starts_with('.')
}

/// tsgo `isPackageFolder`: `node_modules`, `jspm_packages` or
/// `bower_components`, in any case.
fn is_package_folder(name: &str) -> bool {
    ["node_modules", "jspm_packages", "bower_components"]
        .iter()
        .any(|folder| name.eq_ignore_ascii_case(folder))
}

/// tsgo `IsImplicitGlob`: a last component with no extension or wildcard.
fn is_implicit_glob(last_path_component: &str) -> bool {
    !last_path_component.contains(['.', '*', '?'])
}

/// tsgo `RemoveTrailingDirectorySeparator`: a root's too (`/` is the empty
/// root component of a path).
fn remove_trailing_directory_separator(path: &str) -> &str {
    path.strip_suffix(['/', '\\']).unwrap_or(path)
}

/// tsgo `GetNormalizedPathComponents`: the root (empty for a relative
/// path) and the components of `path` against `current_directory`, `.` and
/// `..` resolved.
fn get_normalized_path_components(path: &str, current_directory: &str) -> Vec<String> {
    let combined = combine_paths(JsStr::from_str(current_directory), JsStr::from_str(path))
        .to_string_lossy()
        .into_owned();
    let root_length =
        root_parts(JsStr::from_str(&combined)).map_or(0, |(root, _)| root.as_bytes().len());
    // Always include the root component (empty string for relative paths).
    let mut components = vec![combined[..root_length].to_owned()];
    for component in combined[root_length..].split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if components.len() > 1 {
                if components.last().is_some_and(|last| last != "..") {
                    components.pop();
                    continue;
                }
            } else if !components[0].is_empty() {
                // If this is an absolute path, we can't go above the root.
                continue;
            }
        }
        components.push(component.to_owned());
    }
    components
}

/// Go `strings.EqualFold` over Unicode simple case mappings: two
/// characters are equal under folding when one of their single-character
/// lower or upper cases agree.
fn equal_fold(a: &str, b: &str) -> bool {
    fn single(mut mapped: impl Iterator<Item = char>, ch: char) -> char {
        match (mapped.next(), mapped.next()) {
            (Some(single), None) => single,
            _ => ch,
        }
    }
    let mut b_chars = b.chars();
    for a_char in a.chars() {
        let Some(b_char) = b_chars.next() else {
            return false;
        };
        if a_char != b_char
            && single(a_char.to_lowercase(), a_char) != single(b_char.to_lowercase(), b_char)
            && single(a_char.to_uppercase(), a_char) != single(b_char.to_uppercase(), b_char)
        {
            return false;
        }
    }
    b_chars.next().is_none()
}
