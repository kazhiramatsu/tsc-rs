//! Project IDs (tsgo `project.ID`, project.go:26-107): the inferred
//! project's `/dev/null/inferred`, a synthetic program's
//! `/dev/null/synthetic/<n>` and a configured project's config path.

use std::fmt;

const INFERRED_PROJECT_NAME: &str = "/dev/null/inferred";
const SYNTHETIC_PROJECT_PREFIX: &str = "/dev/null/synthetic/";

/// A project's kind (tsgo `project.Kind`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectKind {
    Inferred,
    Configured,
    Synthetic,
}

/// A project's ID (tsgo `project.ID`). IDs order as their strings, as tsgo
/// sorts them.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectId(String);

impl ProjectId {
    /// An ID as a client spells it, whatever it names.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The inferred project's ID.
    pub fn inferred() -> Self {
        Self(INFERRED_PROJECT_NAME.to_owned())
    }

    /// tsgo `NewSyntheticProjectID`.
    ///
    /// # Panics
    ///
    /// When `number` is 0, as tsgo panics on a nonpositive number.
    pub fn synthetic(number: u64) -> Self {
        assert!(number > 0, "invalid synthetic project ID: {number}");
        Self(format!("{SYNTHETIC_PROJECT_PREFIX}{number}"))
    }

    /// tsgo `ParseConfiguredProjectID`: a nonempty path that names neither
    /// the inferred project nor a synthetic program.
    pub fn configured(config_file_path: &str) -> Option<Self> {
        let id = Self::new(config_file_path);
        (id.kind() == Some(ProjectKind::Configured)).then_some(id)
    }

    /// tsgo `ParseSyntheticProjectID`: `/dev/null/synthetic/<n>` with a
    /// positive `n` (`strconv.Atoi`), canonicalized (`…/01` is `…/1`).
    pub fn parse_synthetic(value: &str) -> Option<Self> {
        let number = go_atoi(value.strip_prefix(SYNTHETIC_PROJECT_PREFIX)?)?;
        u64::try_from(number)
            .ok()
            .filter(|number| *number > 0)
            .map(Self::synthetic)
    }

    /// What the ID names (tsgo `ID.Inferred`, `ID.Synthetic`,
    /// `ID.Configured`); nothing for the empty ID.
    pub fn kind(&self) -> Option<ProjectKind> {
        if self.0.is_empty() {
            None
        } else if self.0 == INFERRED_PROJECT_NAME {
            Some(ProjectKind::Inferred)
        } else if Self::parse_synthetic(&self.0).is_some() {
            Some(ProjectKind::Synthetic)
        } else {
            Some(ProjectKind::Configured)
        }
    }

    /// The ID a project is stored under: a synthetic ID canonicalized,
    /// others as they are.
    pub(crate) fn canonical(&self) -> Self {
        Self::parse_synthetic(&self.0).unwrap_or_else(|| self.clone())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Go's `strconv.Atoi`: an optional sign and decimal digits that fit.
fn go_atoi(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'+') => (false, &text[1..]),
        Some(b'-') => (true, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<i64>().ok()?;
    Some(if negative { -value } else { value })
}
