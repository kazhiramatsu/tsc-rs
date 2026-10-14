//! tsgo `format/rule.go`: a formatting rule takes two tokens (left and
//! right) and the contexts it applies in, and declares what the white space
//! between them should be.

use std::ops::{BitAnd, BitOr, BitOrAssign, Not};

use tsc_syntax::SyntaxKind;

use super::context::FormattingContext;

/// tsgo `contextPredicate`.
pub(super) type ContextPredicate = Box<dyn Fn(&FormattingContext<'_>) -> bool + Send + Sync>;

/// tsgo `ruleImpl`.
pub(super) struct Rule {
    #[allow(dead_code)]
    pub(super) debug_name: &'static str,
    pub(super) context: Vec<ContextPredicate>,
    pub(super) action: RuleAction,
    pub(super) flags: RuleFlags,
}

/// tsgo `ruleAction`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RuleAction(pub(super) u8);

impl RuleAction {
    pub(super) const NONE: Self = Self(0);
    pub(super) const STOP_PROCESSING_SPACE_ACTIONS: Self = Self(1 << 0);
    pub(super) const STOP_PROCESSING_TOKEN_ACTIONS: Self = Self(1 << 1);
    pub(super) const INSERT_SPACE: Self = Self(1 << 2);
    pub(super) const INSERT_NEW_LINE: Self = Self(1 << 3);
    pub(super) const DELETE_SPACE: Self = Self(1 << 4);
    pub(super) const DELETE_TOKEN: Self = Self(1 << 5);
    pub(super) const INSERT_TRAILING_SEMICOLON: Self = Self(1 << 6);

    pub(super) const STOP_ACTION: Self =
        Self(Self::STOP_PROCESSING_SPACE_ACTIONS.0 | Self::STOP_PROCESSING_TOKEN_ACTIONS.0);
    pub(super) const MODIFY_SPACE_ACTION: Self =
        Self(Self::INSERT_SPACE.0 | Self::INSERT_NEW_LINE.0 | Self::DELETE_SPACE.0);
    pub(super) const MODIFY_TOKEN_ACTION: Self =
        Self(Self::DELETE_TOKEN.0 | Self::INSERT_TRAILING_SEMICOLON.0);

    pub(super) fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for RuleAction {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for RuleAction {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl BitAnd for RuleAction {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl Not for RuleAction {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// tsgo `ruleFlags`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuleFlags {
    None,
    CanDeleteNewLines,
}

/// tsgo `tokenRange`.
#[derive(Clone)]
pub(super) struct TokenRange {
    pub(super) tokens: Vec<SyntaxKind>,
    pub(super) is_specific: bool,
}

impl From<SyntaxKind> for TokenRange {
    fn from(kind: SyntaxKind) -> Self {
        Self {
            tokens: vec![kind],
            is_specific: true,
        }
    }
}

impl From<&[SyntaxKind]> for TokenRange {
    fn from(kinds: &[SyntaxKind]) -> Self {
        Self {
            tokens: kinds.to_vec(),
            is_specific: true,
        }
    }
}

impl<const N: usize> From<[SyntaxKind; N]> for TokenRange {
    fn from(kinds: [SyntaxKind; N]) -> Self {
        Self {
            tokens: kinds.to_vec(),
            is_specific: true,
        }
    }
}

impl From<&TokenRange> for TokenRange {
    fn from(range: &TokenRange) -> Self {
        range.clone()
    }
}

/// tsgo `ruleSpec`.
pub(super) struct RuleSpec {
    pub(super) left_token_range: TokenRange,
    pub(super) right_token_range: TokenRange,
    pub(super) rule: Rule,
}

/// tsgo `rule`: a rule between the tokens of `left` and of `right`.
pub(super) fn rule(
    debug_name: &'static str,
    left: impl Into<TokenRange>,
    right: impl Into<TokenRange>,
    context: Vec<ContextPredicate>,
    action: RuleAction,
) -> RuleSpec {
    rule_with_flags(debug_name, left, right, context, action, RuleFlags::None)
}

/// tsgo `rule` with flags.
pub(super) fn rule_with_flags(
    debug_name: &'static str,
    left: impl Into<TokenRange>,
    right: impl Into<TokenRange>,
    context: Vec<ContextPredicate>,
    action: RuleAction,
    flags: RuleFlags,
) -> RuleSpec {
    RuleSpec {
        left_token_range: left.into(),
        right_token_range: right.into(),
        rule: Rule {
            debug_name,
            context,
            action,
            flags,
        },
    }
}

/// A context predicate of a function.
pub(super) fn predicate(
    predicate: impl Fn(&FormattingContext<'_>) -> bool + Send + Sync + 'static,
) -> ContextPredicate {
    Box::new(predicate)
}
