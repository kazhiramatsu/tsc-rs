use std::collections::BTreeMap;

use tsc_syntax::SyntaxKind;

use crate::{
    transform::GeneratedBindingId, SourceRange, TransformNode, TransformNodeArray,
    TransformSourceId,
};

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
    /// The computed property name already carries the cache selected by an
    /// earlier transformer (the local equivalent of tsc's generated-name link).
    pub const GENERATED_COMPUTED_PROPERTY_NAME: Self = Self(64);
    /// A clone of a declaration name is being used as an expression reference.
    ///
    /// TypeScript's mutable transform tree reparents the clone beneath that
    /// expression. Our immutable parse-tree provenance still points at the
    /// declaration, so module substitution needs this typed ownership bit to
    /// distinguish `getDeclarationName(node)` in an IIFE argument from the
    /// same spelling emitted as a binding declaration.
    pub const DECLARATION_NAME_REFERENCE: Self = Self(128);

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

/// Source range used exclusively to decide ownership of parsed comments.
///
/// A transformed node may share semantic provenance with an original node
/// while intentionally owning no source comments, and its source-map range
/// may point somewhere else again. A distinct type keeps those three emitter
/// relationships from being coupled accidentally.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommentRange {
    source: TransformSourceId,
    range: SourceRange,
}

/// Original statement-list provenance retained by a synthetic block after
/// its statements have been relocated into a module wrapper.
///
/// The outer SourceFile list keeps the parsed range and therefore owns its
/// detached prefix. The synthetic inner list still needs the same boundary as
/// a comment-resume seed so its first original statement cannot claim that
/// prefix a second time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RelocatedStatementListComments {
    original: TransformNodeArray,
}

impl RelocatedStatementListComments {
    pub(crate) const fn owned_by_source_file(original: TransformNodeArray) -> Self {
        Self { original }
    }

    pub(crate) const fn original(self) -> TransformNodeArray {
        self.original
    }
}

impl CommentRange {
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

/// Declaration ownership carried by a class expression synthesized by an
/// earlier transform pass.
///
/// A decorated class declaration is represented as a variable initialized by
/// a class expression before class-field lowering runs.  The class expression
/// is still statement-expandable: static initializers belong after that
/// variable statement, rather than in an ordinary expression-local comma
/// sequence.  Keeping that fact as typed emit metadata avoids inferring pass
/// ownership from a mutable parent chain or from printable names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClassExpressionDeclarationOrigin {
    LegacyDecorated { declaration: TransformNode },
}

