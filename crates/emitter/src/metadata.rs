use std::collections::BTreeMap;

use tsc_syntax::SyntaxKind;

use crate::{SourceRange, TransformNode, TransformSourceId};

/// Emitter-only node flags. They live in a sparse session table and never
/// enlarge persistent parsed nodes.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EmitFlags(u32);

impl EmitFlags {
    pub const NONE: Self = Self(0);
    pub const SINGLE_LINE: Self = Self(1);
    pub const MULTI_LINE: Self = Self(2);
    pub const ADVISE_ON_EMIT_NODE: Self = Self(4);
    pub const NO_SUBSTITUTION: Self = Self(8);
    pub const CAPTURES_THIS: Self = Self(16);
    pub const NO_LEADING_SOURCE_MAP: Self = Self(32);
    pub const NO_TRAILING_SOURCE_MAP: Self = Self(64);
    pub const NO_SOURCE_MAP: Self = Self(96);
    pub const NO_NESTED_SOURCE_MAPS: Self = Self(128);
    pub const NO_TOKEN_LEADING_SOURCE_MAPS: Self = Self(256);
    pub const NO_TOKEN_TRAILING_SOURCE_MAPS: Self = Self(512);
    pub const NO_TOKEN_SOURCE_MAPS: Self = Self(768);
    pub const NO_LEADING_COMMENTS: Self = Self(1024);
    pub const NO_TRAILING_COMMENTS: Self = Self(2048);
    pub const NO_COMMENTS: Self = Self(3072);
    pub const NO_NESTED_COMMENTS: Self = Self(4096);
    pub const HELPER_NAME: Self = Self(8192);
    pub const EXPORT_NAME: Self = Self(16384);
    pub const LOCAL_NAME: Self = Self(32768);
    pub const INTERNAL_NAME: Self = Self(65536);
    pub const INDENTED: Self = Self(131072);
    pub const NO_INDENTATION: Self = Self(262144);
    pub const ASYNC_FUNCTION_BODY: Self = Self(524288);
    pub const REUSE_TEMP_VARIABLE_SCOPE: Self = Self(1048576);
    pub const CUSTOM_PROLOGUE: Self = Self(2097152);
    pub const NO_HOISTING: Self = Self(4194304);
    pub const ITERATOR: Self = Self(8388608);
    pub const NO_ASCII_ESCAPING: Self = Self(16777216);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl std::ops::BitOr for EmitFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for EmitFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Internal emit-node bits required by the transform/printer seam.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct InternalEmitFlags(u32);

impl InternalEmitFlags {
    pub const NONE: Self = Self(0);
    pub const TYPE_SCRIPT_CLASS_WRAPPER: Self = Self(1);
    pub const NEVER_APPLY_IMPORT_HELPER: Self = Self(2);
    pub const IGNORE_SOURCE_NEWLINES: Self = Self(4);
    pub const IMMUTABLE: Self = Self(8);
    pub const TRANSFORM_PRIVATE_STATIC_ELEMENTS: Self = Self(32);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Source identity is carried with a map range so a source switch cannot be
/// represented as an untyped position.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceMapRange {
    source: TransformSourceId,
    range: SourceRange,
}

impl SourceMapRange {
    pub const fn new(source: TransformSourceId, range: SourceRange) -> Self {
        Self { source, range }
    }

    pub const fn source(self) -> TransformSourceId {
        self.source
    }

