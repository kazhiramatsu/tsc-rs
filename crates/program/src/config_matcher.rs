//! Compiled matching for TypeScript config `files` wildcard specifications.
//!
//! The matcher deliberately does not build a regular expression. Both the
//! path-component walk and each wildcard-component walk are iterative dynamic
//! programs, so adversarial runs of `*` cannot recurse or backtrack
//! exponentially. A compiled pattern is reusable across directory entries.

use tsc_diagnostics::{JsStr, JsString};

use crate::js_path::{normalize_slashes, root_parts};

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
    root: GlobComponent,
    components: Vec<PatternComponent>,
    case_sensitive: bool,
}

impl ConfigFilePattern {
    /// Compile one config `include` specification relative to `base`.
    ///
    /// An empty specification and a specification ending in a whole-component
    /// `**` produce `None`, as `getSubPatternFromSpec(..., "files")` does.
    /// POSIX, drive, UNC, and URL roots are supported. Path normalization is
    /// lexical and therefore keeps wildcard-bearing components intact.
    pub fn new<'s, 'b>(
        spec: impl Into<JsStr<'s>>,
        base: impl Into<JsStr<'b>>,
        case_sensitive: bool,
    ) -> Result<Option<Self>, String> {
        let spec = spec.into();
        let base = base.into();
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

        // getNormalizedPathComponents always retains the root as component
        // zero. It therefore participates in isImplicitGlob when the path has
        // no tail component (for example `https://host`).
        let implicit_glob = normalized.components.last().map_or_else(
            || {
                !normalized
                    .root
                    .as_bytes()
                    .iter()
                    .any(|byte| matches!(byte, b'.' | b'*' | b'?'))
            },
            |part| {
                !part
                    .as_bytes()
                    .iter()
                    .any(|byte| matches!(byte, b'.' | b'*' | b'?'))
            },
        );
        let mut components = normalized
            .components
            .into_iter()
            .map(PatternComponent::compile)
            .collect::<Vec<_>>();
        if implicit_glob {
            components.push(PatternComponent::Recursive);
            components.push(PatternComponent::compile("*".into()));
        }

        Ok(Some(Self {
            // TypeScript removes exactly one trailing separator from root
            // component zero before compiling the wildcard regexp. Compiling
            // the root (rather than comparing it literally) preserves `*` and
            // `?` in UNC servers and URL schemes/authorities.
            root: GlobComponent::compile(
                normalized
                    .root
                    .as_js()
                    .strip_suffix("/")
                    .unwrap_or(normalized.root.as_js())
                    .to_owned(),
            ),
            components,
            case_sensitive,
        }))
    }