/// A source expression whose same-line trailing trivia moved to a generated
/// class-field operation. The operation (statement for declarations, comma
/// expression for class expressions) is the sole owner of that boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelocatedTrailingCommentOwner {
    ClassFieldOperation,
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
    pub(crate) comment_range: Option<CommentRange>,
    /// Statement-list prefix already emitted by the outer SourceFile after a
    /// transform relocated the original statements into this synthetic body.
    pub(crate) relocated_statement_list_comments: Option<RelocatedStatementListComments>,
    pub(crate) source_map_range: Option<SourceMapRange>,
    pub(crate) token_source_map_ranges: BTreeMap<SyntaxKind, SourceMapRange>,
    pub(crate) constant_value: Option<EmitConstantValue>,
    pub(crate) helpers: Vec<Box<str>>,
    pub(crate) starts_on_new_line: Option<bool>,
    pub(crate) snippet_element: Option<Box<str>>,
    pub(crate) class_this: Option<TransformNode>,
    pub(crate) assigned_name: Option<TransformNode>,
    pub(crate) class_expression_declaration_origin: Option<ClassExpressionDeclarationOrigin>,
    pub(crate) relocated_trailing_comment_owner: Option<RelocatedTrailingCommentOwner>,
    /// Erased TypeScript type annotation whose trailing source boundary still
    /// belongs to this declaration name. The JavaScript printer uses it to
    /// retain comments on either side of the removed annotation without
    /// retaining type syntax in the transformed tree.
    pub(crate) type_node: Option<TransformNode>,
    /// Owning class for a synthesized identifier that semantically denotes
    /// that class constructor. Later class lowering can resolve the reference
    /// without guessing from its printable text.
    pub(crate) class_constructor_reference: Option<TransformNode>,
    /// Source node whose leading trivia is intentionally re-emitted by a
    /// lowered class-field initializer. Decorated static auto-accessors own
    /// this in addition to the generated getter's normal comment range.
    pub(crate) class_field_initializer_comment_source: Option<TransformNode>,
    /// Lossless cooked text for parsed or synthetic nodes whose JavaScript
    /// value may contain an unpaired UTF-16 surrogate. Original raw source
    /// remains authoritative whenever the printer can copy it unchanged.
    pub(crate) javascript_string_value: Option<JavaScriptString>,
    /// TypeScript's synthetic `StringLiteral.singleQuote` preference. JSX
    /// attribute lowering preserves the source delimiter after decoding
    /// entities, so the cooked value and quote choice must travel together.
    pub(crate) string_literal_single_quote: Option<bool>,
    /// Parsed string literal whose token spelling supplies a synthetic
    /// string's emitted text. This is the source-string branch of tsc's
    /// `StringLiteral.textSourceNode`; unlike `original`, it carries only
    /// lexical spelling ownership and grants neither comments nor resolver
    /// identity to the synthesized literal.
    pub(crate) string_literal_text_source: Option<TransformNode>,
    /// Import declaration selected for a synthesized reference. This includes
    /// both classic JSX factory expressions and automatic-runtime helpers.
    /// The declaration identity, rather than the printable local spelling,
    /// drives later module substitution.
    pub(crate) referenced_import_declaration: Option<TransformNode>,
    /// Namespace or enum declaration selected for a synthesized lexical
    /// reference. Upstream gives classic-JSX factory roots a parse-tree
    /// parent and resolves them during TypeScript substitution; this typed
    /// identity carries the same ownership without a mutable parent chain.
    pub(crate) referenced_export_container: Option<TransformNode>,
    /// Whether the reference is TypeScript's generated-import identifier
    /// (`setIdentifierGeneratedImportReference`). CommonJS explicitly permits
    /// substitution for this generated-name class, while SystemJS leaves it
    /// as the generated local binding.
    pub(crate) generated_import_reference: bool,
    /// Stable identity shared by every synthesized identifier that denotes one
    /// generated lexical binding. The printable spelling is finalized after
    /// target-pass composition has fixed declaration order.
    pub(crate) generated_binding_id: Option<GeneratedBindingId>,
    /// Source-derived base for generated names such as `env_1` or `e_2`.
    /// Absence denotes the target ladder's ordinal temporary class (`_a`,
    /// `_b`, ...).
    pub(crate) generated_binding_base: Option<Box<str>>,
    /// Function-scoped optimistic base for generated names such as `_super`.
    /// Preferred names differ from source-derived numbered names because
    /// sibling functions may reuse them.
    pub(crate) generated_binding_preferred_base: Option<Box<str>>,
    /// Semantic suffix of a source-derived optimistic generated name. The
    /// collision ordinal precedes this suffix (`name_1_get`), so it cannot be
    /// folded into `generated_binding_preferred_base` without losing order.
    pub(crate) generated_binding_role_suffix: Option<Box<str>>,
    /// Whether this preferred binding uses TypeScript's file-level optimistic
    /// collision domain. Such a name is checked only against parsed source and
    /// global identifiers, so distinct generated binding identities may share
    /// its printable spelling.
    pub(crate) generated_binding_file_level_optimistic: bool,
    /// Whether an ordinary target-generated temp carries a semantic planned
    /// spelling that must survive final output-tree name reconciliation when
    /// collision-free. The default ordinary-temp policy allocates from the
    /// final lexical scope's traversal-order cursor instead.
    pub(crate) generated_binding_planned_name_authoritative: bool,
    /// Whether this generated binding's printable name must remain reserved
    /// in descendant function scopes. Async-generator forwarding parameters
    /// use this to keep the outer alias distinct from the inner generator's
    /// parameter aliases.
    pub(crate) generated_binding_reserved_in_nested_scopes: bool,
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

    pub const fn comment_range(&self) -> Option<CommentRange> {
        self.comment_range
    }

    pub(crate) const fn relocated_statement_list_comments(
        &self,
    ) -> Option<RelocatedStatementListComments> {
        self.relocated_statement_list_comments
    }

    pub const fn source_map_range(&self) -> Option<SourceMapRange> {
        self.source_map_range
    }

    pub fn token_source_map_ranges(&self) -> &BTreeMap<SyntaxKind, SourceMapRange> {
        &self.token_source_map_ranges
    }

    pub const fn constant_value(&self) -> Option<&EmitConstantValue> {
        self.constant_value.as_ref()
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

    pub(crate) const fn string_literal_text_source(&self) -> Option<TransformNode> {
        self.string_literal_text_source
    }

    pub const fn type_node(&self) -> Option<TransformNode> {
        self.type_node
    }

    pub const fn referenced_import_declaration(&self) -> Option<TransformNode> {
        self.referenced_import_declaration
    }

    pub(crate) const fn referenced_export_container(&self) -> Option<TransformNode> {
        self.referenced_export_container
    }

    pub const fn is_generated_import_reference(&self) -> bool {
        self.generated_import_reference
    }

    pub(crate) const fn generated_binding_id(&self) -> Option<GeneratedBindingId> {
        self.generated_binding_id
    }

    pub(crate) fn generated_binding_base(&self) -> Option<&str> {
        self.generated_binding_base.as_deref()
    }

    pub(crate) fn generated_binding_preferred_base(&self) -> Option<&str> {
        self.generated_binding_preferred_base.as_deref()
    }

    pub(crate) fn generated_binding_role_suffix(&self) -> Option<&str> {
        self.generated_binding_role_suffix.as_deref()
    }

    pub(crate) const fn generated_binding_is_file_level_optimistic(&self) -> bool {
        self.generated_binding_file_level_optimistic
    }

    pub(crate) const fn generated_binding_planned_name_is_authoritative(&self) -> bool {
        self.generated_binding_planned_name_authoritative
    }

    pub(crate) const fn generated_binding_reserved_in_nested_scopes(&self) -> bool {
        self.generated_binding_reserved_in_nested_scopes
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

    pub fn set_comment_range(&mut self, range: CommentRange) {
        self.comment_range = Some(range);
    }

    pub(crate) fn set_relocated_statement_list_comments(
        &mut self,
        comments: RelocatedStatementListComments,
    ) {
        self.relocated_statement_list_comments = Some(comments);
    }

    pub fn set_constant_value(&mut self, value: EmitConstantValue) {
        self.constant_value = Some(value);
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

    pub fn set_type_node(&mut self, value: TransformNode) {
        self.type_node = Some(value);
    }

    pub fn set_javascript_string_value(&mut self, value: JavaScriptString) {
        self.javascript_string_value = Some(value);
    }

    pub fn set_string_literal_single_quote(&mut self, value: bool) {
        self.string_literal_single_quote = Some(value);
    }

    pub fn set_string_literal_text_source(&mut self, value: TransformNode) {
        self.string_literal_text_source = Some(value);
    }

    pub fn set_referenced_import_declaration(&mut self, value: TransformNode) {
        self.referenced_import_declaration = Some(value);
        self.generated_import_reference = false;
    }

    pub(crate) fn set_referenced_export_container(&mut self, value: TransformNode) {
        self.referenced_export_container = Some(value);
    }

    pub fn set_generated_import_reference(&mut self, value: TransformNode) {
        self.referenced_import_declaration = Some(value);
        self.generated_import_reference = true;
    }

    pub(crate) fn set_generated_binding_id(&mut self, value: GeneratedBindingId) {
        self.generated_binding_id = Some(value);
    }

    pub(crate) fn set_generated_binding_base(&mut self, value: &str) {
        self.generated_binding_base = Some(value.into());
    }

    pub(crate) fn set_generated_binding_preferred_base(&mut self, value: &str) {
        self.generated_binding_preferred_base = Some(value.into());
    }

    pub(crate) fn set_generated_binding_role_suffix(&mut self, value: &str) {
        self.generated_binding_role_suffix = Some(value.into());
    }

    pub(crate) fn mark_generated_binding_file_level_optimistic(&mut self) {
        self.generated_binding_file_level_optimistic = true;
    }

    pub(crate) fn mark_generated_binding_planned_name_authoritative(&mut self) {
        self.generated_binding_planned_name_authoritative = true;
    }

    pub(crate) fn reserve_generated_binding_in_nested_scopes(&mut self) {
        self.generated_binding_reserved_in_nested_scopes = true;
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
        if source.relocated_statement_list_comments.is_some() {
            self.relocated_statement_list_comments = source.relocated_statement_list_comments;
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
        if source.class_expression_declaration_origin.is_some() {
            self.class_expression_declaration_origin = source.class_expression_declaration_origin;
        }
        if source.relocated_trailing_comment_owner.is_some() {
            self.relocated_trailing_comment_owner = source.relocated_trailing_comment_owner;
        }
        if source.type_node.is_some() {
            self.type_node = source.type_node;
        }
        if source.class_constructor_reference.is_some() {
            self.class_constructor_reference = source.class_constructor_reference;
        }
        if source.class_field_initializer_comment_source.is_some() {
            self.class_field_initializer_comment_source =
                source.class_field_initializer_comment_source;
        }
        if source.generated_binding_id.is_some() {
            self.generated_binding_id = source.generated_binding_id;
        }
        if source.generated_binding_base.is_some() {
            self.generated_binding_base = source.generated_binding_base.clone();
        }
        if source.generated_binding_preferred_base.is_some() {
            self.generated_binding_preferred_base = source.generated_binding_preferred_base.clone();
        }
        if source.generated_binding_role_suffix.is_some() {
            self.generated_binding_role_suffix = source.generated_binding_role_suffix.clone();
        }
        self.generated_binding_file_level_optimistic |=
            source.generated_binding_file_level_optimistic;
        self.generated_binding_planned_name_authoritative |=
            source.generated_binding_planned_name_authoritative;
        self.generated_binding_reserved_in_nested_scopes |=
            source.generated_binding_reserved_in_nested_scopes;
        if source.javascript_string_value.is_some() {
            self.javascript_string_value = source.javascript_string_value.clone();
        }
        if source.string_literal_single_quote.is_some() {
            self.string_literal_single_quote = source.string_literal_single_quote;
        }
        if source.string_literal_text_source.is_some() {
            self.string_literal_text_source = source.string_literal_text_source;
        }
        if source.referenced_import_declaration.is_some() {
            self.referenced_import_declaration = source.referenced_import_declaration;
            self.generated_import_reference = source.generated_import_reference;
        }
        if source.referenced_export_container.is_some() {
            self.referenced_export_container = source.referenced_export_container;
        }
    }
}