    pub const fn range(self) -> SourceRange {
        self.range
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SyntheticCommentKind {
    SingleLine,
    MultiLine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticComment {
    kind: SyntheticCommentKind,
    text: Box<str>,
    has_leading_new_line: bool,
    has_trailing_new_line: bool,
}

impl SyntheticComment {
    pub fn new(
        kind: SyntheticCommentKind,
        text: impl Into<Box<str>>,
        has_leading_new_line: bool,
        has_trailing_new_line: bool,
    ) -> Self {
        Self {
            kind,
            text: text.into(),
            has_leading_new_line,
            has_trailing_new_line,
        }
    }

    pub const fn kind(&self) -> SyntheticCommentKind {
        self.kind
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn has_leading_new_line(&self) -> bool {
        self.has_leading_new_line
    }

    pub const fn has_trailing_new_line(&self) -> bool {
        self.has_trailing_new_line
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavaScriptString(Box<[u16]>);

impl JavaScriptString {
    pub fn from_code_units(value: impl Into<Box<[u16]>>) -> Self {
        Self(value.into())
    }

    pub fn from_rust_str(value: &str) -> Self {
        Self(value.encode_utf16().collect::<Vec<_>>().into_boxed_slice())
    }

    pub fn code_units(&self) -> &[u16] {
        &self.0
    }
}

/// Bit-exact JavaScript number carried across the checker/emitter seam.
/// Keeping the IEEE-754 representation preserves `-0` and avoids imposing
/// Rust's non-`Eq` floating-point semantics on session metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JavaScriptNumber(u64);

impl JavaScriptNumber {
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub fn from_f64(value: f64) -> Self {
        Self(value.to_bits())
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// Constant values consumed by printer folds without narrowing JavaScript
/// strings to Unicode scalar values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitConstantValue {
    String(JavaScriptString),
    Number(JavaScriptNumber),
    Boolean(bool),
}

/// Full `getEnumMemberValue` observation. A member whose value is not
/// statically known can still be syntactically string-valued, which controls
/// whether the runtime transform emits a numeric reverse mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitEnumMemberValue {
    value: Option<EmitConstantValue>,
    is_syntactically_string: bool,
}

impl EmitEnumMemberValue {
    pub const fn new(value: Option<EmitConstantValue>, is_syntactically_string: bool) -> Self {
        Self {
            value,
            is_syntactically_string,
        }
    }

    pub const fn value(&self) -> Option<&EmitConstantValue> {
        self.value.as_ref()
    }

    pub const fn is_syntactically_string(&self) -> bool {
        self.is_syntactically_string
    }
}

/// Session-owned `emitNode` equivalent. Parsed nodes remain unchanged.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitMetadata {
    pub(crate) original: Option<TransformNode>,
    pub(crate) flags: EmitFlags,
    pub(crate) internal_flags: InternalEmitFlags,
    pub(crate) leading_comments: Vec<SyntheticComment>,
    pub(crate) trailing_comments: Vec<SyntheticComment>,
    pub(crate) comment_range: Option<SourceMapRange>,
    pub(crate) source_map_range: Option<SourceMapRange>,
    pub(crate) token_source_map_ranges: BTreeMap<SyntaxKind, SourceMapRange>,
    pub(crate) constant_value: Option<EmitConstantValue>,
    pub(crate) helpers: Vec<Box<str>>,
    pub(crate) starts_on_new_line: Option<bool>,
    pub(crate) snippet_element: Option<Box<str>>,
    pub(crate) class_this: Option<TransformNode>,
    pub(crate) assigned_name: Option<TransformNode>,
    /// Lossless cooked text for parsed or synthetic nodes whose JavaScript
    /// value may contain an unpaired UTF-16 surrogate. Original raw source
    /// remains authoritative whenever the printer can copy it unchanged.
    pub(crate) javascript_string_value: Option<JavaScriptString>,
    /// TypeScript's synthetic `StringLiteral.singleQuote` preference. JSX
    /// attribute lowering preserves the source delimiter after decoding
    /// entities, so the cooked value and quote choice must travel together.
    pub(crate) string_literal_single_quote: Option<bool>,
    /// Parsed import declaration selected for the root of a synthesized
    /// classic JSX factory expression. This is the Rust ownership analogue
    /// of TypeScript parenting that expression at the JSX parse tree node.
    pub(crate) referenced_import_declaration: Option<TransformNode>,
}

impl EmitMetadata {
    pub const fn original(&self) -> Option<TransformNode> {
        self.original
    }

    pub const fn flags(&self) -> EmitFlags {
        self.flags
    }

    pub const fn internal_flags(&self) -> InternalEmitFlags {
        self.internal_flags
    }

    pub fn leading_comments(&self) -> &[SyntheticComment] {
        &self.leading_comments
    }

    pub fn trailing_comments(&self) -> &[SyntheticComment] {
        &self.trailing_comments
    }

    pub const fn comment_range(&self) -> Option<SourceMapRange> {
        self.comment_range
    }

    pub const fn source_map_range(&self) -> Option<SourceMapRange> {
        self.source_map_range
    }

    pub fn token_source_map_ranges(&self) -> &BTreeMap<SyntaxKind, SourceMapRange> {
        &self.token_source_map_ranges
    }

    pub const fn starts_on_new_line(&self) -> Option<bool> {
        self.starts_on_new_line
    }

    pub fn javascript_string_value(&self) -> Option<&JavaScriptString> {
        self.javascript_string_value.as_ref()
    }

    pub const fn string_literal_single_quote(&self) -> Option<bool> {
        self.string_literal_single_quote
    }

    pub const fn referenced_import_declaration(&self) -> Option<TransformNode> {
        self.referenced_import_declaration
    }

    pub fn set_flags(&mut self, flags: EmitFlags) {
        self.flags = flags;
    }

    pub fn add_flags(&mut self, flags: EmitFlags) {
        self.flags |= flags;
    }

    pub fn set_internal_flags(&mut self, flags: InternalEmitFlags) {
        self.internal_flags = flags;
    }

    pub fn set_source_map_range(&mut self, range: SourceMapRange) {
        self.source_map_range = Some(range);
    }

    pub fn set_token_source_map_range(&mut self, token: SyntaxKind, range: SourceMapRange) {
        self.token_source_map_ranges.insert(token, range);
    }

    pub fn set_comment_range(&mut self, range: SourceMapRange) {
        self.comment_range = Some(range);
    }

    pub fn add_leading_comment(&mut self, comment: SyntheticComment) {
        self.leading_comments.push(comment);
    }

    pub fn add_trailing_comment(&mut self, comment: SyntheticComment) {
        self.trailing_comments.push(comment);
    }

    pub fn set_starts_on_new_line(&mut self, value: bool) {
        self.starts_on_new_line = Some(value);
    }

    pub fn set_javascript_string_value(&mut self, value: JavaScriptString) {
        self.javascript_string_value = Some(value);
    }

    pub fn set_string_literal_single_quote(&mut self, value: bool) {
        self.string_literal_single_quote = Some(value);
    }

    pub fn set_referenced_import_declaration(&mut self, value: TransformNode) {
        self.referenced_import_declaration = Some(value);
    }

    /// tsc-port: mergeEmitNode @6.0.3
    /// tsc-hash: 6d9f4af1f1fa79b494c5ef7b570972925000f7939cd16ffe520855a67583f375
    /// tsc-span: _tsc.js:25218-25277
    pub(crate) fn merge_from(&mut self, source: &Self) {
        if !source.flags.is_empty() {
            self.flags = source.flags;
        }
        if source.internal_flags.bits() != 0 {
            self.internal_flags = InternalEmitFlags::from_bits(
                source.internal_flags.bits() & !InternalEmitFlags::IMMUTABLE.bits(),
            );
        }
        if !source.leading_comments.is_empty() {
            let mut comments = source.leading_comments.clone();
            comments.append(&mut self.leading_comments);
            self.leading_comments = comments;
        }
        if !source.trailing_comments.is_empty() {
            let mut comments = source.trailing_comments.clone();
            comments.append(&mut self.trailing_comments);
            self.trailing_comments = comments;
        }
        if source.comment_range.is_some() {
            self.comment_range = source.comment_range;
        }
        if source.source_map_range.is_some() {
            self.source_map_range = source.source_map_range;
        }
        for (token, range) in &source.token_source_map_ranges {
            self.token_source_map_ranges.insert(*token, *range);
        }
        if source.constant_value.is_some() {
            self.constant_value = source.constant_value.clone();
        }
        for helper in &source.helpers {
            if !self.helpers.contains(helper) {
                self.helpers.push(helper.clone());
            }
        }
        if source.starts_on_new_line.is_some() {
            self.starts_on_new_line = source.starts_on_new_line;
        }
        if source.snippet_element.is_some() {
            self.snippet_element = source.snippet_element.clone();
        }
        if source.class_this.is_some() {
            self.class_this = source.class_this;
        }
        if source.assigned_name.is_some() {
            self.assigned_name = source.assigned_name;
        }
        if source.javascript_string_value.is_some() {
            self.javascript_string_value = source.javascript_string_value.clone();
        }
        if source.string_literal_single_quote.is_some() {
            self.string_literal_single_quote = source.string_literal_single_quote;
        }
        if source.referenced_import_declaration.is_some() {
            self.referenced_import_declaration = source.referenced_import_declaration;
        }
    }
}