    /// Return whether `absolute_path` is selected by this compiled pattern.
    ///
    /// Unsupported or relative candidate paths simply do not match. The path
    /// component DP uses linear scratch space; `**` only has recursive meaning
    /// when it is an entire pattern component.
    pub fn matches<'s>(&self, absolute_path: impl Into<JsStr<'s>>) -> bool {
        MatchInput::new(absolute_path.into(), self.case_sensitive)
            .is_some_and(|input| self.matches_input(&input))
    }

    /// [`Self::matches`] over a candidate prepared once for every pattern of
    /// a walk (all compiled for the same case sensitivity).
    pub(crate) fn matches_input(&self, input: &MatchInput) -> bool {
        if !self
            .root
            .matches(&input.root, self.components.is_empty(), self.case_sensitive)
        {
            return false;
        }

        let inputs = &input.components;
        let input_count = inputs.len();
        // The leading wildcard-free components must match the input's
        // leading components one to one; most of a walk's patterns part
        // from a candidate here, before the component DP below.
        for (index, component) in self.components.iter().enumerate() {
            let PatternComponent::Glob(glob) = component else {
                break;
            };
            if glob.has_wildcard {
                break;
            }
            let Some(input) = inputs.get(index) else {
                return false;
            };
            if !glob.matches(input, index + 1 == input_count, self.case_sensitive) {
                return false;
            }
        }
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

    /// Return whether a directory can contain a path selected by this
    /// pattern.
    ///
    /// `matchFiles` uses a separate directory regexp to avoid descending into
    /// directories which cannot contribute a matching file.  Keeping the
    /// equivalent as a small NFA over compiled components means the host can
    /// retain that pruning without making the file matcher recursive or
    /// allocating a regex for every directory.  A recursive component may
    /// consume a directory only when the same implicit-directory rules used by
    /// [`Self::matches`] allow it; an explicit `node_modules` component thus
    /// remains selectable while `**/*` still skips it.
    #[cfg(test)]
    pub(crate) fn could_match_descendant<'s>(
        &self,
        absolute_directory: impl Into<JsStr<'s>>,
    ) -> bool {
        MatchInput::new(absolute_directory.into(), self.case_sensitive)
            .is_some_and(|input| self.could_match_descendant_input(&input))
    }

    /// [`Self::could_match_descendant`] over a prepared candidate.
    #[cfg(test)]
    pub(crate) fn could_match_descendant_input(&self, input: &MatchInput) -> bool {
        self.directory_states(input).is_some()
    }

    /// The states of this pattern's component automaton after the
    /// components of the directory `input`, or `None` when no descendant
    /// of that directory can match (the directory itself included: an end
    /// state is only a directory match, and the constructor never leaves a
    /// bare trailing `**`). A walk keeps these per directory and steps them
    /// by one entry name with [`Self::advance_directory`] and
    /// [`Self::accepts_entry`] instead of matching each entry's whole path
    /// again; both give exactly [`Self::matches_input`]'s answers.
    pub(crate) fn directory_states(&self, input: &MatchInput) -> Option<Vec<usize>> {
        if !self
            .root
            .matches(&input.root, self.components.is_empty(), self.case_sensitive)
        {
            return None;
        }
        let mut states = vec![0usize];
        for component in &input.components {
            states = self.advance_directory_states(&states, component, false);
            if states.is_empty() {
                return None;
            }
        }
        self.live_states(states)
    }

    /// The states below the child directory named `name` of a directory
    /// with `states`, or `None` when nothing below it can match.
    pub(crate) fn advance_directory(
        &self,
        states: &[usize],
        name: &InputComponent,
    ) -> Option<Vec<usize>> {
        self.live_states(self.advance_directory_states(states, name, false))
    }

    /// Whether the entry named `name` of a directory with `states` matches
    /// this pattern as a whole path (the entry is the path's last
    /// component).
    pub(crate) fn accepts_entry(&self, states: &[usize], name: &InputComponent) -> bool {
        self.advance_directory_states(states, name, true)
            .contains(&self.components.len())
    }

    fn live_states(&self, states: Vec<usize>) -> Option<Vec<usize>> {
        // A state before the end has at least one remaining pattern component
        // which can be supplied by a descendant path.
        states
            .iter()
            .any(|&state| state < self.components.len())
            .then_some(states)
    }

    /// One step of the component automaton: the states after consuming
    /// `input`, with `is_last_path_component` naming the path's last
    /// component (the min.js rule of [`GlobComponent::matches`]).
    fn advance_directory_states(
        &self,
        states: &[usize],
        input: &InputComponent,
        is_last_path_component: bool,
    ) -> Vec<usize> {
        let mut closure = states.to_vec();
        let mut index = 0;
        while index < closure.len() {
            let state = closure[index];
            if matches!(
                self.components.get(state),
                Some(PatternComponent::Recursive)
            ) && !closure.contains(&(state + 1))
            {
                closure.push(state + 1);
            }
            index += 1;
        }

        let mut next = Vec::new();
        for state in closure {
            match self.components.get(state) {
                Some(PatternComponent::Recursive) => {
                    if input.recursive_wildcard_allowed() {
                        next.push(state);
                    }
                }
                Some(PatternComponent::Glob(glob)) => {
                    if glob.matches(input, is_last_path_component, self.case_sensitive) {
                        next.push(state + 1);
                    }
                }
                None => {}
            }
        }
        next.sort_unstable();
        next.dedup();
        next
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternComponent {
    Recursive,
    Glob(GlobComponent),
}

impl PatternComponent {
    fn compile(text: JsString) -> Self {
        if text == "**" {
            return Self::Recursive;
        }
        Self::Glob(GlobComponent::compile(text))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GlobComponent {
    tokens: Vec<GlobToken>,
    has_wildcard: bool,
}

impl GlobComponent {
    fn compile(text: JsString) -> Self {
        let mut has_wildcard = false;
        let tokens = text
            .code_units()
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
        Self {
            tokens,
            has_wildcard,
        }
    }

    fn matches(
        &self,
        input: &InputComponent,
        is_last_path_component: bool,
        case_sensitive: bool,
    ) -> bool {
        if self.has_wildcard && input.common_package_folder {
            return false;
        }
        let input_count = input.characters.len();
        // A wildcard-free component matches exactly its own text; the
        // component DP below is only for `*` and `?`.
        if !self.has_wildcard {
            return self.tokens.len() == input_count
                && self.tokens.iter().zip(&input.characters).all(
                    |(token, &character)| match token {
                        GlobToken::Literal(literal) => {
                            regex_code_unit_eq(*literal, character, case_sensitive)
                        }
                        GlobToken::Star | GlobToken::Question => false,
                    },
                );
        }

        let min_js_dot = is_last_path_component
            .then(|| min_js_dot_index(&input.characters, case_sensitive))
            .flatten();
        // The DP rows live on the stack for every ordinary component name.
        const INLINE_ROW: usize = 128;
        let mut inline_rows = [[false; INLINE_ROW]; 2];
        let mut heap_rows = Vec::new();
        let (mut previous, mut current): (&mut [bool], &mut [bool]) = if input_count < INLINE_ROW {
            let [first, second] = &mut inline_rows;
            (&mut first[..=input_count], &mut second[..=input_count])
        } else {
            heap_rows.resize(2 * (input_count + 1), false);
            heap_rows.split_at_mut(input_count + 1)
        };
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
                            && character != u16::from(b'/')
                            && !(token_index == 0 && character == u16::from(b'.'));
                    }
                }
                GlobToken::Star => {
                    current[0] = previous[0];
                    for input_index in 1..=input_count {
                        let character_index = input_index - 1;
                        let may_consume = input.characters[character_index] != u16::from(b'/')
                            && !(token_index == 0
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

/// One path component prepared for the matchers (see [`MatchInput`]).
pub(crate) struct InputComponent {
    text: JsString,
    characters: Vec<u16>,
    common_package_folder: bool,
}

impl InputComponent {
    pub(crate) fn new(text: JsStr<'_>, case_sensitive: bool) -> Self {
        Self {
            text: text.to_owned(),
            characters: text.code_units().collect(),
            common_package_folder: COMMON_PACKAGE_FOLDERS
                .iter()
                .any(|folder| regex_text_eq(text, (*folder).into(), case_sensitive)),
        }
    }

    fn recursive_wildcard_allowed(&self) -> bool {
        !self.text.as_js().starts_with(".") && !self.common_package_folder
    }
}

/// A candidate path prepared once for every compiled pattern of one walk:
/// normalized and split into components, each carrying the facts the
/// matchers need. A path that is not absolute matches nothing.
pub(crate) struct MatchInput {
    root: InputComponent,
    components: Vec<InputComponent>,
}

impl MatchInput {
    pub(crate) fn new(absolute_path: JsStr<'_>, case_sensitive: bool) -> Option<Self> {
        let path = normalize_absolute(absolute_path).ok()?;
        let root = path
            .root
            .as_js()
            .strip_suffix("/")
            .unwrap_or(path.root.as_js());
        Some(Self {
            root: InputComponent::new(root, case_sensitive),
            components: path
                .components
                .iter()
                .map(|text| InputComponent::new(text.as_js(), case_sensitive))
                .collect(),
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct NormalizedPath {
    root: JsString,
    components: Vec<JsString>,
}

fn normalize_spec(spec: JsStr<'_>, base: JsStr<'_>) -> Result<NormalizedPath, String> {
    let slashed_spec = normalize_slashes(spec);
    if split_root(slashed_spec.as_js())?.is_some() {
        return normalize_absolute_slashed(slashed_spec.as_js());
    }

    let mut normalized = normalize_absolute(base)
        .map_err(|detail| format!("invalid config pattern base {base:?}: {detail}"))?;
    // combinePaths inserts a separator before every non-empty relative spec.
    // That separator is part of root component zero for separator-less roots
    // such as `https://host`, `//server`, and `c:`.
    if !normalized.root.ends_with("/") {
        normalized.root.push('/');
    }
    reduce_components(&mut normalized.components, slashed_spec.as_js());
    Ok(normalized)
}

fn normalize_absolute(path: JsStr<'_>) -> Result<NormalizedPath, String> {
    normalize_absolute_slashed(normalize_slashes(path).as_js())
}

fn normalize_absolute_slashed(path: JsStr<'_>) -> Result<NormalizedPath, String> {
    let Some((root, tail)) = split_root(path)? else {
        return Err(format!("path {path:?} is not absolute"));
    };
    let mut components = Vec::new();
    reduce_components(&mut components, tail);
    Ok(NormalizedPath { root, components })
}

fn split_root(path: JsStr<'_>) -> Result<Option<(JsString, JsStr<'_>)>, String> {
    Ok(root_parts(path).map(|(root, tail)| (root.to_owned(), tail)))
}

fn reduce_components(components: &mut Vec<JsString>, tail: JsStr<'_>) {
    for component in tail.split_ascii(b'/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            components.pop();
        } else {
            components.push(component.to_owned());
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

fn regex_text_eq(left: JsStr<'_>, right: JsStr<'_>, case_sensitive: bool) -> bool {
    let mut left = left.code_units();
    let mut right = right.code_units();
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
#[path = "../tests/unit/config_matcher/tests.rs"]
mod tests;
