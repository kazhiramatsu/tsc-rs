use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use tsc_diagnostics::compute_line_starts;
use tsc_syntax::{
    for_each_child, is_js_whitespace, is_line_break, is_whitespace_like, skip_trivia, NodeData,
    NodeId, SyntaxKind,
};
use tsc_types::{NodeFlags, ScriptTarget, TokenFlags};

use crate::comment_cursor::{
    CommentCursor, CommentEmissionScope, CommentResume, CommentResumeError,
};
use crate::token_cursor::{
    FixedToken, TokenAnchor, TokenCommentBoundary, TokenCursor, TokenEmission, TokenLeadingSpace,
    TokenWriteKind,
};
use crate::{
    create_text_writer, CommentRange, EmitFlags, EmitHelper, EmitHint, GeneratedUtf16Location,
    NewLineKind, SourceBytePosition, SourceByteRange, SourceMapRange, SourcePositionError,
    SourceRange, SourceUtf16Location, SyntheticComment, SyntheticCommentKind, TextWriter,
    TransformBundle, TransformError, TransformNode, TransformNodeArray, TransformSourceId,
    TransformationResult, UnsupportedEmitFeature,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModifierListItemKind {
    Decorator,
    Modifier,
}

impl ModifierListItemKind {
    const fn from_syntax_kind(kind: SyntaxKind) -> Self {
        if matches!(kind, SyntaxKind::Decorator) {
            Self::Decorator
        } else {
            Self::Modifier
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModifierListItem {
    node: NodeId,
    kind: ModifierListItemKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParenthesizedNoAsiExpression {
    /// `factory.createParenthesizedExpression(node)`: the wrapper and all of
    /// its comments remain inside a pair of synthetic delimiters.
    SyntheticWhole { wrapper: TransformNode },
    /// `createParenthesizedExpression(node.expression)` followed by
    /// `setOriginalNode(parens, node)` and `setTextRange(parens, parseNode)`.
    ///
    /// tsc gives the generated container two independent owners: the partial
    /// wrapper supplies synthetic emit metadata, while the parsed
    /// ParenthesizedExpression supplies source-token/comment positions. Keep
    /// those roles explicit instead of manufacturing a temporary arena node.
    Parsed {
        metadata_owner: TransformNode,
        token_owner: TransformNode,
        inner: TransformNode,
    },
}

/// Source-comment topology of a grammar parenthesis created by NodeFactory.
/// `setTextRange(createParenthesizedExpression(node), node)` gives the new
/// container the child's source comments, so those comments surround the
/// delimiters. A plain `createParenthesizedExpression(node)` has no source
/// range and leaves the child's source comments inside the delimiters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GrammarParentheses {
    SourceRanged,
    Synthetic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredSourceCommentExtent {
    LeadingOnly,
    LeadingAndTrailing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrailingSourceCommentOwnership {
    VisitedHere { anchor: TokenAnchor },
    RetainedByParent,
    Suppressed,
    NoSourceRange,
    EmptySourceRange,
}

impl ExpressionSourceCommentsOutcome {
    const fn visited_trailing_anchor(self) -> Option<TokenAnchor> {
        match self {
            Self::Complete {
                trailing: TrailingSourceCommentOwnership::VisitedHere { anchor },
            } => Some(anchor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
enum ExpressionSourceCommentsOutcome {
    None,
    LeadingConsumed,
    Complete {
        trailing: TrailingSourceCommentOwnership,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpressionCommentPhaseOwner {
    range: CommentRange,
    flags: EmitFlags,
    kind: SyntaxKind,
    relocated_trailing: bool,
}

/// Records whether this expression invocation actually ran an ordinary
/// source-leading-comments phase. Some transformed class-field expressions
/// retain an explicit source anchor for contextless printer routes. When a
/// typed expression phase has already visited that exact range, replaying the
/// anchor would emit the same leading comments twice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
enum SourceLeadingCommentPhaseVisit {
    NotVisited,
    Suppressed,
    Visited { range: CommentRange },
}

impl SourceLeadingCommentPhaseVisit {
    fn visited_range(self, range: CommentRange) -> bool {
        matches!(self, Self::Visited { range: visited } if visited == range)
    }
}

/// A fixed token has already established the boundary before this expression,
/// but source comments cannot be placed until substitution and parenthesis
/// topology are known. Moving this value into the expression pipeline keeps
/// the phase single-owner; return/throw additionally move their final-child
/// trailing boundary so it cannot be emitted again after a generated `)`.
#[derive(Debug)]
struct DeferredExpressionSourceComments {
    container: Option<ExpressionCommentContainer>,
    preceding_token: Option<TokenEmission>,
    extent: DeferredSourceCommentExtent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpressionCommentContainer {
    Node(TransformNode),
    Scope(CommentEmissionScope),
}

#[derive(Debug, Default)]
enum DeferredExpressionSourceCommentsState {
    #[default]
    Inactive,
    Pending(DeferredExpressionSourceComments),
    Consumed(ExpressionSourceCommentsOutcome),
}

impl DeferredExpressionSourceCommentsState {
    const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending(_))
    }

    fn take_pending(&mut self) -> Option<DeferredExpressionSourceComments> {
        let state = std::mem::take(self);
        match state {
            Self::Pending(deferred) => Some(deferred),
            state => {
                *self = state;
                None
            }
        }
    }

    fn record_outcome(&mut self, outcome: ExpressionSourceCommentsOutcome) {
        assert!(matches!(self, Self::Inactive));
        *self = Self::Consumed(outcome);
    }

    fn visited_trailing_anchor_at(&self, cursor: TokenCursor) -> Option<TokenAnchor> {
        match self {
            Self::Consumed(outcome) => outcome.visited_trailing_anchor(),
            _ => None,
        }
        .filter(|anchor| anchor.cursor() == cursor)
    }
}

impl DeferredExpressionSourceComments {
    /// A parent deferring its comment phase hands the child the enclosing
    /// scope it would have guarded against. With no active container the
    /// parent itself is the container, claimed lazily at consumption —
    /// through the same per-side producer as every eager claim.
    const fn container_for_parent(
        parent: TransformNode,
        inherited: CommentEmissionScope,
    ) -> Option<ExpressionCommentContainer> {
        if inherited.container_pos().is_some() || inherited.container_end().is_some() {
            Some(ExpressionCommentContainer::Scope(inherited))
        } else {
            Some(ExpressionCommentContainer::Node(parent))
        }
    }

    const fn leading_only(
        parent: TransformNode,
        token: TokenEmission,
        inherited: CommentEmissionScope,
    ) -> Self {
        Self {
            container: Self::container_for_parent(parent, inherited),
            preceding_token: Some(token),
            extent: DeferredSourceCommentExtent::LeadingOnly,
        }
    }

    const fn leading_and_trailing(
        parent: TransformNode,
        token: TokenEmission,
        inherited: CommentEmissionScope,
    ) -> Self {
        Self {
            container: Self::container_for_parent(parent, inherited),
            preceding_token: Some(token),
            extent: DeferredSourceCommentExtent::LeadingAndTrailing,
        }
    }

    const fn without_preceding_token(
        parent: TransformNode,
        extent: DeferredSourceCommentExtent,
        inherited: CommentEmissionScope,
    ) -> Self {
        Self {
            container: Self::container_for_parent(parent, inherited),
            preceding_token: None,
            extent,
        }
    }

    const fn nested(scope: CommentEmissionScope, extent: DeferredSourceCommentExtent) -> Self {
        Self {
            container: Some(ExpressionCommentContainer::Scope(scope)),
            preceding_token: None,
            extent,
        }
    }

    const fn owns_trailing(&self) -> bool {
        matches!(self.extent, DeferredSourceCommentExtent::LeadingAndTrailing)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmbeddedStatementAnchor {
    Unspecified,
    AfterToken(TokenEmission),
}

/// Source-leading comments separated from the first statement by a blank
/// line are owned by their statement-list boundary, not by whichever
/// transformed statement eventually retains that source range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DetachedSourceCommentPolicy {
    All,
    PinnedOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DetachedCommentPrefix {
    emitted_through: CommentCursor,
    resume: CommentResume,
    policy: DetachedSourceCommentPolicy,
}

/// A detached prefix can be resumed by exactly one node at the same source
/// boundary. Keeping it local to the statement-list emission gives the same
/// lifetime discipline as tsc's detached-comments-info stack without adding
/// mutable printer-global state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PendingDetachedComments {
    resume: Option<CommentResume>,
}

impl PendingDetachedComments {
    fn from_prefix(prefix: Option<DetachedCommentPrefix>) -> Self {
        Self {
            resume: prefix.map(|prefix| prefix.resume),
        }
    }

    fn take_for(&mut self, owner_start: CommentCursor) -> Option<CommentResume> {
        if self
            .resume
            .is_some_and(|resume| resume.owner_start() == owner_start)
        {
            self.resume.take()
        } else {
            None
        }
    }
}

/// A trivia slice that retains its coordinates in the complete source text.
/// Multiline comment indentation is relative to the original source line, so
/// passing only `&str` would discard semantic formatting context that tsc's
/// `writeCommentRange` keeps through its absolute positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceTrivia<'a> {
    source: &'a str,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceCommentKind {
    Line,
    Block,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceCommentRange {
    start: usize,
    end: usize,
    kind: SourceCommentKind,
    has_trailing_new_line: bool,
}

impl<'a> SourceTrivia<'a> {
    fn new(source: &'a str, start: usize, end: usize) -> Self {
        debug_assert!(start <= end);
        debug_assert!(source.is_char_boundary(start));
        debug_assert!(source.is_char_boundary(end));
        Self { source, start, end }
    }

    fn whole(source: &'a str) -> Self {
        Self::new(source, 0, source.len())
    }

    fn from_start(source: &'a str, start: usize) -> Self {
        Self::new(source, start, source.len())
    }

    fn text(self) -> &'a str {
        &self.source[self.start..self.end]
    }

    fn advance(self, byte_count: usize) -> Self {
        Self::new(self.source, self.start + byte_count, self.end)
    }
}

/// Identifies which emitter boundary owns trivia immediately before a node.
/// Delimited-list starts must retain comments after `{`/`[`, while ordinary
/// nodes and later siblings suppress trivia already owned by their container
/// or predecessor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LeadingCommentContext {
    Normal,
    AfterSibling,
    DelimitedListStart,
}

/// Typed projection of ListFormat::Indented for the expression/binding lists
/// emitted by `emit_delimited_expression_list`.
///
/// Array and object literals own an indentation scope even when printed on a
/// single logical line; their children may contain multiline syntax. Binding
/// patterns deliberately do not: tsc's ArrayBindingPatternElements and
/// ObjectBindingPatternElements formats retain the surrounding declaration's
/// indentation for source comments inside the delimiters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DelimitedListIndentation {
    Current,
    Indented,
}

impl DelimitedListIndentation {
    const fn is_indented(self) -> bool {
        matches!(self, Self::Indented)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DelimitedListLinePolicy {
    Compact,
    PreserveSource,
}

impl DelimitedListLinePolicy {
    const fn preserves_source(self) -> bool {
        matches!(self, Self::PreserveSource)
    }
}

/// The two independent list-format responsibilities used by delimited
/// expressions. Keeping them together prevents binding-pattern comments from
/// inheriting literal indentation while still retaining parsed line breaks in
/// array/object literal sibling lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DelimitedListFormat {
    indentation: DelimitedListIndentation,
    lines: DelimitedListLinePolicy,
}

impl DelimitedListFormat {
    const LITERAL: Self = Self {
        indentation: DelimitedListIndentation::Indented,
        lines: DelimitedListLinePolicy::PreserveSource,
    };

    const BINDING_PATTERN: Self = Self {
        indentation: DelimitedListIndentation::Current,
        lines: DelimitedListLinePolicy::Compact,
    };
}

/// Typed projection of the `Parenthesis` bit in tsc's parameter-list format.
///
/// `emitParametersForArrow` does not bypass list emission for a simple arrow
/// head. It emits the ordinary `Parameters` list with only its parentheses bit
/// removed. Keeping that distinction typed ensures the arrow path retains the
/// same list/comment ownership without making delimiter omission available to
/// unrelated function-like syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParameterListParentheses {
    Present,
    OmittedForSimpleArrow,
}

impl ParameterListParentheses {
    const fn are_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExpressionGrammarContext {
    #[default]
    Normal,
    ExpressionStatement,
    LeftSideOfAccess {
        optional_chain: bool,
    },
    NewCallee,
    PrefixUnaryOperand,
    PostfixUnaryOperand,
    ComputedPropertyName,
    ArrowConciseBody,
    AssignmentRightSide,
    ExportDefault,
    /// Expression position whose surrounding grammar treats a comma as a
    /// separator. A comma expression must therefore retain an explicit pair
    /// of parentheses.
    DisallowedComma,
}

/// Expression grammar and ASI safety are independent printer obligations.
///
/// In particular, `parenthesizeExpressionForNoAsi` carries its obligation
/// through the left edge of access/call expressions while each edge still
/// applies its own precedence grammar. Keeping the dimensions separate avoids
/// losing ASI safety when a child needs a more specific grammar context.
///
/// tsc-port: parenthesizeExpressionForNoAsi @6.0.3
/// tsc-hash: e0efdc025b86d2ce47abf2a53f3090af033533205b0ae238ae92b4d95299a98e
/// tsc-span: _tsc.js:118768-118876
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExpressionSyntaxContext {
    grammar: ExpressionGrammarContext,
    no_asi_left_edge: bool,
}

impl ExpressionSyntaxContext {
    const NORMAL: Self = Self {
        grammar: ExpressionGrammarContext::Normal,
        no_asi_left_edge: false,
    };
    const NO_ASI: Self = Self {
        grammar: ExpressionGrammarContext::Normal,
        no_asi_left_edge: true,
    };
    const YIELD_OPERAND: Self = Self {
        grammar: ExpressionGrammarContext::DisallowedComma,
        no_asi_left_edge: true,
    };
    const EXPRESSION_STATEMENT: Self = Self {
        grammar: ExpressionGrammarContext::ExpressionStatement,
        no_asi_left_edge: false,
    };
    const NEW_CALLEE: Self = Self {
        grammar: ExpressionGrammarContext::NewCallee,
        no_asi_left_edge: false,
    };
    const PREFIX_UNARY_OPERAND: Self = Self {
        grammar: ExpressionGrammarContext::PrefixUnaryOperand,
        no_asi_left_edge: false,
    };
    const COMPUTED_PROPERTY_NAME: Self = Self {
        grammar: ExpressionGrammarContext::ComputedPropertyName,
        no_asi_left_edge: false,
    };
    const ARROW_CONCISE_BODY: Self = Self {
        grammar: ExpressionGrammarContext::ArrowConciseBody,
        no_asi_left_edge: false,
    };
    const ASSIGNMENT_RIGHT_SIDE: Self = Self {
        grammar: ExpressionGrammarContext::AssignmentRightSide,
        no_asi_left_edge: false,
    };
    const EXPORT_DEFAULT: Self = Self {
        grammar: ExpressionGrammarContext::ExportDefault,
        no_asi_left_edge: false,
    };
    const DISALLOWED_COMMA: Self = Self {
        grammar: ExpressionGrammarContext::DisallowedComma,
        no_asi_left_edge: false,
    };

    const fn left_side_of_access(optional_chain: bool) -> Self {
        Self {
            grammar: ExpressionGrammarContext::LeftSideOfAccess { optional_chain },
            no_asi_left_edge: false,
        }
    }
}

/// The syntax obligations of one expression edge and the ambient comment
/// scope have different lifetimes. A child selects fresh grammar and ASI
/// requirements while inheriting the complete comment scope established by
/// its enclosing comments phase.
///
/// This is the Rust counterpart of tsc's per-node printer context: the
/// comment half is the threaded [`CommentEmissionScope`] triple, replacing
/// the closure variables that tsc saves and restores around every commented
/// node. There is no `Default`; [`EmitContext::file_root`] is the single
/// zero-scope constructor, and every nested context is derived by
/// threading.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EmitContext {
    syntax: ExpressionSyntaxContext,
    comments: CommentEmissionScope,
    /// tsc's `commentsDisabled` dynamic extent, threaded immutably: a node
    /// carrying `NoNestedComments` hands its entire subtree a context whose
    /// comment phases are suppressed (set in `emit_node_with_hint`; the
    /// node's OWN phases run at the parent level and stay live).
    ///
    /// tsc-port: emitCommentsBeforeNode/emitCommentsAfterNode @6.0.3
    /// tsc-span: _tsc.js:120987-121006
    nested_comments_suppressed: bool,
}

impl EmitContext {
    /// The printer root: normal syntax obligations and the initial
    /// `-1/-1/-1` comment scope. This is tsc's `createPrinter` state and is
    /// constructed exactly once per emitted source file.
    const fn file_root() -> Self {
        Self {
            syntax: ExpressionSyntaxContext::NORMAL,
            comments: CommentEmissionScope::empty(),
            nested_comments_suppressed: false,
        }
    }

    /// A wrapper re-entry: the composed active comment scope under fresh
    /// syntax obligations. The wrapper's parentheses discharge every
    /// grammar and ASI duty of the edge it replaces, so only the comment
    /// half survives it.
    const fn for_wrapper(self, comments: CommentEmissionScope) -> Self {
        self.for_child(ExpressionSyntaxContext::NORMAL)
            .with_comments(comments)
    }

    const fn grammar(self) -> ExpressionGrammarContext {
        self.syntax.grammar
    }

    const fn carries_no_asi_left_edge(self) -> bool {
        self.syntax.no_asi_left_edge
    }

    const fn with_grammar(self, grammar: ExpressionGrammarContext) -> Self {
        Self {
            syntax: ExpressionSyntaxContext {
                grammar,
                no_asi_left_edge: self.syntax.no_asi_left_edge,
            },
            comments: self.comments,
            nested_comments_suppressed: self.nested_comments_suppressed,
        }
    }

    const fn for_child(self, syntax: ExpressionSyntaxContext) -> Self {
        Self {
            syntax,
            comments: self.comments,
            nested_comments_suppressed: self.nested_comments_suppressed,
        }
    }

    const fn with_comments(self, comments: CommentEmissionScope) -> Self {
        Self {
            syntax: self.syntax,
            comments,
            nested_comments_suppressed: self.nested_comments_suppressed,
        }
    }

    /// The threaded comment scope, for the routes that carry ambient
    /// comment state without the syntax half.
    const fn comments(self) -> CommentEmissionScope {
        self.comments
    }

    const fn with_nested_comments_suppressed(self) -> Self {
        Self {
            syntax: self.syntax,
            comments: self.comments,
            nested_comments_suppressed: true,
        }
    }

    const fn nested_comments_suppressed(self) -> bool {
        self.nested_comments_suppressed
    }
}

/// Selects whether an unchanged source-file root is a text-preserving printer
/// request or a compiler JavaScript emit. The standalone printer keeps H1's
/// exact identity contract by default; compiler emit always walks the AST,
/// matching tsc's `emitSourceFileWorker` even when no transformer cloned the
/// root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceFileTextMode {
    #[default]
    PreserveUnchanged,
    Canonical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrinterOptions {
    new_line: NewLineKind,
    remove_comments: bool,
    no_emit_helpers: bool,
    import_helpers: bool,
    target: Option<ScriptTarget>,
    source_file_text_mode: SourceFileTextMode,
}

impl PrinterOptions {
    pub const fn new(new_line: NewLineKind) -> Self {
        Self {
            new_line,
            remove_comments: false,
            no_emit_helpers: false,
            import_helpers: false,
            target: None,
            source_file_text_mode: SourceFileTextMode::PreserveUnchanged,
        }
    }

    pub const fn with_remove_comments(mut self, value: bool) -> Self {
        self.remove_comments = value;
        self
    }

    pub const fn with_no_emit_helpers(mut self, value: bool) -> Self {
        self.no_emit_helpers = value;
        self
    }

    /// `importHelpers`: unscoped helper bodies are imported from `tslib`
    /// by the module transformer instead of being inlined
    /// (`hasRecordedExternalHelpers`, `_tsc.js:117730`); the printer
    /// suppresses them for external-module sources.
    pub const fn with_import_helpers(mut self, value: bool) -> Self {
        self.import_helpers = value;
        self
    }

    pub const fn with_target(mut self, target: ScriptTarget) -> Self {
        self.target = Some(target);
        self
    }

    pub const fn with_source_file_text_mode(mut self, mode: SourceFileTextMode) -> Self {
        self.source_file_text_mode = mode;
        self
    }

    pub const fn new_line(self) -> NewLineKind {
        self.new_line
    }

    pub const fn remove_comments(self) -> bool {
        self.remove_comments
    }

    pub const fn no_emit_helpers(self) -> bool {
        self.no_emit_helpers
    }

    pub const fn target(self) -> Option<ScriptTarget> {
        self.target
    }

    pub const fn source_file_text_mode(self) -> SourceFileTextMode {
        self.source_file_text_mode
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrintRequest {
    SourceFile(TransformSourceId),
    StandaloneNode(TransformNode),
    NodeList(TransformNodeArray),
    Bundle(TransformBundle),
    JavaScriptMap(TransformSourceId),
    Declaration(TransformSourceId),
}

/// The two sides of a recorded map range (h2-6a-m-2 §4): Before is the
/// skip-trivia'd leading position, After the raw trailing position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MapBoundary {
    Before,
    After,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrintedText {
    text: String,
    end: GeneratedUtf16Location,
    source_map: Option<crate::source_map::SourceMapGenerator>,
}

impl PrintedText {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn end(&self) -> GeneratedUtf16Location {
        self.end
    }

    /// h2-6a-m-2: the recorded generator, present exactly when the
    /// print ran with a `SourceMapRecordingInputs` (the m-3 caller
    /// serializes it; tests replay it).
    pub fn source_map(&self) -> Option<&crate::source_map::SourceMapGenerator> {
        self.source_map.as_ref()
    }

    pub fn into_source_map(self) -> Option<crate::source_map::SourceMapGenerator> {
        self.source_map
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Printer {
    options: PrinterOptions,
    emission_plan: EmissionPlan,
}

/// Immutable structural decisions derived once from the final transformed
/// tree. Keeping these in the printer avoids rebuilding parent links on
/// session-owned synthetic nodes and keeps target-specific spelling out of
/// ECMAScript transformers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EmissionPlan {
    structured_nodes: BTreeSet<TransformNode>,
    function_body_blocks: BTreeSet<TransformNode>,
}

/// tsc-port: createPrinter @6.0.3
/// tsc-hash: b227b66a85178f81faf58d6de65ed31fe2a87de1448ec6ec61e535fd36194697
/// tsc-span: _tsc.js:116912-121378
///
/// H1.2 implements the pipeline foundation and whole-source identity arm.
pub fn create_printer(options: PrinterOptions) -> Printer {
    Printer {
        options,
        emission_plan: EmissionPlan::default(),
    }
}

impl Printer {
    pub const fn options(&self) -> PrinterOptions {
        self.options
    }

    fn prepare_emission_plan(
        &mut self,
        transformation: &TransformationResult<'_>,
        root: TransformNode,
    ) -> Result<(), PrinterError> {
        let mut structured_nodes = BTreeSet::new();
        let mut function_body_blocks = BTreeSet::new();
        let mut memo = BTreeMap::new();
        Self::collect_emission_plan(
            transformation,
            root,
            self.options.target,
            &mut memo,
            &mut structured_nodes,
            &mut function_body_blocks,
        )?;
        self.emission_plan = EmissionPlan {
            structured_nodes,
            function_body_blocks,
        };
        Ok(())
    }

    fn collect_emission_plan(
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        target: Option<ScriptTarget>,
        memo: &mut BTreeMap<TransformNode, bool>,
        structured_nodes: &mut BTreeSet<TransformNode>,
        function_body_blocks: &mut BTreeSet<TransformNode>,
    ) -> Result<bool, PrinterError> {
        if let Some(requires_structured_emit) = memo.get(&node) {
            return Ok(*requires_structured_emit);
        }

        let record = transformation.arena().node(node)?;
        let function_body = match &record.data {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::ClassStaticBlockDeclaration(data) => data.body,
            _ => None,
        };
        if let Some(body) = function_body
            .and_then(|body| transformation.arena().node_ref(node.source(), body))
            .filter(|body| {
                transformation
                    .arena()
                    .node(*body)
                    .is_ok_and(|body| body.kind == SyntaxKind::Block)
            })
        {
            function_body_blocks.insert(body);
        }
        let requires_literal_rewrite = match &record.data {
            NodeData::NumericLiteral(_) => {
                let flags = TokenFlags::from_bits(record.numeric_literal_flags);
                flags.intersects(TokenFlags::IS_INVALID)
                    || flags.contains(TokenFlags::CONTAINS_SEPARATOR)
                        && target.is_none_or(|target| target < ScriptTarget::ES2021)
            }
            // tsc's canUseOriginalText always rejects BigIntLiteral nodes.
            NodeData::BigIntLiteral(_) => true,
            _ => false,
        };
        let mut children = Vec::new();
        let source = transformation.arena().source(node.source())?.syntax();
        for_each_child(&source.arena, record, |child| {
            children.push(child);
            false
        });

        let mut requires_structured_emit = requires_literal_rewrite;
        for child in children {
            let child = transformation
                .arena()
                .node_ref(node.source(), child)
                .ok_or(PrinterError::UnknownStatement(child.0))?;
            requires_structured_emit |= Self::collect_emission_plan(
                transformation,
                child,
                target,
                memo,
                structured_nodes,
                function_body_blocks,
            )?;
        }
        if requires_structured_emit {
            structured_nodes.insert(node);
        }
        memo.insert(node, requires_structured_emit);
        Ok(requires_structured_emit)
    }

    /// The generic H1 printer surface. H1.2 established the exact whole-source
    /// identity arm; H1.3 adds the bounded changed-node JavaScript workers
    /// while the remaining request/product axes stay typed controls.
    pub fn print(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        request: PrintRequest,
        recording: Option<crate::source_map::SourceMapRecordingInputs>,
    ) -> Result<PrintedText, PrinterError> {
        match request {
            PrintRequest::SourceFile(source) => {
                self.print_source_file(transformation, source, recording)
            }
            PrintRequest::StandaloneNode(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::StandaloneNodePrinting,
            )),
            PrintRequest::NodeList(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::NodeListPrinting,
            )),
            PrintRequest::Bundle(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            )),
            PrintRequest::JavaScriptMap(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::JavaScriptMap,
            )),
            PrintRequest::Declaration(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::Declaration,
            )),
        }
    }

    fn print_source_file(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        recording: Option<crate::source_map::SourceMapRecordingInputs>,
    ) -> Result<PrintedText, PrinterError> {
        if !transformation.roots().iter().any(
            |root| matches!(root, crate::TransformRoot::SourceFile(source) if *source == source_id),
        ) {
            return Err(PrinterError::SourceIsNotATransformedRoot(source_id));
        }

        let root = transformation.arena().root(source_id)?;
        self.prepare_emission_plan(transformation, root)?;
        if transformation
            .arena()
            .source(source_id)?
            .syntax()
            .file_name
            .to_ascii_lowercase()
            .ends_with(".json")
        {
            // h2-6a-m-2 §4: JSON sources never record (the upstream
            // triple guard); requesting a recording here is fail-closed.
            if recording.is_some() {
                return Err(PrinterError::Unsupported(
                    UnsupportedEmitFeature::JavaScriptMap,
                ));
            }
            return self.print_json_source_file(transformation, source_id, root);
        }
        let (text, language_variant, statement_array, statements) = {
            let source = transformation.arena().source(source_id)?.syntax();
            let root_record = source.arena.node(root.node());
            let statement_array = match &root_record.data {
                NodeData::SourceFile(data) => data.statements,
                _ => return Err(PrinterError::RootIsNotSourceFile(root)),
            };
            let statements = statement_array
                .map(|array| source.arena.node_array(array).nodes.clone())
                .unwrap_or_default();
            (
                source.text().to_owned(),
                source.language_variant,
                statement_array.map(|array| TransformNodeArray::new(source_id, array)),
                statements,
            )
        };

        if transformation
            .arena()
            .metadata(root)
            .and_then(crate::EmitMetadata::original)
            .is_some()
            || self.emission_plan.structured_nodes.contains(&root)
            || self.options.source_file_text_mode == SourceFileTextMode::Canonical
        {
            return self.print_transformed_source_file(
                transformation,
                source_id,
                root,
                statement_array,
                statements,
                recording,
            );
        }

        // h2-6a-m-2 §4: the identity arm is unreachable under compiler
        // emit (Canonical is pinned there) and records nothing; a
        // recording request on this arm is fail-closed.
        if recording.is_some() {
            return Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::JavaScriptMap,
            ));
        }

        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let substituted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if substituted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(
                substituted_root,
            ));
        }

        let mut writer = create_text_writer(self.options.new_line);
        let _ = language_variant;
        let mut cursor = 0u32;
        for raw_statement in statements {
            let statement = transformation
                .arena()
                .node_ref(source_id, raw_statement)
                .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
            if emitted != statement {
                transformation.after_emit_node(EmitHint::Unspecified, statement)?;
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted));
            }

            let range = self.node_range(transformation, statement)?;
            let start = range.start().value();
            let end = range.end().value();
            if start < cursor {
                return Err(PrinterError::OverlappingSourceRange {
                    previous_end: cursor,
                    start,
                });
            }
            raw_write_range(&mut writer, &text, cursor, start)?;
            self.write_original_node(
                transformation,
                statement,
                OriginalNodeText { range, text: &text },
                &mut writer,
            )?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            cursor = end;
        }
        raw_write_range(
            &mut writer,
            &text,
            cursor,
            u32::try_from(text.len()).expect("source text exceeds u32"),
        )?;
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
            source_map: None,
        })
    }

    /// tsc-port: emitExpressionStatement @6.0.3
    /// tsc-hash: 2735e6c85cd4ac9311765eef71de22d5fd8e247cfc4f67ed4e04cc688fe3f2a2
    /// tsc-span: _tsc.js:118623-118628
    ///
    /// JSON SourceFiles deliberately bypass the whole-source identity arm:
    /// TypeScript prints their single value as an expression, omits the
    /// statement semicolon, normalizes whitespace/newlines, and lets the
    /// write callback own BOM materialization.
    fn print_json_source_file(
        &self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        root: TransformNode,
    ) -> Result<PrintedText, PrinterError> {
        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let emitted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if emitted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted_root));
        }

        let statements = match &transformation.arena().node(root)?.data {
            NodeData::SourceFile(data) => data
                .statements
                .and_then(|array| transformation.arena().node_array_ref(source_id, array))
                .map(|array| transformation.arena().node_array(array))
                .transpose()?
                .map(|array| array.nodes.clone())
                .unwrap_or_default(),
            _ => return Err(PrinterError::RootIsNotSourceFile(root)),
        };
        let mut writer = create_text_writer(self.options.new_line);
        if let Some(statement_id) = statements.first().copied() {
            if statements.len() != 1 {
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::UnsupportedTransformedSyntax {
                    node: root,
                    kind: SyntaxKind::SourceFile,
                });
            }
            let statement = transformation
                .arena()
                .node_ref(source_id, statement_id)
                .ok_or(PrinterError::UnknownStatement(statement_id.0))?;
            let expression = match &transformation.arena().node(statement)?.data {
                NodeData::ExpressionStatement(data) => {
                    data.expression
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::ExpressionStatement,
                            field: "expression",
                        })?
                }
                _ => {
                    let kind = transformation.arena().node(statement)?.kind;
                    transformation.after_emit_node(EmitHint::SourceFile, root)?;
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node: statement,
                        kind,
                    });
                }
            };
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted_statement =
                transformation.substitute_node(EmitHint::Unspecified, statement)?;
            if emitted_statement != statement {
                transformation.after_emit_node(EmitHint::Unspecified, statement)?;
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::TransformedNodeWorkerUnavailable(
                    emitted_statement,
                ));
            }
            self.emit_statement_leading_comments(transformation, statement, &mut writer)?;
            self.emit_node_id_with_context(
                transformation,
                source_id,
                expression,
                EmitContext::file_root(),
                &mut writer,
            )?;
            self.emit_statement_trailing_comments(transformation, statement, &mut writer)?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            writer.write_line(false);
        }
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
            source_map: None,
        })
    }

    fn print_transformed_source_file(
        &self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        root: TransformNode,
        statement_array: Option<TransformNodeArray>,
        statements: Vec<tsc_syntax::NodeId>,
        recording: Option<crate::source_map::SourceMapRecordingInputs>,
    ) -> Result<PrintedText, PrinterError> {
        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let emitted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if emitted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted_root));
        }

        let (original_source_was_statementless, original_first_statement) = {
            let original_root = transformation.arena().get_original_node(root);
            match &transformation.arena().node(original_root)?.data {
                NodeData::SourceFile(data) => {
                    let statements = data.statements.and_then(|array| {
                        transformation
                            .arena()
                            .node_array_ref(original_root.source(), array)
                    });
                    let statements = statements
                        .map(|array| transformation.arena().node_array(array))
                        .transpose()?;
                    let first = statements
                        .and_then(|array| array.nodes.first().copied())
                        .and_then(|id| transformation.arena().node_ref(original_root.source(), id));
                    (statements.is_none_or(|array| array.nodes.is_empty()), first)
                }
                _ => (false, None),
            }
        };
        let mut writer = create_text_writer(self.options.new_line);
        if let Some(inputs) = recording {
            let mut active = crate::source_map::SourceMapRecording::new(inputs);
            let file_name = transformation
                .arena()
                .source(source_id)?
                .syntax()
                .file_name
                .clone();
            active.set_current_source(source_id, &file_name);
            writer.set_source_map_recording(Some(active));
        }
        let source_text = transformation.arena().source(source_id)?.syntax().text();
        if let Some(shebang) = source_shebang(source_text) {
            writer.write_comment(shebang);
            writer.write_line(false);
        }
        // `shouldSkip = printerOptions.noEmitHelpers || hasRecordedExternalHelpers(sourceFile)`
        // (`_tsc.js:117729-117736`): under `importHelpers` an external
        // module's unscoped helpers were rewritten into the tslib import by
        // the module transformer, so their bodies never inline. The
        // external-module test is equivalent to the recorded flag: the
        // import is created exactly when unscoped helpers exist there.
        let suppress_unscoped = self.options.import_helpers
            && transformation
                .arena()
                .source(source_id)?
                .syntax()
                .external_module_indicator
                .is_some();
        let helpers = if self.options.no_emit_helpers {
            Vec::new()
        } else {
            let mut helpers = transformation.emit_helpers().to_vec();
            if suppress_unscoped {
                helpers.retain(|helper| helper.scoped());
            }
            helpers.sort_by_key(|helper| {
                helper
                    .priority()
                    .map_or((true, 0), |priority| (false, priority))
            });
            helpers
        };
        let system_scoped_helpers = !helpers.is_empty()
            && statements.first().is_some_and(|statement| {
                transformation
                    .arena()
                    .node_ref(source_id, *statement)
                    .is_some_and(|statement| {
                        self.is_system_register_statement(transformation, statement)
                    })
            });
        let source_helpers = if system_scoped_helpers {
            &[][..]
        } else {
            helpers.as_slice()
        };
        let helper_offset = statements
            .iter()
            .take_while(|statement| {
                transformation
                    .arena()
                    .node_ref(source_id, **statement)
                    .is_some_and(|statement| self.is_prologue_statement(transformation, statement))
            })
            .count();
        let detached_source_prefix = original_first_statement
            .map(|first| self.detached_source_prefix(transformation, first))
            .transpose()?
            .flatten();
        let source_owned_detached_prefix = self
            .source_file_owns_detached_prefix(
                transformation,
                statement_array,
                statements.first().copied(),
            )?
            .then_some(detached_source_prefix)
            .flatten();
        let mut pending_detached_comments = PendingDetachedComments::default();
        let mut accounted_for_original_prefix = source_owned_detached_prefix.is_some();
        let mut last_original_statement = None;
        if statements.is_empty() {
            self.emit_detached_comment_prefix(
                transformation,
                source_owned_detached_prefix,
                &mut writer,
            )?;
            self.emit_helpers(source_helpers, &mut writer)?;
        }
        for (statement_index, raw_statement) in statements.into_iter().enumerate() {
            if statement_index == helper_offset {
                self.emit_detached_comment_prefix(
                    transformation,
                    source_owned_detached_prefix,
                    &mut writer,
                )?;
                pending_detached_comments =
                    PendingDetachedComments::from_prefix(source_owned_detached_prefix);
                self.emit_helpers(source_helpers, &mut writer)?;
            }
            let statement = transformation
                .arena()
                .node_ref(source_id, raw_statement)
                .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
            let original = transformation.arena().get_original_node(emitted);
            let original_source = transformation.arena().source(original.source())?.syntax();
            let original_record = transformation.arena().node(original)?;
            let had_previous_original_statement = last_original_statement.is_some();
            let emitted_has_original_range = matches!(
                SourceRange::from_raw(
                    original_record.pos,
                    original_record.end,
                    original_source.positions(),
                )?,
                SourceRange::Original(_)
            );
            if emitted_has_original_range {
                last_original_statement = Some(original);
            }
            if !accounted_for_original_prefix && emitted_has_original_range {
                if original_first_statement.is_some_and(|first| first != original) {
                    self.emit_detached_comment_prefix(
                        transformation,
                        detached_source_prefix,
                        &mut writer,
                    )?;
                }
                accounted_for_original_prefix = true;
            }
            let detached_resume = self.take_detached_comment_resume_for_node(
                transformation,
                &mut pending_detached_comments,
                original,
            )?;
            if let Some(detached_resume) = detached_resume {
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    emitted,
                    if had_previous_original_statement && emitted_has_original_range {
                        LeadingCommentContext::AfterSibling
                    } else {
                        LeadingCommentContext::Normal
                    },
                    Some(detached_resume),
                    &mut writer,
                )?;
            } else if had_previous_original_statement && emitted_has_original_range {
                self.emit_statement_leading_comments_after_sibling(
                    transformation,
                    emitted,
                    &mut writer,
                )?;
            } else {
                self.emit_statement_leading_comments(transformation, emitted, &mut writer)?;
            }
            // h2-6a-m-2 §4: the statement-level map pair is gone — the
            // node bracket inside emit_transformed_node records the
            // boundary BEFORE the trailing statement comments (the
            // upstream order the old pair violated).
            self.emit_transformed_node(
                transformation,
                emitted,
                EmitContext::file_root(),
                &mut writer,
            )?;
            self.emit_statement_trailing_comments(transformation, emitted, &mut writer)?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            writer.write_line(false);
        }
        if original_source_was_statementless && !self.options.remove_comments {
            let source = transformation.arena().source(source_id)?.syntax();
            emit_leading_comments(SourceTrivia::whole(source.text()), &mut writer, true);
        } else if !transformation
            .arena()
            .metadata(root)
            .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS))
        {
            if let Some(statement_array) = statement_array {
                self.emit_source_file_statement_list_trailing_comments(
                    transformation,
                    statement_array,
                    &mut writer,
                )?;
            }
        }
        // tsc's source-file writer closes the MultiLine statement list
        // with writeLine. This is normally a no-op because statements
        // already end at line start, but it is observable when an EOF
        // multiline comment has just written its separating space.
        writer.write_line(false);
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        // h2-6a-m-2 §12a: the system-helper splice rewrites finished
        // output after emission and would invalidate recorded generated
        // positions; recording under that lane is fail-closed until the
        // m-3 resolution.
        if system_scoped_helpers && writer.has_source_map_recording() {
            return Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::JavaScriptMap,
            ));
        }
        let text = if system_scoped_helpers {
            self.insert_system_scoped_helpers(writer.text(), &helpers)?
        } else {
            writer.text().to_owned()
        };
        let end = if system_scoped_helpers {
            let mut measured = create_text_writer(self.options.new_line);
            measured.raw_write(&text);
            measured.location()
        } else {
            writer.location()
        };
        let source_map = writer
            .take_source_map_recording()
            .map(crate::source_map::SourceMapRecording::into_generator);
        Ok(PrintedText {
            text,
            end,
            source_map,
        })
    }

    fn is_system_register_statement(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
    ) -> bool {
        let Ok(statement_record) = transformation.arena().node(statement) else {
            return false;
        };
        let Some(call) = (match &statement_record.data {
            NodeData::ExpressionStatement(data) => data.expression,
            _ => None,
        })
        .and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        }) else {
            return false;
        };
        let Ok(call_record) = transformation.arena().node(call) else {
            return false;
        };
        let Some(access) = (match &call_record.data {
            NodeData::CallExpression(data) => data.expression,
            _ => None,
        })
        .and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        }) else {
            return false;
        };
        let Ok(access_record) = transformation.arena().node(access) else {
            return false;
        };
        let NodeData::PropertyAccessExpression(data) = &access_record.data else {
            return false;
        };
        let expression = data.expression.and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        });
        let name = data
            .name
            .and_then(|name| transformation.arena().node_ref(statement.source(), name));
        expression
            .and_then(|node| transformation.arena().node(node).ok())
            .is_some_and(
                |node| matches!(&node.data, NodeData::Identifier(data) if data.text == "System"),
            )
            && name
                .and_then(|node| transformation.arena().node(node).ok())
                .is_some_and(
                    |node| matches!(&node.data, NodeData::Identifier(data) if data.text == "register"),
                )
    }

    fn insert_system_scoped_helpers(
        &self,
        text: &str,
        helpers: &[EmitHelper],
    ) -> Result<String, PrinterError> {
        let new_line = self.options.new_line.text();
        let first_line_end = text
            .find(new_line)
            .map(|offset| offset + new_line.len())
            .unwrap_or(0);
        let strict = format!("    \"use strict\";{new_line}");
        let insertion_offset = if text[first_line_end..].starts_with(&strict) {
            first_line_end + strict.len()
        } else {
            first_line_end
        };
        let mut helper_writer = create_text_writer(self.options.new_line);
        helper_writer.increase_indent();
        self.emit_helpers(helpers, &mut helper_writer)?;
        let mut output = String::with_capacity(text.len() + helper_writer.text().len());
        output.push_str(&text[..insertion_offset]);
        output.push_str(helper_writer.text());
        output.push_str(&text[insertion_offset..]);
        Ok(output)
    }

    fn emit_transformed_node(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        // tsc-port: emitCommentsBeforeNode/emitCommentsAfterNode @6.0.3
        // tsc-span: _tsc.js:120987-121006
        //
        // NoNestedComments disables the comments pipeline for the node's
        // subtree; the node's own leading/trailing phases run at the parent
        // level and stay live, so only the child-facing context flips. The
        // extent ends when this context goes out of scope - tsc's mutable
        // enable/disable pair, threaded immutably.
        let node_flags = transformation
            .arena()
            .metadata(node)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags());
        let expression_context = if node_flags.intersects(EmitFlags::NO_NESTED_COMMENTS) {
            expression_context.with_nested_comments_suppressed()
        } else {
            expression_context
        };
        // h2-6a-m-2 §4 node phase: Before after the leading comment
        // phases, After before the trailing ones (upstream
        // pipelineEmitWithSourceMaps nesting), with the
        // NO_NESTED_SOURCE_MAPS extent suppressing every record inside
        // the subtree while the node's own boundaries stay live
        // (upstream emitSourceMapsBeforeNode order: record, then
        // disable; AfterNode: enable, then record).
        self.record_node_map_boundary(transformation, MapBoundary::Before, node, writer)?;
        let suppress_nested_maps = node_flags.intersects(EmitFlags::NO_NESTED_SOURCE_MAPS);
        if suppress_nested_maps {
            if let Some(recording) = writer.recording_mut() {
                recording.suppress();
            }
        }
        let mut deferred_source_comments = DeferredExpressionSourceCommentsState::default();
        let worker_result = self.emit_transformed_node_worker(
            transformation,
            node,
            expression_context,
            &mut deferred_source_comments,
            writer,
        );
        if suppress_nested_maps {
            if let Some(recording) = writer.recording_mut() {
                recording.unsuppress();
            }
        }
        worker_result?;
        self.record_node_map_boundary(transformation, MapBoundary::After, node, writer)?;
        debug_assert!(matches!(
            deferred_source_comments,
            DeferredExpressionSourceCommentsState::Inactive
        ));
        Ok(())
    }

    fn emit_transformed_node_worker(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        expression_context: EmitContext,
        deferred_source_comments: &mut DeferredExpressionSourceCommentsState,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let record = transformation.arena().node(node)?.clone();
        let changed = transformation
            .arena()
            .metadata(node)
            .and_then(crate::EmitMetadata::original)
            .is_some()
            || NodeFlags::from_bits(record.flags).contains(NodeFlags::SYNTHESIZED)
            || self.emission_plan.structured_nodes.contains(&node);
        let multi_line = record.multi_line == Some(true);
        let json_source = transformation
            .arena()
            .source(node.source())?
            .syntax()
            .file_name
            .to_ascii_lowercase()
            .ends_with(".json");

        match record.data {
            NodeData::Token if record.kind == SyntaxKind::JsxOpeningFragment => {
                writer.write_punctuation("<>");
                Ok(())
            }
            NodeData::Token if record.kind == SyntaxKind::JsxClosingFragment => {
                writer.write_punctuation("</>");
                Ok(())
            }
            NodeData::Token if changed => {
                let text = tsc_syntax::tokens::token_to_string(record.kind).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write(text);
                Ok(())
            }
            NodeData::Identifier(data) if changed => {
                if self.transformed_identifier_can_reuse_source_spelling(
                    transformation,
                    node,
                    &data.text,
                )? {
                    self.write_original_without_leading_trivia(transformation, node, writer)
                } else {
                    writer.write_symbol(&data.text);
                    Ok(())
                }
            }
            NodeData::PrivateIdentifier(data) if changed => {
                writer.write_symbol(&data.text);
                Ok(())
            }
            NodeData::NumericLiteral(data) if changed => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::BigIntLiteral(data) => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::Decorator(data) => {
                writer.write_punctuation("@");
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::Decorator,
                    "expression",
                    expression_context
                        .for_child(ExpressionSyntaxContext::left_side_of_access(false)),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )
            }
            NodeData::ExpressionStatement(data) => {
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::ExpressionStatement,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::EXPRESSION_STATEMENT),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::DebuggerStatement(_) => {
                // h2-6a-m-2 §4 route table: upstream writes the debugger
                // keyword via writeToken (118931) — the keyword itself
                // maps, anchored at the statement start with the keyword
                // length.
                let keyword_default = {
                    let record = transformation.arena().node(node)?;
                    let positions = transformation
                        .arena()
                        .source(node.source())?
                        .syntax()
                        .positions();
                    match SourceRange::from_raw(record.pos, record.end, positions) {
                        Ok(SourceRange::Original(range)) => self.token_map_range_spanning(
                            transformation,
                            node.source(),
                            range.start().value(),
                            "debugger".len(),
                            writer,
                        )?,
                        _ => None,
                    }
                };
                self.record_brace_write(
                    transformation,
                    node,
                    SyntaxKind::DebuggerKeyword,
                    keyword_default,
                    "debugger",
                    |writer, spelling| writer.write_keyword(spelling),
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ReturnStatement(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ReturnKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                if let Some(expression) = expression {
                    writer.write_space(" ");
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        keyword,
                        expression,
                        expression_context.for_child(ExpressionSyntaxContext::NO_ASI),
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ThrowStatement(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ThrowStatement,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ThrowKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::NO_ASI),
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::OmittedExpression(_) => Ok(()),
            NodeData::StringLiteral(data) => {
                if !changed {
                    // tsc's `getLiteralText` returns the original token bytes
                    // and `writeStringLiteral` writes them verbatim. In
                    // particular, a line continuation keeps the source's LF
                    // or CRLF independently of the configured newline used
                    // for emitted syntax boundaries.
                    self.write_original_without_leading_trivia_verbatim(
                        transformation,
                        node,
                        writer,
                    )
                } else {
                    let metadata = transformation.arena().metadata(node);
                    if let Some(text_source) =
                        metadata.and_then(crate::EmitMetadata::string_literal_text_source)
                    {
                        if transformation.arena().node(text_source)?.kind
                            == SyntaxKind::StringLiteral
                            && self.node_has_source_text_range(transformation, text_source)?
                        {
                            return self.write_original_without_leading_trivia_verbatim(
                                transformation,
                                text_source,
                                writer,
                            );
                        }
                    }
                    let single_quote = metadata
                        .and_then(crate::EmitMetadata::string_literal_single_quote)
                        .unwrap_or(false);
                    let no_ascii_escaping = metadata.is_some_and(|metadata| {
                        metadata.flags().contains(EmitFlags::NO_ASCII_ESCAPING)
                    });
                    let quoted = metadata
                        .and_then(crate::EmitMetadata::javascript_string_value)
                        .map(|value| {
                            quote_javascript_string(
                                value.code_units(),
                                single_quote,
                                no_ascii_escaping,
                            )
                        })
                        .unwrap_or_else(|| {
                            quote_string_literal(&data.text, single_quote, no_ascii_escaping)
                        });
                    writer.write_string_literal(&quoted);
                    Ok(())
                }
            }
            NodeData::JsxElement(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.opening_element,
                    SyntaxKind::JsxElement,
                    "opening_element",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_jsx_children(
                    transformation,
                    node.source(),
                    data.children,
                    expression_context,
                    writer,
                )?;
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.closing_element,
                    SyntaxKind::JsxElement,
                    "closing_element",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::JsxSelfClosingElement(data) => {
                writer.write_punctuation("<");
                self.emit_required_jsx_tag_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxSelfClosingElement,
                    "tag_name",
                    expression_context,
                    writer,
                )?;
                self.emit_type_arguments(
                    transformation,
                    node.source(),
                    data.type_arguments,
                    expression_context,
                    writer,
                )?;
                if let Some(tag_name) = data
                    .tag_name
                    .and_then(|tag_name| transformation.arena().node_ref(node.source(), tag_name))
                {
                    self.emit_trailing_comments_for_node(transformation, tag_name, writer)?;
                }
                writer.write_space(" ");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.attributes,
                    SyntaxKind::JsxSelfClosingElement,
                    "attributes",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation("/>");
                Ok(())
            }
            NodeData::JsxFragment(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.opening_fragment,
                    SyntaxKind::JsxFragment,
                    "opening_fragment",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_jsx_children(
                    transformation,
                    node.source(),
                    data.children,
                    expression_context,
                    writer,
                )?;
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.closing_fragment,
                    SyntaxKind::JsxFragment,
                    "closing_fragment",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::JsxOpeningElement(data) => {
                writer.write_punctuation("<");
                self.emit_required_jsx_tag_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxOpeningElement,
                    "tag_name",
                    expression_context,
                    writer,
                )?;
                self.emit_type_arguments(
                    transformation,
                    node.source(),
                    data.type_arguments,
                    expression_context,
                    writer,
                )?;
                if let Some(tag_name) = data
                    .tag_name
                    .and_then(|tag_name| transformation.arena().node_ref(node.source(), tag_name))
                {
                    self.emit_trailing_comments_for_node(transformation, tag_name, writer)?;
                }
                let has_attributes = data
                    .attributes
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .and_then(|attributes| transformation.arena().node(attributes).ok())
                    .and_then(|attributes| attributes.data.as_jsx_attributes())
                    .and_then(|attributes| attributes.properties)
                    .and_then(|id| transformation.arena().node_array_ref(node.source(), id))
                    .is_some_and(|array| {
                        transformation
                            .arena()
                            .node_array(array)
                            .is_ok_and(|array| !array.nodes.is_empty())
                    });
                if has_attributes {
                    writer.write_space(" ");
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.attributes,
                    SyntaxKind::JsxOpeningElement,
                    "attributes",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation(">");
                Ok(())
            }
            NodeData::JsxClosingElement(data) => {
                writer.write_punctuation("</");
                self.emit_required_jsx_tag_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxClosingElement,
                    "tag_name",
                    expression_context,
                    writer,
                )?;
                writer.write_punctuation(">");
                Ok(())
            }
            NodeData::JsxAttributes(data) => self.emit_jsx_attributes(
                transformation,
                node.source(),
                data.properties,
                expression_context,
                writer,
            ),
            NodeData::JsxAttribute(data) => {
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::JsxAttribute,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_punctuation("=");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        initializer,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::JsxSpreadAttribute(data) => {
                writer.write_punctuation("{...");
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::JsxSpreadAttribute,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )?;
                writer.write_punctuation("}");
                Ok(())
            }
            NodeData::JsxExpression(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                if expression.is_none()
                    && (self.options.remove_comments
                        || !self.original_jsx_has_comments_at_open(transformation, node)?)
                {
                    return Ok(());
                }
                let multiline = !self.source_node_range_is_on_single_line(transformation, node)?;
                if multiline {
                    writer.increase_indent();
                }
                let first_child = data
                    .dot_dot_dot_token
                    .and_then(|token| transformation.arena().node_ref(node.source(), token))
                    .or(expression);
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenBraceToken),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                let open_prefix =
                    self.token_owned_child_prefix(transformation, open, first_child)?;
                if let Some(dot_dot_dot) = data.dot_dot_dot_token {
                    if let Some(first_child) = first_child {
                        self.emit_leading_comments_for_node_worker(
                            transformation,
                            first_child,
                            LeadingCommentContext::Normal,
                            open_prefix,
                            writer,
                        )?;
                    }
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        dot_dot_dot,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                if let Some(expression) = expression {
                    if data.dot_dot_dot_token.is_none() {
                        self.emit_leading_comments_for_node_worker(
                            transformation,
                            expression,
                            LeadingCommentContext::Normal,
                            open_prefix,
                            writer,
                        )?;
                    }
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        expression.node(),
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                let close_anchor = expression
                    .map(|expression| {
                        self.original_node_end_cursor(transformation, expression)
                            .map(TokenAnchor::from)
                    })
                    .transpose()?
                    .unwrap_or_else(|| TokenAnchor::from(open));
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseBraceToken),
                    close_anchor,
                    false,
                    writer,
                )?;
                if multiline {
                    writer.decrease_indent();
                }
                Ok(())
            }
            NodeData::JsxNamespacedName(data) => {
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.namespace,
                    SyntaxKind::JsxNamespacedName,
                    "namespace",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation(":");
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::JsxNamespacedName,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::JsxText(data) => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::NoSubstitutionTemplateLiteral(_) if !changed => {
                self.write_original_without_leading_trivia_verbatim(transformation, node, writer)
            }
            NodeData::TemplateExpression(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.head,
                    SyntaxKind::TemplateExpression,
                    "head",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.template_spans,
                    "",
                    expression_context,
                    writer,
                )
            }
            NodeData::TemplateHead(data) => {
                writer.write_punctuation("`");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("${");
                Ok(())
            }
            NodeData::TemplateMiddle(data) => {
                writer.write_punctuation("}");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("${");
                Ok(())
            }
            NodeData::TemplateTail(data) => {
                writer.write_punctuation("}");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("`");
                Ok(())
            }
            NodeData::TemplateSpan(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                if let Some(expression) = expression {
                    // TemplateExpressionSpans suppress list-intervening
                    // comments, but the span and its first expression share
                    // the source boundary immediately after `${`. The normal
                    // expression comments phase owns both same-line trailing
                    // comments after the opener and later leading comments.
                    self.emit_leading_comments_for_delimited_list_start_with_space(
                        transformation,
                        expression,
                        TokenLeadingSpace::Required,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::TemplateSpan,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(expression) = expression {
                    self.emit_trailing_comments_for_node(transformation, expression, writer)?;
                }
                if let Some(literal) = data
                    .literal
                    .and_then(|literal| transformation.arena().node_ref(node.source(), literal))
                {
                    // Comments between the expression and `}` are ordinary
                    // leading comments of the template-middle/tail token.
                    self.emit_leading_comments_for_node(transformation, literal, writer)?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.literal,
                    SyntaxKind::TemplateSpan,
                    "literal",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::ImportDeclaration(data) => {
                let import_anchor =
                    self.token_after_modifiers_cursor(transformation, node, data.modifiers)?;
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                let import_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ImportKeyword),
                    import_anchor,
                    false,
                    writer,
                )?;
                let mut module_prefix = import_keyword;
                if let Some(clause_id) = data.import_clause {
                    let clause = transformation
                        .arena()
                        .node_ref(node.source(), clause_id)
                        .ok_or(PrinterError::UnknownStatement(clause_id.0))?;
                    writer.write_space(" ");
                    let prefix = self.token_owned_child_prefix(
                        transformation,
                        import_keyword,
                        Some(clause),
                    )?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        clause,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        clause_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    writer.write_space(" ");
                    module_prefix = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::keyword(SyntaxKind::FromKeyword),
                        self.original_node_end_cursor(transformation, clause)?,
                        false,
                        writer,
                    )?;
                }
                writer.write_space(" ");
                let module_id =
                    data.module_specifier
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::ImportDeclaration,
                            field: "module_specifier",
                        })?;
                let module = transformation
                    .arena()
                    .node_ref(node.source(), module_id)
                    .ok_or(PrinterError::UnknownStatement(module_id.0))?;
                let prefix =
                    self.token_owned_child_prefix(transformation, module_prefix, Some(module))?;
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    module,
                    LeadingCommentContext::Normal,
                    prefix,
                    writer,
                )?;
                self.emit_node_id_with_context(
                    transformation,
                    node.source(),
                    module_id,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let mut semicolon_owner = module;
                if let Some(attributes_id) = data.attributes {
                    let attributes = transformation
                        .arena()
                        .node_ref(node.source(), attributes_id)
                        .ok_or(PrinterError::UnknownStatement(attributes_id.0))?;
                    writer.write_space(" ");
                    self.emit_leading_comments_for_node(transformation, attributes, writer)?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        attributes_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    semicolon_owner = attributes;
                }
                self.emit_trailing_block_comments_before_semicolon(
                    transformation,
                    semicolon_owner,
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ImportClause(data) => {
                if data.is_type_only || data.phase_modifier == Some(SyntaxKind::TypeKeyword) {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                let mut phase = None;
                if let Some(phase_modifier) = data.phase_modifier {
                    if phase_modifier != SyntaxKind::DeferKeyword {
                        return Err(PrinterError::UnsupportedTransformedSyntax {
                            node,
                            kind: record.kind,
                        });
                    }
                    phase = Some(self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::keyword(phase_modifier),
                        self.original_node_start_cursor(transformation, node)?,
                        false,
                        writer,
                    )?);
                    writer.write_space(" ");
                }
                if let Some(name_id) = data.name {
                    let name = transformation
                        .arena()
                        .node_ref(node.source(), name_id)
                        .ok_or(PrinterError::UnknownStatement(name_id.0))?;
                    if let Some(phase) = phase {
                        let prefix =
                            self.token_owned_child_prefix(transformation, phase, Some(name))?;
                        self.emit_leading_comments_for_node_worker(
                            transformation,
                            name,
                            LeadingCommentContext::Normal,
                            prefix,
                            writer,
                        )?;
                    }
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        name_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    if data.named_bindings.is_some() {
                        let comma = self.emit_token_with_comments(
                            transformation,
                            node,
                            FixedToken::punctuation(SyntaxKind::CommaToken),
                            self.original_node_end_cursor(transformation, name)?,
                            false,
                            writer,
                        )?;
                        writer.write_space(" ");
                        if let Some(bindings_id) = data.named_bindings {
                            let bindings = transformation
                                .arena()
                                .node_ref(node.source(), bindings_id)
                                .ok_or(PrinterError::UnknownStatement(bindings_id.0))?;
                            let prefix = self.token_owned_child_prefix(
                                transformation,
                                comma,
                                Some(bindings),
                            )?;
                            self.emit_leading_comments_for_node_worker(
                                transformation,
                                bindings,
                                LeadingCommentContext::Normal,
                                prefix,
                                writer,
                            )?;
                        }
                    }
                } else if let (Some(phase), Some(bindings_id)) = (phase, data.named_bindings) {
                    let bindings = transformation
                        .arena()
                        .node_ref(node.source(), bindings_id)
                        .ok_or(PrinterError::UnknownStatement(bindings_id.0))?;
                    let prefix =
                        self.token_owned_child_prefix(transformation, phase, Some(bindings))?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        bindings,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                }
                if let Some(bindings) = data.named_bindings {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        bindings,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::NamespaceImport(data) => {
                let asterisk = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::AsteriskToken),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let as_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::AsKeyword),
                    asterisk,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name));
                let prefix = self.token_owned_child_prefix(transformation, as_keyword, name)?;
                if let Some(name) = name {
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        name,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::NamespaceImport,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::NamedImports(data) => self.emit_named_import_or_export_list(
                transformation,
                node,
                data.elements,
                expression_context,
                writer,
            ),
            NodeData::ImportSpecifier(data) => {
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                self.emit_renamed_specifier(
                    transformation,
                    node,
                    data.property_name,
                    data.name,
                    SyntaxKind::ImportSpecifier,
                    expression_context,
                    writer,
                )
            }
            NodeData::ExportDeclaration(data) => {
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                let export_anchor =
                    self.token_after_modifiers_cursor(transformation, node, data.modifiers)?;
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                let export_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ExportKeyword),
                    export_anchor,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let mut semicolon_owner = None;
                let from_anchor = if let Some(clause_id) = data.export_clause {
                    let clause = transformation
                        .arena()
                        .node_ref(node.source(), clause_id)
                        .ok_or(PrinterError::UnknownStatement(clause_id.0))?;
                    let prefix = self.token_owned_child_prefix(
                        transformation,
                        export_keyword,
                        Some(clause),
                    )?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        clause,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        clause_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    semicolon_owner = Some(clause);
                    TokenAnchor::from(self.original_node_end_cursor(transformation, clause)?)
                } else {
                    TokenAnchor::from(self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::punctuation(SyntaxKind::AsteriskToken),
                        export_keyword,
                        false,
                        writer,
                    )?)
                };
                if let Some(module_id) = data.module_specifier {
                    writer.write_space(" ");
                    let from_keyword = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::keyword(SyntaxKind::FromKeyword),
                        from_anchor,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    let module = transformation
                        .arena()
                        .node_ref(node.source(), module_id)
                        .ok_or(PrinterError::UnknownStatement(module_id.0))?;
                    let prefix =
                        self.token_owned_child_prefix(transformation, from_keyword, Some(module))?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        module,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        module_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    semicolon_owner = Some(module);
                }
                if let Some(attributes_id) = data.attributes {
                    let attributes = transformation
                        .arena()
                        .node_ref(node.source(), attributes_id)
                        .ok_or(PrinterError::UnknownStatement(attributes_id.0))?;
                    writer.write_space(" ");
                    self.emit_leading_comments_for_node(transformation, attributes, writer)?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        attributes_id,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    semicolon_owner = Some(attributes);
                }
                if let Some(semicolon_owner) = semicolon_owner {
                    self.emit_trailing_block_comments_before_semicolon(
                        transformation,
                        semicolon_owner,
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ImportAttributes(data) => {
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(data.token),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_delimited_expression_list(
                    transformation,
                    node,
                    data.elements,
                    "{",
                    "}",
                    data.multi_line == Some(true) || multi_line,
                    DelimitedListFormat::LITERAL,
                    ExpressionSyntaxContext::NORMAL,
                    expression_context,
                    writer,
                )
            }
            NodeData::ImportAttribute(data) => {
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::ImportAttribute,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation(":");
                writer.write_space(" ");
                if let Some(value) = data
                    .value
                    .and_then(|value| transformation.arena().node_ref(node.source(), value))
                {
                    let skipped =
                        self.emit_intervening_comments_before_node(transformation, value, writer)?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        value,
                        LeadingCommentContext::Normal,
                        skipped,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.value,
                    SyntaxKind::ImportAttribute,
                    "value",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::NamedExports(data) => self.emit_named_import_or_export_list(
                transformation,
                node,
                data.elements,
                expression_context,
                writer,
            ),
            NodeData::NamespaceExport(data) => {
                let asterisk = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::AsteriskToken),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let as_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::AsKeyword),
                    asterisk,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name));
                let prefix = self.token_owned_child_prefix(transformation, as_keyword, name)?;
                if let Some(name) = name {
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        name,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::NamespaceExport,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::ExportSpecifier(data) => {
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                self.emit_renamed_specifier(
                    transformation,
                    node,
                    data.property_name,
                    data.name,
                    SyntaxKind::ExportSpecifier,
                    expression_context,
                    writer,
                )
            }
            NodeData::ExportAssignment(data) => {
                let export_anchor =
                    self.token_after_modifiers_cursor(transformation, node, data.modifiers)?;
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                let export_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ExportKeyword),
                    export_anchor,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let assignment_token = if data.is_export_equals == Some(true) {
                    FixedToken::operator(SyntaxKind::EqualsToken)
                } else {
                    FixedToken::keyword(SyntaxKind::DefaultKeyword)
                };
                let assignment_token = self.emit_token_with_comments(
                    transformation,
                    node,
                    assignment_token,
                    export_keyword,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                // Parser recovery represents a missing expression with a
                // zero-width identifier, while factory consumers can supply
                // no child at all. Both forms still own a printable export
                // assignment; grammar diagnostics must not suppress emit.
                if let Some(expression_id) = data.expression {
                    let expression = transformation
                        .arena()
                        .node_ref(node.source(), expression_id)
                        .ok_or(PrinterError::UnknownStatement(expression_id.0))?;
                    let child_syntax = if data.is_export_equals == Some(true) {
                        ExpressionSyntaxContext::ASSIGNMENT_RIGHT_SIDE
                    } else {
                        ExpressionSyntaxContext::EXPORT_DEFAULT
                    };
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        assignment_token,
                        expression,
                        expression_context.for_child(child_syntax),
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::VariableStatement(data) => {
                // tsc keeps the statement's `containerEnd` active while its
                // synthetic declaration shells and initializer are emitted.
                // Thread that ownership explicitly so a ranged initializer
                // cannot claim the statement's trailing boundary first.
                let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
                let (pos, end) = Self::established_container_sides(owner);
                let declaration_context = expression_context
                    .with_comments(expression_context.comments().claim_sides(pos, end));
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    declaration_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                let declaration_list = data.declaration_list.and_then(|declaration_list| {
                    transformation
                        .arena()
                        .node_ref(node.source(), declaration_list)
                });
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.declaration_list,
                    SyntaxKind::VariableStatement,
                    "declaration_list",
                    declaration_context,
                    writer,
                )?;
                if let Some(declaration_list) = declaration_list {
                    self.emit_child_boundary_comments_before_terminator(
                        transformation,
                        node,
                        declaration_list,
                        declaration_context.comments(),
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::VariableDeclarationList(data) => {
                // A parsed list replaces the inherited container; a
                // synthesized list keeps the statement container alive. The
                // list is tsc's single `declarationListContainerEnd`
                // producer: its claimed end also arms the trailing dedupe.
                let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
                let (pos, end) = Self::established_container_sides(owner);
                let declaration_context = expression_context.with_comments(
                    expression_context
                        .comments()
                        .claim_declaration_list_sides(pos, end),
                );
                let flags = NodeFlags::from_bits(record.flags);
                if flags.contains(NodeFlags::AWAIT_USING) {
                    writer.write_keyword("await");
                    writer.write_space(" ");
                    writer.write_keyword("using");
                } else if flags.contains(NodeFlags::USING) {
                    writer.write_keyword("using");
                } else if flags.contains(NodeFlags::LET) {
                    writer.write_keyword("let");
                } else if flags.contains(NodeFlags::CONST) {
                    writer.write_keyword("const");
                } else {
                    writer.write_keyword("var");
                }
                writer.write_space(" ");
                let declarations = data
                    .declarations
                    .and_then(|declarations| {
                        transformation
                            .arena()
                            .node_array_ref(node.source(), declarations)
                    })
                    .map(|declarations| transformation.arena().node_array(declarations))
                    .transpose()?
                    .map(|declarations| declarations.nodes.clone())
                    .unwrap_or_default();
                for (index, declaration) in declarations.iter().copied().enumerate() {
                    if index != 0 {
                        writer.write(", ");
                    }
                    let declaration = transformation
                        .arena()
                        .node_ref(node.source(), declaration)
                        .ok_or(PrinterError::UnknownStatement(declaration.0))?;
                    // `var`/`let`/`const` is a textual list head in tsc, not
                    // a token-cursor anchor. The list item therefore owns the
                    // intervening comment boundary (including same-line
                    // comments immediately after the head or a comma).
                    self.emit_leading_comments_for_delimited_list_start_in_container(
                        transformation,
                        declaration,
                        declaration_context.comments(),
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        declaration.node(),
                        declaration_context,
                        writer,
                    )?;
                    if self.child_trailing_comments_escape_active_container(
                        transformation,
                        node,
                        declaration,
                        declaration_context.comments(),
                    )? {
                        self.emit_trailing_comments_for_node(transformation, declaration, writer)?;
                    }
                }
                Ok(())
            }
            NodeData::VariableDeclaration(data) => {
                // As above, only a declaration with its own source range
                // replaces the ambient variable-statement container.
                let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
                let (pos, end) = Self::established_container_sides(owner);
                let initializer_context = expression_context
                    .with_comments(expression_context.comments().claim_sides(pos, end));
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::VariableDeclaration,
                        field: "name",
                    })?;
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::VariableDeclaration,
                    "name",
                    initializer_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    let erased_type = transformation
                        .arena()
                        .metadata(name)
                        .and_then(crate::EmitMetadata::type_node);
                    if erased_type.is_some() {
                        // setTypeNode makes the declaration name retain
                        // comments before an erased annotation.
                        self.emit_trailing_comments_at_node_position(transformation, name, writer)?;
                    }
                    let equal_cursor = if let Some(r#type) = erased_type {
                        self.original_node_end_cursor(transformation, r#type)?
                    } else {
                        self.original_node_end_cursor(transformation, name)?
                    };
                    let equals = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::operator(SyntaxKind::EqualsToken),
                        equal_cursor,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    let initializer_node = transformation
                        .arena()
                        .node_ref(node.source(), initializer)
                        .ok_or(PrinterError::UnknownStatement(initializer.0))?;
                    self.emit_child_after_token_with_context(
                        transformation,
                        node,
                        equals,
                        initializer_node,
                        initializer_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ArrayLiteralExpression(data) if json_source => self
                .emit_json_delimited_expression_list(
                    transformation,
                    node,
                    data.elements,
                    "[",
                    "]",
                    multi_line,
                    true,
                    ExpressionSyntaxContext::DISALLOWED_COMMA,
                    expression_context,
                    writer,
                ),
            NodeData::ArrayLiteralExpression(data) => self.emit_delimited_expression_list(
                transformation,
                node,
                data.elements,
                "[",
                "]",
                multi_line,
                DelimitedListFormat::LITERAL,
                ExpressionSyntaxContext::DISALLOWED_COMMA,
                expression_context,
                writer,
            ),
            NodeData::ArrayBindingPattern(data) => self.emit_delimited_expression_list(
                transformation,
                node,
                data.elements,
                "[",
                "]",
                multi_line,
                DelimitedListFormat::BINDING_PATTERN,
                ExpressionSyntaxContext::NORMAL,
                expression_context,
                writer,
            ),
            NodeData::ObjectBindingPattern(data) => self.emit_delimited_expression_list(
                transformation,
                node,
                data.elements,
                "{",
                "}",
                multi_line,
                DelimitedListFormat::BINDING_PATTERN,
                ExpressionSyntaxContext::NORMAL,
                expression_context,
                writer,
            ),
            NodeData::BindingElement(data) => {
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::BindingElement,
                        field: "name",
                    })?;
                if let Some(dot_dot_dot) = data.dot_dot_dot_token {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        dot_dot_dot,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                if let Some(property_name) = data.property_name {
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        property_name,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    writer.write_punctuation(":");
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::BindingElement,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    let equals = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::operator(SyntaxKind::EqualsToken),
                        self.original_node_end_cursor(transformation, name)?,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    let initializer = transformation
                        .arena()
                        .node_ref(node.source(), initializer)
                        .ok_or(PrinterError::UnknownStatement(initializer.0))?;
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        equals,
                        initializer,
                        expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ComputedPropertyName(data) => {
                writer.write_punctuation("[");
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::ComputedPropertyName,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::COMPUTED_PROPERTY_NAME),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )?;
                writer.write_punctuation("]");
                Ok(())
            }
            NodeData::AwaitExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::AwaitExpression,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::AwaitKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::PREFIX_UNARY_OPERAND),
                    writer,
                )
            }
            NodeData::VoidExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::VoidExpression,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::VoidKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::PREFIX_UNARY_OPERAND),
                    writer,
                )
            }
            NodeData::DeleteExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::DeleteExpression,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::DeleteKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::PREFIX_UNARY_OPERAND),
                    writer,
                )
            }
            NodeData::TypeOfExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::TypeOfExpression,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::TypeOfKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::PREFIX_UNARY_OPERAND),
                    writer,
                )
            }
            NodeData::ConditionalExpression(data) => {
                let condition = data
                    .condition
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ConditionalExpression,
                        field: "condition",
                    })?;
                let question = data
                    .question_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ConditionalExpression,
                        field: "question_token",
                    })?;
                let when_true = data
                    .when_true
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ConditionalExpression,
                        field: "when_true",
                    })?;
                let colon = data
                    .colon_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ConditionalExpression,
                        field: "colon_token",
                    })?;
                let when_false = data
                    .when_false
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::ConditionalExpression,
                        field: "when_false",
                    })?;
                let lines_before_question = self.lines_between_optional_nodes(
                    transformation,
                    node,
                    Some(condition),
                    Some(question),
                )?;
                let lines_after_question = self.lines_between_optional_nodes(
                    transformation,
                    node,
                    Some(question),
                    Some(when_true),
                )?;
                let lines_before_colon = self.lines_between_optional_nodes(
                    transformation,
                    node,
                    Some(when_true),
                    Some(colon),
                )?;
                let lines_after_colon = self.lines_between_optional_nodes(
                    transformation,
                    node,
                    Some(colon),
                    Some(when_false),
                )?;
                self.emit_node_id_with_forwarded_source_comments(
                    transformation,
                    condition.source(),
                    condition.node(),
                    expression_context,
                    deferred_source_comments,
                    writer,
                )?;
                let condition_end = self.original_node_end_cursor(transformation, condition)?;
                let question_anchor = if let Some(anchor) =
                    deferred_source_comments.visited_trailing_anchor_at(condition_end)
                {
                    anchor
                } else {
                    self.separator_anchor_between_child_and_token(
                        transformation,
                        node,
                        condition,
                        question,
                        writer,
                    )?
                };
                Self::write_lines_and_indent(writer, lines_before_question, true);
                let question = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::QuestionToken),
                    question_anchor,
                    false,
                    writer,
                )?;
                Self::write_lines_and_indent(writer, lines_after_question, true);
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    question,
                    when_true,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let colon_anchor = self.separator_anchor_between_child_and_token(
                    transformation,
                    node,
                    when_true,
                    colon,
                    writer,
                )?;
                Self::decrease_indent_if(writer, lines_before_question, lines_after_question);
                Self::write_lines_and_indent(writer, lines_before_colon, true);
                let colon = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::ColonToken),
                    colon_anchor,
                    false,
                    writer,
                )?;
                Self::write_lines_and_indent(writer, lines_after_colon, true);
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    colon,
                    when_false,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                Self::decrease_indent_if(writer, lines_before_colon, lines_after_colon);
                Ok(())
            }
            NodeData::YieldExpression(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let yield_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::YieldKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                let operand_anchor = if data.asterisk_token.is_some() {
                    self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::operator(SyntaxKind::AsteriskToken),
                        yield_keyword,
                        false,
                        writer,
                    )?
                } else {
                    yield_keyword
                };
                if let Some(expression) = expression {
                    writer.write_space(" ");
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        operand_anchor,
                        expression,
                        expression_context.for_child(ExpressionSyntaxContext::YIELD_OPERAND),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ObjectLiteralExpression(data) if json_source => self
                .emit_json_delimited_expression_list(
                    transformation,
                    node,
                    data.properties,
                    "{",
                    "}",
                    multi_line,
                    false,
                    ExpressionSyntaxContext::NORMAL,
                    expression_context,
                    writer,
                ),
            NodeData::ObjectLiteralExpression(data) => {
                // tsc-port: emitObjectLiteralExpression @6.0.3 (the
                // Indented arm)
                // tsc-span: _tsc.js:118208-118222
                // `const indentedFlag = getEmitFlags(node) & Indented` —
                // the class arm above already ports the same protocol;
                // the sole object-literal producer is the (dormant)
                // ES2015 computed-name chunking, so the arm is
                // corpus-inert (B-4 packet §12.11; ratchet-enforced).
                let indented = transformation
                    .arena()
                    .metadata(node)
                    .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::INDENTED));
                if indented {
                    writer.increase_indent();
                }
                let outcome = self.emit_delimited_expression_list(
                    transformation,
                    node,
                    data.properties,
                    "{",
                    "}",
                    multi_line,
                    DelimitedListFormat::LITERAL,
                    ExpressionSyntaxContext::NORMAL,
                    expression_context,
                    writer,
                );
                if indented {
                    writer.decrease_indent();
                }
                outcome
            }
            NodeData::PropertyAssignment(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::PropertyAssignment,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation(":");
                writer.write_space(" ");
                let initializer =
                    data.initializer
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::PropertyAssignment,
                            field: "initializer",
                        })?;
                let initializer_node = transformation
                    .arena()
                    .node_ref(node.source(), initializer)
                    .ok_or(PrinterError::UnknownStatement(initializer.0))?;
                let skipped_prefix_bytes = self.emit_intervening_comments_before_node(
                    transformation,
                    initializer_node,
                    writer,
                )?;
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    initializer_node,
                    LeadingCommentContext::Normal,
                    skipped_prefix_bytes,
                    writer,
                )?;
                self.emit_node_id_with_context(
                    transformation,
                    node.source(),
                    initializer,
                    expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                    writer,
                )?;
                if self.child_trailing_comments_escape_active_container(
                    transformation,
                    node,
                    initializer_node,
                    expression_context.comments(),
                )? {
                    self.emit_trailing_comments_for_node(transformation, initializer_node, writer)?;
                }
                Ok(())
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::ShorthandPropertyAssignment,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.object_assignment_initializer {
                    writer.write_space(" ");
                    writer.write_operator("=");
                    writer.write_space(" ");
                    self.emit_required_node_with_context_and_source_extent(
                        transformation,
                        node.source(),
                        Some(initializer),
                        node,
                        SyntaxKind::ShorthandPropertyAssignment,
                        "object_assignment_initializer",
                        expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                        DeferredSourceCommentExtent::LeadingAndTrailing,
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::HeritageClause(data) => {
                let keyword = match data.token {
                    SyntaxKind::ExtendsKeyword => "extends",
                    SyntaxKind::ImplementsKeyword => "implements",
                    _ => {
                        return Err(PrinterError::UnsupportedTransformedSyntax {
                            node,
                            kind: record.kind,
                        });
                    }
                };
                // Upstream emitHeritageClause writes its own leading space
                // INSIDE the pipelined route (h2-6a-m-2: the clause's
                // Before map precedes the space); the class head no longer
                // writes it.
                writer.write_space(" ");
                writer.write_keyword(keyword);
                writer.write_space(" ");
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.types,
                    ", ",
                    expression_context,
                    writer,
                )
            }
            NodeData::ExpressionWithTypeArguments(data) => {
                // tsc emits every heritage expression through
                // parenthesizeLeftSideOfAccess. Earlier transforms may turn
                // an optional chain into a conditional expression; retaining
                // the left-hand-side grammar boundary preserves the meaning
                // of `extends (condition ? left : right)`.
                //
                // tsc-port: emitExpressionWithTypeArguments @6.0.3
                // tsc-span: _tsc.js:118536-118539
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::ExpressionWithTypeArguments,
                    "expression",
                    expression_context
                        .for_child(ExpressionSyntaxContext::left_side_of_access(false)),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )?;
                if data.type_arguments.is_some() {
                    writer.write_punctuation("<");
                    self.emit_node_array(
                        transformation,
                        node.source(),
                        data.type_arguments,
                        ", ",
                        expression_context,
                        writer,
                    )?;
                    writer.write_punctuation(">");
                }
                Ok(())
            }
            NodeData::JSDocAllType(_) => {
                writer.write_punctuation("*");
                Ok(())
            }
            NodeData::JSDocUnknownType(_) => {
                writer.write_punctuation("?");
                Ok(())
            }
            NodeData::JSDocNullableType(data) => {
                writer.write_punctuation("?");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.r#type,
                    SyntaxKind::JSDocNullableType,
                    "type",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::JSDocNonNullableType(data) => {
                writer.write_punctuation("!");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.r#type,
                    SyntaxKind::JSDocNonNullableType,
                    "type",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::JSDocOptionalType(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.r#type,
                    SyntaxKind::JSDocOptionalType,
                    "type",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_punctuation("=");
                Ok(())
            }
            NodeData::JSDocVariadicType(data) => {
                writer.write_punctuation("...");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.r#type,
                    SyntaxKind::JSDocVariadicType,
                    "type",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::SpreadAssignment(data) => {
                writer.write_punctuation("...");
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::SpreadAssignment,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )
            }
            NodeData::SpreadElement(data) => {
                writer.write_punctuation("...");
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.expression,
                    node,
                    SyntaxKind::SpreadElement,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )
            }
            NodeData::FunctionDeclaration(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("function");
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        asterisk,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                if let Some(name) = data.name {
                    writer.write_space(" ");
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        name,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                } else {
                    writer.write_space(" ");
                }
                self.emit_parameter_list(
                    transformation,
                    node.source(),
                    data.parameters,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::FunctionExpression(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("function");
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        asterisk,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                if let Some(name) = data.name {
                    writer.write_space(" ");
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        name,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                } else {
                    writer.write_space(" ");
                }
                // tsc-port: emitSignatureAndBody @6.0.3 (the Indented arm)
                // tsc-span: _tsc.js:118969-118982
                // `const indentedFlag = getEmitFlags(node) & Indented` —
                // brackets the signature and body one level deeper. The sole
                // producer is the ES2015 class lowering's function expression
                // (which inherits the flag class-fields stamps on the class
                // node, `_tsc.js:105203`), so the arm is byte-inert for every
                // target at or above ES2015 (no function node carries the
                // flag there; the ratchet is the enforcement).
                let indented = transformation
                    .arena()
                    .metadata(node)
                    .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::INDENTED));
                if indented {
                    writer.increase_indent();
                }
                self.emit_parameter_list(
                    transformation,
                    node.source(),
                    data.parameters,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                if indented {
                    writer.decrease_indent();
                }
                Ok(())
            }
            NodeData::ArrowFunction(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                self.emit_arrow_parameter_list(
                    transformation,
                    node,
                    &data,
                    expression_context,
                    writer,
                )?;
                writer.write_space(" ");
                // `emitArrowFunctionHead` emits the retained token node
                // itself. That node passes through the ordinary comments
                // pipeline: same-line trivia before `=>` is not a leading
                // comment, while a comment immediately after `=>` is the
                // token's trailing comment. Preserve that ownership and pass
                // its typed continuation to the concise body.
                let arrow = match data
                    .equals_greater_than_token
                    .and_then(|token| transformation.arena().node_ref(node.source(), token))
                {
                    Some(token) => self.emit_retained_arrow_token_with_comments(
                        transformation,
                        token,
                        expression_context,
                        writer,
                    )?,
                    None => {
                        writer.write_operator("=>");
                        TokenEmission::new(TokenCursor::Synthetic, None)
                    }
                };
                writer.write_space(" ");
                let body = data.body.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::ArrowFunction,
                    field: "body",
                })?;
                let body_node = transformation
                    .arena()
                    .node_ref(node.source(), body)
                    .ok_or(PrinterError::UnknownStatement(body.0))?;
                let concise = transformation.arena().node(body_node)?.kind != SyntaxKind::Block;
                if concise {
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        arrow,
                        body_node,
                        expression_context.for_child(ExpressionSyntaxContext::ARROW_CONCISE_BODY),
                        writer,
                    )
                } else {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )
                }
            }
            NodeData::Parameter(data) => {
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::Parameter,
                        field: "name",
                    })?;
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                if let Some(rest) = data.dot_dot_dot_token {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        rest,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::Parameter,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    let equal_cursor = transformation
                        .arena()
                        .metadata(name)
                        .and_then(crate::EmitMetadata::type_node)
                        .map(|r#type| self.original_node_end_cursor(transformation, r#type))
                        .transpose()?
                        .unwrap_or(self.original_node_end_cursor(transformation, name)?);
                    let equals = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::operator(SyntaxKind::EqualsToken),
                        equal_cursor,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    let initializer = transformation
                        .arena()
                        .node_ref(node.source(), initializer)
                        .ok_or(PrinterError::UnknownStatement(initializer.0))?;
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        equals,
                        initializer,
                        expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ClassDeclaration(data) => self.emit_class(
                transformation,
                node,
                node.source(),
                data.modifiers,
                data.name,
                data.heritage_clauses,
                data.members,
                false,
                expression_context,
                writer,
            ),
            NodeData::ClassExpression(data) => self.emit_class(
                transformation,
                node,
                node.source(),
                data.modifiers,
                data.name,
                data.heritage_clauses,
                data.members,
                true,
                expression_context,
                writer,
            ),
            NodeData::ClassStaticBlockDeclaration(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("static");
                writer.write_space(" ");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.body,
                    SyntaxKind::ClassStaticBlockDeclaration,
                    "body",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::PropertyDeclaration(data) => {
                let name = data
                    .name
                    .and_then(|name| transformation.arena().node_ref(node.source(), name))
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "name",
                    })?;
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::PropertyDeclaration,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    let equal_cursor = transformation
                        .arena()
                        .metadata(name)
                        .and_then(crate::EmitMetadata::type_node)
                        .map(|r#type| self.original_node_end_cursor(transformation, r#type))
                        .transpose()?
                        .unwrap_or(self.original_node_end_cursor(transformation, name)?);
                    let equals = self.emit_space_prefixed_token_with_comments(
                        transformation,
                        node,
                        FixedToken::operator(SyntaxKind::EqualsToken),
                        equal_cursor,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    let initializer = transformation
                        .arena()
                        .node_ref(node.source(), initializer)
                        .ok_or(PrinterError::UnknownStatement(initializer.0))?;
                    self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        equals,
                        initializer,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::Constructor(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("constructor");
                self.emit_signature_head(
                    transformation,
                    node.source(),
                    data.type_parameters,
                    data.parameters,
                    data.r#type,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::MethodDeclaration(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        asterisk,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::MethodDeclaration,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_signature_head(
                    transformation,
                    node.source(),
                    data.type_parameters,
                    data.parameters,
                    data.r#type,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::GetAccessor(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("get");
                writer.write_space(" ");
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::GetAccessor,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_signature_head(
                    transformation,
                    node.source(),
                    data.type_parameters,
                    data.parameters,
                    data.r#type,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::SetAccessor(data) => {
                if self.emit_modifiers(
                    transformation,
                    node.source(),
                    data.modifiers,
                    expression_context,
                    writer,
                )? {
                    writer.write_space(" ");
                }
                writer.write_keyword("set");
                writer.write_space(" ");
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::SetAccessor,
                    "name",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_signature_head(
                    transformation,
                    node.source(),
                    data.type_parameters,
                    data.parameters,
                    data.r#type,
                    expression_context,
                    writer,
                )?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        body,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ForStatement(data) => {
                let initializer = data.initializer.and_then(|initializer| {
                    transformation.arena().node_ref(node.source(), initializer)
                });
                let condition = data.condition.and_then(|condition| {
                    transformation.arena().node_ref(node.source(), condition)
                });
                let incrementor = data.incrementor.and_then(|incrementor| {
                    transformation.arena().node_ref(node.source(), incrementor)
                });
                let for_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ForKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    for_keyword,
                    false,
                    writer,
                )?;
                if let Some(initializer) = initializer {
                    self.emit_child_after_token_with_context(
                        transformation,
                        node,
                        open,
                        initializer,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                let first_semicolon_anchor = initializer
                    .map(|initializer| self.original_node_end_cursor(transformation, initializer))
                    .transpose()?
                    .map(TokenAnchor::from)
                    .unwrap_or_else(|| TokenAnchor::from(open));
                let first_semicolon = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::SemicolonToken),
                    first_semicolon_anchor,
                    false,
                    writer,
                )?;
                if let Some(condition) = condition {
                    writer.write_space(" ");
                    self.emit_child_after_token_with_context(
                        transformation,
                        node,
                        first_semicolon,
                        condition,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                let second_semicolon_anchor = condition
                    .map(|condition| self.original_node_end_cursor(transformation, condition))
                    .transpose()?
                    .map(TokenAnchor::from)
                    .unwrap_or_else(|| TokenAnchor::from(first_semicolon));
                let second_semicolon = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::SemicolonToken),
                    second_semicolon_anchor,
                    false,
                    writer,
                )?;
                if let Some(incrementor) = incrementor {
                    writer.write_space(" ");
                    self.emit_child_after_token_with_context(
                        transformation,
                        node,
                        second_semicolon,
                        incrementor,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                let close_anchor = incrementor
                    .map(|incrementor| self.original_node_end_cursor(transformation, incrementor))
                    .transpose()?
                    .map(TokenAnchor::from)
                    .unwrap_or_else(|| TokenAnchor::from(second_semicolon));
                let close = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    close_anchor,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    data.statement,
                    close,
                    expression_context,
                    writer,
                )
            }
            NodeData::ForInStatement(data) => {
                let initializer = data.initializer.and_then(|initializer| {
                    transformation.arena().node_ref(node.source(), initializer)
                });
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let for_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ForKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    for_keyword,
                    false,
                    writer,
                )?;
                let initializer = initializer.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::ForInStatement,
                    field: "initializer",
                })?;
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    open,
                    initializer,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let in_keyword = self.emit_for_binding_keyword_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::InKeyword),
                    self.original_node_end_cursor(transformation, initializer)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let expression = expression.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::ForInStatement,
                    field: "expression",
                })?;
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    in_keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    self.original_node_end_cursor(transformation, expression)?,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    data.statement,
                    close,
                    expression_context,
                    writer,
                )
            }
            NodeData::ForOfStatement(data) => {
                let initializer = data.initializer.and_then(|initializer| {
                    transformation.arena().node_ref(node.source(), initializer)
                });
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let for_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ForKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                if let Some(await_modifier) = data.await_modifier {
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        await_modifier,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    writer.write_space(" ");
                }
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    for_keyword,
                    false,
                    writer,
                )?;
                let initializer = initializer.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::ForOfStatement,
                    field: "initializer",
                })?;
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    open,
                    initializer,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let of_keyword = self.emit_for_binding_keyword_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::OfKeyword),
                    self.original_node_end_cursor(transformation, initializer)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let expression = expression.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::ForOfStatement,
                    field: "expression",
                })?;
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    of_keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    self.original_node_end_cursor(transformation, expression)?,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    data.statement,
                    close,
                    expression_context,
                    writer,
                )
            }
            NodeData::IfStatement(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let then_statement = data.then_statement.and_then(|statement| {
                    transformation.arena().node_ref(node.source(), statement)
                });
                let if_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::IfKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    if_keyword,
                    false,
                    writer,
                )?;
                let token_owned_prefix =
                    self.token_owned_child_prefix(transformation, open, expression)?;
                if let Some(expression) = expression {
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        expression,
                        LeadingCommentContext::Normal,
                        token_owned_prefix,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::IfStatement,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close_cursor = expression
                    .map(|expression| self.original_node_end_cursor(transformation, expression))
                    .transpose()?
                    .unwrap_or(open.cursor());
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    close_cursor,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement(
                    transformation,
                    node,
                    data.then_statement,
                    expression_context,
                    writer,
                )?;
                if let Some(else_statement) = data.else_statement {
                    let else_anchor = then_statement
                        .map(|then_statement| {
                            self.emit_trailing_comments_for_node_as_token_anchor(
                                transformation,
                                then_statement,
                                writer,
                            )
                        })
                        .transpose()?
                        .unwrap_or_else(|| TokenCursor::Synthetic.into());
                    writer.write_line(false);
                    self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::keyword(SyntaxKind::ElseKeyword),
                        else_anchor,
                        false,
                        writer,
                    )?;
                    let else_is_if = transformation
                        .arena()
                        .node_ref(node.source(), else_statement)
                        .is_some_and(|statement| {
                            transformation
                                .arena()
                                .node(statement)
                                .is_ok_and(|statement| statement.kind == SyntaxKind::IfStatement)
                        });
                    if else_is_if {
                        writer.write_space(" ");
                        self.emit_node_id_with_context(
                            transformation,
                            node.source(),
                            else_statement,
                            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                            writer,
                        )
                    } else {
                        self.emit_embedded_statement(
                            transformation,
                            node,
                            Some(else_statement),
                            expression_context,
                            writer,
                        )
                    }
                } else {
                    Ok(())
                }
            }
            NodeData::SwitchStatement(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let switch_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::SwitchKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    switch_keyword,
                    false,
                    writer,
                )?;
                let token_owned_prefix =
                    self.token_owned_child_prefix(transformation, open, expression)?;
                if let Some(expression) = expression {
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        expression,
                        LeadingCommentContext::Normal,
                        token_owned_prefix,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::SwitchStatement,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close_cursor = expression
                    .map(|expression| self.original_node_end_cursor(transformation, expression))
                    .transpose()?
                    .unwrap_or(open.cursor());
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    close_cursor,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.case_block,
                    SyntaxKind::SwitchStatement,
                    "case_block",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::CaseBlock(data) => self.emit_case_block(
                transformation,
                node,
                data.clauses,
                expression_context,
                writer,
            ),
            NodeData::CaseClause(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let case_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::CaseKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                let token_owned_prefix =
                    self.token_owned_child_prefix(transformation, case_keyword, expression)?;
                writer.write_space(" ");
                if let Some(expression) = expression {
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        expression,
                        LeadingCommentContext::Normal,
                        token_owned_prefix,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::CaseClause,
                    "expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let colon_cursor = expression
                    .map(|expression| self.original_node_end_cursor(transformation, expression))
                    .transpose()?
                    .unwrap_or(case_keyword.cursor());
                // emitCaseOrDefaultClauseRest decides the single-statement
                // polarity BEFORE the colon: the same-line arm is upstream
                // `writeToken` (the mapped token pipeline, h2-6a-m-3),
                // the list arm is `emitTokenWithComment` (unmapped).
                let single_line_statement = self.clause_single_statement_same_line(
                    transformation,
                    node,
                    node.source(),
                    data.statements,
                )?;
                let colon_map_default =
                    if single_line_statement && writer.has_source_map_recording() {
                        match colon_cursor.source_position() {
                            Some((cursor_source, position)) => self.token_map_range_at(
                                transformation,
                                cursor_source,
                                position.value(),
                                writer,
                            )?,
                            None => None,
                        }
                    } else {
                        None
                    };
                if single_line_statement {
                    self.record_token_map_side(
                        transformation,
                        MapBoundary::Before,
                        node,
                        SyntaxKind::ColonToken,
                        colon_map_default,
                        writer,
                    )?;
                }
                let colon = self.emit_list_boundary_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::ColonToken),
                    colon_cursor,
                    false,
                    writer,
                )?;
                if single_line_statement {
                    self.record_token_map_side(
                        transformation,
                        MapBoundary::After,
                        node,
                        SyntaxKind::ColonToken,
                        colon_map_default,
                        writer,
                    )?;
                }
                self.emit_case_clause_statements(
                    transformation,
                    node.source(),
                    data.statements,
                    colon,
                    single_line_statement,
                    expression_context,
                    writer,
                )
            }
            NodeData::DefaultClause(data) => {
                let default_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::DefaultKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                let single_line_statement = self.clause_single_statement_same_line(
                    transformation,
                    node,
                    node.source(),
                    data.statements,
                )?;
                let colon_map_default =
                    if single_line_statement && writer.has_source_map_recording() {
                        match default_keyword.cursor().source_position() {
                            Some((cursor_source, position)) => self.token_map_range_at(
                                transformation,
                                cursor_source,
                                position.value(),
                                writer,
                            )?,
                            None => None,
                        }
                    } else {
                        None
                    };
                if single_line_statement {
                    self.record_token_map_side(
                        transformation,
                        MapBoundary::Before,
                        node,
                        SyntaxKind::ColonToken,
                        colon_map_default,
                        writer,
                    )?;
                }
                let colon = self.emit_list_boundary_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::ColonToken),
                    default_keyword,
                    false,
                    writer,
                )?;
                if single_line_statement {
                    self.record_token_map_side(
                        transformation,
                        MapBoundary::After,
                        node,
                        SyntaxKind::ColonToken,
                        colon_map_default,
                        writer,
                    )?;
                }
                self.emit_case_clause_statements(
                    transformation,
                    node.source(),
                    data.statements,
                    colon,
                    single_line_statement,
                    expression_context,
                    writer,
                )
            }
            NodeData::BreakStatement(data) => {
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::BreakKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                if let Some(label) = data.label {
                    writer.write_space(" ");
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        label,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    if let Some(label) = transformation.arena().node_ref(node.source(), label) {
                        self.emit_jump_label_comments_before_terminator(
                            transformation,
                            node,
                            label,
                            writer,
                        )?;
                    }
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ContinueStatement(data) => {
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::ContinueKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                if let Some(label) = data.label {
                    writer.write_space(" ");
                    self.emit_identifier_name_with_context(
                        transformation,
                        node.source(),
                        label,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    if let Some(label) = transformation.arena().node_ref(node.source(), label) {
                        self.emit_jump_label_comments_before_terminator(
                            transformation,
                            node,
                            label,
                            writer,
                        )?;
                    }
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::LabeledStatement(data) => {
                let label = data
                    .label
                    .and_then(|label| transformation.arena().node_ref(node.source(), label));
                self.emit_required_identifier_name_with_context(
                    transformation,
                    node.source(),
                    data.label,
                    SyntaxKind::LabeledStatement,
                    "label",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let colon_cursor = label
                    .map(|label| self.original_node_end_cursor(transformation, label))
                    .transpose()?
                    .unwrap_or(TokenCursor::Synthetic);
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::ColonToken),
                    colon_cursor,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::LabeledStatement,
                    "statement",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::WithStatement(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::WithStatement,
                        field: "expression",
                    })?;
                let with_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::WithKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    with_keyword,
                    false,
                    writer,
                )?;
                self.emit_child_after_token_with_context(
                    transformation,
                    node,
                    open,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    self.original_node_end_cursor(transformation, expression)?,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    data.statement,
                    close,
                    expression_context,
                    writer,
                )
            }
            NodeData::EmptyStatement(_) => {
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::NotEmittedStatement(_) => Ok(()),
            NodeData::PartiallyEmittedExpression(data) => {
                let expression = data.expression.and_then(|expression| {
                    transformation.arena().node_ref(node.source(), expression)
                });
                let expression = expression.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::PartiallyEmittedExpression,
                    field: "expression",
                })?;
                self.emit_partially_emitted_boundary_comments(
                    transformation,
                    node,
                    expression,
                    true,
                    writer,
                )?;
                self.emit_node_id_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    expression.node(),
                    expression_context,
                    deferred_source_comments,
                    writer,
                )?;
                self.emit_partially_emitted_boundary_comments(
                    transformation,
                    node,
                    expression,
                    false,
                    writer,
                )
            }
            NodeData::NewExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| {
                        transformation.arena().node_ref(node.source(), expression)
                    })
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::NewExpression,
                        field: "expression",
                    })?;
                let keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::NewKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_child_after_token_with_complete_source_comments(
                    transformation,
                    node,
                    keyword,
                    expression,
                    expression_context.for_child(ExpressionSyntaxContext::NEW_CALLEE),
                    writer,
                )?;
                if data.arguments.is_some() {
                    writer.write_punctuation("(");
                    self.emit_call_arguments(
                        transformation,
                        node,
                        node.source(),
                        data.arguments,
                        multi_line,
                        expression_context,
                        writer,
                    )?;
                    writer.write_punctuation(")");
                }
                Ok(())
            }
            NodeData::WhileStatement(data) => {
                let close = self.emit_while_clause(
                    transformation,
                    node,
                    self.original_node_start_cursor(transformation, node)?
                        .into(),
                    data.expression,
                    expression_context,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    data.statement,
                    close,
                    expression_context,
                    writer,
                )
            }
            NodeData::DoStatement(data) => {
                let statement = data
                    .statement
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::DoStatement,
                        field: "statement",
                    })?;
                let statement_node = transformation
                    .arena()
                    .node_ref(node.source(), statement)
                    .ok_or(PrinterError::UnknownStatement(statement.0))?;
                let statement_is_block =
                    transformation.arena().node(statement_node)?.kind == SyntaxKind::Block;
                let do_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::DoKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                self.emit_embedded_statement_after_token(
                    transformation,
                    node,
                    Some(statement),
                    do_keyword,
                    expression_context,
                    writer,
                )?;
                // emitDoStatement 118654-118663: ordinary compiler
                // emit keeps a block and `while` on one line, while a
                // non-block body ends before the while clause unless
                // the DoStatement itself requests SingleLine.
                if statement_is_block {
                    writer.write_space(" ");
                } else {
                    self.write_line_or_space(transformation, node, writer);
                }
                self.emit_while_clause(
                    transformation,
                    node,
                    self.original_node_end_cursor(transformation, statement_node)?
                        .into(),
                    data.expression,
                    expression_context,
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::TryStatement(data) => {
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::TryKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                let try_block = data
                    .try_block
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::TryStatement,
                        field: "try_block",
                    })?;
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    Some(try_block),
                    SyntaxKind::TryStatement,
                    "try_block",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let try_block = transformation
                    .arena()
                    .node_ref(node.source(), try_block)
                    .ok_or(PrinterError::UnknownStatement(try_block.0))?;
                let mut preceding_clause = try_block;
                if let Some(catch_clause) = data.catch_clause {
                    // The block's trailing phase owns same-line comments;
                    // after the clause separator, CatchClause's leading phase
                    // owns comments on their own lines. This mirrors the two
                    // comment-pipeline passes around tsc's `emit(catchClause)`.
                    self.emit_trailing_comments_for_node(transformation, try_block, writer)?;
                    self.write_line_or_space(transformation, node, writer);
                    let catch_clause = transformation
                        .arena()
                        .node_ref(node.source(), catch_clause)
                        .ok_or(PrinterError::UnknownStatement(catch_clause.0))?;
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        catch_clause,
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        catch_clause.node(),
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    preceding_clause = catch_clause;
                }
                if let Some(finally_block) = data.finally_block {
                    // `finally` has no clause node whose leading phase could
                    // own this boundary. Carry the preceding clause's typed
                    // end cursor (and any already-emitted trailing comment)
                    // into the fixed token, as tsc's emitTokenWithComment does.
                    let finally_anchor = self.emit_trailing_comments_for_node_as_token_anchor(
                        transformation,
                        preceding_clause,
                        writer,
                    )?;
                    self.write_line_or_space(transformation, node, writer);
                    self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::keyword(SyntaxKind::FinallyKeyword),
                        finally_anchor,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        finally_block,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::CatchClause(data) => {
                let catch_keyword = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::keyword(SyntaxKind::CatchKeyword),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                writer.write_space(" ");
                if let Some(variable) = data.variable_declaration {
                    let variable_node = transformation
                        .arena()
                        .node_ref(node.source(), variable)
                        .ok_or(PrinterError::UnknownStatement(variable.0))?;
                    let open = self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::punctuation(SyntaxKind::OpenParenToken),
                        catch_keyword,
                        false,
                        writer,
                    )?;
                    let prefix =
                        self.token_owned_child_prefix(transformation, open, Some(variable_node))?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        variable_node,
                        LeadingCommentContext::Normal,
                        prefix,
                        writer,
                    )?;
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        variable,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    self.emit_token_with_comments(
                        transformation,
                        node,
                        FixedToken::punctuation(SyntaxKind::CloseParenToken),
                        self.original_node_end_cursor(transformation, variable_node)?,
                        false,
                        writer,
                    )?;
                    writer.write_space(" ");
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.block,
                    SyntaxKind::CatchClause,
                    "block",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::CallExpression(data) => {
                let call_is_optional_chain = NodeFlags::from_bits(record.flags)
                    .contains(NodeFlags::OPTIONAL_CHAIN)
                    || data.question_dot_token.is_some();
                let callee_grammar = if expression_context.grammar()
                    == ExpressionGrammarContext::ExpressionStatement
                {
                    ExpressionGrammarContext::ExpressionStatement
                } else {
                    ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: call_is_optional_chain,
                    }
                };
                let callee_context = expression_context.with_grammar(callee_grammar);
                self.emit_required_node_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::CallExpression,
                    "expression",
                    callee_context,
                    deferred_source_comments,
                    writer,
                )?;
                if data.question_dot_token.is_some() {
                    writer.write_punctuation("?.");
                }
                writer.write_punctuation("(");
                self.emit_call_arguments(
                    transformation,
                    node,
                    node.source(),
                    data.arguments,
                    multi_line,
                    expression_context,
                    writer,
                )?;
                writer.write_punctuation(")");
                Ok(())
            }
            NodeData::TaggedTemplateExpression(data) => {
                if data.type_arguments.is_some() {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                // `a?.\`text\`` is a grammar error, but the parser retains
                // the question-dot token on the tagged-template node so the
                // checker can report it. It is not an emit-time optional-chain
                // segment: tsc's tagged-template emitter deliberately ignores
                // that recovery-only field and prints the ordinary tag. When
                // the tag itself is an optional chain, the ES2020 transform
                // still lowers that child before it reaches this worker.
                //
                // tsc-port: emitTaggedTemplateExpression @6.0.3
                // tsc-span: _tsc.js:118298-118311
                self.emit_required_node_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    data.tag,
                    SyntaxKind::TaggedTemplateExpression,
                    "tag",
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: false,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.template,
                    SyntaxKind::TaggedTemplateExpression,
                    "template",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )
            }
            NodeData::ParenthesizedExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let has_source_parentheses =
                    self.node_has_source_token_shape(transformation, node)?;
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    self.original_node_start_cursor(transformation, node)?,
                    false,
                    writer,
                )?;
                if let Some(expression) = expression.filter(|_| has_source_parentheses) {
                    let token_owned_prefix =
                        self.token_owned_child_prefix(transformation, open, Some(expression))?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        expression,
                        LeadingCommentContext::Normal,
                        token_owned_prefix,
                        writer,
                    )?;
                }
                if has_source_parentheses {
                    self.emit_required_node_with_context(
                        transformation,
                        node.source(),
                        data.expression,
                        SyntaxKind::ParenthesizedExpression,
                        "expression",
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                } else {
                    // A precedence parenthesis created by a transform has no
                    // source close-token to run the ordinary comments phase.
                    // Complete the retained child against the ambient source
                    // container before writing `)`. In particular, a parsed
                    // outer statement keeps ownership of its final trailing
                    // boundary through any number of synthetic wrappers.
                    self.emit_required_node_with_context_and_source_extent(
                        transformation,
                        node.source(),
                        data.expression,
                        node,
                        SyntaxKind::ParenthesizedExpression,
                        "expression",
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        DeferredSourceCommentExtent::LeadingAndTrailing,
                        writer,
                    )?;
                }
                let close_cursor = expression
                    .map(|expression| self.original_node_end_cursor(transformation, expression))
                    .transpose()?
                    .unwrap_or(open.cursor());
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    close_cursor,
                    false,
                    writer,
                )?;
                Ok(())
            }
            NodeData::PrefixUnaryExpression(data) => {
                let operator = tsc_syntax::tokens::token_to_string(data.operator).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write_operator(operator);
                let nested_operator = data
                    .operand
                    .and_then(|operand| transformation.arena().node_ref(node.source(), operand))
                    .map(|operand| transformation.arena().node(operand))
                    .transpose()?
                    .and_then(|operand| match &operand.data {
                        NodeData::PrefixUnaryExpression(data) => Some(data.operator),
                        _ => None,
                    });
                let needs_lexical_separator = matches!(
                    (data.operator, nested_operator),
                    (
                        SyntaxKind::PlusToken,
                        Some(SyntaxKind::PlusToken | SyntaxKind::PlusPlusToken)
                    ) | (
                        SyntaxKind::MinusToken,
                        Some(SyntaxKind::MinusToken | SyntaxKind::MinusMinusToken)
                    )
                );
                if needs_lexical_separator {
                    writer.write_space(" ");
                }
                self.emit_required_node_with_context_and_source_extent(
                    transformation,
                    node.source(),
                    data.operand,
                    node,
                    SyntaxKind::PrefixUnaryExpression,
                    "operand",
                    expression_context.for_child(ExpressionSyntaxContext::PREFIX_UNARY_OPERAND),
                    DeferredSourceCommentExtent::LeadingAndTrailing,
                    writer,
                )
            }
            NodeData::PostfixUnaryExpression(data) => {
                let operand_context =
                    expression_context.with_grammar(ExpressionGrammarContext::PostfixUnaryOperand);
                if deferred_source_comments.is_pending() {
                    self.emit_required_node_with_forwarded_source_comments(
                        transformation,
                        node.source(),
                        data.operand,
                        SyntaxKind::PostfixUnaryExpression,
                        "operand",
                        operand_context,
                        deferred_source_comments,
                        writer,
                    )?;
                } else {
                    self.emit_required_node_with_context_and_source_extent(
                        transformation,
                        node.source(),
                        data.operand,
                        node,
                        SyntaxKind::PostfixUnaryExpression,
                        "operand",
                        operand_context,
                        DeferredSourceCommentExtent::LeadingAndTrailing,
                        writer,
                    )?;
                }
                let operator = tsc_syntax::tokens::token_to_string(data.operator).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write_operator(operator);
                Ok(())
            }
            NodeData::PropertyAccessExpression(data) => {
                let access_is_optional_chain = NodeFlags::from_bits(record.flags)
                    .contains(NodeFlags::OPTIONAL_CHAIN)
                    || data.question_dot_token.is_some();
                let expression_id =
                    data.expression
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::PropertyAccessExpression,
                            field: "expression",
                        })?;
                let name_id = data.name.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::PropertyAccessExpression,
                    field: "name",
                })?;
                let expression = transformation
                    .arena()
                    .node_ref(node.source(), expression_id)
                    .ok_or(PrinterError::UnknownStatement(expression_id.0))?;
                let name = transformation
                    .arena()
                    .node_ref(node.source(), name_id)
                    .ok_or(PrinterError::UnknownStatement(name_id.0))?;
                self.emit_node_id_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    expression_id,
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let token_kind = if data.question_dot_token.is_some() {
                    SyntaxKind::QuestionDotToken
                } else {
                    SyntaxKind::DotToken
                };
                let token_cursor = self.original_node_end_cursor(transformation, expression)?;
                // getLinesBetweenNodes only consults source lines when the
                // parent and both children carry source positions. A class
                // field access receives the member-name range for mapping and
                // comment containment, but its generated receiver remains
                // synthetic, so that range alone must not move `.field` onto
                // a source-derived line.
                let preserve_source_lines = self
                    .node_has_source_text_range(transformation, node)?
                    && self.node_has_source_text_range(transformation, expression)?
                    && self.node_has_source_text_range(transformation, name)?;
                let break_before_dot = preserve_source_lines
                    && self.source_gap_has_line_break(
                        transformation,
                        node.source(),
                        expression_id,
                        name_id,
                    )?;
                let token_anchor = if let Some(anchor) =
                    deferred_source_comments.visited_trailing_anchor_at(token_cursor)
                {
                    anchor
                } else if break_before_dot {
                    self.emit_trailing_comments_for_node_as_token_anchor(
                        transformation,
                        expression,
                        writer,
                    )?
                } else {
                    TokenAnchor::from(token_cursor)
                };
                if break_before_dot {
                    // The nested expression's comments phase owns same-line
                    // trailing comments before getLinesBetweenNodes opens the
                    // indentation scope for the access token. The typed
                    // anchor carries that ownership into token emission so
                    // the dot cannot claim the comment a second time.
                    writer.write_line(false);
                    writer.increase_indent();
                }
                if data.question_dot_token.is_none()
                    && self.may_need_dot_dot_for_property_access(transformation, expression)?
                    && !writer.has_trailing_comment()
                    && !writer.has_trailing_whitespace()
                {
                    writer.write_punctuation(".");
                }
                let token = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(token_kind),
                    token_anchor,
                    false,
                    writer,
                )?;
                let token_owned_prefix =
                    self.token_owned_child_prefix(transformation, token, Some(name))?;
                let container_owned_prefix =
                    self.parent_comment_container_owned_prefix(transformation, node, name)?;
                let break_after_dot = preserve_source_lines
                    && self.source_node_leading_trivia_has_line_break(
                        transformation,
                        node.source(),
                        name_id,
                    )?;
                if break_after_dot {
                    // The token's same-line trailing comments stay in the
                    // outer scope; only the name's leading boundary opens the
                    // nested lines-after-token scope.
                    writer.write_line(false);
                    writer.increase_indent();
                }
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    name,
                    LeadingCommentContext::Normal,
                    Self::furthest_comment_resume(token_owned_prefix, container_owned_prefix)?,
                    writer,
                )?;
                self.emit_identifier_name_with_context(
                    transformation,
                    node.source(),
                    name_id,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                if break_after_dot {
                    writer.decrease_indent();
                }
                if break_before_dot {
                    writer.decrease_indent();
                }
                Ok(())
            }
            NodeData::ElementAccessExpression(data) => {
                let access_is_optional_chain = NodeFlags::from_bits(record.flags)
                    .contains(NodeFlags::OPTIONAL_CHAIN)
                    || data.question_dot_token.is_some();
                let expression = data
                    .expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let argument = data
                    .argument_expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                self.emit_required_node_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ElementAccessExpression,
                    "expression",
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let open_cursor = if let Some(question_dot) = data
                    .question_dot_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                {
                    writer.write_punctuation("?.");
                    self.original_node_end_cursor(transformation, question_dot)?
                } else {
                    expression
                        .map(|expression| self.original_node_end_cursor(transformation, expression))
                        .transpose()?
                        .unwrap_or(TokenCursor::Synthetic)
                };
                let open_anchor = deferred_source_comments
                    .visited_trailing_anchor_at(open_cursor)
                    .unwrap_or_else(|| TokenAnchor::from(open_cursor));
                let open = self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::OpenBracketToken),
                    open_anchor,
                    false,
                    writer,
                )?;
                if let Some(argument) = argument {
                    let token_owned_prefix =
                        self.token_owned_child_prefix(transformation, open, Some(argument))?;
                    let container_owned_prefix =
                        self.parent_comment_container_owned_prefix(transformation, node, argument)?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        argument,
                        LeadingCommentContext::Normal,
                        Self::furthest_comment_resume(token_owned_prefix, container_owned_prefix)?,
                        writer,
                    )?;
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.argument_expression,
                    SyntaxKind::ElementAccessExpression,
                    "argument_expression",
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                let close_cursor = argument
                    .map(|argument| self.original_node_end_cursor(transformation, argument))
                    .transpose()?
                    .unwrap_or(open.cursor());
                self.emit_token_with_comments(
                    transformation,
                    node,
                    FixedToken::punctuation(SyntaxKind::CloseBracketToken),
                    close_cursor,
                    false,
                    writer,
                )?;
                Ok(())
            }
            NodeData::BinaryExpression(data) => {
                let left_node = data
                    .left
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let operator_node = data
                    .operator_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let right_node = data
                    .right
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let line_before_operator = match (left_node, operator_node) {
                    (Some(left), Some(operator)) => !self
                        .source_node_end_and_node_start_are_on_same_line(
                            transformation,
                            left,
                            operator,
                        )?,
                    _ => false,
                };
                let line_after_operator = match (operator_node, right_node) {
                    (Some(operator), Some(right)) => {
                        transformation
                            .arena()
                            .metadata(right)
                            .and_then(|metadata| metadata.starts_on_new_line())
                            == Some(true)
                            || !self.source_node_end_and_node_start_are_on_same_line(
                                transformation,
                                operator,
                                right,
                            )?
                    }
                    _ => false,
                };
                let operator_kind = operator_node
                    .map(|operator| transformation.arena().node(operator))
                    .transpose()?
                    .map(|operator| operator.kind)
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::BinaryExpression,
                        field: "operator_token",
                    })?;
                self.emit_required_node_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    data.left,
                    SyntaxKind::BinaryExpression,
                    "left",
                    expression_context,
                    deferred_source_comments,
                    writer,
                )?;
                let operator_anchor = if let Some(left) = left_node {
                    let cursor = self.original_node_end_cursor(transformation, left)?;
                    if let Some(anchor) =
                        deferred_source_comments.visited_trailing_anchor_at(cursor)
                    {
                        anchor
                    } else {
                        self.separator_anchor_after_child(transformation, node, left, writer)?
                    }
                } else {
                    TokenAnchor::from(TokenCursor::Synthetic)
                };
                if line_before_operator {
                    writer.write_line(false);
                    writer.increase_indent();
                } else if operator_kind != SyntaxKind::CommaToken {
                    writer.write_space(" ");
                }
                let operator_token = if operator_kind == SyntaxKind::InKeyword {
                    FixedToken::keyword(operator_kind)
                } else {
                    FixedToken::operator(operator_kind)
                };
                let operator = self.emit_token_with_comments(
                    transformation,
                    node,
                    operator_token,
                    operator_anchor,
                    false,
                    writer,
                )?;
                if line_after_operator {
                    writer.write_line(false);
                    writer.increase_indent();
                } else {
                    writer.write_space(" ");
                }
                let result = match right_node {
                    Some(right) => self.emit_child_after_token_with_complete_source_comments(
                        transformation,
                        node,
                        operator,
                        right,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    ),
                    None => Err(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::BinaryExpression,
                        field: "right",
                    }),
                };
                if line_after_operator {
                    writer.decrease_indent();
                }
                if line_before_operator {
                    writer.decrease_indent();
                }
                result
            }
            NodeData::Block(data) => {
                let function_body = self.is_function_body_block(transformation, node)?;
                // h2-6a-m-2 §4 route table: block braces map (upstream
                // emitBlockStatements emitTokenWithComment pairs), EXCEPT
                // the function-body open brace (upstream
                // emitBlockFunctionBody writes it via bare
                // writePunctuation — never mapped, not even by override).
                let block_source_start = {
                    let record = transformation.arena().node(node)?;
                    let positions = transformation
                        .arena()
                        .source(node.source())?
                        .syntax()
                        .positions();
                    match SourceRange::from_raw(record.pos, record.end, positions) {
                        Ok(SourceRange::Original(range)) => Some(range.start().value()),
                        _ => None,
                    }
                };
                if function_body {
                    writer.write_punctuation("{");
                } else {
                    let open_default = match block_source_start {
                        Some(start) => {
                            self.token_map_range_at(transformation, node.source(), start, writer)?
                        }
                        None => None,
                    };
                    self.record_brace_write(
                        transformation,
                        node,
                        SyntaxKind::OpenBraceToken,
                        open_default,
                        "{",
                        |writer, spelling| writer.write_punctuation(spelling),
                        writer,
                    )?;
                }
                // tsc-port: shouldEmitBlockFunctionBodyOnSingleLine @6.0.3
                // tsc-hash: f1644748bb2314796a601992b80c26925e740f7bd10b23e23300df3614367b1a
                // tsc-span: _tsc.js:118999-119020
                let force_single_line = transformation
                    .arena()
                    .metadata(node)
                    .is_some_and(|metadata| metadata.flags().contains(EmitFlags::SINGLE_LINE));
                let array = data
                    .statements
                    .and_then(|id| transformation.arena().node_array_ref(node.source(), id));
                let (statements, statement_list_end) = if let Some(array) = array {
                    let array = transformation.arena().node_array(array)?;
                    (
                        array.nodes.clone(),
                        (array.end != u32::MAX).then_some(array.end as usize),
                    )
                } else {
                    (Vec::new(), None)
                };
                let relocated_statement_list_comments = transformation
                    .arena()
                    .metadata(node)
                    .and_then(crate::EmitMetadata::relocated_statement_list_comments);
                // emitBlockFunctionBody emits directive prologues before it
                // decides whether the remaining statements can use the
                // compact list form. Consequently even a source-inline
                // directive (`() => { '' }`) owns a multi-line function body.
                // Synthetic concise-arrow blocks without a directive remain
                // eligible for tsc's single-line form.
                let function_body_has_prologue = function_body
                    && statements.first().is_some_and(|statement| {
                        transformation
                            .arena()
                            .node_ref(node.source(), *statement)
                            .is_some_and(|statement| {
                                self.is_prologue_statement(transformation, statement)
                            })
                    });
                let detached_body_prefix = if function_body
                    && !statements.is_empty()
                    && !transformation
                        .arena()
                        .metadata(node)
                        .is_some_and(|metadata| {
                            metadata.flags().contains(EmitFlags::NO_LEADING_COMMENTS)
                        }) {
                    if let Some(relocated) = relocated_statement_list_comments {
                        self.detached_source_file_prefix_for_relocated_statement_list(
                            transformation,
                            relocated.original(),
                        )?
                    } else {
                        array
                            .map(|array| {
                                self.detached_comment_prefix_for_node_array(transformation, array)
                            })
                            .transpose()?
                            .flatten()
                    }
                } else {
                    None
                };
                // A relocated module body shares the original prefix boundary
                // only as a resume seed. Its outer SourceFile list retained the
                // parsed range and already emitted the prefix.
                let body_owned_detached_prefix = relocated_statement_list_comments
                    .is_none()
                    .then_some(detached_body_prefix)
                    .flatten();
                let multi_line = !force_single_line
                    && (multi_line
                        || function_body_has_prologue
                        || function_body
                            && !self.source_node_range_is_on_single_line(transformation, node)?);
                // tsc-port: emitBlock/emitBlockStatements @6.0.3
                // tsc-hash: 9c296db81136b7d3b5fb7f0e5d47f750926728a1146ec273677021fd6249e90a
                // tsc-span: _tsc.js:118579-118601
                //
                // A regular non-empty block always uses the multi-line list
                // format. Function bodies have their own single-line
                // eligibility rules and retain the parser/factory decision.
                let multi_line = (if force_single_line {
                    false
                } else if function_body {
                    multi_line
                } else {
                    multi_line || !statements.is_empty()
                }) || body_owned_detached_prefix.is_some();
                if statements.is_empty() {
                    let emitted_comments = self.emit_empty_block_comments(
                        transformation,
                        node,
                        array,
                        multi_line,
                        function_body,
                        writer,
                    )?;
                    if !emitted_comments && multi_line {
                        writer.write_line(false);
                    } else if !emitted_comments {
                        writer.write_space(" ");
                    }
                } else if !multi_line {
                    if !function_body {
                        self.emit_comment_after_open_brace(transformation, node, writer)?;
                    }
                    writer.write_space(" ");
                    let mut forced_line_indent = false;
                    for (index, statement) in statements.into_iter().enumerate() {
                        let statement_node = transformation
                            .arena()
                            .node_ref(node.source(), statement)
                            .ok_or(PrinterError::UnknownStatement(statement.0))?;
                        let starts_on_new_line = transformation
                            .arena()
                            .metadata(statement_node)
                            .and_then(crate::EmitMetadata::starts_on_new_line)
                            == Some(true);
                        // tsc applies `startsOnNewLine` as a separator between
                        // synthesized siblings. It does not turn the first
                        // statement of an otherwise single-line block into a
                        // multi-line block.
                        if index != 0 && starts_on_new_line {
                            writer.write_line(false);
                            if !forced_line_indent {
                                writer.increase_indent();
                                forced_line_indent = true;
                            }
                        } else if index != 0 {
                            writer.write_space(" ");
                        }
                        // tsc's `SingleLineFunctionBodyStatements` list phase
                        // owns the boundary immediately after the opening
                        // brace. Later same-line boundaries are already owned
                        // by the preceding statement's trailing phase; asking
                        // both phases to visit them would duplicate a comment.
                        // The synthetic function shell must not replace the
                        // first retained statement's source provenance, but
                        // it must supply this one missing list phase.
                        if function_body && index == 0 {
                            self.emit_leading_comments_for_delimited_list_start(
                                transformation,
                                statement_node,
                                writer,
                            )?;
                        }
                        self.emit_node_id_with_context(
                            transformation,
                            node.source(),
                            statement,
                            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                            writer,
                        )?;
                        self.emit_trailing_comments_for_node(
                            transformation,
                            statement_node,
                            writer,
                        )?;
                    }
                    if forced_line_indent {
                        writer.decrease_indent();
                    }
                    writer.write_space(" ");
                } else {
                    if !function_body {
                        self.emit_comment_after_open_brace(transformation, node, writer)?;
                    }
                    writer.write_line(false);
                    writer.increase_indent();
                    self.emit_detached_comment_prefix(
                        transformation,
                        body_owned_detached_prefix,
                        writer,
                    )?;
                    let mut pending_detached_comments =
                        PendingDetachedComments::from_prefix(detached_body_prefix);
                    let mut has_previous_original_statement = false;
                    for statement in statements {
                        let statement = transformation
                            .arena()
                            .node_ref(node.source(), statement)
                            .ok_or(PrinterError::UnknownStatement(statement.0))?;
                        let original = transformation.arena().get_original_node(statement);
                        let original_source =
                            transformation.arena().source(original.source())?.syntax();
                        let original_record = transformation.arena().node(original)?;
                        let has_original_range = matches!(
                            SourceRange::from_raw(
                                original_record.pos,
                                original_record.end,
                                original_source.positions(),
                            )?,
                            SourceRange::Original(_)
                        );
                        let detached_resume = self.take_detached_comment_resume_for_node(
                            transformation,
                            &mut pending_detached_comments,
                            statement,
                        )?;
                        if expression_context.nested_comments_suppressed() {
                            // shouldEmitComments is false for the whole
                            // subtree of a NoNestedComments owner.
                        } else if let Some(detached_resume) = detached_resume {
                            self.emit_leading_comments_for_node_worker(
                                transformation,
                                statement,
                                if has_previous_original_statement && has_original_range {
                                    LeadingCommentContext::AfterSibling
                                } else {
                                    LeadingCommentContext::Normal
                                },
                                Some(detached_resume),
                                writer,
                            )?;
                        } else if has_previous_original_statement && has_original_range {
                            self.emit_leading_comments_for_node_after_sibling(
                                transformation,
                                statement,
                                writer,
                            )?;
                        } else {
                            // H2.5h CA-2a B(i) postscript: tsc's `containerPos`
                            // is LINEAR printer state (the last node emitted
                            // with a source position, `_tsc.js:121012-121022`),
                            // not an ancestor-scoped claim — an ancestor-claim
                            // consultation here suppressed real comments when a
                            // preceding ranged sibling should have re-claimed
                            // (System-module bodies, h2-5g case 4119). The
                            // wrapper dup family is the named residual
                            // h2-5h-ca-2a-r5 pending the faithful linear model.
                            self.emit_leading_comments_for_node(transformation, statement, writer)?;
                        }
                        if has_original_range {
                            has_previous_original_statement = true;
                        }
                        self.emit_node_id_with_context(
                            transformation,
                            node.source(),
                            statement.node(),
                            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                            writer,
                        )?;
                        if !expression_context.nested_comments_suppressed() {
                            self.emit_trailing_comments_for_node(
                                transformation,
                                statement,
                                writer,
                            )?;
                        }
                        writer.write_line(false);
                    }
                    self.emit_comments_before_close_brace(
                        transformation,
                        node,
                        statement_list_end,
                        writer,
                    )?;
                    writer.decrease_indent();
                }
                // Close brace maps for every block kind (upstream
                // emitBlockStatements 118596 / emitBlockFunctionBody
                // writeToken 119030), anchored at the statement-list end.
                let close_default = match statement_list_end {
                    Some(end) => match u32::try_from(end) {
                        Ok(end) => {
                            self.token_map_range_at(transformation, node.source(), end, writer)?
                        }
                        Err(_) => None,
                    },
                    None => None,
                };
                self.record_brace_write(
                    transformation,
                    node,
                    SyntaxKind::CloseBraceToken,
                    close_default,
                    "}",
                    |writer, spelling| writer.write_punctuation(spelling),
                    writer,
                )?;
                Ok(())
            }
            _ if !changed => {
                self.write_original_without_leading_trivia(transformation, node, writer)
            }
            _ => Err(PrinterError::UnsupportedTransformedSyntax {
                node,
                kind: record.kind,
            }),
        }
    }

    fn is_prologue_statement(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
    ) -> bool {
        let Ok(record) = transformation.arena().node(statement) else {
            return false;
        };
        let NodeData::ExpressionStatement(data) = &record.data else {
            return false;
        };
        data.expression
            .and_then(|expression| {
                transformation
                    .arena()
                    .node_ref(statement.source(), expression)
            })
            .and_then(|expression| transformation.arena().node(expression).ok())
            .is_some_and(|expression| matches!(expression.data, NodeData::StringLiteral(_)))
    }

    fn emit_helpers(
        &self,
        helpers: &[EmitHelper],
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        for helper in helpers {
            let text = helper
                .text()
                .ok_or_else(|| PrinterError::EmitHelperTextUnavailable(helper.name().into()))?;
            for line in text.lines() {
                writer.write(line);
                writer.write_line(false);
            }
        }
        Ok(())
    }

    fn emit_case_block(
        &self,
        transformation: &mut TransformationResult<'_>,
        case_block: TransformNode,
        clauses: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = case_block.source();
        // h2-6a-m-2 §4 route table: case-block braces map (upstream
        // emitCaseBlock emitTokenWithComment pairs 119176/119178),
        // open anchored at the case-block start, close at the clause
        // NodeArray end.
        let case_block_start = {
            let record = transformation.arena().node(case_block)?;
            let positions = transformation.arena().source(source)?.syntax().positions();
            match SourceRange::from_raw(record.pos, record.end, positions) {
                Ok(SourceRange::Original(range)) => Some(range.start().value()),
                _ => None,
            }
        };
        let open_default = match case_block_start {
            Some(start) => self.token_map_range_at(transformation, source, start, writer)?,
            None => None,
        };
        self.record_brace_write(
            transformation,
            case_block,
            SyntaxKind::OpenBraceToken,
            open_default,
            "{",
            |writer, spelling| writer.write_punctuation(spelling),
            writer,
        )?;
        let (clauses, clause_list_end) = if let Some(array) =
            clauses.and_then(|array| transformation.arena().node_array_ref(source, array))
        {
            let array = transformation.arena().node_array(array)?;
            (
                array.nodes.clone(),
                (array.end != u32::MAX).then_some(array.end as usize),
            )
        } else {
            (Vec::new(), None)
        };
        if clauses.is_empty() {
            // tsc's CaseBlockClauses list format is MultiLine | Indented
            // (129). The ordinary compiler printer does not enable
            // preserveSourceNewlines, so emitNodeList writes a line even for
            // an empty parsed list. updateCaseBlock retains original identity
            // and source range, but neither changes that default list policy.
            writer.write_line(false);
        } else {
            writer.write_line(false);
            writer.increase_indent();
            for (index, clause) in clauses.into_iter().enumerate() {
                let clause = transformation
                    .arena()
                    .node_ref(source, clause)
                    .ok_or(PrinterError::UnknownStatement(clause.0))?;
                if index == 0 {
                    self.emit_leading_comments_for_node(transformation, clause, writer)?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        clause,
                        writer,
                    )?;
                }
                self.emit_node_id_with_context(
                    transformation,
                    source,
                    clause.node(),
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                writer.write_line(false);
            }
            // Like tsc's close-brace token, this boundary is anchored at the
            // clause NodeArray end rather than at the final emitted clause.
            self.emit_comments_before_close_brace(
                transformation,
                case_block,
                clause_list_end,
                writer,
            )?;
            writer.decrease_indent();
        }
        let close_default = match clause_list_end.and_then(|end| u32::try_from(end).ok()) {
            Some(end) => self.token_map_range_at(transformation, source, end, writer)?,
            None => None,
        };
        self.record_brace_write(
            transformation,
            case_block,
            SyntaxKind::CloseBraceToken,
            close_default,
            "}",
            |writer, spelling| writer.write_punctuation(spelling),
            writer,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// The single-statement clause polarity, hoisted above the colon
    /// write (upstream emitCaseOrDefaultClauseRest computes it first;
    /// the same-line arm's colon is the mapped `writeToken`).
    fn clause_single_statement_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        clause: TransformNode,
        source: TransformSourceId,
        statements: Option<tsc_syntax::NodeArrayId>,
    ) -> Result<bool, PrinterError> {
        let statements = statements
            .and_then(|array| transformation.arena().node_array_ref(source, array))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| array.nodes.clone())
            .unwrap_or_default();
        if statements.len() != 1 {
            return Ok(false);
        }
        let Some(first) = transformation.arena().node_ref(source, statements[0]) else {
            return Err(PrinterError::UnknownStatement(statements[0].0));
        };
        self.source_nodes_start_on_same_line(transformation, clause, first)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_case_clause_statements(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        statements: Option<tsc_syntax::NodeArrayId>,
        colon: TokenEmission,
        single_line: bool,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let statements = statements
            .and_then(|array| transformation.arena().node_array_ref(source, array))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| array.nodes.clone())
            .unwrap_or_default();
        let first = statements
            .first()
            .copied()
            .and_then(|statement| transformation.arena().node_ref(source, statement));
        let token_owned_prefix = self.token_owned_child_prefix(transformation, colon, first)?;
        if statements.is_empty() {
            return Ok(());
        }
        let first = first.ok_or(PrinterError::UnknownStatement(statements[0].0))?;
        if single_line {
            writer.write_space(" ");
            self.emit_leading_comments_for_node_worker(
                transformation,
                first,
                LeadingCommentContext::Normal,
                token_owned_prefix,
                writer,
            )?;
            self.emit_node_id_with_context(
                transformation,
                source,
                first.node(),
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            self.emit_trailing_comments_for_node(transformation, first, writer)?;
            return Ok(());
        }
        writer.write_line(false);
        writer.increase_indent();
        for (index, statement) in statements.into_iter().enumerate() {
            let statement = transformation
                .arena()
                .node_ref(source, statement)
                .ok_or(PrinterError::UnknownStatement(statement.0))?;
            if index == 0 {
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    statement,
                    LeadingCommentContext::Normal,
                    token_owned_prefix,
                    writer,
                )?;
            } else {
                self.emit_leading_comments_for_node_after_sibling(
                    transformation,
                    statement,
                    writer,
                )?;
            }
            self.emit_node_id_with_context(
                transformation,
                source,
                statement.node(),
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            self.emit_trailing_comments_for_node(transformation, statement, writer)?;
            writer.write_line(false);
        }
        writer.decrease_indent();
        Ok(())
    }

    fn source_nodes_start_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let SourceRange::Original(left_range) =
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?
        else {
            return Ok(true);
        };
        let SourceRange::Original(right_range) =
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?
        else {
            return Ok(true);
        };
        let left_start = skip_trivia(source.text(), left_range.start().value() as usize);
        let right_start = skip_trivia(source.text(), right_range.start().value() as usize);
        if left_start > right_start || right_start > source.text().len() {
            return Ok(false);
        }
        Ok(!source.text()[left_start..right_start]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    /// Mirrors the `containerEnd` guard in tsc's comment pipeline. A parsed
    /// parent whose range ends with its child owns that trailing boundary;
    /// a synthesized parent has no source container and leaves the boundary
    /// with the retained child.
    fn child_trailing_comments_escape_parent_container(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original_parent = transformation.arena().get_original_node(parent);
        let original_child = transformation.arena().get_original_node(child);
        if original_parent.source() != original_child.source() {
            return Ok(true);
        }
        let source = transformation
            .arena()
            .source(original_parent.source())?
            .syntax();
        let parent_record = transformation.arena().node(original_parent)?;
        let child_record = transformation.arena().node(original_child)?;
        let parent_range =
            SourceRange::from_raw(parent_record.pos, parent_record.end, source.positions())?;
        let child_range =
            SourceRange::from_raw(child_record.pos, child_record.end, source.positions())?;
        Ok(match (parent_range, child_range) {
            (SourceRange::Original(parent), SourceRange::Original(child)) => {
                parent.end() != child.end()
            }
            (SourceRange::Synthesized, SourceRange::Original(_)) => true,
            _ => false,
        })
    }

    fn child_trailing_comments_escape_active_container(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
        active_scope: CommentEmissionScope,
    ) -> Result<bool, PrinterError> {
        if active_scope.container_end().is_some() {
            let child_range = self.comment_range_for_node(transformation, child)?;
            if let SourceRange::Original(range) = child_range.range() {
                return Ok(!active_scope
                    .retains_end(CommentCursor::new(child_range.source(), range.end())));
            }
        }
        self.child_trailing_comments_escape_parent_container(transformation, parent, child)
    }

    /// Emit the comment boundary between a retained final child and the end
    /// of its parent container.
    ///
    /// tsc restores `containerEnd` after emitting each nested node. A final
    /// child therefore emits comments at its end only when the enclosing
    /// source container extends beyond that boundary. Keep that ownership
    /// transition explicit in Rust and carry the source through a typed token
    /// cursor; when both ends coincide, ownership remains with the enclosing
    /// caller instead of being visited twice.
    fn emit_child_boundary_comments_before_parent_end(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
        active_scope: CommentEmissionScope,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        // `NoTrailingComments` still establishes tsc's `containerEnd`, but
        // `emitTrailingCommentsOfNode` must not visit that boundary.  This
        // explicit parent/child handoff is the Rust equivalent of that trailing
        // phase, so honor the child's ownership metadata before completing it.
        // In particular, a downleveled private postfix update deliberately
        // suppresses the synthesized comma expression's trailing boundary and
        // leaves the source update's comment with the original-linked outer
        // helper call.
        if transformation
            .arena()
            .metadata(child)
            .is_some_and(|metadata| {
                metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                    || metadata.relocated_trailing_comment_owner.is_some()
            })
        {
            return Ok(());
        }
        if !self.child_trailing_comments_escape_active_container(
            transformation,
            parent,
            child,
            active_scope,
        )? {
            return Ok(());
        }
        self.emit_comments_at_cursor(
            transformation,
            self.comment_range_end_cursor(transformation, child)?,
            None,
            false,
            writer,
        )
    }

    /// Statement-facing name for the common final-child boundary. A parsed
    /// semicolon is simply one concrete parent end that extends past its
    /// expression or declaration list.
    fn emit_child_boundary_comments_before_terminator(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
        active_scope: CommentEmissionScope,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_child_boundary_comments_before_parent_end(
            transformation,
            parent,
            child,
            active_scope,
            writer,
        )
    }

    fn source_node_range_is_on_single_line(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(true);
        };
        let start = range.start().value() as usize;
        let end = range.end().value() as usize;
        if start > end || end > source.text().len() {
            return Ok(true);
        }
        Ok(!source.text()[start..end]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    fn source_node_end_and_node_start_are_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        Ok(self
            .source_node_end_and_node_start_same_line_comparable(transformation, left, right)?
            .unwrap_or(true))
    }

    /// `siblingNodePositionsAreComparable` + the text scan: `None` when the
    /// sibling positions are not comparable (synthesized, cross-source, or
    /// out of order) — the caller supplies tsc's per-list fallback
    /// (`format & MultiLine ? line : none`).
    fn source_node_end_and_node_start_same_line_comparable(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<Option<bool>, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        if left.source() != right.source() {
            return Ok(None);
        }
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let (SourceRange::Original(left_range), SourceRange::Original(right_range)) = (
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?,
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?,
        ) else {
            return Ok(None);
        };
        let left_end = left_range.end().value() as usize;
        let right_start = skip_trivia(source.text(), right_range.start().value() as usize);
        if left_end > right_start || right_start > source.text().len() {
            return Ok(None);
        }
        Ok(Some(
            !source.text()[left_end..right_start]
                .bytes()
                .any(|byte| matches!(byte, b'\r' | b'\n')),
        ))
    }

    fn source_node_starts_are_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        if left.source() != right.source() {
            return Ok(true);
        }
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let (SourceRange::Original(left_range), SourceRange::Original(right_range)) = (
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?,
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?,
        ) else {
            return Ok(true);
        };
        let left_start = skip_trivia(source.text(), left_range.start().value() as usize);
        let right_start = skip_trivia(source.text(), right_range.start().value() as usize);
        if left_start > right_start || right_start > source.text().len() {
            return Ok(true);
        }
        Ok(!source.text()[left_start..right_start]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    fn source_node_ends_are_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        if left.source() != right.source() {
            return Ok(true);
        }
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let (SourceRange::Original(left_range), SourceRange::Original(right_range)) = (
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?,
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?,
        ) else {
            return Ok(true);
        };
        let left_end = left_range.end().value() as usize;
        let right_end = right_range.end().value() as usize;
        if left_end > right_end || right_end > source.text().len() {
            return Ok(true);
        }
        Ok(!source.text()[left_end..right_end]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    /// tsc-port: getLinesBetweenNodes @6.0.3 (the normal compiler-emit face)
    /// tsc-hash: e316d6faf745db22fd80474617d7ceaf9521378e2e4894c0b2f32e02b1148c2c
    /// tsc-span: _tsc.js:120408-120431
    ///
    /// Compiler JavaScript emit preserves whether two parsed children cross a
    /// source line, but collapses any number of source lines to one unless the
    /// explicit preserveSourceNewlines printer option is active. That option
    /// is not part of this printer's public contract yet, so this typed helper
    /// returns the exact 0/1 face used by ordinary `tsc` emit.
    fn lines_between_optional_nodes(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        left: Option<TransformNode>,
        right: Option<TransformNode>,
    ) -> Result<u32, PrinterError> {
        if transformation
            .arena()
            .metadata(parent)
            .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::NO_INDENTATION))
        {
            return Ok(0);
        }
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(0);
        };
        if transformation
            .arena()
            .metadata(right)
            .and_then(crate::EmitMetadata::starts_on_new_line)
            == Some(true)
        {
            return Ok(1);
        }
        Ok(
            (!self.source_node_end_and_node_start_are_on_same_line(transformation, left, right)?)
                as u32,
        )
    }

    /// tsc-port: writeLinesAndIndent @6.0.3
    /// tsc-hash: 6512c5e7da1541bb74a661fb97ea2bc154fcf5be5087d374910f7459b782308c
    /// tsc-span: _tsc.js:120252-120259
    fn write_lines_and_indent(
        writer: &mut TextWriter,
        line_count: u32,
        write_space_if_not_indenting: bool,
    ) {
        if line_count > 0 {
            writer.increase_indent();
            for line in 0..line_count {
                writer.write_line(line > 0);
            }
        } else if write_space_if_not_indenting {
            writer.write_space(" ");
        }
    }

    /// tsc-port: decreaseIndentIf @6.0.3
    /// tsc-hash: ddb27cbafaa022c56d1eb472c7940b54c632506e906d095df34405ae37b64857
    /// tsc-span: _tsc.js:120260-120267
    fn decrease_indent_if(writer: &mut TextWriter, first: u32, second: u32) {
        if first > 0 {
            writer.decrease_indent();
        }
        if second > 0 {
            writer.decrease_indent();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_delimited_expression_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        elements: Option<tsc_syntax::NodeArrayId>,
        open: &str,
        close: &str,
        multi_line: bool,
        format: DelimitedListFormat,
        item_syntax: ExpressionSyntaxContext,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = parent.source();
        writer.write_punctuation(open);
        let array = elements.and_then(|id| transformation.arena().node_array_ref(source, id));
        let (ids, trailing_comma, synthesized_array) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (
                record.nodes.clone(),
                record.has_trailing_comma,
                record.pos == u32::MAX && record.end == u32::MAX,
            )
        } else {
            (Vec::new(), false, false)
        };
        if ids.is_empty() {
            self.emit_empty_node_array_comments(transformation, source, elements, writer)?;
            writer.write_punctuation(close);
            return Ok(());
        }

        if multi_line {
            writer.write_line(false);
            if format.indentation.is_indented() {
                writer.increase_indent();
            }
            let count = ids.len();
            let mut pending_delimited_comment = None;
            for (index, id) in ids.iter().copied().enumerate() {
                let child = transformation
                    .arena()
                    .node_ref(source, id)
                    .ok_or(PrinterError::UnknownStatement(id.0))?;
                if index == 0 && !synthesized_array {
                    self.emit_leading_comments_for_multiline_delimited_list_start(
                        transformation,
                        child,
                        writer,
                    )?;
                } else if index == 0 {
                    self.emit_leading_comments_for_delimited_list_start(
                        transformation,
                        child,
                        writer,
                    )?;
                } else if synthesized_array {
                    // A transformed expression list has no source delimiter
                    // between its retained children. The next child's trivia
                    // may therefore belong to an operator that the transform
                    // replaced (for example `left ** /* comment */ right`).
                    // When a retained source comma *does* exist, the
                    // preceding item returns an explicit comment cursor
                    // through the trivia it already emitted. Otherwise the
                    // child retains its full comment range.
                    let resume = self.delimited_comment_resume_for_node(
                        transformation,
                        child,
                        pending_delimited_comment.take(),
                    )?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        child,
                        LeadingCommentContext::DelimitedListStart,
                        resume,
                        writer,
                    )?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        child,
                        writer,
                    )?;
                }
                self.emit_node_id_with_context(
                    transformation,
                    source,
                    id,
                    expression_context.for_child(item_syntax),
                    writer,
                )?;
                self.emit_list_element_end_comments_in_container(
                    transformation,
                    child,
                    expression_context.comments(),
                    writer,
                )?;
                let emit_delimiter = index + 1 < count || trailing_comma;
                if emit_delimiter {
                    writer.write_punctuation(",");
                    pending_delimited_comment = self.emit_delimited_trailing_comments_for_node(
                        transformation,
                        child,
                        writer,
                    )?;
                }
                let same_source_line = if format.lines.preserves_source() && !synthesized_array {
                    ids.get(index + 1).copied()
                } else {
                    None
                }
                .map(|next| {
                    let next = transformation
                        .arena()
                        .node_ref(source, next)
                        .ok_or(PrinterError::UnknownStatement(next.0))?;
                    // Non-comparable sibling positions in a multi-line list
                    // take tsc's `format & MultiLine` fallback: one line
                    // (H2.5h CA-2a — synthesized class-wrapper elements in
                    // a parsed multi-line array).
                    self.source_node_end_and_node_start_same_line_comparable(
                        transformation,
                        child,
                        next,
                    )
                    .map(|comparable| comparable.unwrap_or(false))
                })
                .transpose()?
                .unwrap_or(false);
                if same_source_line {
                    writer.write_space(" ");
                } else {
                    writer.write_line(false);
                }
            }
            self.emit_trailing_node_array_end_comments(transformation, source, elements, writer)?;
            if format.indentation.is_indented() {
                writer.decrease_indent();
            }
        } else {
            let space_between_braces = open == "{";
            let first = TransformNode::new(source, ids[0]);
            let leading_source_line = format.lines.preserves_source()
                && !synthesized_array
                && !self.source_node_starts_are_on_same_line(transformation, parent, first)?;
            if leading_source_line {
                writer.write_line(false);
            } else if space_between_braces {
                writer.write_space(" ");
            }
            // Array- and object-literal ListFormats carry `Indented` even
            // when their opening delimiter and first child share a line.
            // The scope becomes observable when that child emits its own
            // multiline body (for example `[{\n  key: value\n}]`).
            if format.indentation.is_indented() {
                writer.increase_indent();
            }
            let count = ids.len();
            let mut pending_delimited_comment = None;
            for (index, id) in ids.iter().copied().enumerate() {
                let child = transformation
                    .arena()
                    .node_ref(source, id)
                    .ok_or(PrinterError::UnknownStatement(id.0))?;
                if index == 0 {
                    self.emit_leading_comments_for_delimited_list_start(
                        transformation,
                        child,
                        writer,
                    )?;
                } else if synthesized_array {
                    let resume = self.delimited_comment_resume_for_node(
                        transformation,
                        child,
                        pending_delimited_comment.take(),
                    )?;
                    self.emit_leading_comments_for_node_worker(
                        transformation,
                        child,
                        LeadingCommentContext::DelimitedListStart,
                        resume,
                        writer,
                    )?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        child,
                        writer,
                    )?;
                }
                self.emit_node_id_with_context(
                    transformation,
                    source,
                    id,
                    expression_context.for_child(item_syntax),
                    writer,
                )?;
                self.emit_list_element_end_comments_in_container(
                    transformation,
                    child,
                    expression_context.comments(),
                    writer,
                )?;
                let emit_delimiter = index + 1 < count || trailing_comma;
                if emit_delimiter {
                    // emitNodeListItems emits the item's end comments before
                    // its delimiter, so a non-final element's same-line
                    // trailing comment sits between the element and its comma.
                    self.emit_list_element_end_comments_in_container(
                        transformation,
                        child,
                        expression_context.comments(),
                        writer,
                    )?;
                    writer.write_punctuation(",");
                    pending_delimited_comment = self.emit_delimited_trailing_comments_for_node(
                        transformation,
                        child,
                        writer,
                    )?;
                }
                if index + 1 < count {
                    let next = TransformNode::new(source, ids[index + 1]);
                    if format.lines.preserves_source()
                        && !synthesized_array
                        && !self.source_node_end_and_node_start_are_on_same_line(
                            transformation,
                            child,
                            next,
                        )?
                    {
                        writer.write_line(false);
                    } else if !writer.is_at_start_of_line() {
                        // A line comment after the source comma has already
                        // completed the line. Like parameter and call-argument
                        // lists, a compact binding-pattern list must not turn
                        // its ordinary separator space into indentation on the
                        // following line.
                        writer.write_space(" ");
                    }
                }
            }
            self.emit_trailing_node_array_end_comments(transformation, source, elements, writer)?;
            if format.indentation.is_indented() {
                writer.decrease_indent();
            }
            let last = TransformNode::new(source, *ids.last().expect("nonempty delimited list"));
            let closing_source_line = format.lines.preserves_source()
                && !synthesized_array
                && !self.source_node_ends_are_on_same_line(transformation, last, parent)?;
            if closing_source_line {
                writer.write_line(false);
            } else if space_between_braces {
                writer.write_space(" ");
            }
        }
        writer.write_punctuation(close);
        Ok(())
    }

    /// tsc-port: emitNodeListItems @6.0.3
    /// tsc-hash: 8b9d9ba40ccad81aa5e0a79b002bc7be89f4a18456ebba923d01aefdfb315901
    /// tsc-span: _tsc.js:120068-120360
    ///
    /// This is the JSON subset of the `PreserveLines | Indented` list
    /// formats used by array and object literals. Object trailing commas are
    /// suppressed for a JSON SourceFile; array trailing commas remain the
    /// upstream parser/printer behavior.
    #[allow(clippy::too_many_arguments)]
    fn emit_json_delimited_expression_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        elements: Option<tsc_syntax::NodeArrayId>,
        open: &str,
        close: &str,
        prefer_new_line: bool,
        allow_trailing_comma: bool,
        item_syntax: ExpressionSyntaxContext,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation(open);
        let array =
            elements.and_then(|id| transformation.arena().node_array_ref(parent.source(), id));
        let (ids, trailing_comma) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (record.nodes.clone(), record.has_trailing_comma)
        } else {
            (Vec::new(), false)
        };
        if ids.is_empty() {
            self.emit_empty_node_array_comments(transformation, parent.source(), elements, writer)?;
            writer.write_punctuation(close);
            return Ok(());
        }

        let first = transformation
            .arena()
            .node_ref(parent.source(), ids[0])
            .ok_or(PrinterError::UnknownStatement(ids[0].0))?;
        let leading_line = prefer_new_line
            || self.json_node_start_line(transformation, parent)?
                != self.json_node_start_line(transformation, first)?;
        if leading_line {
            writer.write_line(false);
        } else if open == "{" {
            writer.write_space(" ");
        }
        writer.increase_indent();

        for (index, id) in ids.iter().copied().enumerate() {
            let child = transformation
                .arena()
                .node_ref(parent.source(), id)
                .ok_or(PrinterError::UnknownStatement(id.0))?;
            if index == 0 {
                self.emit_leading_comments_for_node(transformation, child, writer)?;
            } else {
                self.emit_leading_comments_for_node_after_sibling(transformation, child, writer)?;
            }
            self.emit_node_id_with_context(
                transformation,
                parent.source(),
                id,
                expression_context.for_child(item_syntax),
                writer,
            )?;

            let has_next = index + 1 < ids.len();
            if has_next || trailing_comma && allow_trailing_comma {
                writer.write_punctuation(",");
            }
            self.emit_delimited_trailing_comments_for_node(transformation, child, writer)?;
            if has_next {
                let next = transformation
                    .arena()
                    .node_ref(parent.source(), ids[index + 1])
                    .ok_or(PrinterError::UnknownStatement(ids[index + 1].0))?;
                if self.json_node_end_line(transformation, child)?
                    != self.json_node_start_line(transformation, next)?
                {
                    writer.write_line(false);
                } else {
                    writer.write_space(" ");
                }
            }
        }

        let last = transformation
            .arena()
            .node_ref(parent.source(), *ids.last().expect("nonempty JSON list"))
            .expect("validated JSON list child");
        let closing_line = prefer_new_line
            || self.json_node_end_line(transformation, parent)?
                != self.json_node_end_line(transformation, last)?;
        writer.decrease_indent();
        if closing_line {
            writer.write_line(false);
        } else if open == "{" {
            writer.write_space(" ");
        }
        writer.write_punctuation(close);
        Ok(())
    }

    fn json_node_start_line(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<u32, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let range = range.without_leading_trivia(source.text(), source.positions())?;
        Ok(SourceUtf16Location::from_byte(range.start(), source.positions())?.line())
    }

    fn json_node_end_line(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<u32, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        Ok(SourceUtf16Location::from_byte(range.end(), source.positions())?.line())
    }

    /// Emit the shared `while (expression)` token topology used by both a
    /// while statement and the trailing clause of a do statement. The caller
    /// supplies the source anchor because tsc starts the former at `node.pos`
    /// and the latter at `node.statement.end`.
    fn emit_while_clause(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        start_anchor: TokenAnchor,
        expression: Option<NodeId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        let parent_kind = transformation.arena().node(node)?.kind;
        let expression = expression
            .and_then(|expression| transformation.arena().node_ref(node.source(), expression))
            .ok_or(PrinterError::MissingTransformedChild {
                parent: parent_kind,
                field: "expression",
            })?;
        let while_keyword = self.emit_space_prefixed_token_with_comments(
            transformation,
            node,
            FixedToken::keyword(SyntaxKind::WhileKeyword),
            start_anchor,
            false,
            writer,
        )?;
        writer.write_space(" ");
        let open = self.emit_token_with_comments(
            transformation,
            node,
            FixedToken::punctuation(SyntaxKind::OpenParenToken),
            while_keyword,
            false,
            writer,
        )?;
        self.emit_child_after_token_with_context(
            transformation,
            node,
            open,
            expression,
            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
            writer,
        )?;
        self.emit_token_with_comments(
            transformation,
            node,
            FixedToken::punctuation(SyntaxKind::CloseParenToken),
            self.original_node_end_cursor(transformation, expression)?,
            false,
            writer,
        )
    }

    fn emit_embedded_statement(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        statement: Option<tsc_syntax::NodeId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_embedded_statement_with_anchor(
            transformation,
            parent,
            statement,
            EmbeddedStatementAnchor::Unspecified,
            expression_context,
            writer,
        )
    }

    fn emit_embedded_statement_after_token(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        statement: Option<tsc_syntax::NodeId>,
        token: TokenEmission,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_embedded_statement_with_anchor(
            transformation,
            parent,
            statement,
            EmbeddedStatementAnchor::AfterToken(token),
            expression_context,
            writer,
        )
    }

    fn emit_embedded_statement_with_anchor(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        statement: Option<tsc_syntax::NodeId>,
        anchor: EmbeddedStatementAnchor,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let parent_kind = transformation.arena().node(parent)?.kind;
        let statement = statement.ok_or(PrinterError::MissingTransformedChild {
            parent: parent_kind,
            field: "statement",
        })?;
        let statement_node = transformation
            .arena()
            .node_ref(parent.source(), statement)
            .ok_or(PrinterError::UnknownStatement(statement.0))?;
        let parent_is_single_line = transformation
            .arena()
            .metadata(parent)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::SINGLE_LINE));
        let token_resume = match anchor {
            EmbeddedStatementAnchor::Unspecified => None,
            EmbeddedStatementAnchor::AfterToken(token) => {
                self.token_owned_child_prefix(transformation, token, Some(statement_node))?
            }
        };
        if transformation.arena().node(statement_node)?.kind == SyntaxKind::Block
            || parent_is_single_line
        {
            writer.write_space(" ");
            if matches!(anchor, EmbeddedStatementAnchor::AfterToken(_)) {
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    statement_node,
                    LeadingCommentContext::Normal,
                    token_resume,
                    writer,
                )?;
            }
            self.emit_node_id_with_context(
                transformation,
                parent.source(),
                statement,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )
        } else {
            writer.write_line(false);
            writer.increase_indent();
            self.emit_leading_comments_for_node_worker(
                transformation,
                statement_node,
                LeadingCommentContext::Normal,
                token_resume,
                writer,
            )?;
            self.emit_node_id_with_context(
                transformation,
                parent.source(),
                statement,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            writer.decrease_indent();
            Ok(())
        }
    }

    /// tsc-port: writeLineOrSpace @6.0.3 (ordinary compiler-emit face)
    /// tsc-hash: acd7f035eb5c3afb2ed65db0f1637bb9164f8185d8303811c578363333223db7
    /// tsc-span: _tsc.js:120227-120239
    ///
    /// The public printer does not yet expose preserveSourceNewlines,
    /// so the non-SingleLine path is always one canonical newline.
    fn write_line_or_space(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        writer: &mut TextWriter,
    ) {
        if transformation
            .arena()
            .metadata(parent)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::SINGLE_LINE))
        {
            writer.write_space(" ");
        } else {
            writer.write_line(false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_class(
        &self,
        transformation: &mut TransformationResult<'_>,
        class_node: TransformNode,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        name: Option<tsc_syntax::NodeId>,
        heritage_clauses: Option<tsc_syntax::NodeArrayId>,
        members: Option<tsc_syntax::NodeArrayId>,
        expression: bool,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let anonymous_default_declaration = !expression
            && modifiers
                .and_then(|array| transformation.arena().node_array_ref(source, array))
                .map(|array| transformation.arena().node_array(array))
                .transpose()?
                .is_some_and(|array| {
                    array.nodes.iter().any(|id| {
                        transformation
                            .arena()
                            .node_ref(source, *id)
                            .and_then(|modifier| transformation.arena().node(modifier).ok())
                            .is_some_and(|modifier| modifier.kind == SyntaxKind::DefaultKeyword)
                    })
                });
        if self.emit_modifiers(
            transformation,
            source,
            modifiers,
            expression_context,
            writer,
        )? {
            writer.write_space(" ");
        }
        writer.write_keyword("class");
        if let Some(name) = name {
            writer.write_space(" ");
            self.emit_identifier_name_with_context(
                transformation,
                source,
                name,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            let name = transformation
                .arena()
                .node_ref(source, name)
                .ok_or(PrinterError::UnknownStatement(name.0))?;
            // `emitIdentifierName` participates in tsc's ordinary comment
            // phase. Keep the name's end boundary explicit here so comments
            // in an erased heritage slot (for example
            // `class C /* extends Error */ {}`) remain between the name and
            // the opening brace instead of disappearing with the type-only
            // syntax.
            self.emit_comments_at_cursor(
                transformation,
                self.original_node_end_cursor(transformation, name)?,
                None,
                false,
                writer,
            )?;
        } else if !expression && !anonymous_default_declaration {
            return Err(PrinterError::MissingTransformedChild {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        }
        let indented = transformation
            .arena()
            .metadata(class_node)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::INDENTED));
        if indented {
            writer.increase_indent();
        }
        let has_heritage_clauses = heritage_clauses
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .is_some_and(|array| !array.nodes.is_empty());
        if has_heritage_clauses {
            self.emit_node_array(
                transformation,
                source,
                heritage_clauses,
                "",
                expression_context,
                writer,
            )?;
        }
        writer.write_space(" ");
        writer.write_punctuation("{");
        let member_array = members.and_then(|id| transformation.arena().node_array_ref(source, id));
        if let Some(member_array) = member_array {
            let member_ids = transformation
                .arena()
                .node_array(member_array)?
                .nodes
                .clone();
            if !member_ids.is_empty() {
                writer.write_line(false);
                writer.increase_indent();
                for (index, member) in member_ids.into_iter().enumerate() {
                    let member_node = transformation
                        .arena()
                        .node_ref(source, member)
                        .ok_or(PrinterError::UnknownStatement(member.0))?;
                    if index == 0
                        && !self.first_list_item_follows_elided_source_item(
                            transformation,
                            member_array,
                            member_node,
                        )?
                    {
                        self.emit_leading_comments_for_node(transformation, member_node, writer)?;
                    } else {
                        self.emit_leading_comments_for_node_after_sibling(
                            transformation,
                            member_node,
                            writer,
                        )?;
                    }
                    self.emit_node_id_with_context(
                        transformation,
                        source,
                        member,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    self.emit_trailing_comments_for_node(transformation, member_node, writer)?;
                    writer.write_line(false);
                }
                writer.decrease_indent();
            } else {
                writer.write_line(false);
            }
        } else {
            writer.write_line(false);
        }
        writer.write_punctuation("}");
        if indented {
            writer.decrease_indent();
        }
        Ok(())
    }

    /// Whether a transformed list starts after the parsed list's first item.
    ///
    /// Class members use tsc's `ListFormat::NoInterveningComments`. When an
    /// earlier member is erased, a same-line comment immediately before the
    /// first retained member remains trailing trivia of that erased member;
    /// it must not become leading trivia of the retained member. Comparing
    /// typed source ranges preserves that ownership without retaining erased
    /// syntax in the transformed list.
    fn first_list_item_follows_elided_source_item(
        &self,
        transformation: &TransformationResult<'_>,
        list: TransformNodeArray,
        first_item: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original_item = transformation.arena().get_original_node(first_item);
        if original_item.source() != list.source() {
            return Ok(false);
        }
        let source = transformation.arena().source(list.source())?.syntax();
        let list_record = transformation.arena().node_array(list)?;
        let item_record = transformation.arena().node(original_item)?;
        let SourceRange::Original(list_range) =
            SourceRange::from_raw(list_record.pos, list_record.end, source.positions())?
        else {
            return Ok(false);
        };
        let SourceRange::Original(item_range) =
            SourceRange::from_raw(item_record.pos, item_record.end, source.positions())?
        else {
            return Ok(false);
        };
        Ok(item_range.start() > list_range.start())
    }

    /// Emit TypeArguments list delimiters as part of the syntax surface.
    /// transformTypeScript removes JSX type arguments before JavaScript/JSX
    /// output; an identity/standalone TSX print still owns a non-empty
    /// `<T, U>` list, including recovery type nodes. TypeScript's list format
    /// is `OptionalIfEmpty`, so an empty recovery NodeArray emits no brackets.
    ///
    /// tsc-port: emitTypeArguments @6.0.3
    /// tsc-hash: 095bb2e591a182100c1586f39db20b74f024c9ca9e743692af8894fde3317d60
    /// tsc-span: _tsc.js:119967-119969
    fn emit_type_arguments(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        type_arguments: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.node_array_has_elements(transformation, source, type_arguments)? {
            writer.write_punctuation("<");
            self.emit_node_array(
                transformation,
                source,
                type_arguments,
                ", ",
                expression_context,
                writer,
            )?;
            writer.write_punctuation(">");
        }
        Ok(())
    }

    /// Emit function-like signature fields in tsc's grammar order. Several
    /// invalid constructor/accessor fields are retained for parser recovery;
    /// the corresponding NodeFactory updaters decide which survive a runtime
    /// update, while the JavaScript printer uniformly reproduces the fields
    /// that remain on the node.
    ///
    /// tsc-port: emitSignatureHead @6.0.3
    /// tsc-hash: 1051bc6f6d403e11ae463222deba4cc157d1615716c2c426b26dea7e6804defb
    /// tsc-span: _tsc.js:118994-118998
    #[allow(clippy::too_many_arguments)]
    fn emit_signature_head(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        type_parameters: Option<tsc_syntax::NodeArrayId>,
        parameters: Option<tsc_syntax::NodeArrayId>,
        r#type: Option<NodeId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.node_array_has_elements(transformation, source, type_parameters)? {
            writer.write_punctuation("<");
            self.emit_node_array(
                transformation,
                source,
                type_parameters,
                ", ",
                expression_context,
                writer,
            )?;
            writer.write_punctuation(">");
        }
        self.emit_parameter_list(
            transformation,
            source,
            parameters,
            expression_context,
            writer,
        )?;
        if let Some(r#type) = r#type {
            writer.write_punctuation(":");
            writer.write_space(" ");
            self.emit_node_id_with_context(
                transformation,
                source,
                r#type,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
        }
        Ok(())
    }

    fn emit_parameter_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        parameters: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_parameter_list_with_parentheses(
            transformation,
            source,
            parameters,
            ParameterListParentheses::Present,
            expression_context,
            writer,
        )
    }

    /// Emit an arrow's `Parameters` list with the same single format change as
    /// tsc: a simple head removes only the `Parenthesis` bit. The list worker
    /// remains responsible for intervening, leading, and element-end comments.
    ///
    /// tsc-port: canEmitSimpleArrowHead/emitParametersForArrow @6.0.3
    /// tsc-hash: 49144b810a6f5fe0c7dabac67f6282674df43251436e201114e531340db594a2
    /// tsc-span: _tsc.js:119970-119982
    fn emit_arrow_parameter_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        arrow: TransformNode,
        data: &tsc_syntax::nodes::ArrowFunctionData,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let parentheses = if self.can_emit_simple_arrow_head(transformation, arrow, data)? {
            ParameterListParentheses::OmittedForSimpleArrow
        } else {
            ParameterListParentheses::Present
        };
        self.emit_parameter_list_with_parentheses(
            transformation,
            arrow.source(),
            data.parameters,
            parentheses,
            expression_context,
            writer,
        )
    }

    fn emit_parameter_list_with_parentheses(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        parameters: Option<tsc_syntax::NodeArrayId>,
        parentheses: ParameterListParentheses,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if parentheses.are_present() {
            writer.write_punctuation("(");
        }
        let array = parameters.and_then(|id| transformation.arena().node_array_ref(source, id));
        let (ids, synthesized_array) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (
                record.nodes.clone(),
                record.pos == u32::MAX && record.end == u32::MAX,
            )
        } else {
            (Vec::new(), false)
        };
        if ids.is_empty() {
            self.emit_empty_node_array_comments(transformation, source, parameters, writer)?;
        } else {
            let count = ids.len();
            for (index, id) in ids.into_iter().enumerate() {
                let parameter = transformation
                    .arena()
                    .node_ref(source, id)
                    .ok_or(PrinterError::UnknownStatement(id.0))?;
                if index == 0 {
                    // `emitParametersForArrow` removes only the Parenthesis
                    // format bit. Its list-start intervening-comment phase is
                    // otherwise identical to every Parameters list, including
                    // tsc's observable replay when the arrow and its simple
                    // parameter share a comment range start.
                    self.emit_leading_comments_for_delimited_list_start(
                        transformation,
                        parameter,
                        writer,
                    )?;
                } else if synthesized_array {
                    self.emit_leading_comments_for_delimited_list_start(
                        transformation,
                        parameter,
                        writer,
                    )?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        parameter,
                        writer,
                    )?;
                }
                self.emit_node_id_with_context(
                    transformation,
                    source,
                    id,
                    expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                    writer,
                )?;
                self.emit_list_element_end_comments(transformation, parameter, writer)?;
                if index + 1 < count {
                    writer.write_punctuation(",");
                    self.emit_delimited_trailing_comments_for_node(
                        transformation,
                        parameter,
                        writer,
                    )?;
                    if !writer.is_at_start_of_line() {
                        writer.write_space(" ");
                    }
                }
            }
        }
        if parentheses.are_present() {
            writer.write_punctuation(")");
        }
        Ok(())
    }

    fn source_gap_has_line_break(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        left: tsc_syntax::NodeId,
        right: tsc_syntax::NodeId,
    ) -> Result<bool, PrinterError> {
        let left = transformation
            .arena()
            .node_ref(source, left)
            .ok_or(PrinterError::UnknownStatement(left.0))?;
        let right = transformation
            .arena()
            .node_ref(source, right)
            .ok_or(PrinterError::UnknownStatement(right.0))?;
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        let syntax = transformation.arena().source(source)?.syntax();
        let SourceRange::Original(left_range) = SourceRange::from_raw(
            transformation.arena().node(left)?.pos,
            transformation.arena().node(left)?.end,
            syntax.positions(),
        )?
        else {
            return Ok(false);
        };
        let SourceRange::Original(right_range) = SourceRange::from_raw(
            transformation.arena().node(right)?.pos,
            transformation.arena().node(right)?.end,
            syntax.positions(),
        )?
        else {
            return Ok(false);
        };
        let start = left_range.end().value() as usize;
        let end = right_range.start().value() as usize;
        if start > end || end > syntax.text().len() {
            return Ok(false);
        }
        Ok(syntax.text()[start..end].contains('\r') || syntax.text()[start..end].contains('\n'))
    }

    fn source_node_leading_trivia_has_line_break(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        node: tsc_syntax::NodeId,
    ) -> Result<bool, PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, node)
            .ok_or(PrinterError::UnknownStatement(node.0))?;
        let node = transformation.arena().get_original_node(node);
        let syntax = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, syntax.positions())?
        else {
            return Ok(false);
        };
        let start = range.start().value() as usize;
        let code_start = skip_trivia(syntax.text(), start);
        if start > code_start || code_start > syntax.text().len() {
            return Ok(false);
        }
        Ok(syntax.text()[start..code_start]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    fn can_emit_simple_arrow_head(
        &self,
        transformation: &TransformationResult<'_>,
        arrow: TransformNode,
        data: &tsc_syntax::nodes::ArrowFunctionData,
    ) -> Result<bool, PrinterError> {
        if data.r#type.is_some()
            || self.node_array_has_elements(transformation, arrow.source(), data.modifiers)?
            || self.node_array_has_elements(transformation, arrow.source(), data.type_parameters)?
        {
            return Ok(false);
        }
        let Some(parameters) = data
            .parameters
            .and_then(|id| transformation.arena().node_array_ref(arrow.source(), id))
        else {
            return Ok(false);
        };
        let parameters = transformation.arena().node_array(parameters)?;
        if parameters.nodes.len() != 1 {
            return Ok(false);
        }
        let parameter_id = parameters.nodes[0];
        let parameter = transformation
            .arena()
            .node_ref(arrow.source(), parameter_id)
            .ok_or(PrinterError::UnknownStatement(parameter_id.0))?;
        let NodeData::Parameter(parameter_data) = &transformation.arena().node(parameter)?.data
        else {
            return Ok(false);
        };
        let simple_name = parameter_data
            .name
            .and_then(|id| transformation.arena().node_ref(arrow.source(), id))
            .is_some_and(|name| {
                transformation
                    .arena()
                    .node(name)
                    .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
            });
        let arrow_pos = transformation.arena().node(arrow)?.pos;
        let parameter_pos = transformation.arena().node(parameter)?.pos;
        Ok(arrow_pos == parameter_pos
            && simple_name
            && !self.node_array_has_elements(
                transformation,
                arrow.source(),
                parameter_data.modifiers,
            )?
            && parameter_data.dot_dot_dot_token.is_none()
            && parameter_data.question_token.is_none()
            && parameter_data.r#type.is_none()
            && parameter_data.initializer.is_none())
    }

    /// Access and `new` callees share a left-hand-side grammar boundary, but
    /// `new` additionally rejects a call (or an argument-less nested `new`) at
    /// its left edge. Keeping the context typed prevents precedence fixes for
    /// one parent from leaking into unrelated expressions.
    ///
    /// tsc-port: parenthesizeLeftSideOfAccess/parenthesizeExpressionOfNew @6.0.3
    /// tsc-hash: 9dfe0c3ffe587f7886e4281931f8b33a74ea773c8c66ebce98c42f88ebe3f116
    /// tsc-span: _tsc.js:20451-20471
    fn context_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
        context: ExpressionGrammarContext,
    ) -> Result<Option<GrammarParentheses>, PrinterError> {
        let emitted = self.skip_partially_emitted_expressions(transformation, expression)?;
        let parentheses = match context {
            ExpressionGrammarContext::PrefixUnaryOperand => (!self
                .is_unary_expression_kind(transformation.arena().node(emitted)?.kind))
            .then_some(GrammarParentheses::SourceRanged),
            ExpressionGrammarContext::PostfixUnaryOperand => (!self
                .is_left_hand_side_expression_kind(transformation.arena().node(emitted)?.kind))
            .then_some(GrammarParentheses::SourceRanged),
            ExpressionGrammarContext::NewCallee => {
                let leftmost = self.leftmost_expression(transformation, emitted, true)?;
                let leftmost_record = transformation.arena().node(leftmost)?;
                if leftmost_record.kind == SyntaxKind::CallExpression
                    || matches!(
                        &leftmost_record.data,
                        NodeData::NewExpression(data) if data.arguments.is_none()
                    )
                {
                    Some(GrammarParentheses::Synthetic)
                } else {
                    self.left_side_of_access_requires_parentheses(transformation, emitted, false)?
                        .then_some(GrammarParentheses::SourceRanged)
                }
            }
            ExpressionGrammarContext::LeftSideOfAccess { optional_chain } => self
                .left_side_of_access_requires_parentheses(transformation, emitted, optional_chain)?
                .then_some(GrammarParentheses::SourceRanged),
            ExpressionGrammarContext::ComputedPropertyName => self
                .is_comma_sequence(transformation, emitted)?
                .then_some(GrammarParentheses::Synthetic),
            ExpressionGrammarContext::ArrowConciseBody => self
                .arrow_concise_body_requires_parentheses(
                    transformation,
                    emitted.source(),
                    emitted.node(),
                )?
                .then_some(GrammarParentheses::SourceRanged),
            ExpressionGrammarContext::AssignmentRightSide => self
                .assignment_right_side_requires_parentheses(transformation, emitted)?
                .then_some(GrammarParentheses::Synthetic),
            ExpressionGrammarContext::ExportDefault => self
                .export_default_requires_parentheses(transformation, emitted)?
                .then_some(GrammarParentheses::Synthetic),
            ExpressionGrammarContext::DisallowedComma => self
                .is_comma_sequence(transformation, emitted)?
                .then_some(GrammarParentheses::SourceRanged),
            _ => None,
        };
        Ok(parentheses)
    }

    /// tsc isCommaSequence after partially-emitted wrappers are skipped.
    fn is_comma_sequence(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let expression = self.skip_partially_emitted_expressions(transformation, expression)?;
        let record = transformation.arena().node(expression)?;
        match &record.data {
            NodeData::CommaListExpression(_) => Ok(true),
            NodeData::BinaryExpression(data) => Ok(data
                .operator_token
                .and_then(|operator| {
                    transformation
                        .arena()
                        .node_ref(expression.source(), operator)
                })
                .map(|operator| transformation.arena().node(operator))
                .transpose()?
                .is_some_and(|operator| operator.kind == SyntaxKind::CommaToken)),
            _ => Ok(false),
        }
    }

    fn left_side_of_access_requires_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
        optional_chain: bool,
    ) -> Result<bool, PrinterError> {
        let record = transformation.arena().node(expression)?;
        if !self.is_left_hand_side_expression_kind(record.kind)
            || matches!(&record.data, NodeData::NewExpression(data) if data.arguments.is_none())
        {
            return Ok(true);
        }
        if !optional_chain && self.is_optional_chain(record) {
            return Ok(true);
        }
        Ok(false)
    }

    fn is_left_hand_side_expression_kind(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::NewExpression
                | SyntaxKind::CallExpression
                | SyntaxKind::JsxElement
                | SyntaxKind::JsxSelfClosingElement
                | SyntaxKind::JsxFragment
                | SyntaxKind::TaggedTemplateExpression
                | SyntaxKind::ArrayLiteralExpression
                | SyntaxKind::ParenthesizedExpression
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ClassExpression
                | SyntaxKind::FunctionExpression
                | SyntaxKind::Identifier
                | SyntaxKind::PrivateIdentifier
                | SyntaxKind::RegularExpressionLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TemplateExpression
                | SyntaxKind::FalseKeyword
                | SyntaxKind::NullKeyword
                | SyntaxKind::ThisKeyword
                | SyntaxKind::TrueKeyword
                | SyntaxKind::SuperKeyword
                | SyntaxKind::NonNullExpression
                | SyntaxKind::ExpressionWithTypeArguments
                | SyntaxKind::MetaProperty
                | SyntaxKind::ImportKeyword
        )
    }

    fn is_unary_expression_kind(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PrefixUnaryExpression
                | SyntaxKind::PostfixUnaryExpression
                | SyntaxKind::DeleteExpression
                | SyntaxKind::TypeOfExpression
                | SyntaxKind::VoidExpression
                | SyntaxKind::AwaitExpression
                | SyntaxKind::TypeAssertionExpression
        ) || self.is_left_hand_side_expression_kind(kind)
    }

    fn is_optional_chain(&self, record: &tsc_syntax::Node) -> bool {
        NodeFlags::from_bits(record.flags).contains(NodeFlags::OPTIONAL_CHAIN)
            || match &record.data {
                NodeData::PropertyAccessExpression(data) => data.question_dot_token.is_some(),
                NodeData::ElementAccessExpression(data) => data.question_dot_token.is_some(),
                NodeData::CallExpression(data) => data.question_dot_token.is_some(),
                _ => false,
            }
    }

    /// `parenthesizeExpressionOfExpressionStatement`: an object literal or
    /// function expression at the left edge of a statement must remain an
    /// expression after TypeScript-only outer wrappers have been erased.
    /// Calls whose callee is a function/arrow own the narrower callee pair.
    ///
    /// tsc-port: parenthesizeExpressionOfExpressionStatement @6.0.3
    /// tsc-hash: 9ea85a5c131f0252de709e1b57d4bd71d25752e600c1ff6436c02ac4c156c5bb
    /// tsc-span: _tsc.js:20489-20510
    fn expression_statement_requires_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let emitted = self.skip_partially_emitted_expressions(transformation, expression)?;
        if let NodeData::CallExpression(data) = &transformation.arena().node(emitted)?.data {
            if let Some(callee) = data
                .expression
                .and_then(|callee| transformation.arena().node_ref(emitted.source(), callee))
            {
                let callee = self.skip_partially_emitted_expressions(transformation, callee)?;
                if matches!(
                    transformation.arena().node(callee)?.kind,
                    SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                ) {
                    return Ok(false);
                }
            }
        }
        let leftmost = self.leftmost_expression(transformation, emitted, false)?;
        Ok(matches!(
            transformation.arena().node(leftmost)?.kind,
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::FunctionExpression
        ))
    }

    /// `export default` has a narrower left-edge ambiguity than an ordinary
    /// expression statement: class/function expressions and comma sequences
    /// require a generated pair, while calls of those expressions do not.
    ///
    /// tsc-port: parenthesizeExpressionOfExportDefault @6.0.3
    /// tsc-hash: b679ce5fcfe28d204e724e3f74823d351eb0d86503befebc7fbdaa10faf999e5
    /// tsc-span: _tsc.js:20436-20450
    fn export_default_requires_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let emitted = self.skip_partially_emitted_expressions(transformation, expression)?;
        if self.is_comma_sequence(transformation, emitted)? {
            return Ok(true);
        }
        let leftmost = self.leftmost_expression(transformation, emitted, false)?;
        Ok(matches!(
            transformation.arena().node(leftmost)?.kind,
            SyntaxKind::ClassExpression | SyntaxKind::FunctionExpression
        ))
    }

    /// For the right side of `=`, tsc's general binary-operand rule reduces
    /// to the two precedences below assignment: comma and spread. `yield` is
    /// the deliberate right-associative exception and remains unwrapped.
    ///
    /// tsc-port: getParenthesizeRightSideOfBinaryForOperator(EqualsToken) @6.0.3
    /// tsc-hash: c082a99a3883046ad8f86bee8dfb592228089bcb8d28281b56bda781695c3318
    /// tsc-span: _tsc.js:20313-20324
    /// tsc-port: parenthesizeRightSideOfBinary @6.0.3
    /// tsc-hash: 19c3ea90e1320cd4de06da5b5407e64efcbddf729a0f73ebfa0821401cd3de33
    /// tsc-span: _tsc.js:20411-20434
    fn assignment_right_side_requires_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let emitted = self.skip_partially_emitted_expressions(transformation, expression)?;
        Ok(self.is_comma_sequence(transformation, emitted)?
            || transformation.arena().node(emitted)?.kind == SyntaxKind::SpreadElement)
    }

    fn may_need_dot_dot_for_property_access(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let expression = self.skip_partially_emitted_expressions(transformation, expression)?;
        let record = transformation.arena().node(expression)?;
        if let NodeData::NumericLiteral(data) = &record.data {
            if TokenFlags::from_bits(record.numeric_literal_flags)
                .intersects(TokenFlags::WITH_SPECIFIER)
            {
                return Ok(false);
            }
            let changed = transformation
                .arena()
                .metadata(expression)
                .and_then(crate::EmitMetadata::original)
                .is_some()
                || NodeFlags::from_bits(record.flags).contains(NodeFlags::SYNTHESIZED)
                || self.emission_plan.structured_nodes.contains(&expression);
            let text = if changed {
                data.text.clone()
            } else {
                let source = transformation.arena().source(expression.source())?.syntax();
                let SourceRange::Original(range) =
                    SourceRange::from_raw(record.pos, record.end, source.positions())?
                else {
                    return Ok(false);
                };
                let range = range.without_leading_trivia(source.text(), source.positions())?;
                source
                    .text()
                    .get(range.start().value() as usize..range.end().value() as usize)
                    .ok_or(PrinterError::InvalidTextSlice {
                        start: range.start().value(),
                        end: range.end().value(),
                    })?
                    .to_owned()
            };
            return Ok(!text.contains('.') && !text.contains('e') && !text.contains('E'));
        }
        if matches!(
            record.kind,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            let constant_value = transformation
                .arena()
                .metadata(expression)
                .and_then(crate::EmitMetadata::constant_value);
            return Ok(matches!(
                constant_value,
                Some(crate::EmitConstantValue::Number(value))
                    if value.as_f64().is_finite()
                        && value.as_f64() >= 0.0
                        && value.as_f64().floor() == value.as_f64()
            ));
        }
        Ok(false)
    }

    fn skip_partially_emitted_expressions(
        &self,
        transformation: &TransformationResult<'_>,
        mut expression: TransformNode,
    ) -> Result<TransformNode, PrinterError> {
        loop {
            let NodeData::PartiallyEmittedExpression(data) =
                &transformation.arena().node(expression)?.data
            else {
                return Ok(expression);
            };
            let Some(inner) = data
                .expression
                .and_then(|inner| transformation.arena().node_ref(expression.source(), inner))
            else {
                return Ok(expression);
            };
            expression = inner;
        }
    }

    fn leftmost_expression(
        &self,
        transformation: &TransformationResult<'_>,
        mut expression: TransformNode,
        stop_at_call_expressions: bool,
    ) -> Result<TransformNode, PrinterError> {
        loop {
            let record = transformation.arena().node(expression)?;
            let next = match &record.data {
                NodeData::PostfixUnaryExpression(data) => data.operand,
                NodeData::BinaryExpression(data) => data.left,
                NodeData::ConditionalExpression(data) => data.condition,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                NodeData::CallExpression(_) if stop_at_call_expressions => None,
                NodeData::CallExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next else {
                return Ok(expression);
            };
            expression = transformation
                .arena()
                .node_ref(expression.source(), next)
                .ok_or(PrinterError::UnknownStatement(next.0))?;
        }
    }

    /// `parenthesizeConciseBodyOfArrowFunction`: a concise comma sequence,
    /// or any expression whose leftmost expression is an object literal,
    /// must not be printed as an arrow block. This is a printer-owned grammar
    /// invariant and therefore applies after TypeScript-only wrappers have
    /// been erased by the transformation pipeline.
    ///
    /// tsc-port: parenthesizeConciseBodyOfArrowFunction @6.0.3
    /// tsc-hash: d897c9ce122afdf3756c59034131f8169a92e08d9acd972f5f0856b6084c4cf9
    /// tsc-span: _tsc.js:20514-20521
    fn arrow_concise_body_requires_parentheses(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        body: NodeId,
    ) -> Result<bool, PrinterError> {
        let body = transformation
            .arena()
            .node_ref(source, body)
            .ok_or(PrinterError::UnknownStatement(body.0))?;
        let body_record = transformation.arena().node(body)?;
        if body_record.kind == SyntaxKind::Block {
            return Ok(false);
        }
        if self.is_comma_sequence(transformation, body)? {
            return Ok(true);
        }

        let leftmost = self.leftmost_expression(transformation, body, false)?;
        Ok(transformation.arena().node(leftmost)?.kind == SyntaxKind::ObjectLiteralExpression)
    }

    fn emit_named_import_or_export_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        elements: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = parent.source();
        writer.write_punctuation("{");
        let array = elements.and_then(|id| transformation.arena().node_array_ref(source, id));
        let (ids, trailing_comma, synthesized) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (
                record.nodes.clone(),
                record.has_trailing_comma,
                record.pos == u32::MAX && record.end == u32::MAX,
            )
        } else {
            (Vec::new(), false, false)
        };
        if ids.is_empty() {
            self.emit_empty_node_array_comments(transformation, source, elements, writer)?;
            writer.write_punctuation("}");
            return Ok(());
        }

        writer.write_space(" ");
        for (index, id) in ids.iter().copied().enumerate() {
            let child = transformation
                .arena()
                .node_ref(source, id)
                .ok_or(PrinterError::UnknownStatement(id.0))?;
            if index == 0 || synthesized {
                self.emit_leading_comments_for_delimited_list_start(transformation, child, writer)?;
            } else {
                self.emit_leading_comments_for_node_after_sibling(transformation, child, writer)?;
            }
            self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            self.emit_list_element_end_comments(transformation, child, writer)?;
            if index + 1 < ids.len() || trailing_comma {
                writer.write_punctuation(",");
                self.emit_delimited_trailing_comments_for_node(transformation, child, writer)?;
            }
            if index + 1 < ids.len() {
                writer.write_space(" ");
            }
        }
        self.emit_trailing_node_array_end_comments(transformation, source, elements, writer)?;
        writer.write_space(" ");
        writer.write_punctuation("}");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_renamed_specifier(
        &self,
        transformation: &mut TransformationResult<'_>,
        specifier: TransformNode,
        property_name: Option<tsc_syntax::NodeId>,
        name: Option<tsc_syntax::NodeId>,
        parent: SyntaxKind,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = specifier.source();
        if let Some(property_id) = property_name {
            let property = transformation
                .arena()
                .node_ref(source, property_id)
                .ok_or(PrinterError::UnknownStatement(property_id.0))?;
            self.emit_identifier_name_with_context(
                transformation,
                source,
                property_id,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            writer.write_space(" ");
            let as_keyword = self.emit_space_prefixed_token_with_comments(
                transformation,
                specifier,
                FixedToken::keyword(SyntaxKind::AsKeyword),
                self.original_node_end_cursor(transformation, property)?,
                false,
                writer,
            )?;
            writer.write_space(" ");
            let name_node = name.and_then(|name| transformation.arena().node_ref(source, name));
            let prefix = self.token_owned_child_prefix(transformation, as_keyword, name_node)?;
            if let Some(name_node) = name_node {
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    name_node,
                    LeadingCommentContext::Normal,
                    prefix,
                    writer,
                )?;
            }
        }
        self.emit_required_identifier_name_with_context(
            transformation,
            source,
            name,
            parent,
            "name",
            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
            writer,
        )
    }

    fn emit_modifiers(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<bool, PrinterError> {
        let Some(array) =
            modifiers.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(false);
        };
        let items = transformation
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .copied()
            .map(|node| {
                let kind = transformation
                    .arena()
                    .node(TransformNode::new(source, node))?
                    .kind;
                Ok(ModifierListItem {
                    node,
                    kind: ModifierListItemKind::from_syntax_kind(kind),
                })
            })
            .collect::<Result<Vec<_>, TransformError>>()?;

        for (index, item) in items.iter().enumerate() {
            match item.kind {
                ModifierListItemKind::Decorator => {
                    writer.write_line(false);
                    self.emit_node_id_with_context(
                        transformation,
                        source,
                        item.node,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    writer.write_line(false);
                }
                ModifierListItemKind::Modifier => {
                    if index != 0 && items[index - 1].kind == ModifierListItemKind::Modifier {
                        writer.write_space(" ");
                    }
                    self.emit_node_id_with_context(
                        transformation,
                        source,
                        item.node,
                        expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                        writer,
                    )?;
                    if items
                        .get(index + 1)
                        .is_some_and(|next| next.kind == ModifierListItemKind::Decorator)
                    {
                        writer.write_space(" ");
                    }
                }
            }
        }

        Ok(items
            .last()
            .is_some_and(|item| item.kind == ModifierListItemKind::Modifier))
    }

    fn parenthesized_no_asi_expression(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<Option<ParenthesizedNoAsiExpression>, PrinterError> {
        if self.options.remove_comments {
            return Ok(None);
        }
        let NodeData::PartiallyEmittedExpression(data) =
            &transformation.arena().node(expression)?.data
        else {
            return Ok(None);
        };
        if !self.will_emit_leading_new_line(transformation, expression)? {
            return Ok(None);
        }

        let Some(parse_node) = transformation.arena().parse_tree_node(expression)? else {
            return Ok(Some(ParenthesizedNoAsiExpression::SyntheticWhole {
                wrapper: expression,
            }));
        };
        let parse_record = transformation.arena().node(parse_node)?;
        if parse_record.kind != SyntaxKind::ParenthesizedExpression {
            return Ok(Some(ParenthesizedNoAsiExpression::SyntheticWhole {
                wrapper: expression,
            }));
        }
        let Some(inner) = data
            .expression
            .and_then(|inner| transformation.arena().node_ref(expression.source(), inner))
        else {
            return Ok(None);
        };

        Ok(Some(ParenthesizedNoAsiExpression::Parsed {
            metadata_owner: expression,
            token_owner: parse_node,
            inner,
        }))
    }

    fn no_asi_left_edge_will_parenthesize(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let mut current = Some(expression);
        while let Some(expression) = current {
            if self
                .parenthesized_no_asi_expression(transformation, expression)?
                .is_some()
            {
                return Ok(true);
            }
            let next = match &transformation.arena().node(expression)?.data {
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::CallExpression(data) => data.expression,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                NodeData::PostfixUnaryExpression(data) => data.operand,
                NodeData::BinaryExpression(data) => data.left,
                NodeData::ConditionalExpression(data) => data.condition,
                _ => None,
            };
            current =
                next.and_then(|next| transformation.arena().node_ref(expression.source(), next));
        }
        Ok(false)
    }

    /// Rust-native `willEmitLeadingNewLine`. Comment ranges retain their
    /// source kind/newline bit, and synthetic comments expose the same typed
    /// information, so ASI safety never depends on scanning comment text.
    ///
    /// tsc-port: willEmitLeadingNewLine @6.0.3
    /// tsc-hash: 8cb68561fa1cfbb44b49b2b64e763e26fe9867765597f7eec92c32bfe3924e9c
    /// tsc-span: _tsc.js:118768-118789
    fn will_emit_leading_new_line(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<bool, PrinterError> {
        let record = transformation.arena().node(expression)?;
        let source = transformation.arena().source(expression.source())?.syntax();
        let position = record.pos as usize;
        let leading_comments = collect_source_comment_ranges(source.text(), position, false);

        if !leading_comments.is_empty() {
            let parse_parent = if let Some(parse_node) =
                transformation.arena().parse_tree_node(expression)?
            {
                transformation
                    .arena()
                    .node(parse_node)?
                    .parent
                    .and_then(|parent| transformation.arena().node_ref(parse_node.source(), parent))
            } else {
                None
            };
            let parse_parent_is_parenthesized = parse_parent
                .map(|parent| transformation.arena().node(parent))
                .transpose()?
                .is_some_and(|parent| parent.kind == SyntaxKind::ParenthesizedExpression);
            if parse_parent_is_parenthesized {
                return Ok(true);
            }
        }
        if leading_comments
            .iter()
            .any(source_comment_will_emit_new_line)
        {
            return Ok(true);
        }
        if transformation
            .arena()
            .metadata(expression)
            .is_some_and(|metadata| {
                metadata
                    .leading_comments()
                    .iter()
                    .any(synthetic_comment_will_emit_new_line)
            })
        {
            return Ok(true);
        }

        let NodeData::PartiallyEmittedExpression(data) = &record.data else {
            return Ok(false);
        };
        let Some(inner) = data
            .expression
            .and_then(|inner| transformation.arena().node_ref(expression.source(), inner))
        else {
            return Ok(false);
        };
        let inner_record = transformation.arena().node(inner)?;
        if record.pos != inner_record.pos
            && collect_source_comment_ranges(source.text(), inner_record.pos as usize, true)
                .iter()
                .any(source_comment_will_emit_new_line)
        {
            return Ok(true);
        }
        self.will_emit_leading_new_line(transformation, inner)
    }

    /// JSX preserve lists deliberately use tsc's `NoInterveningComments`
    /// list format: source comments stay with each child instead of being
    /// claimed by a generated separator. The ordinary node pipeline in tsc
    /// performs that phase; this printer makes the ownership explicit at the
    /// JSX container boundary.
    fn emit_jsx_children(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(array) = array.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(());
        };
        let ids = transformation.arena().node_array(array)?.nodes.clone();
        for id in ids {
            let child = transformation
                .arena()
                .node_ref(source, id)
                .ok_or(PrinterError::UnknownStatement(id.0))?;
            let jsx_text = matches!(
                transformation.arena().node(child)?.kind,
                SyntaxKind::JsxText | SyntaxKind::JsxTextAllWhiteSpaces
            );
            if !jsx_text {
                self.emit_leading_comments_for_node(transformation, child, writer)?;
            }
            self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            if !jsx_text {
                self.emit_trailing_comments_for_node(transformation, child, writer)?;
            }
        }
        Ok(())
    }

    fn emit_jsx_attributes(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        properties: Option<tsc_syntax::NodeArrayId>,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(array) =
            properties.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(());
        };
        let ids = transformation.arena().node_array(array)?.nodes.clone();
        for (index, id) in ids.into_iter().enumerate() {
            let attribute = transformation
                .arena()
                .node_ref(source, id)
                .ok_or(PrinterError::UnknownStatement(id.0))?;
            if index != 0 && !writer.is_at_start_of_line() {
                writer.write_space(" ");
            }
            self.emit_leading_comments_for_node(transformation, attribute, writer)?;
            self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
            self.emit_trailing_comments_for_node(transformation, attribute, writer)?;
        }
        Ok(())
    }

    fn emit_node_array(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
        separator: &str,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(array) = array.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(());
        };
        let ids = transformation.arena().node_array(array)?.nodes.clone();
        for (index, id) in ids.into_iter().enumerate() {
            if index != 0 {
                writer.write(separator);
            }
            self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context.for_child(ExpressionSyntaxContext::NORMAL),
                writer,
            )?;
        }
        Ok(())
    }

    fn node_array_has_elements(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
    ) -> Result<bool, PrinterError> {
        let Some(array) = array.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(false);
        };
        Ok(!transformation.arena().node_array(array)?.nodes.is_empty())
    }

    /// Empty delimited lists retain comments at both NodeArray boundaries.
    /// This is the typed equivalent of tsc's empty `emitNodeList` branch:
    /// trailing comments belong to `children.pos`, while leading comments
    /// before the closing delimiter belong to `children.end`.
    fn emit_empty_node_array_comments(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let Some(array) =
            array.and_then(|array| transformation.arena().node_array_ref(source, array))
        else {
            return Ok(());
        };
        let array = transformation.arena().node_array(array)?;
        if !array.nodes.is_empty() || array.pos == u32::MAX || array.end == u32::MAX {
            return Ok(());
        }
        let syntax = transformation.arena().source(source)?.syntax();
        emit_empty_node_array_boundary_comments(
            syntax.text(),
            array.pos as usize,
            array.end as usize,
            false,
            writer,
        );
        Ok(())
    }

    fn emit_trailing_node_array_end_comments(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let Some(array) =
            array.and_then(|array| transformation.arena().node_array_ref(source, array))
        else {
            return Ok(());
        };
        let array = transformation.arena().node_array(array)?;
        if array.nodes.is_empty() || !array.has_trailing_comma || array.end == u32::MAX {
            return Ok(());
        }
        let syntax = transformation.arena().source(source)?.syntax();
        emit_source_leading_comments_of_position(
            syntax.text(),
            array.end as usize,
            &BTreeSet::new(),
            writer,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_call_arguments(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        source: TransformSourceId,
        arguments: Option<tsc_syntax::NodeArrayId>,
        multi_line: bool,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let (ids, synthesized_array) = arguments
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| {
                (
                    array.nodes.clone(),
                    array.pos == u32::MAX && array.end == u32::MAX,
                )
            })
            .unwrap_or_default();
        if ids.is_empty() {
            self.emit_empty_node_array_comments(transformation, source, arguments, writer)?;
            return Ok(());
        }
        let mut increased_indent = false;
        for (index, id) in ids.iter().copied().enumerate() {
            let node = TransformNode::new(source, id);
            if index == 0 {
                if synthesized_array {
                    self.emit_leading_comments_for_delimited_list_start_in_parent(
                        transformation,
                        parent,
                        node,
                        writer,
                    )?;
                } else {
                    self.emit_leading_comments_for_delimited_list_start(
                        transformation,
                        node,
                        writer,
                    )?;
                }
            } else {
                if !synthesized_array {
                    // Parsed argument arrays: the previous element's end
                    // comments (f(1 /*t1*/, 2)) emit before its comma,
                    // exactly as emitNodeListItems orders them; the
                    // synthesized branch already runs this per element.
                    self.emit_list_element_end_comments_in_container(
                        transformation,
                        TransformNode::new(source, ids[index - 1]),
                        expression_context.comments(),
                        writer,
                    )?;
                }
                writer.write_punctuation(",");
                self.emit_delimited_trailing_comments_for_node(
                    transformation,
                    TransformNode::new(source, ids[index - 1]),
                    writer,
                )?;
                if multi_line && index >= 2 {
                    writer.write_line(false);
                    if index == 2 {
                        writer.increase_indent();
                        increased_indent = true;
                    }
                } else if !writer.is_at_start_of_line() {
                    // A single-line comment after the comma has already
                    // advanced the writer. CallExpressionArguments is a
                    // SingleLine list in tsc, so its ordinary sibling space
                    // is written before that intervening comment and must not
                    // become indentation on the following line.
                    writer.write_space(" ");
                }
                if synthesized_array {
                    self.emit_leading_comments_for_delimited_list_start_in_parent(
                        transformation,
                        parent,
                        node,
                        writer,
                    )?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        node,
                        writer,
                    )?;
                }
            }
            self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context.for_child(ExpressionSyntaxContext::DISALLOWED_COMMA),
                writer,
            )?;
            if synthesized_array {
                // A transformed call owns a fresh argument-list container.
                // Retained source expressions therefore keep the comments at
                // their own end boundary (notably classic JSX children such
                // as `{value/* comment */}`). Parsed argument arrays continue
                // to use their comma/close-delimiter ownership below.
                self.emit_list_element_end_comments_in_container(
                    transformation,
                    node,
                    expression_context.comments(),
                    writer,
                )?;
            }
        }
        if increased_indent {
            writer.decrease_indent();
        }
        if !synthesized_array {
            if let Some(last) = ids.last().copied() {
                self.emit_delimited_list_end_comments_in_container(
                    transformation,
                    TransformNode::new(source, last),
                    expression_context.comments(),
                    writer,
                )?;
            }
        }
        Ok(())
    }

    /// Emit a parsed child whose leading boundary was already visited by a
    /// fixed token. The token owns same-line trailing trivia at that boundary;
    /// the child resumes after it and remains responsible for ordinary
    /// leading comments. Keeping the handoff typed prevents the same resume
    /// from being applied to an adjacent child that merely shares a source.
    fn emit_child_after_token_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        token: TokenEmission,
        child: TransformNode,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let outcome = self.emit_child_after_token_with_context_and_source_extent(
            transformation,
            parent,
            token,
            child,
            expression_context,
            DeferredSourceCommentExtent::LeadingOnly,
            writer,
        )?;
        debug_assert_eq!(outcome, ExpressionSourceCommentsOutcome::LeadingConsumed);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_child_after_token_with_complete_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        token: TokenEmission,
        child: TransformNode,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let outcome = self.emit_child_after_token_with_context_and_source_extent(
            transformation,
            parent,
            token,
            child,
            expression_context,
            DeferredSourceCommentExtent::LeadingAndTrailing,
            writer,
        )?;
        assert!(
            matches!(outcome, ExpressionSourceCommentsOutcome::Complete { .. }),
            "a complete expression comments phase must report trailing ownership"
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_child_after_token_with_context_and_source_extent(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        token: TokenEmission,
        child: TransformNode,
        expression_context: EmitContext,
        extent: DeferredSourceCommentExtent,
        writer: &mut TextWriter,
    ) -> Result<ExpressionSourceCommentsOutcome, PrinterError> {
        let deferred = match extent {
            DeferredSourceCommentExtent::LeadingOnly => {
                DeferredExpressionSourceComments::leading_only(
                    parent,
                    token,
                    expression_context.comments(),
                )
            }
            DeferredSourceCommentExtent::LeadingAndTrailing => {
                DeferredExpressionSourceComments::leading_and_trailing(
                    parent,
                    token,
                    expression_context.comments(),
                )
            }
        };
        self.emit_node_id_with_context_and_source_comments(
            transformation,
            child.source(),
            child.node(),
            expression_context,
            deferred,
            writer,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_required_identifier_name_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_identifier_name_with_context(
            transformation,
            source,
            id,
            expression_context,
            writer,
        )
    }

    /// tsc-port: emitJsxTagName @6.0.3
    /// tsc-hash: ccfb711b5b88cdca03af28c671c9d0a40699f53dec23a65421ffa94f988effaf
    /// tsc-span: _tsc.js:119469-119475
    ///
    /// A plain JSX identifier is a value expression and must pass through
    /// namespace/enum substitution. Qualified and namespaced tag shapes use
    /// their ordinary unspecified emission path.
    #[allow(clippy::too_many_arguments)]
    fn emit_required_jsx_tag_name(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_required_node_with_context(
            transformation,
            source,
            id,
            parent,
            field,
            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
            writer,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_required_node_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_node_id_with_context(transformation, source, id, expression_context, writer)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_required_node_with_context_and_source_extent(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent_node: TransformNode,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: EmitContext,
        extent: DeferredSourceCommentExtent,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        let deferred = DeferredExpressionSourceComments::without_preceding_token(
            parent_node,
            extent,
            expression_context.comments(),
        );
        let outcome = self.emit_node_id_with_context_and_source_comments(
            transformation,
            source,
            id,
            expression_context,
            deferred,
            writer,
        )?;
        match extent {
            DeferredSourceCommentExtent::LeadingOnly => {
                assert_eq!(outcome, ExpressionSourceCommentsOutcome::LeadingConsumed);
            }
            DeferredSourceCommentExtent::LeadingAndTrailing => {
                assert!(matches!(
                    outcome,
                    ExpressionSourceCommentsOutcome::Complete { .. }
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_required_node_with_forwarded_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: EmitContext,
        deferred_source_comments: &mut DeferredExpressionSourceCommentsState,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_node_id_with_forwarded_source_comments(
            transformation,
            source,
            id,
            expression_context,
            deferred_source_comments,
            writer,
        )
    }

    fn emit_node_id_with_forwarded_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        deferred_source_comments: &mut DeferredExpressionSourceCommentsState,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(deferred) = deferred_source_comments.take_pending() else {
            return self.emit_node_id_with_context(
                transformation,
                source,
                id,
                expression_context,
                writer,
            );
        };
        let outcome = self.emit_node_id_with_context_and_source_comments(
            transformation,
            source,
            id,
            expression_context,
            deferred,
            writer,
        )?;
        deferred_source_comments.record_outcome(outcome);
        Ok(())
    }

    fn emit_node_id_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::Expression
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(transformation, node, hint, expression_context, writer)
    }

    fn emit_node_id_with_context_and_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        deferred: DeferredExpressionSourceComments,
        writer: &mut TextWriter,
    ) -> Result<ExpressionSourceCommentsOutcome, PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::Expression
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint_and_source_comments(
            transformation,
            node,
            hint,
            expression_context,
            Some(deferred),
            writer,
        )
    }

    fn emit_identifier_name_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::IdentifierName
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(transformation, node, hint, expression_context, writer)
    }

    fn emit_node_with_hint(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        hint: EmitHint,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let outcome = self.emit_node_with_hint_and_source_comments(
            transformation,
            node,
            hint,
            expression_context,
            None,
            writer,
        )?;
        debug_assert_eq!(outcome, ExpressionSourceCommentsOutcome::None);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_node_with_hint_and_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        hint: EmitHint,
        expression_context: EmitContext,
        mut deferred_source_comments: Option<DeferredExpressionSourceComments>,
        writer: &mut TextWriter,
    ) -> Result<ExpressionSourceCommentsOutcome, PrinterError> {
        transformation.before_emit_node(hint, node)?;
        let emitted = (|| {
            let substituted = transformation.substitute_node(hint, node)?;
            let was_substituted = substituted != node;
            let grammar = expression_context.grammar();
            // pipelineEmit 117173-117211 applies the parenthesizer to
            // a substitution before entering the comments phase. The
            // substituted node's synthetic comments therefore belong
            // inside the generated parentheses (not after them).
            let grammar_parentheses = if grammar == ExpressionGrammarContext::ExpressionStatement {
                self.expression_statement_requires_parentheses(transformation, substituted)?
                    .then_some(GrammarParentheses::SourceRanged)
            } else if matches!(
                grammar,
                ExpressionGrammarContext::LeftSideOfAccess { .. }
                    | ExpressionGrammarContext::NewCallee
                    | ExpressionGrammarContext::PrefixUnaryOperand
                    | ExpressionGrammarContext::PostfixUnaryOperand
                    | ExpressionGrammarContext::ArrowConciseBody
                    | ExpressionGrammarContext::AssignmentRightSide
                    | ExpressionGrammarContext::ExportDefault
                    | ExpressionGrammarContext::DisallowedComma
            ) || was_substituted
                && grammar == ExpressionGrammarContext::ComputedPropertyName
            {
                self.context_parentheses(transformation, substituted, grammar)?
            } else {
                None
            };

            let trailing = if let Some(parentheses) = grammar_parentheses {
                match parentheses {
                    GrammarParentheses::SourceRanged => {
                        let owner = self.expression_comment_phase_owner_for_text_range(
                            transformation,
                            substituted,
                        )?;
                        let active_scope = self.active_expression_comment_scope(
                            transformation,
                            deferred_source_comments.as_ref(),
                            expression_context,
                            owner,
                        )?;
                        let _outer_source_leading_phase = self
                            .emit_deferred_expression_leading_comments(
                                transformation,
                                deferred_source_comments.as_ref(),
                                owner,
                                writer,
                            )?;
                        writer.write_punctuation("(");
                        let inner_owner = self
                            .expression_comment_phase_owner_for_node(transformation, substituted)?;
                        let inner_comments = DeferredExpressionSourceComments::nested(
                            active_scope,
                            DeferredSourceCommentExtent::LeadingAndTrailing,
                        );
                        let inner_source_leading_phase = self
                            .emit_deferred_expression_leading_comments(
                                transformation,
                                Some(&inner_comments),
                                inner_owner,
                                writer,
                            )?;
                        let inner_active_scope = self.active_expression_comment_scope(
                            transformation,
                            Some(&inner_comments),
                            expression_context.for_wrapper(active_scope),
                            inner_owner,
                        )?;
                        self.emit_substituted_node_with_comments(
                            transformation,
                            substituted,
                            expression_context.for_wrapper(inner_active_scope),
                            inner_source_leading_phase,
                            writer,
                        )?;
                        self.emit_deferred_expression_trailing_comments(
                            transformation,
                            Some(&inner_comments),
                            inner_owner,
                            writer,
                        )?;
                        writer.write_punctuation(")");
                        self.emit_deferred_expression_trailing_comments(
                            transformation,
                            deferred_source_comments.as_ref(),
                            owner,
                            writer,
                        )?
                    }
                    GrammarParentheses::Synthetic => {
                        let owner = self
                            .expression_comment_phase_owner_for_node(transformation, substituted)?;
                        let active_scope = self.active_expression_comment_scope(
                            transformation,
                            deferred_source_comments.as_ref(),
                            expression_context,
                            owner,
                        )?;
                        writer.write_punctuation("(");
                        let source_leading_phase = self.emit_deferred_expression_leading_comments(
                            transformation,
                            deferred_source_comments.as_ref(),
                            owner,
                            writer,
                        )?;
                        self.emit_substituted_node_with_comments(
                            transformation,
                            substituted,
                            expression_context.for_wrapper(active_scope),
                            source_leading_phase,
                            writer,
                        )?;
                        let trailing = self.emit_deferred_expression_trailing_comments(
                            transformation,
                            deferred_source_comments.as_ref(),
                            owner,
                            writer,
                        )?;
                        writer.write_punctuation(")");
                        trailing
                    }
                }
            } else if expression_context.carries_no_asi_left_edge() {
                if let Some(parenthesized) =
                    self.parenthesized_no_asi_expression(transformation, substituted)?
                {
                    self.emit_parenthesized_no_asi_expression(
                        transformation,
                        parenthesized,
                        expression_context,
                        deferred_source_comments.as_ref(),
                        writer,
                    )?
                } else if deferred_source_comments.is_some()
                    && self.no_asi_left_edge_will_parenthesize(transformation, substituted)?
                {
                    return self.emit_substituted_node_with_forwarded_source_comments(
                        transformation,
                        substituted,
                        expression_context,
                        deferred_source_comments
                            .take()
                            .expect("checked deferred source comments"),
                        writer,
                    );
                } else {
                    let owner =
                        self.expression_comment_phase_owner_for_node(transformation, substituted)?;
                    let active_scope = self.active_expression_comment_scope(
                        transformation,
                        deferred_source_comments.as_ref(),
                        expression_context,
                        owner,
                    )?;
                    let source_leading_phase = self.emit_deferred_expression_leading_comments(
                        transformation,
                        deferred_source_comments.as_ref(),
                        owner,
                        writer,
                    )?;
                    self.emit_substituted_node_with_comments(
                        transformation,
                        substituted,
                        expression_context.with_comments(active_scope),
                        source_leading_phase,
                        writer,
                    )?;
                    self.emit_deferred_expression_trailing_comments(
                        transformation,
                        deferred_source_comments.as_ref(),
                        owner,
                        writer,
                    )?
                }
            } else {
                let owner =
                    self.expression_comment_phase_owner_for_node(transformation, substituted)?;
                let active_scope = self.active_expression_comment_scope(
                    transformation,
                    deferred_source_comments.as_ref(),
                    expression_context,
                    owner,
                )?;
                let source_leading_phase = self.emit_deferred_expression_leading_comments(
                    transformation,
                    deferred_source_comments.as_ref(),
                    owner,
                    writer,
                )?;
                self.emit_substituted_node_with_comments(
                    transformation,
                    substituted,
                    expression_context.with_comments(active_scope),
                    source_leading_phase,
                    writer,
                )?;
                self.emit_deferred_expression_trailing_comments(
                    transformation,
                    deferred_source_comments.as_ref(),
                    owner,
                    writer,
                )?
            };
            let outcome = deferred_source_comments.as_ref().map_or(
                ExpressionSourceCommentsOutcome::None,
                |deferred| {
                    if deferred.owns_trailing() {
                        ExpressionSourceCommentsOutcome::Complete {
                            trailing: trailing.expect(
                                "a complete expression comments phase returns trailing ownership",
                            ),
                        }
                    } else {
                        debug_assert!(trailing.is_none());
                        ExpressionSourceCommentsOutcome::LeadingConsumed
                    }
                },
            );
            Ok(outcome)
        })();
        let notification = transformation.after_emit_node(hint, node);
        match emitted {
            Ok(outcome) => {
                notification?;
                Ok(outcome)
            }
            Err(error) => {
                // Restore transformer notification state even when the node
                // worker fails, while preserving the primary printer error.
                let _ = notification;
                Err(error)
            }
        }
    }

    /// Emit the node shape selected by `parenthesizeExpressionForNoAsi`.
    /// Synthetic parentheses contain the complete wrapper comments phase;
    /// source-owned parentheses instead place wrapper metadata outside their
    /// token/comment phase, matching tsc's `setOriginalNode` + `setTextRange`
    /// virtual container without allocating a transient arena node.
    fn emit_parenthesized_no_asi_expression(
        &self,
        transformation: &mut TransformationResult<'_>,
        parenthesized: ParenthesizedNoAsiExpression,
        expression_context: EmitContext,
        deferred_source_comments: Option<&DeferredExpressionSourceComments>,
        writer: &mut TextWriter,
    ) -> Result<Option<TrailingSourceCommentOwnership>, PrinterError> {
        let trailing = match parenthesized {
            ParenthesizedNoAsiExpression::SyntheticWhole { wrapper } => {
                let owner =
                    self.expression_comment_phase_owner_for_node(transformation, wrapper)?;
                let active_scope = self.active_expression_comment_scope(
                    transformation,
                    deferred_source_comments,
                    expression_context,
                    owner,
                )?;
                writer.write_punctuation("(");
                let source_leading_phase = self.emit_deferred_expression_leading_comments(
                    transformation,
                    deferred_source_comments,
                    owner,
                    writer,
                )?;
                self.emit_substituted_node_with_comments(
                    transformation,
                    wrapper,
                    expression_context.for_wrapper(active_scope),
                    source_leading_phase,
                    writer,
                )?;
                let trailing = self.emit_deferred_expression_trailing_comments(
                    transformation,
                    deferred_source_comments,
                    owner,
                    writer,
                )?;
                writer.write_punctuation(")");
                trailing
            }
            ParenthesizedNoAsiExpression::Parsed {
                metadata_owner,
                token_owner,
                inner,
            } => {
                let owner = self.parsed_no_asi_comment_phase_owner(
                    transformation,
                    metadata_owner,
                    token_owner,
                )?;
                let active_scope = self.active_expression_comment_scope(
                    transformation,
                    deferred_source_comments,
                    expression_context,
                    owner,
                )?;
                let source_leading_phase = self.emit_deferred_expression_leading_comments(
                    transformation,
                    deferred_source_comments,
                    owner,
                    writer,
                )?;
                self.emit_substituted_leading_metadata(
                    transformation,
                    metadata_owner,
                    source_leading_phase,
                    writer,
                )?;
                // h2-6a-m-2 §4 (review F7): the parsed no-ASI wrapper
                // bypasses both node brackets; upstream pipelines the
                // parsed parenthesized expression, so its boundary maps
                // bracket the whole `( child )` extent.
                self.record_node_map_boundary(
                    transformation,
                    MapBoundary::Before,
                    metadata_owner,
                    writer,
                )?;
                let open = self.emit_token_with_comments(
                    transformation,
                    token_owner,
                    FixedToken::punctuation(SyntaxKind::OpenParenToken),
                    self.node_start_cursor(transformation, token_owner)?,
                    false,
                    writer,
                )?;
                self.emit_child_after_token_with_context(
                    transformation,
                    metadata_owner,
                    open,
                    inner,
                    expression_context.for_wrapper(active_scope),
                    writer,
                )?;
                self.emit_token_with_comments(
                    transformation,
                    token_owner,
                    FixedToken::punctuation(SyntaxKind::CloseParenToken),
                    self.node_end_cursor(transformation, inner)?,
                    false,
                    writer,
                )?;
                self.record_node_map_boundary(
                    transformation,
                    MapBoundary::After,
                    metadata_owner,
                    writer,
                )?;
                self.emit_synthetic_trailing_comments_for_node(
                    transformation,
                    metadata_owner,
                    writer,
                )?;
                self.emit_deferred_expression_trailing_comments(
                    transformation,
                    deferred_source_comments,
                    owner,
                    writer,
                )?
            }
        };
        Ok(trailing)
    }

    /// Comments-phase adapter for a node whose emit substitution has
    /// already completed. Keeping this phase separate makes it impossible
    /// for a grammar parenthesis selected above to strand the substituted
    /// node's synthetic comments outside its container.
    fn emit_substituted_node_with_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        substituted: TransformNode,
        expression_context: EmitContext,
        source_leading_phase: SourceLeadingCommentPhaseVisit,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_substituted_leading_metadata(
            transformation,
            substituted,
            source_leading_phase,
            writer,
        )?;
        self.emit_transformed_node(transformation, substituted, expression_context, writer)?;
        self.emit_synthetic_trailing_comments_for_node(transformation, substituted, writer)
    }

    /// A no-ASI parenthesizer updates only the left edge of compound
    /// expressions. Move the single source-comment request through that same
    /// edge until the node that either creates the virtual parentheses or
    /// owns the ordinary phase consumes it. This models tsc's updated factory
    /// chain without allocating transient arena nodes or using printer-global
    /// comment state.
    fn emit_substituted_node_with_forwarded_source_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        substituted: TransformNode,
        expression_context: EmitContext,
        deferred: DeferredExpressionSourceComments,
        writer: &mut TextWriter,
    ) -> Result<ExpressionSourceCommentsOutcome, PrinterError> {
        self.emit_substituted_leading_metadata(
            transformation,
            substituted,
            SourceLeadingCommentPhaseVisit::NotVisited,
            writer,
        )?;
        // h2-6a-m-2 §4: the no-ASI forwarding lane bypasses
        // emit_transformed_node, so it carries the same node-boundary
        // map bracket and NO_NESTED_SOURCE_MAPS extent.
        self.record_node_map_boundary(transformation, MapBoundary::Before, substituted, writer)?;
        let suppress_nested_maps =
            transformation
                .arena()
                .metadata(substituted)
                .is_some_and(|metadata| {
                    metadata
                        .flags()
                        .intersects(EmitFlags::NO_NESTED_SOURCE_MAPS)
                });
        if suppress_nested_maps {
            if let Some(recording) = writer.recording_mut() {
                recording.suppress();
            }
        }
        let mut state = DeferredExpressionSourceCommentsState::Pending(deferred);
        let worker_result = self.emit_transformed_node_worker(
            transformation,
            substituted,
            expression_context,
            &mut state,
            writer,
        );
        if suppress_nested_maps {
            if let Some(recording) = writer.recording_mut() {
                recording.unsuppress();
            }
        }
        worker_result?;
        self.record_node_map_boundary(transformation, MapBoundary::After, substituted, writer)?;
        self.emit_synthetic_trailing_comments_for_node(transformation, substituted, writer)?;
        match state {
            DeferredExpressionSourceCommentsState::Consumed(outcome) => Ok(outcome),
            DeferredExpressionSourceCommentsState::Pending(_) => {
                panic!("a no-ASI left-edge worker must move its source-comment request")
            }
            DeferredExpressionSourceCommentsState::Inactive => {
                panic!("a no-ASI left-edge child must report source-comment ownership")
            }
        }
    }

    /// The comments phase that tsc applies to a node before its worker. Keep
    /// it separate so a virtual node can place the same metadata either
    /// inside synthetic parentheses or outside source-owned parentheses.
    fn emit_substituted_leading_metadata(
        &self,
        transformation: &TransformationResult<'_>,
        substituted: TransformNode,
        source_leading_phase: SourceLeadingCommentPhaseVisit,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_synthetic_leading_comments_for_node(transformation, substituted, writer)?;
        if let Some(comment_source) = transformation
            .arena()
            .metadata(substituted)
            .and_then(|metadata| metadata.class_field_initializer_comment_source)
        {
            let comment_range = self.comment_range_for_node(transformation, comment_source)?;
            if !source_leading_phase.visited_range(comment_range) {
                self.emit_leading_comments_for_node(transformation, comment_source, writer)?;
            }
        }
        Ok(())
    }

    fn write_original_without_leading_trivia(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let range = SourceRange::from_raw(record.pos, record.end, source.positions())?;
        let SourceRange::Original(range) = range else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let range = range.without_leading_trivia(source.text(), source.positions())?;
        let start = range.start().value();
        let end = range.end().value();
        let slice = source
            .text()
            .get(start as usize..end as usize)
            .ok_or(PrinterError::InvalidTextSlice { start, end })?;
        let normalized = normalize_new_lines(slice, self.options.new_line.text());
        writer.write(&normalized);
        Ok(())
    }

    /// A cloned identifier can be structurally synthetic while its spelling
    /// still belongs to the parsed identifier it denotes. Reuse the source
    /// slice only when both semantic text and the complete token range agree;
    /// renamed identifiers continue through the canonical writer.
    ///
    /// tsc-port: printer/getTextOfNode @6.0.3
    /// tsc-hash: bfe06a8f5079928e8ff23d6e34aad8110eb12b343b26a300861290242c5403e4
    /// tsc-span: _tsc.js:120442-120465
    fn transformed_identifier_can_reuse_source_spelling(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        text: &str,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        if original == node || original.source() != node.source() {
            return Ok(false);
        }
        let record = transformation.arena().node(node)?;
        let original_record = transformation.arena().node(original)?;
        let NodeData::Identifier(original_identifier) = &original_record.data else {
            return Ok(false);
        };
        if original_identifier.text != text
            || record.pos != original_record.pos
            || record.end != original_record.end
        {
            return Ok(false);
        }
        let source = transformation.arena().source(node.source())?.syntax();
        Ok(matches!(
            SourceRange::from_raw(record.pos, record.end, source.positions())?,
            SourceRange::Original(_)
        ))
    }

    fn write_original_without_leading_trivia_verbatim(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let range = range.without_leading_trivia(source.text(), source.positions())?;
        let start = range.start().value() as usize;
        let end = range.end().value() as usize;
        let slice = source
            .text()
            .get(start..end)
            .ok_or(PrinterError::InvalidTextSlice {
                start: u32::try_from(start).expect("source position exceeds u32"),
                end: u32::try_from(end).expect("source position exceeds u32"),
            })?;
        writer.write(slice);
        Ok(())
    }

    /// tsc's empty-JSX guard probes comments immediately after the opening
    /// brace (`hasCommentsAtPosition(node.pos)`) rather than lexing the node or
    /// complete source file.
    fn original_jsx_has_comments_at_open(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        if transformation.arena().node(original)?.kind != SyntaxKind::JsxExpression {
            return Ok(false);
        }
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(false);
        };
        let open = skip_trivia(source.text(), range.start().value() as usize);
        if source.text().as_bytes().get(open) != Some(&b'{') {
            return Ok(false);
        }
        let position = open + 1;
        Ok(
            !collect_source_comment_ranges(source.text(), position, true).is_empty()
                || !collect_source_comment_ranges(source.text(), position, false).is_empty(),
        )
    }

    fn emit_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::Normal,
            None,
            writer,
        )
    }

    fn emit_leading_comments_for_node_after_sibling(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::AfterSibling,
            None,
            writer,
        )
    }

    /// The statement-position comments phase: the source leading walk and
    /// claims, then the node's synthetic leading comments — tsc's
    /// emitLeadingCommentsOfNode tail order, outside the claim gate
    /// (token and expression routes own their synthetic phases separately).
    ///
    /// tsc-port: emitLeadingCommentsOfNode @6.0.3
    /// tsc-hash: f19ebe6d4e44cddc371b73bea80781c08de19bc1f5747b3e0118aaad0dd28eb4
    /// tsc-span: _tsc.js:121030-121030
    fn emit_statement_leading_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node(transformation, node, writer)?;
        self.emit_synthetic_leading_comments_for_node(transformation, node, writer)
    }

    fn emit_statement_leading_comments_after_sibling(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_after_sibling(transformation, node, writer)?;
        self.emit_synthetic_leading_comments_for_node(transformation, node, writer)
    }

    /// The statement-position trailing phase: synthetic trailing comments
    /// first, then the source trailing walk — tsc's
    /// emitTrailingCommentsOfNode head order, before the source-side
    /// suppressions.
    ///
    /// tsc-port: emitTrailingCommentsOfNode @6.0.3
    /// tsc-hash: 042bc00356dfd3b1d0b40f94c72428f0ae6c43b9743cf8435b716a01fc7b6a1f
    /// tsc-span: _tsc.js:121036-121036
    fn emit_statement_trailing_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_synthetic_trailing_comments_for_node(transformation, node, writer)?;
        self.emit_trailing_comments_for_node(transformation, node, writer)
    }

    fn emit_leading_comments_for_delimited_list_start(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_delimited_list_start_in_container_with_space(
            transformation,
            node,
            CommentEmissionScope::empty(),
            TokenLeadingSpace::None,
            writer,
        )
    }

    fn emit_leading_comments_for_delimited_list_start_with_space(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        leading_space: TokenLeadingSpace,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_delimited_list_start_in_container_with_space(
            transformation,
            node,
            CommentEmissionScope::empty(),
            leading_space,
            writer,
        )
    }

    fn emit_leading_comments_for_delimited_list_start_in_container(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        active_scope: CommentEmissionScope,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_delimited_list_start_in_container_with_space(
            transformation,
            node,
            active_scope,
            TokenLeadingSpace::None,
            writer,
        )
    }

    fn emit_leading_comments_for_delimited_list_start_in_container_with_space(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        active_scope: CommentEmissionScope,
        leading_space: TokenLeadingSpace,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let list_owned = self.emit_intervening_comments_before_node_with_policy(
            transformation,
            node,
            false,
            leading_space,
            writer,
        )?;
        let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
        let container_owned = self.parent_comment_container_owned_prefix_for_owner(
            transformation,
            active_scope.container_pos(),
            owner,
        )?;
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::DelimitedListStart,
            Self::furthest_comment_resume(list_owned, container_owned)?,
            writer,
        )
    }

    /// A synthesized list has no parsed opening delimiter of its own. When
    /// its parent and a retained item start at the same source boundary, the
    /// parent container has already claimed that prefix; otherwise the item
    /// follows the ordinary delimited-list comments phase.
    fn emit_leading_comments_for_delimited_list_start_in_parent(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        // tsc's list callback owns a trailing-position comment phase even
        // when a generated parent and its retained first child share a source
        // start. Parent `containerPos` suppresses only the later ordinary
        // leading phase; it does not suppress that list phase. This distinction
        // is observable in recovery exponentiation lowered to `Math.pow`,
        // where the same source comment is intentionally printed at both the
        // parent boundary and the synthesized argument-list boundary.
        let parent_owner = self.expression_comment_phase_owner_for_node(transformation, parent)?;
        let (pos, end) = Self::established_container_sides(parent_owner);
        self.emit_leading_comments_for_delimited_list_start_in_container(
            transformation,
            node,
            CommentEmissionScope::empty().claim_sides(pos, end),
            writer,
        )
    }

    fn emit_leading_comments_for_multiline_delimited_list_start(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let resume = self.emit_intervening_comments_before_node_with_policy(
            transformation,
            node,
            true,
            TokenLeadingSpace::None,
            writer,
        )?;
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::DelimitedListStart,
            resume,
            writer,
        )
    }

    fn emit_intervening_comments_before_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<Option<CommentResume>, PrinterError> {
        self.emit_intervening_comments_before_node_with_policy(
            transformation,
            node,
            false,
            TokenLeadingSpace::None,
            writer,
        )
    }

    fn emit_intervening_comments_before_node_with_policy(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        suppress_opening_line_comments: bool,
        leading_space: TokenLeadingSpace,
        writer: &mut TextWriter,
    ) -> Result<Option<CommentResume>, PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::NO_LEADING_COMMENTS))
        {
            return Ok(None);
        }
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let source = transformation
            .arena()
            .source(comment_range.source())?
            .syntax();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(None);
        };
        let start = range.start().value() as usize;
        let comments = collect_source_comment_ranges(source.text(), start, true);
        let Some(last_comment) = comments.last().copied() else {
            return Ok(None);
        };
        if suppress_opening_line_comments {
            for comment in comments {
                if !source.text()[start..comment.start]
                    .chars()
                    .any(is_line_break)
                {
                    continue;
                }
                write_source_comment(source.text(), comment.start, comment.end, writer);
                if comment.has_trailing_new_line {
                    writer.write_line(false);
                } else {
                    writer.write_space(" ");
                }
            }
        } else {
            Self::ensure_token_leading_space(writer, leading_space);
            emit_source_intervening_comments_of_position(source.text(), start, writer);
        }
        let code_start = range
            .without_leading_trivia(source.text(), source.positions())?
            .start()
            .value() as usize;
        let position = SourceBytePosition::new(
            u32::try_from(last_comment.end.min(code_start)).unwrap_or(u32::MAX),
            source.positions(),
        )?;
        let owner_start = CommentCursor::new(comment_range.source(), range.start());
        let next = CommentCursor::new(comment_range.source(), position);
        Ok(Some(
            CommentResume::new(owner_start, next).map_err(Self::comment_resume_error)?,
        ))
    }

    /// Project tsc's `containerPos` guard onto one explicit parent/child
    /// boundary. Emitting a parent establishes its source start as the active
    /// comment container whether it emitted the leading trivia or suppressed
    /// it with `NoLeadingComments`. A nested child at that same start must
    /// therefore not claim the trivia again. This is especially observable
    /// when transformModule wraps an assignment in one or more export
    /// assignments positioned at the original assignment.
    ///
    /// The printer keeps this ownership local instead of maintaining tsc's
    /// mutable global comment position. That preserves the same topology
    /// while making the already-owned prefix explicit in Rust's call graph.
    ///
    /// tsc-port: emitLeadingCommentsOfNode @6.0.3
    /// tsc-hash: e2e23efa7ca721b980bc4cd1f391d79d2996a557d99206ae92b86a88c09cf7ee
    /// tsc-span: _tsc.js:120997-121022
    /// tsc-port: forEachLeadingCommentToEmit @6.0.3
    /// tsc-hash: e430ab3df1939b475619858cc8fc260b0004cc801a9522208c9cd0142b04a58e
    /// tsc-span: _tsc.js:121205-121218
    fn token_owned_comment_phase_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        emission: TokenEmission,
        owner: ExpressionCommentPhaseOwner,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let Some(token_resume) = emission.comment_resume() else {
            return Ok(None);
        };
        let Some((cursor_source, _)) = emission.cursor().source_position() else {
            return Ok(None);
        };
        let SourceRange::Original(owner_range) = owner.range.range() else {
            return Ok(None);
        };
        let owner_source = owner.range.source();
        let token_owner = token_resume.owner_start();
        // Substitution and virtual parenthesization may legitimately choose a
        // different comment owner from the fixed token's original child.
        // That is a fresh comments phase, not a malformed continuation.
        if cursor_source != owner_source
            || token_owner.source() != owner_source
            || token_owner.position() != owner_range.start()
        {
            return Ok(None);
        }
        let source = transformation.arena().source(owner_source)?.syntax();
        let code_start = owner_range
            .without_leading_trivia(source.text(), source.positions())?
            .start();
        let next = SourceBytePosition::new(
            token_resume
                .next()
                .position()
                .value()
                .min(code_start.value()),
            source.positions(),
        )?;
        Ok(Some(
            CommentResume::new(
                CommentCursor::new(owner_source, owner_range.start()),
                CommentCursor::new(owner_source, next),
            )
            .map_err(Self::comment_resume_error)?,
        ))
    }

    fn parent_comment_container_owned_prefix_for_owner(
        &self,
        transformation: &TransformationResult<'_>,
        container_pos: Option<CommentCursor>,
        owner: ExpressionCommentPhaseOwner,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let source_id = owner.range.source();
        let SourceRange::Original(owner_range) = owner.range.range() else {
            return Ok(None);
        };
        // tsc's leading guard: source trivia at `pos` belongs to the
        // enclosing container when the child starts exactly where that
        // container was claimed.
        //
        // tsc-port: forEachLeadingCommentToEmit @6.0.3
        // tsc-span: _tsc.js:121219-121233
        if container_pos != Some(CommentCursor::new(source_id, owner_range.start())) {
            return Ok(None);
        }
        let source = transformation.arena().source(source_id)?.syntax();
        let next = owner_range
            .without_leading_trivia(source.text(), source.positions())?
            .start();
        Ok(Some(
            CommentResume::new(
                CommentCursor::new(source_id, owner_range.start()),
                CommentCursor::new(source_id, next),
            )
            .map_err(Self::comment_resume_error)?,
        ))
    }

    fn emit_deferred_expression_leading_comments(
        &self,
        transformation: &TransformationResult<'_>,
        deferred: Option<&DeferredExpressionSourceComments>,
        owner: ExpressionCommentPhaseOwner,
        writer: &mut TextWriter,
    ) -> Result<SourceLeadingCommentPhaseVisit, PrinterError> {
        let Some(deferred) = deferred else {
            return Ok(SourceLeadingCommentPhaseVisit::NotVisited);
        };
        let token_owned = deferred
            .preceding_token
            .map(|token| self.token_owned_comment_phase_prefix(transformation, token, owner))
            .transpose()?
            .flatten();
        let container_owned = deferred
            .container
            .map(|container| self.deferred_container_scope(transformation, container))
            .transpose()?
            .map(|scope| {
                self.parent_comment_container_owned_prefix_for_owner(
                    transformation,
                    scope.container_pos(),
                    owner,
                )
            })
            .transpose()?
            .flatten();
        self.emit_leading_comments_for_comment_phase_owner(
            transformation,
            owner,
            LeadingCommentContext::Normal,
            Self::furthest_comment_resume(token_owned, container_owned)?,
            writer,
        )
    }

    fn emit_deferred_expression_trailing_comments(
        &self,
        transformation: &TransformationResult<'_>,
        deferred: Option<&DeferredExpressionSourceComments>,
        owner: ExpressionCommentPhaseOwner,
        writer: &mut TextWriter,
    ) -> Result<Option<TrailingSourceCommentOwnership>, PrinterError> {
        let Some(deferred) = deferred.filter(|deferred| deferred.owns_trailing()) else {
            return Ok(None);
        };
        if self.options.remove_comments
            || owner.flags.intersects(EmitFlags::NO_TRAILING_COMMENTS)
            || owner.relocated_trailing
            || owner.kind == SyntaxKind::NotEmittedStatement
        {
            return Ok(Some(TrailingSourceCommentOwnership::Suppressed));
        }
        let SourceRange::Original(owner_range) = owner.range.range() else {
            return Ok(Some(TrailingSourceCommentOwnership::NoSourceRange));
        };
        if owner_range.start() == owner_range.end() {
            return Ok(Some(TrailingSourceCommentOwnership::EmptySourceRange));
        }
        if deferred
            .container
            .map(|container| self.deferred_container_scope(transformation, container))
            .transpose()?
            .is_some_and(|scope| {
                scope.retains_end(CommentCursor::new(owner.range.source(), owner_range.end()))
            })
        {
            return Ok(Some(TrailingSourceCommentOwnership::RetainedByParent));
        }
        let source = transformation
            .arena()
            .source(owner.range.source())?
            .syntax();
        let boundary = owner_range.end().value() as usize;
        let emitted_through = emit_same_line_trailing_comments(
            SourceTrivia::from_start(source.text(), boundary),
            writer,
        )
        .map(|end| SourceBytePosition::new(end as u32, source.positions()))
        .transpose()?;
        let cursor = TokenCursor::source(owner.range.source(), owner_range.end());
        let resume = emitted_through
            .map(|next| {
                CommentResume::new(
                    CommentCursor::new(owner.range.source(), owner_range.end()),
                    CommentCursor::new(owner.range.source(), next),
                )
                .map_err(Self::comment_resume_error)
            })
            .transpose()?;
        let anchor = TokenAnchor::new(cursor, resume);
        Ok(Some(TrailingSourceCommentOwnership::VisitedHere { anchor }))
    }

    fn parent_comment_container_owned_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let parent_range = self.comment_range_for_node(transformation, parent)?;
        let owner = self.expression_comment_phase_owner_for_node(transformation, child)?;
        self.parent_comment_container_owned_prefix_for_owner(
            transformation,
            CommentEmissionScope::container_pos_of(parent_range),
            owner,
        )
    }

    fn furthest_comment_resume(
        left: Option<CommentResume>,
        right: Option<CommentResume>,
    ) -> Result<Option<CommentResume>, PrinterError> {
        match (left, right) {
            (None, resume) | (resume, None) => Ok(resume),
            (Some(left), Some(right)) => left
                .furthest(right)
                .map(Some)
                .map_err(Self::comment_resume_error),
        }
    }

    fn comment_resume_error(error: CommentResumeError) -> PrinterError {
        match error {
            CommentResumeError::SourceMismatch { owner_start, next } => {
                PrinterError::CommentCursorSourceMismatch {
                    cursor: next.source(),
                    owner: owner_start.source(),
                }
            }
            CommentResumeError::BeforeOwner { owner_start, next } => {
                PrinterError::CommentResumeBeforeOwner {
                    source: owner_start.source(),
                    owner_start: owner_start.position().value(),
                    next: next.position().value(),
                }
            }
            CommentResumeError::OwnerMismatch { left, right } => {
                if left.source() != right.source() {
                    PrinterError::CommentCursorSourceMismatch {
                        cursor: right.source(),
                        owner: left.source(),
                    }
                } else {
                    PrinterError::CommentResumeOwnerMismatch {
                        source: left.source(),
                        left_start: left.position().value(),
                        right_start: right.position().value(),
                    }
                }
            }
        }
    }

    fn emit_leading_comments_for_node_worker(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        context: LeadingCommentContext,
        resume: Option<CommentResume>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
        self.emit_leading_comments_for_comment_phase_owner(
            transformation,
            owner,
            context,
            resume,
            writer,
        )
        .map(|_| ())
    }

    fn emit_leading_comments_for_comment_phase_owner(
        &self,
        transformation: &TransformationResult<'_>,
        owner: ExpressionCommentPhaseOwner,
        context: LeadingCommentContext,
        resume: Option<CommentResume>,
        writer: &mut TextWriter,
    ) -> Result<SourceLeadingCommentPhaseVisit, PrinterError> {
        if self.options.remove_comments || owner.flags.intersects(EmitFlags::NO_LEADING_COMMENTS) {
            return Ok(SourceLeadingCommentPhaseVisit::Suppressed);
        }
        let source = transformation
            .arena()
            .source(owner.range.source())?
            .syntax();
        let SourceRange::Original(range) = owner.range.range() else {
            return Ok(SourceLeadingCommentPhaseVisit::Suppressed);
        };
        if range.start() == range.end() {
            return Ok(SourceLeadingCommentPhaseVisit::Suppressed);
        }
        let start = range.start().value() as usize;
        let code_start = range
            .without_leading_trivia(source.text(), source.positions())?
            .start()
            .value() as usize;
        // A NotEmittedStatement is a range/ownership anchor, not an emitted
        // statement. tsc suppresses its ordinary comments, but its special
        // `isEmittedNode=false` branch preserves recognized triple-slash
        // pragmas when the erased statement starts at source position zero.
        if owner.kind == SyntaxKind::NotEmittedStatement {
            if start == 0 && code_start > start && resume.is_none() {
                emit_triple_slash_leading_comments(&source.text()[start..code_start], writer);
            }
            return Ok(SourceLeadingCommentPhaseVisit::Suppressed);
        }
        if code_start > start {
            let trivia_start = if let Some(resume) = resume {
                let owner_start = resume.owner_start();
                if owner_start.source() != owner.range.source() {
                    return Err(PrinterError::CommentCursorSourceMismatch {
                        cursor: owner_start.source(),
                        owner: owner.range.source(),
                    });
                }
                if owner_start.position() != range.start() {
                    return Err(PrinterError::CommentResumeOwnerMismatch {
                        source: owner.range.source(),
                        left_start: range.start().value(),
                        right_start: owner_start.position().value(),
                    });
                }
                usize::try_from(resume.next().position().value())
                    .expect("source position fits usize")
            } else {
                start
            };
            source
                .text()
                .get(trivia_start..code_start)
                .ok_or(PrinterError::InvalidTextSlice {
                    start: u32::try_from(trivia_start).unwrap_or(u32::MAX),
                    end: u32::try_from(code_start).unwrap_or(u32::MAX),
                })?;
            if context != LeadingCommentContext::DelimitedListStart {
                // `getLeadingCommentRanges(text, node.pos)` starts collecting
                // only after the first line break. Trivia on the opening line
                // belongs to the preceding token/container, regardless of
                // that token's spelling. This matters when a transform erases
                // or shortens a token (for example optional `?.` becoming
                // `.`): the generated token cursor remains arithmetic, while
                // the child's ordinary leading-comment boundary must still
                // use tsc's source classification. Delimited-list starts are
                // different: their opening delimiter deliberately owns and
                // emits the intervening same-line comments.
                emit_source_leading_comments_of_position(
                    source.text(),
                    trivia_start,
                    &BTreeSet::new(),
                    writer,
                );
            } else {
                emit_leading_comments(
                    SourceTrivia::new(source.text(), trivia_start, code_start),
                    writer,
                    true,
                );
            }
        }
        Ok(SourceLeadingCommentPhaseVisit::Visited { range: owner.range })
    }

    fn detached_source_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<Option<DetachedCommentPrefix>, PrinterError> {
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let comment_source = comment_range.source();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(None);
        };
        self.detached_comment_prefix_at(transformation, comment_source, range.start())
    }

    /// The detached-comment boundary belongs to the container range, rather
    /// than to the first transformed list item. This is the typed equivalent
    /// of tsc's `emitBodyWithDetachedComments(node, node.statements, ...)` and
    /// keeps a generated prologue from taking over comments that precede the
    /// original body.
    fn detached_comment_prefix_for_node_array(
        &self,
        transformation: &TransformationResult<'_>,
        array: TransformNodeArray,
    ) -> Result<Option<DetachedCommentPrefix>, PrinterError> {
        let source = transformation.arena().source(array.source())?.syntax();
        let record = transformation.arena().node_array(array)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(None);
        };
        self.detached_comment_prefix_at(transformation, array.source(), range.start())
    }

    /// A relocated module body does not own an ordinary block statement-list
    /// prefix. Its prefix was emitted by the outer SourceFile, whose detached
    /// comment owner is the first parsed statement rather than the current
    /// (possibly synthesized) NodeArray boundary. Recreate that exact owner so
    /// `PendingDetachedComments` can resume the first retained statement and
    /// cannot emit the SourceFile-owned prefix a second time.
    fn detached_source_file_prefix_for_relocated_statement_list(
        &self,
        transformation: &TransformationResult<'_>,
        original: TransformNodeArray,
    ) -> Result<Option<DetachedCommentPrefix>, PrinterError> {
        let first = transformation
            .arena()
            .node_array(original)?
            .nodes
            .first()
            .copied()
            .and_then(|node| transformation.arena().node_ref(original.source(), node));
        first
            .map(|first| self.detached_source_prefix(transformation, first))
            .transpose()
            .map(Option::flatten)
    }

    fn detached_comment_prefix_at(
        &self,
        transformation: &TransformationResult<'_>,
        source_id: TransformSourceId,
        owner_start: SourceBytePosition,
    ) -> Result<Option<DetachedCommentPrefix>, PrinterError> {
        let source = transformation.arena().source(source_id)?.syntax();
        let start = owner_start.value() as usize;
        let code_start = skip_trivia(source.text(), start);
        let (emitted_end, policy) = if self.options.remove_comments {
            let Some(detached_end) = detached_pinned_comment_end(source.text(), start, code_start)
            else {
                return Ok(None);
            };
            (detached_end, DetachedSourceCommentPolicy::PinnedOnly)
        } else {
            let Some(detached) = detached_leading_trivia(&source.text()[start..code_start]) else {
                return Ok(None);
            };
            (
                start.saturating_add(detached.len()),
                DetachedSourceCommentPolicy::All,
            )
        };
        let Some(last_detached_comment) =
            collect_source_comment_ranges(source.text(), start, false)
                .into_iter()
                .take_while(|comment| comment.end <= emitted_end)
                .last()
        else {
            return Ok(None);
        };
        let emitted_through = CommentCursor::new(
            source_id,
            SourceBytePosition::new(
                u32::try_from(emitted_end).unwrap_or(u32::MAX),
                source.positions(),
            )?,
        );
        let resume_next = CommentCursor::new(
            source_id,
            SourceBytePosition::new(
                u32::try_from(last_detached_comment.end).unwrap_or(u32::MAX),
                source.positions(),
            )?,
        );
        let resume = CommentResume::new(CommentCursor::new(source_id, owner_start), resume_next)
            .map_err(Self::comment_resume_error)?;
        Ok(Some(DetachedCommentPrefix {
            emitted_through,
            resume,
            policy,
        }))
    }

    /// Mirrors the source-file branch of tsc's
    /// `emitBodyWithDetachedComments`: the transformed statement array owns
    /// the detached prefix only while it still has a parsed range. A parsed
    /// leading directive owns its own comments; a synthesized directive lets
    /// the source-file body own them after the directive has been emitted.
    fn source_file_owns_detached_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        statements: Option<TransformNodeArray>,
        first_statement: Option<NodeId>,
    ) -> Result<bool, PrinterError> {
        let Some(statements) = statements else {
            return Ok(false);
        };
        let source = transformation.arena().source(statements.source())?.syntax();
        let statement_array = transformation.arena().node_array(statements)?;
        if !matches!(
            SourceRange::from_raw(statement_array.pos, statement_array.end, source.positions())?,
            SourceRange::Original(_)
        ) {
            return Ok(false);
        }
        let Some(first_statement) = first_statement.and_then(|statement| {
            transformation
                .arena()
                .node_ref(statements.source(), statement)
        }) else {
            return Ok(true);
        };
        if !self.is_prologue_statement(transformation, first_statement) {
            return Ok(true);
        }
        let first = transformation.arena().node(first_statement)?;
        Ok(matches!(
            SourceRange::from_raw(first.pos, first.end, source.positions())?,
            SourceRange::Synthesized
        ))
    }

    fn emit_detached_comment_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        prefix: Option<DetachedCommentPrefix>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(prefix) = prefix else {
            return Ok(());
        };
        let owner_start = prefix.resume.owner_start();
        let source = transformation
            .arena()
            .source(owner_start.source())?
            .syntax();
        let start = owner_start.position().value() as usize;
        if prefix.emitted_through.source() != owner_start.source() {
            return Err(PrinterError::CommentCursorSourceMismatch {
                cursor: prefix.emitted_through.source(),
                owner: owner_start.source(),
            });
        }
        let end = usize::try_from(prefix.emitted_through.position().value())
            .expect("source position fits usize");
        source
            .text()
            .get(start..end)
            .ok_or(PrinterError::InvalidTextSlice {
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: u32::try_from(end).unwrap_or(u32::MAX),
            })?;
        let trivia = SourceTrivia::new(source.text(), start, end);
        match prefix.policy {
            DetachedSourceCommentPolicy::All => emit_leading_comments(trivia, writer, true),
            DetachedSourceCommentPolicy::PinnedOnly => emit_pinned_leading_comments(trivia, writer),
        }
        Ok(())
    }

    fn take_detached_comment_resume_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        pending: &mut PendingDetachedComments,
        node: TransformNode,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let comment_source = comment_range.source();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(None);
        };
        Ok(pending.take_for(CommentCursor::new(comment_source, range.start())))
    }

    fn emit_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                        || metadata.relocated_trailing_comment_owner.is_some()
                })
        {
            return Ok(());
        }
        if transformation.arena().node(node)?.kind == SyntaxKind::NotEmittedStatement {
            return Ok(());
        }
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let source = transformation
            .arena()
            .source(comment_range.source())?
            .syntax();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(());
        };
        if range.start() == range.end() {
            return Ok(());
        }
        emit_same_line_trailing_comments(
            SourceTrivia::from_start(source.text(), range.end().value() as usize),
            writer,
        );
        Ok(())
    }

    /// Emit a child's same-line trailing comments and carry their ownership
    /// into the fixed token anchored at the same source boundary.
    ///
    /// This is the local, typed equivalent of tsc consulting its emitted
    /// comment map when a fixed clause token such as `else` or `finally`
    /// follows an already-emitted child.
    fn emit_trailing_comments_for_node_as_token_anchor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<TokenAnchor, PrinterError> {
        let cursor = self.original_node_end_cursor(transformation, node)?;
        self.emit_trailing_comments_for_node_at_cursor(transformation, node, cursor, writer)
    }

    /// Emit the retained arrow token through the comments phases of tsc's
    /// generic `emit(node.equalsGreaterThanToken)` path.
    ///
    /// This adapter is intentionally arrow-only and requires substitution and
    /// emit-notification capabilities to be disabled for the token. Those
    /// hooks surround/select the comments phase in tsc, while this worker must
    /// return the comments phase's typed continuation to the arrow body. A
    /// release-safe guard below prevents a future transformer from silently
    /// violating that ordering. Fixed-token emission remains a separate
    /// operation: a retained arrow owns comments at its own `end`, even though
    /// that position equals the end of its spelling.
    ///
    /// The returned cursor is based on the token's comment range rather than
    /// its semantic original. A genuinely synthetic token therefore cannot
    /// borrow source-comment ownership merely by carrying original-node
    /// provenance for substitution or source maps.
    ///
    /// tsc-port: emitArrowFunctionHead @6.0.3
    /// tsc-hash: 8840bc47361e2a70f5699813f733287d8b015aff6fc2963c0e91796e372eb0eb
    /// tsc-span: _tsc.js:118336-118350
    /// tsc-port: pipelineEmitWithComments @6.0.3
    /// tsc-hash: 86ab696d12e92fc0736baab71aa4028842693c982a92e2456d414c055e1124fd
    /// tsc-span: _tsc.js:120969-121000
    fn emit_retained_arrow_token_with_comments(
        &self,
        transformation: &mut TransformationResult<'_>,
        token: TransformNode,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        let record = transformation.arena().node(token)?;
        if record.kind != SyntaxKind::EqualsGreaterThanToken
            || !matches!(&record.data, NodeData::Token)
        {
            return Err(PrinterError::UnsupportedTransformedSyntax {
                node: token,
                kind: record.kind,
            });
        }
        let hooks = transformation.emit_pipeline_hooks(token)?;
        if !hooks.is_empty() {
            return Err(PrinterError::RetainedArrowTokenPipelineHooks {
                token,
                substitution: hooks.substitution(),
                notification: hooks.notification(),
            });
        }
        self.emit_leading_comments_for_node(transformation, token, writer)?;
        self.emit_node_id_with_context(
            transformation,
            token.source(),
            token.node(),
            expression_context.for_child(ExpressionSyntaxContext::NORMAL),
            writer,
        )?;
        let cursor = self.comment_range_end_cursor(transformation, token)?;
        let anchor =
            self.emit_trailing_comments_for_node_at_cursor(transformation, token, cursor, writer)?;
        Ok(TokenEmission::new(anchor.cursor(), anchor.comment_resume()))
    }

    /// Emit a node's same-line trailing comments and attach their ownership
    /// to an explicitly selected cursor. Separator emission uses the parsed
    /// original boundary; retained token-node emission uses its comment-range
    /// boundary. Keeping that distinction at the call site prevents semantic
    /// provenance from silently becoming comment provenance.
    fn emit_trailing_comments_for_node_at_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        cursor: TokenCursor,
        writer: &mut TextWriter,
    ) -> Result<TokenAnchor, PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                        || metadata.relocated_trailing_comment_owner.is_some()
                })
            || transformation.arena().node(node)?.kind == SyntaxKind::NotEmittedStatement
        {
            return Ok(cursor.into());
        }
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let source = transformation
            .arena()
            .source(comment_range.source())?
            .syntax();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(cursor.into());
        };
        if range.start() == range.end() {
            return Ok(cursor.into());
        }
        let boundary = range.end().value() as usize;
        let emitted_through = emit_same_line_trailing_comments(
            SourceTrivia::from_start(source.text(), boundary),
            writer,
        )
        .map(|end| SourceBytePosition::new(end as u32, source.positions()))
        .transpose()?;
        let comment_resume = match cursor.source_position() {
            Some((cursor_source, cursor_position))
                if cursor_source == comment_range.source()
                    && cursor_position.value() as usize == boundary =>
            {
                emitted_through
                    .map(|next| {
                        CommentResume::new(
                            CommentCursor::new(cursor_source, cursor_position),
                            CommentCursor::new(cursor_source, next),
                        )
                        .map_err(Self::comment_resume_error)
                    })
                    .transpose()?
            }
            _ => None,
        };
        Ok(TokenAnchor::new(cursor, comment_resume))
    }

    /// Complete a retained child before a parent-owned separator is emitted.
    ///
    /// The child phase precedes separator layout, so a block comment receives
    /// spacing after the comment and a line comment can end its line before
    /// indentation is opened. The returned anchor carries that ownership into
    /// token emission and prevents a parsed separator from visiting the
    /// boundary twice. A generated separator cannot consume the source anchor,
    /// but still receives the same typed completion rather than recovering the
    /// child's range ad hoc.
    fn separator_anchor_after_child(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<TokenAnchor, PrinterError> {
        let cursor = self.original_node_end_cursor(transformation, child)?;
        if !self.child_trailing_comments_escape_parent_container(transformation, parent, child)? {
            return Ok(cursor.into());
        }
        self.emit_trailing_comments_for_node_as_token_anchor(transformation, child, writer)
    }

    /// Select the shared source boundary between a child and a retained
    /// separator token. Parsed trees place the token at the child's end, so
    /// the child can donate its trailing-comment resume. If a transform
    /// replaced that child with a synthetic expression, the retained token's
    /// own typed start remains the authoritative source anchor instead of an
    /// unrelated synthetic child boundary.
    fn separator_anchor_between_child_and_token(
        &self,
        transformation: &TransformationResult<'_>,
        parent: TransformNode,
        child: TransformNode,
        separator: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<TokenAnchor, PrinterError> {
        let child_end = self.original_node_end_cursor(transformation, child)?;
        let separator_start = self.original_node_start_cursor(transformation, separator)?;
        if child_end == separator_start || separator_start.source_position().is_none() {
            return self.separator_anchor_after_child(transformation, parent, child, writer);
        }
        Ok(TokenAnchor::new(
            separator_start,
            self.child_owned_trailing_resume_at_cursor(transformation, separator_start)?,
        ))
    }

    /// Record trivia immediately before a retained separator as already
    /// owned by a synthetic child's emitted source descendants. The resume
    /// covers both same-line trailing comments and leading comments after a
    /// line break; a later fixed token may continue only beyond that point.
    fn child_owned_trailing_resume_at_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        cursor: TokenCursor,
    ) -> Result<Option<CommentResume>, PrinterError> {
        if self.options.remove_comments {
            return Ok(None);
        }
        let Some((source_id, position)) = cursor.source_position() else {
            return Ok(None);
        };
        let source = transformation.arena().source(source_id)?.syntax();
        let start = position.value() as usize;
        let last_comment = collect_source_comment_ranges(source.text(), start, true)
            .into_iter()
            .chain(collect_source_comment_ranges(source.text(), start, false))
            .max_by_key(|comment| comment.end);
        let Some(last_comment) = last_comment else {
            return Ok(None);
        };
        let owner_start = CommentCursor::new(source_id, position);
        let next = CommentCursor::new(
            source_id,
            SourceBytePosition::new(
                u32::try_from(last_comment.end).unwrap_or(u32::MAX),
                source.positions(),
            )?,
        );
        Ok(Some(
            CommentResume::new(owner_start, next).map_err(Self::comment_resume_error)?,
        ))
    }

    fn emit_trailing_comments_at_node_position(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                })
        {
            return Ok(());
        }
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let source = transformation
            .arena()
            .source(comment_range.source())?
            .syntax();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(());
        };
        emit_source_trailing_comments_of_position(
            source.text(),
            range.end().value() as usize,
            writer,
        );
        Ok(())
    }

    /// A partially-emitted expression preserves the source interval of an
    /// erased TypeScript wrapper while emitting only its runtime child. tsc
    /// gives the two gaps explicit ownership: trailing comments at the
    /// child's start precede it, and leading comments at the child's end
    /// follow it. Keeping those boundaries separate avoids treating an
    /// erased assertion/satisfies node as a textual token.
    ///
    /// tsc-port: emitPartiallyEmittedExpression @6.0.3
    /// tsc-hash: b9f0a56cdd87c34839b164ebfd072c94080d685dcef320caf538a563871a7966
    /// tsc-span: _tsc.js:119770-119778
    fn emit_partially_emitted_boundary_comments(
        &self,
        transformation: &TransformationResult<'_>,
        wrapper: TransformNode,
        expression: TransformNode,
        before_expression: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let flags = transformation
            .arena()
            .metadata(wrapper)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags());
        if before_expression && flags.contains(EmitFlags::NO_LEADING_COMMENTS)
            || !before_expression && flags.contains(EmitFlags::NO_TRAILING_COMMENTS)
        {
            return Ok(());
        }
        if wrapper.source() != expression.source() {
            return Err(PrinterError::CommentCursorSourceMismatch {
                cursor: expression.source(),
                owner: wrapper.source(),
            });
        }
        let source = transformation.arena().source(wrapper.source())?.syntax();
        let wrapper_record = transformation.arena().node(wrapper)?;
        let expression_record = transformation.arena().node(expression)?;
        // A later transform may recreate a PartiallyEmittedExpression with a
        // synthesized text range while retaining its parsed child. tsc
        // compares the raw sentinel-bearing positions; it does not require
        // the wrapper itself to own source text. Validate both ranges, but
        // materialize a source position only for the child boundary where
        // comments can actually be read.
        SourceRange::from_raw(wrapper_record.pos, wrapper_record.end, source.positions())?;
        let expression_range = SourceRange::from_raw(
            expression_record.pos,
            expression_record.end,
            source.positions(),
        )?;
        if before_expression {
            if wrapper_record.pos != expression_record.pos {
                let SourceRange::Original(expression_range) = expression_range else {
                    return Ok(());
                };
                let position = expression_range.start().value() as usize;
                let trailing = collect_source_comment_ranges(source.text(), position, true);
                let excluded = trailing
                    .iter()
                    .map(|comment| (comment.start, comment.end))
                    .collect::<BTreeSet<_>>();
                emit_source_trailing_comments_of_position(source.text(), position, writer);
                if trailing.last().is_some_and(|comment| {
                    comment.kind == SourceCommentKind::Block && !comment.has_trailing_new_line
                }) && !writer.has_trailing_whitespace()
                {
                    writer.write_space(" ");
                }
                emit_source_leading_comments_of_position(
                    source.text(),
                    position,
                    &excluded,
                    writer,
                );
            }
        } else if wrapper_record.end != expression_record.end {
            let SourceRange::Original(expression_range) = expression_range else {
                return Ok(());
            };
            emit_source_leading_comments_of_position(
                source.text(),
                expression_range.end().value() as usize,
                &BTreeSet::new(),
                writer,
            );
        }
        Ok(())
    }

    /// Typed `getCommentRange`: comment ownership is independent from the
    /// node's source-map/text range. Transforms can therefore position a
    /// synthesized node at a source declaration without making it re-emit
    /// that declaration's boundary comments.
    fn comment_range_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<CommentRange, PrinterError> {
        let arena = transformation.arena();
        if let Some(range) = arena
            .metadata(node)
            .and_then(crate::EmitMetadata::comment_range)
        {
            return Ok(range);
        }
        // tsc's getCommentRange falls back to the node's own text range, not
        // its semantic `original` link. Original provenance is consumed by
        // resolver/substitution and source maps; it must not grant comments
        // to an otherwise synthetic child.
        let source = arena.source(node.source())?.syntax();
        let record = arena.node(node)?;
        Ok(CommentRange::new(
            node.source(),
            SourceRange::from_raw(record.pos, record.end, source.positions())?,
        ))
    }

    fn expression_comment_phase_owner_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<ExpressionCommentPhaseOwner, PrinterError> {
        let metadata = transformation.arena().metadata(node);
        Ok(ExpressionCommentPhaseOwner {
            range: self.comment_range_for_node(transformation, node)?,
            flags: metadata.map_or(EmitFlags::NONE, crate::EmitMetadata::flags),
            kind: transformation.arena().node(node)?.kind,
            relocated_trailing: metadata
                .is_some_and(|metadata| metadata.relocated_trailing_comment_owner.is_some()),
        })
    }

    /// Comment owner of `setTextRange(createParenthesizedExpression(node),
    /// node)`. The virtual parenthesis receives only the node's current text
    /// range: an explicit comment range and emit flags remain on the child.
    fn expression_comment_phase_owner_for_text_range(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<ExpressionCommentPhaseOwner, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        Ok(ExpressionCommentPhaseOwner {
            range: CommentRange::new(
                node.source(),
                SourceRange::from_raw(record.pos, record.end, source.positions())?,
            ),
            flags: EmitFlags::NONE,
            kind: SyntaxKind::ParenthesizedExpression,
            relocated_trailing: false,
        })
    }

    /// Comment owner of the virtual no-ASI parenthesis created with both
    /// `setOriginalNode` and `setTextRange`. Explicit wrapper comment ranges
    /// win; otherwise the parsed parenthesis donates its token/text range.
    /// Emit flags and relocated ownership still come from the wrapper's
    /// metadata, matching the split ownership of the tsc factory node.
    fn parsed_no_asi_comment_phase_owner(
        &self,
        transformation: &TransformationResult<'_>,
        metadata_owner: TransformNode,
        token_owner: TransformNode,
    ) -> Result<ExpressionCommentPhaseOwner, PrinterError> {
        let metadata = transformation.arena().metadata(metadata_owner);
        let range = if let Some(range) = metadata.and_then(crate::EmitMetadata::comment_range) {
            range
        } else {
            let source = transformation
                .arena()
                .source(token_owner.source())?
                .syntax();
            let record = transformation.arena().node(token_owner)?;
            CommentRange::new(
                token_owner.source(),
                SourceRange::from_raw(record.pos, record.end, source.positions())?,
            )
        };
        Ok(ExpressionCommentPhaseOwner {
            range,
            flags: metadata.map_or(EmitFlags::NONE, crate::EmitMetadata::flags),
            kind: SyntaxKind::ParenthesizedExpression,
            relocated_trailing: metadata
                .is_some_and(|metadata| metadata.relocated_trailing_comment_owner.is_some()),
        })
    }

    /// The statement-family claim: the full range pair when the owner has
    /// a nonempty original range, flags not consulted. This is the H2.5g
    /// paired projection those routes stay on until their own migration
    /// packet lands the per-side producer there.
    /// tsc's per-side claim conditions over one comment-phase owner.
    ///
    /// For a range this representation can express, a side goes unclaimed
    /// only for `JsxText` without that side's suppression flag — a
    /// suppression flag claims while suppressing the emission itself —
    /// and a synthesized or zero-width range claims nothing at all, so
    /// the enclosing scope stays active. The `pos < 0` arms of the
    /// upstream predicate are unreachable here: `SourceRange` is either
    /// `Original` with both positions or `Synthesized`, and the outer
    /// `(pos > 0 || end > 0)` gate is always satisfied by a nonempty
    /// original range through its end.
    ///
    /// tsc-port: emitLeadingCommentsOfNode @6.0.3
    /// tsc-hash: ce6bf342a94094cccc4bf56debcb99390c8e232705263609dfcf068589284ebb
    /// tsc-span: _tsc.js:121007-121032
    fn established_container_sides(
        owner: ExpressionCommentPhaseOwner,
    ) -> (Option<CommentCursor>, Option<CommentCursor>) {
        let pos = CommentEmissionScope::container_pos_of(owner.range);
        let end = CommentEmissionScope::container_end_of(owner.range);
        let jsx_text = owner.kind == SyntaxKind::JsxText;
        let claim_pos = if jsx_text && !owner.flags.intersects(EmitFlags::NO_LEADING_COMMENTS) {
            None
        } else {
            pos
        };
        let claim_end = if jsx_text && !owner.flags.intersects(EmitFlags::NO_TRAILING_COMMENTS) {
            None
        } else {
            end
        };
        (claim_pos, claim_end)
    }

    /// The enclosing scope one deferred container denotes: the scope the
    /// parent captured, or the parent itself claimed lazily — through the
    /// same per-side producer as every eager claim — when no container
    /// was active at capture time.
    fn deferred_container_scope(
        &self,
        transformation: &TransformationResult<'_>,
        container: ExpressionCommentContainer,
    ) -> Result<CommentEmissionScope, PrinterError> {
        Ok(match container {
            ExpressionCommentContainer::Scope(scope) => scope,
            ExpressionCommentContainer::Node(parent) => {
                let parent_owner =
                    self.expression_comment_phase_owner_for_node(transformation, parent)?;
                let (pos, end) = Self::established_container_sides(parent_owner);
                CommentEmissionScope::empty().claim_sides(pos, end)
            }
        })
    }

    /// The active scope for one expression comment phase: the inherited
    /// (deferred or ambient) scope with the owner's own per-side claim
    /// applied on top.
    fn active_expression_comment_scope(
        &self,
        transformation: &TransformationResult<'_>,
        deferred: Option<&DeferredExpressionSourceComments>,
        expression_context: EmitContext,
        owner: ExpressionCommentPhaseOwner,
    ) -> Result<CommentEmissionScope, PrinterError> {
        let inherited = match deferred.and_then(|deferred| deferred.container) {
            Some(container) => self.deferred_container_scope(transformation, container)?,
            None => expression_context.comments(),
        };
        let (pos, end) = Self::established_container_sides(owner);
        Ok(inherited.claim_sides(pos, end))
    }

    fn comment_range_end_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<TokenCursor, PrinterError> {
        let comment_range = self.comment_range_for_node(transformation, node)?;
        Ok(match comment_range.range() {
            SourceRange::Original(range) => {
                TokenCursor::source(comment_range.source(), range.end())
            }
            SourceRange::Synthesized => TokenCursor::Synthetic,
        })
    }

    /// A label's trailing trivia belongs before an explicit source
    /// terminator, but after the synthesized terminator when the source relied
    /// on ASI. The parse-tree statement range distinguishes those cases:
    /// without a terminator it ends exactly with the label.
    fn emit_jump_label_comments_before_terminator(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
        label: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.original_node_end(transformation, statement)?
            == self.original_node_end(transformation, label)?
        {
            return Ok(());
        }
        self.emit_trailing_comments_at_node_position(transformation, label, writer)
    }

    fn original_node_end(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<Option<usize>, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(None);
        };
        Ok(Some(range.end().value() as usize))
    }

    fn original_node_start_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<TokenCursor, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        Ok(
            match SourceRange::from_raw(record.pos, record.end, source.positions())? {
                SourceRange::Original(range) => {
                    TokenCursor::source(original.source(), range.start())
                }
                SourceRange::Synthesized => TokenCursor::Synthetic,
            },
        )
    }

    fn original_node_end_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<TokenCursor, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        Ok(
            match SourceRange::from_raw(record.pos, record.end, source.positions())? {
                SourceRange::Original(range) => TokenCursor::source(original.source(), range.end()),
                SourceRange::Synthesized => TokenCursor::Synthetic,
            },
        )
    }

    /// Cursor for the transformed node's current text-range start. A virtual
    /// source-owned ParenthesizedExpression uses the parsed container range
    /// copied by tsc's `setTextRange`, not the semantic original endpoint of
    /// that parsed node's own metadata chain.
    fn node_start_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<TokenCursor, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        Ok(
            match SourceRange::from_raw(record.pos, record.end, source.positions())? {
                SourceRange::Original(range) => TokenCursor::source(node.source(), range.start()),
                SourceRange::Synthesized => TokenCursor::Synthetic,
            },
        )
    }

    /// Cursor for the transformed node's current text-range end. Fixed-token
    /// emitters normally follow semantic originals, but tsc's generated
    /// ParenthesizedExpression deliberately anchors `)` at
    /// `node.expression.end` after transformation.
    fn node_end_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<TokenCursor, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        Ok(
            match SourceRange::from_raw(record.pos, record.end, source.positions())? {
                SourceRange::Original(range) => TokenCursor::source(node.source(), range.end()),
                SourceRange::Synthesized => TokenCursor::Synthetic,
            },
        )
    }

    /// The source position immediately following an emitted modifier list.
    ///
    /// tsc anchors declaration keywords at `modifiers.end` when a parsed
    /// list exists and at `node.pos` otherwise. A synthesized list has no
    /// meaningful source continuation, so it enters the explicit synthetic
    /// cursor state instead of borrowing an unrelated source position.
    fn token_after_modifiers_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        modifiers: Option<tsc_syntax::NodeArrayId>,
    ) -> Result<TokenCursor, PrinterError> {
        let Some(modifiers) = modifiers.and_then(|modifiers| {
            transformation
                .arena()
                .node_array_ref(node.source(), modifiers)
        }) else {
            return self.original_node_start_cursor(transformation, node);
        };
        let modifiers = transformation.arena().node_array(modifiers)?;
        if modifiers.end == u32::MAX {
            return Ok(TokenCursor::Synthetic);
        }
        let source = transformation.arena().source(node.source())?.syntax();
        Ok(TokenCursor::source(
            node.source(),
            SourceBytePosition::new(modifiers.end, source.positions())?,
        ))
    }

    /// Rust-native position form of tsc's `emitTokenWithComment`.
    ///
    /// This deliberately performs no lexical search and no source-token-kind
    /// validation. `writeTokenText` advances `pos` by the fixed spelling's
    /// length; ES2019 optional-catch lowering relies on that behavior when a
    /// synthetic `(` advances over the original block's `{` position.
    fn emit_token_with_comments(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token: FixedToken,
        anchor: impl Into<TokenAnchor>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        self.emit_token_with_comments_at_boundary(
            transformation,
            owner,
            token,
            anchor,
            TokenCommentBoundary::OwnerEnd,
            TokenLeadingSpace::None,
            indent_leading,
            writer,
        )
    }

    fn emit_list_boundary_token_with_comments(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token: FixedToken,
        anchor: impl Into<TokenAnchor>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        self.emit_token_with_comments_at_boundary(
            transformation,
            owner,
            token,
            anchor,
            TokenCommentBoundary::AdjacentListItem,
            TokenLeadingSpace::None,
            indent_leading,
            writer,
        )
    }

    fn emit_space_prefixed_token_with_comments(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token: FixedToken,
        anchor: impl Into<TokenAnchor>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        self.emit_token_with_comments_at_boundary(
            transformation,
            owner,
            token,
            anchor,
            TokenCommentBoundary::OwnerEnd,
            TokenLeadingSpace::Required,
            indent_leading,
            writer,
        )
    }

    /// Emit the `in`/`of` separator after a for-binding. tsc performs an
    /// unconditional `writeSpace()` before emitting this token. That differs
    /// observably from collision-prevention spacing when parser recovery has
    /// produced `var` plus a zero-width missing binding name: the declaration
    /// list already ends in its own head space, and the separator contributes
    /// a second one (`var  in` / `var  of`).
    ///
    /// tsc-port: emitForInStatement/emitForOfStatement @6.0.3
    /// tsc-hash: 8348ada2caf681cfe56013a24e4036e164eddbddb13779816964d48b7d731f1c
    /// tsc-span: _tsc.js:118687-118715
    fn emit_for_binding_keyword_with_comments(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token: FixedToken,
        anchor: impl Into<TokenAnchor>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        writer.write_space(" ");
        self.emit_token_with_comments_at_boundary(
            transformation,
            owner,
            token,
            anchor,
            TokenCommentBoundary::OwnerEnd,
            TokenLeadingSpace::Required,
            indent_leading,
            writer,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_token_with_comments_at_boundary(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token: FixedToken,
        anchor: impl Into<TokenAnchor>,
        comment_boundary: TokenCommentBoundary,
        leading_space: TokenLeadingSpace,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenEmission, PrinterError> {
        let anchor = anchor.into();
        let cursor = anchor.cursor();
        let spelling = tsc_syntax::tokens::token_to_string(token.kind).ok_or(
            PrinterError::UnsupportedTransformedSyntax {
                node: owner,
                kind: token.kind,
            },
        )?;
        let original_owner = transformation.arena().get_original_node(owner);
        let owner_record = transformation.arena().node(owner)?;
        // tsc's `getParseTreeNode(contextNode, isSimilarNode)` returns no
        // context unless the original chain reaches a parsed node of the same
        // token shape. A mere source text range is not enough: transforms
        // routinely place generated containers at a child's range for source
        // maps. Treating those containers as parsed lets synthetic delimiters
        // steal the surrounding source container's comments.
        let similar = self.node_has_source_token_shape(transformation, owner)?;

        // h2-6a-m-2 §4 route table: the upstream emitTokenWithComment
        // BRACE arm records here (every other token kind there is
        // writeTokenText, unmapped), plus the conditional `?`/`:`
        // lane — upstream pipelines those as token NODES, whose
        // boundary maps land at exactly this write with the token's
        // skip-trivia'd span as the default range. A synthesized
        // conditional carries factory `createToken` question/colon
        // nodes (pos -1): no boundary map, however real the
        // neighboring cursor looks — hence the `similar` gate
        // (replay-falsified: the default-value conditional's colon
        // otherwise records the source comma after the initializer).
        let record_braces = matches!(
            token.kind,
            SyntaxKind::OpenBraceToken | SyntaxKind::CloseBraceToken
        ) || (matches!(
            token.kind,
            SyntaxKind::QuestionToken | SyntaxKind::ColonToken
        ) && owner_record.kind == SyntaxKind::ConditionalExpression
            && similar);
        let Some((cursor_source, start_position)) = cursor.source_position() else {
            #[cfg(test)]
            crate::token_cursor::record_cursor_work(0);
            Self::ensure_token_leading_space(writer, leading_space);
            if record_braces {
                // Review F6: a synthetic-cursor token records if and only
                // if a token_source_map_ranges override exists (default
                // range None).
                self.record_token_map_side(
                    transformation,
                    MapBoundary::Before,
                    owner,
                    token.kind,
                    None,
                    writer,
                )?;
            }
            Self::write_fixed_token(writer, token.write_as, spelling);
            if record_braces {
                self.record_token_map_side(
                    transformation,
                    MapBoundary::After,
                    owner,
                    token.kind,
                    None,
                    writer,
                )?;
            }
            return Ok(TokenEmission::new(TokenCursor::Synthetic, None));
        };
        if similar && original_owner.source() != cursor_source {
            return Err(PrinterError::TokenCursorSourceMismatch {
                cursor: cursor_source,
                owner: original_owner.source(),
            });
        }
        let source = transformation.arena().source(cursor_source)?.syntax();
        let start = start_position.value() as usize;
        let token_start = if similar {
            skip_trivia(source.text(), start)
        } else {
            start
        };
        #[cfg(test)]
        crate::token_cursor::record_cursor_work(token_start.saturating_sub(start) + spelling.len());

        if similar && owner_record.pos != start_position.value() {
            self.emit_comments_at_cursor(
                transformation,
                cursor,
                anchor.comment_resume(),
                indent_leading,
                writer,
            )?;
        }

        Self::ensure_token_leading_space(writer, leading_space);
        let token_end =
            token_start
                .checked_add(spelling.len())
                .ok_or(PrinterError::TokenPositionOverflow {
                    position: start_position.value(),
                    token: token.kind,
                })?;
        // The positioned brace default range spans the raw start (the
        // record side re-skips trivia exactly as upstream
        // emitTokenWithSourceMap does) to the arithmetic token end; a
        // range that cannot be constructed on char boundaries records
        // nothing (§8-A records the deviation from upstream's arithmetic
        // continuation — unwitnessed corner).
        let token_map_range = if record_braces && writer.has_source_map_recording() {
            u32::try_from(token_end).ok().and_then(|end_raw| {
                SourceRange::from_raw(
                    u32::try_from(start).expect("token start exceeds u32"),
                    end_raw,
                    source.positions(),
                )
                .ok()
                .map(|range| SourceMapRange::new(cursor_source, range))
            })
        } else {
            None
        };
        if record_braces {
            self.record_token_map_side(
                transformation,
                MapBoundary::Before,
                owner,
                token.kind,
                token_map_range,
                writer,
            )?;
        }
        Self::write_fixed_token(writer, token.write_as, spelling);
        if record_braces {
            self.record_token_map_side(
                transformation,
                MapBoundary::After,
                owner,
                token.kind,
                token_map_range,
                writer,
            )?;
        }
        let token_end_raw =
            u32::try_from(token_end).map_err(|_| PrinterError::TokenPositionOverflow {
                position: start_position.value(),
                token: token.kind,
            })?;
        let Ok(token_end_position) = SourceBytePosition::new(token_end_raw, source.positions())
        else {
            // A transformed token can be inserted at a real source boundary
            // that has no corresponding source spelling. tsc keeps an
            // arithmetic position in that case; Rust converts the unusable
            // continuation into the explicit synthetic state.
            return Ok(TokenEmission::new(TokenCursor::Synthetic, None));
        };
        let returned = TokenCursor::source(cursor_source, token_end_position);

        let mut comment_resume = None;
        if similar
            && (comment_boundary == TokenCommentBoundary::AdjacentListItem
                || owner_record.end != token_end_raw)
            && !self.options.remove_comments
        {
            let comments = collect_source_comment_ranges(source.text(), token_end, true);
            let last_trailing_comment_end = comments
                .last()
                .map(|comment| SourceBytePosition::new(comment.end as u32, source.positions()))
                .transpose()?;
            comment_resume = last_trailing_comment_end
                .map(|next| {
                    CommentResume::new(
                        CommentCursor::new(cursor_source, token_end_position),
                        CommentCursor::new(cursor_source, next),
                    )
                    .map_err(Self::comment_resume_error)
                })
                .transpose()?;
            if owner_record.kind == SyntaxKind::JsxExpression {
                emit_source_jsx_trailing_comments_of_position(source.text(), token_end, writer);
            } else {
                emit_source_trailing_comments_of_position(source.text(), token_end, writer);
            }
        }

        Ok(TokenEmission::new(returned, comment_resume))
    }

    /// Whether a transformed node still represents the fixed-token shape of
    /// a parsed original. Copying a text range onto a generated node does not
    /// transfer token or comment ownership; only an original chain ending in
    /// the same parse-tree kind does.
    ///
    /// This is the local equivalent of tsc's
    /// `getParseTreeNode(contextNode, isSimilarNode)`. In particular, parsed
    /// parentheses own comments after their source `(`, while precedence
    /// parentheses synthesized after type erasure or downleveling leave the
    /// child's leading comments with the surrounding source container.
    fn node_has_source_token_shape(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let Some(original) = transformation.arena().parse_tree_node(node)? else {
            return Ok(false);
        };
        let record = transformation.arena().node(node)?;
        let original_record = transformation.arena().node(original)?;
        Ok(original_record.kind == record.kind)
    }

    /// Typed `!nodeIsSynthesized(node)` for layout decisions. Unlike token
    /// ownership, tsc's line-preservation check is based only on the node's
    /// current text range, so a generated node positioned by a transform can
    /// participate when every node at that boundary has a source range.
    fn node_has_source_text_range(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        Ok(matches!(
            SourceRange::from_raw(record.pos, record.end, source.positions())?,
            SourceRange::Original(_)
        ))
    }

    /// Emit the trailing/leading comment union at a typed source position.
    ///
    /// The synthetic statement terminator uses this directly because tsc's
    /// `emitExpressionStatement` writes its semicolon outside
    /// `emitTokenWithComment`; advancing past a source EOF would be incorrect.
    fn emit_comments_at_cursor(
        &self,
        transformation: &TransformationResult<'_>,
        cursor: TokenCursor,
        comment_resume: Option<CommentResume>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_comments_at_cursor_with_anchor(
            transformation,
            cursor,
            comment_resume,
            indent_leading,
            writer,
        )?;
        Ok(())
    }

    fn emit_comments_at_cursor_with_anchor(
        &self,
        transformation: &TransformationResult<'_>,
        cursor: TokenCursor,
        comment_resume: Option<CommentResume>,
        indent_leading: bool,
        writer: &mut TextWriter,
    ) -> Result<TokenAnchor, PrinterError> {
        if self.options.remove_comments {
            return Ok(cursor.into());
        }
        let Some((source_id, position)) = cursor.source_position() else {
            return Ok(cursor.into());
        };
        if let Some(resume) = comment_resume {
            let owner_start = resume.owner_start();
            if owner_start.source() != source_id {
                return Err(PrinterError::CommentCursorSourceMismatch {
                    cursor: owner_start.source(),
                    owner: source_id,
                });
            }
            if owner_start.position() != position {
                return Err(PrinterError::CommentResumeOwnerMismatch {
                    source: source_id,
                    left_start: position.value(),
                    right_start: owner_start.position().value(),
                });
            }
        }
        let source = transformation.arena().source(source_id)?.syntax();
        let start = position.value() as usize;
        let code_start = skip_trivia(source.text(), start);
        let needs_indent = indent_leading
            && source
                .text()
                .get(start..code_start)
                .is_some_and(|trivia| trivia.chars().any(is_line_break));
        if needs_indent {
            writer.increase_indent();
        }
        let trailing = collect_source_comment_ranges(source.text(), start, true);
        let mut excluded = trailing
            .iter()
            .map(|comment| (comment.start, comment.end))
            .collect::<BTreeSet<_>>();
        let mut emitted_through = comment_resume.map(|resume| resume.next().position().value());
        if let Some(resume) = comment_resume {
            let emitted_through = resume.next().position().value() as usize;
            excluded.extend(
                collect_source_comment_ranges(source.text(), start, false)
                    .into_iter()
                    .filter(|comment| comment.end <= emitted_through)
                    .map(|comment| (comment.start, comment.end)),
            );
        }
        if comment_resume.is_none() {
            emit_source_trailing_comments_of_position(source.text(), start, writer);
            emitted_through = trailing
                .iter()
                .map(|comment| u32::try_from(comment.end).unwrap_or(u32::MAX))
                .max();
        }
        let leading = collect_source_comment_ranges(source.text(), start, false);
        emitted_through = leading
            .iter()
            .filter(|comment| !excluded.contains(&(comment.start, comment.end)))
            .map(|comment| u32::try_from(comment.end).unwrap_or(u32::MAX))
            .chain(emitted_through)
            .max();
        emit_source_leading_comments_of_position(source.text(), start, &excluded, writer);
        if needs_indent {
            writer.decrease_indent();
        }
        let comment_resume = emitted_through
            .map(|next| {
                CommentResume::new(
                    CommentCursor::new(source_id, position),
                    CommentCursor::new(
                        source_id,
                        SourceBytePosition::new(next, source.positions())?,
                    ),
                )
                .map_err(Self::comment_resume_error)
            })
            .transpose()?;
        Ok(TokenAnchor::new(cursor, comment_resume))
    }

    fn write_fixed_token(writer: &mut TextWriter, write_as: TokenWriteKind, spelling: &str) {
        match write_as {
            TokenWriteKind::Keyword => writer.write_keyword(spelling),
            TokenWriteKind::Operator => writer.write_operator(spelling),
            TokenWriteKind::Punctuation => writer.write_punctuation(spelling),
        }
    }

    fn ensure_token_leading_space(writer: &mut TextWriter, leading_space: TokenLeadingSpace) {
        if leading_space == TokenLeadingSpace::Required
            && !writer.is_at_start_of_line()
            && !writer.has_trailing_whitespace()
        {
            writer.write_space(" ");
        }
    }

    fn token_owned_child_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        emission: TokenEmission,
        child: Option<TransformNode>,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let (Some(child), Some(token_resume)) = (child, emission.comment_resume()) else {
            return Ok(None);
        };
        let Some((source_id, _)) = emission.cursor().source_position() else {
            return Ok(None);
        };
        let original_child = transformation.arena().get_original_node(child);
        if original_child.source() != source_id {
            return Ok(None);
        }
        let source = transformation.arena().source(source_id)?.syntax();
        let child_record = transformation.arena().node(original_child)?;
        let SourceRange::Original(child_range) =
            SourceRange::from_raw(child_record.pos, child_record.end, source.positions())?
        else {
            return Ok(None);
        };
        let token_owner = token_resume.owner_start();
        if token_owner.source() != source_id {
            return Err(PrinterError::CommentCursorSourceMismatch {
                cursor: token_owner.source(),
                owner: source_id,
            });
        }
        if token_owner.position() != child_range.start() {
            return Err(PrinterError::CommentResumeOwnerMismatch {
                source: source_id,
                left_start: child_range.start().value(),
                right_start: token_owner.position().value(),
            });
        }
        let child_code_start = child_range
            .without_leading_trivia(source.text(), source.positions())?
            .start()
            .value() as usize;
        let position = SourceBytePosition::new(
            token_resume
                .next()
                .position()
                .value()
                .min(u32::try_from(child_code_start).unwrap_or(u32::MAX)),
            source.positions(),
        )?;
        let owner_start = CommentCursor::new(source_id, child_range.start());
        let next = CommentCursor::new(source_id, position);
        Ok(Some(
            CommentResume::new(owner_start, next).map_err(Self::comment_resume_error)?,
        ))
    }

    fn emit_trailing_block_comments_before_semicolon(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                })
        {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        emit_same_line_trailing_block_comments(
            SourceTrivia::from_start(source.text(), range.end().value() as usize),
            writer,
        );
        Ok(())
    }

    fn emit_synthetic_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let comments = transformation
            .arena()
            .metadata(node)
            .map(|metadata| metadata.leading_comments().to_vec())
            .unwrap_or_default();
        for comment in comments {
            if comment.has_leading_new_line() || comment.kind() == SyntheticCommentKind::SingleLine
            {
                writer.write_line(false);
            }
            write_synthetic_comment(&comment, writer);
            if comment.has_trailing_new_line() || comment.kind() == SyntheticCommentKind::SingleLine
            {
                writer.write_line(false);
            } else {
                writer.write_space(" ");
            }
        }
        Ok(())
    }

    fn emit_synthetic_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let comments = transformation
            .arena()
            .metadata(node)
            .map(|metadata| metadata.trailing_comments().to_vec())
            .unwrap_or_default();
        for comment in comments {
            if comment.has_leading_new_line() {
                writer.write_line(false);
            } else {
                writer.write_space(" ");
            }
            write_synthetic_comment(&comment, writer);
            if comment.has_trailing_new_line() {
                writer.write_line(false);
            }
        }
        Ok(())
    }

    /// Emit the comments owned by the source-file statement-list end.
    ///
    /// This is the typed counterpart of tsc's
    /// `emitBodyWithDetachedComments(sourceFile, sourceFile.statements, ...)`:
    /// the list range remains authoritative even when its final parse-tree
    /// statement was erased rather than replaced by a `NotEmittedStatement`.
    /// Using the last retained node here would lose the end boundary for an
    /// unused/type-only import and incorrectly drop its following EOF
    /// comments.
    ///
    /// tsc-port: emitBodyWithDetachedComments @6.0.3
    /// tsc-hash: 1a150ecfba06d23ef96b8c4227e73bc9fdf2dff6a436f0a08db41e1458800157
    /// tsc-span: _tsc.js:121075-121100
    fn emit_source_file_statement_list_trailing_comments(
        &self,
        transformation: &TransformationResult<'_>,
        statements: TransformNodeArray,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let source = transformation.arena().source(statements.source())?.syntax();
        let record = transformation.arena().node_array(statements)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let mut start = range.end().value() as usize;
        if source.text().as_bytes().get(start) == Some(&b';') {
            start += 1;
        }
        let tail = strip_same_line_comment_prefix(SourceTrivia::from_start(source.text(), start));
        if skip_trivia(tail.text(), 0) == tail.text().len()
            && tail
                .text()
                .as_bytes()
                .windows(2)
                .any(|pair| pair == b"/*" || pair == b"//")
        {
            emit_leading_comments(tail, writer, true);
        }
        Ok(())
    }

    fn emit_comment_after_open_brace(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let start = skip_trivia(source.text(), range.start().value() as usize);
        let end = range.end().value() as usize;
        if start < end && source.text().as_bytes()[start] == b'{' {
            emit_same_line_trailing_comments(
                SourceTrivia::new(source.text(), start + 1, end),
                writer,
            );
        }
        Ok(())
    }

    fn emit_empty_block_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        statements: Option<TransformNodeArray>,
        multi_line: bool,
        function_body: bool,
        writer: &mut TextWriter,
    ) -> Result<bool, PrinterError> {
        if self.options.remove_comments {
            return Ok(false);
        }
        // A transformed block's statement-list range is the precise trivia
        // owner. In particular, its `end` stops before a removed statement's
        // trailing comment, while a genuinely empty parsed list still owns
        // comments at its two boundaries. Falling back to the whole block
        // range here would resurrect comments from erased declarations.
        if let Some(statements) = statements {
            let array = transformation.arena().node_array(statements)?;
            if array.nodes.is_empty() && array.pos != u32::MAX && array.end != u32::MAX {
                let source = transformation.arena().source(statements.source())?.syntax();
                let trailing_position = array.pos as usize;
                let leading_position = array.end as usize;
                if !empty_node_array_boundary_has_comments(
                    source.text(),
                    trailing_position,
                    leading_position,
                    function_body,
                ) {
                    return Ok(false);
                }
                if multi_line {
                    emit_empty_multiline_block_boundary_comments(
                        source.text(),
                        trailing_position,
                        leading_position,
                        function_body,
                        writer,
                    );
                } else {
                    writer.write_space(" ");
                    emit_empty_node_array_boundary_comments(
                        source.text(),
                        trailing_position,
                        leading_position,
                        function_body,
                        writer,
                    );
                }
                return Ok(true);
            }
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(false);
        };
        let start = skip_trivia(source.text(), range.start().value() as usize);
        let end = range.end().value() as usize;
        if start >= end || source.text().as_bytes()[start] != b'{' {
            return Ok(false);
        }
        let inner_end = source.text()[start + 1..end]
            .rfind('}')
            .map_or(end, |offset| start + 1 + offset);
        let inner = SourceTrivia::new(source.text(), start + 1, inner_end);
        let inner = if function_body {
            strip_same_line_comment_prefix(inner)
        } else {
            inner
        };
        if !inner
            .text()
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"/*" || pair == b"//")
        {
            return Ok(false);
        }
        if multi_line {
            writer.write_line(false);
            writer.increase_indent();
            emit_leading_comments(inner, writer, true);
            writer.decrease_indent();
        } else {
            writer.write_space(" ");
            emit_leading_comments(inner, writer, true);
        }
        Ok(true)
    }

    fn is_function_body_block(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        if self.emission_plan.function_body_blocks.contains(&node) {
            return Ok(true);
        }
        let original = transformation.arena().get_original_node(node);
        let Some(parent) = transformation.arena().node(original)?.parent else {
            return Ok(false);
        };
        let parent = transformation
            .arena()
            .node_ref(original.source(), parent)
            .ok_or(PrinterError::UnknownStatement(parent.0))?;
        Ok(matches!(
            transformation.arena().node(parent)?.kind,
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::Constructor
                | SyntaxKind::ClassStaticBlockDeclaration
        ))
    }

    /// Emits the leading comments owned by a close-brace token. `tsc` anchors
    /// this token at the source NodeArray's end, not at the final emitted
    /// child. That distinction prevents comments attached to a removed tail
    /// declaration from being rediscovered while retaining genuine comments
    /// between the statement-list boundary and the closing brace.
    fn emit_comments_before_close_brace(
        &self,
        transformation: &TransformationResult<'_>,
        block: TransformNode,
        list_end: Option<usize>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments || list_end.is_none() {
            return Ok(());
        }
        // `setEmitFlags(block, NoComments)` suppresses every comment lane of
        // the node; upstream `createDefaultConstructorBody` (_tsc.js 105307)
        // and the ES5 class-IIFE tail rely on it to drop class-body comments
        // that their artificial block ranges would otherwise re-discover.
        // This pickup forgot the flag: the class-body comment of an
        // otherwise-empty class leaked after both synthesized `return`
        // statements (H2.5h aliasUsageInAccessorsOfClass#target=es5,
        // h2-6a ca-2 regression).
        if transformation
            .arena()
            .metadata(block)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::NO_COMMENTS))
        {
            return Ok(());
        }
        let start = list_end.expect("checked above");
        let original_block = transformation.arena().get_original_node(block);
        let source = transformation
            .arena()
            .source(original_block.source())?
            .syntax();
        let block_record = transformation.arena().node(original_block)?;
        let block_range =
            SourceRange::from_raw(block_record.pos, block_record.end, source.positions())?;
        let close = match block_range {
            SourceRange::Original(block_range) => {
                let block_end = block_range.end().value() as usize;
                source.text()[block_range.start().value() as usize..block_end]
                    .rfind('}')
                    .map(|offset| block_range.start().value() as usize + offset)
                    .unwrap_or(block_end)
            }
            SourceRange::Synthesized => {
                // A synthetic constructor body can deliberately reuse the
                // containing class-member NodeArray range. tsc's list emitter
                // then owns comments between that range and the next closing
                // delimiter even though the synthetic Block has no source
                // range of its own.
                source
                    .text()
                    .get(start..)
                    .and_then(|tail| tail.find('}'))
                    .map(|offset| start + offset)
                    .unwrap_or(start)
            }
        };
        if start >= close {
            return Ok(());
        }
        let trivia = strip_same_line_comment_prefix(SourceTrivia::new(source.text(), start, close));
        if trivia
            .text()
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"/*" || pair == b"//")
        {
            emit_leading_comments(trivia, writer, true);
        }
        Ok(())
    }

    fn emit_delimited_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<Option<CommentResume>, PrinterError> {
        if self.options.remove_comments {
            return Ok(None);
        }
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let comment_source = comment_range.source();
        let source = transformation.arena().source(comment_source)?.syntax();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(None);
        };
        let element_end = range.end().value() as usize;
        let delimiter = skip_trivia(source.text(), element_end);
        if source.text().as_bytes().get(delimiter) != Some(&b',') {
            return Ok(None);
        }
        let owner = delimiter + 1;
        let emitted_through = collect_source_comment_ranges(source.text(), owner, true)
            .last()
            .map(|comment| comment.end);
        emit_source_trailing_comments_of_position(source.text(), owner, writer);
        let Some(emitted_through) = emitted_through else {
            return Ok(None);
        };
        let owner =
            SourceBytePosition::new(u32::try_from(owner).unwrap_or(u32::MAX), source.positions())?;
        let emitted_through = SourceBytePosition::new(
            u32::try_from(emitted_through).unwrap_or(u32::MAX),
            source.positions(),
        )?;
        Ok(Some(
            CommentResume::new(
                CommentCursor::new(comment_source, owner),
                CommentCursor::new(comment_source, emitted_through),
            )
            .map_err(Self::comment_resume_error)?,
        ))
    }

    /// Project a cursor returned by a source comma onto the retained next
    /// list item. A synthesized list can mix parsed comma-separated siblings
    /// with genuinely generated adjacency, so source identity and the exact
    /// child trivia boundary decide whether the cursor is applicable.
    fn delimited_comment_resume_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        resume: Option<CommentResume>,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let Some(resume) = resume else {
            return Ok(None);
        };
        let comment_range = self.comment_range_for_node(transformation, node)?;
        let range_source = comment_range.source();
        let SourceRange::Original(range) = comment_range.range() else {
            return Ok(None);
        };
        if resume.owner_start().source() == range_source
            && resume.owner_start().position() == range.start()
        {
            Ok(Some(resume))
        } else {
            Ok(None)
        }
    }

    fn emit_list_element_end_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_list_element_end_comments_in_container(
            transformation,
            node,
            CommentEmissionScope::empty(),
            writer,
        )
    }

    fn emit_list_element_end_comments_in_container(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        ambient_scope: CommentEmissionScope,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                })
        {
            return Ok(());
        }
        // `emitNodeListItems` tests the emitted list item's own `end`. It does
        // not follow `original`: a cloned module specifier is a synthetic
        // argument to a generated `require()` call, so the surrounding import
        // statement still owns comments after the source specifier. Parsed or
        // range-preserving updated children continue to expose their ordinary
        // end boundary here.
        //
        // tsc-port: emitNodeListItems @6.0.3
        // tsc-span: _tsc.js:120184-120193
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let position = range.end().value() as usize;
        if !ambient_scope.retains_end(CommentCursor::new(node.source(), range.end())) {
            emit_source_trailing_comments_of_position(source.text(), position, writer);
        }
        emit_source_leading_comments_of_position(source.text(), position, &BTreeSet::new(), writer);
        Ok(())
    }

    /// `emitNodeListItems` emits leading comments at the final sibling's
    /// end before it writes the closing delimiter. `skipTrivia` gives the
    /// same lexical boundary: it includes comments before the next token
    /// (`)` here) and cannot capture comments after that delimiter.
    fn emit_delimited_list_end_comments_in_container(
        &self,
        transformation: &TransformationResult<'_>,
        last: TransformNode,
        ambient_scope: CommentEmissionScope,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(last);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let start = range.end().value() as usize;
        let end = skip_trivia(source.text(), start);
        source
            .text()
            .get(start..end)
            .ok_or(PrinterError::InvalidTextSlice {
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: u32::try_from(end).unwrap_or(u32::MAX),
            })?;
        // The final list item's boundary can own a same-line trailing
        // comment as well as leading comments before the close delimiter.
        // Keep those two comment modes distinct: trailing comments prefix a
        // separating space, while later leading comments retain their line
        // boundary. This mirrors emitNodeListItems' final
        // emitLeadingCommentsOfPosition ownership without concatenating the
        // comment directly onto the emitted expression.
        let trailing = collect_source_comment_ranges(source.text(), start, true);
        let excluded = trailing
            .iter()
            .map(|comment| (comment.start, comment.end))
            .collect::<BTreeSet<_>>();
        if !ambient_scope.retains_end(CommentCursor::new(original.source(), range.end())) {
            emit_source_trailing_comments_of_position(source.text(), start, writer);
        }
        emit_source_leading_comments_of_position(source.text(), start, &excluded, writer);
        Ok(())
    }

    fn node_range(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<SourceByteRange, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        match SourceRange::from_raw(record.pos, record.end, source.positions())? {
            SourceRange::Original(range) => Ok(range),
            SourceRange::Synthesized => Err(PrinterError::SyntheticNodeWorkerUnavailable(node)),
        }
    }

    fn write_original_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        original: OriginalNodeText<'_>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let _ = transformation.arena().metadata(node);
        raw_write_range(
            writer,
            original.text,
            original.range.start().value(),
            original.range.end().value(),
        )
    }
    fn record_node_map_boundary(
        &self,
        transformation: &TransformationResult<'_>,
        boundary: MapBoundary,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if !writer.has_source_map_recording() {
            return Ok(());
        }
        let arena = transformation.arena();
        if arena.node(node)?.kind == SyntaxKind::NotEmittedStatement {
            return Ok(());
        }
        // Upstream emitSignatureAndBody calls emitBlockFunctionBody
        // DIRECTLY — a function body block is never pipeline-emitted and
        // carries no node-boundary maps (its close brace maps through the
        // token lane); the Rust funnel pipelines every child, so the
        // asymmetry is restored here.
        if arena.node(node)?.kind == SyntaxKind::Block
            && self.is_function_body_block(transformation, node)?
        {
            return Ok(());
        }
        let metadata = arena.metadata(node);
        let flags = metadata.map_or(EmitFlags::NONE, |metadata| metadata.flags());
        if boundary == MapBoundary::Before && flags.intersects(EmitFlags::NO_LEADING_SOURCE_MAP)
            || boundary == MapBoundary::After && flags.intersects(EmitFlags::NO_TRAILING_SOURCE_MAP)
        {
            return Ok(());
        }
        let default_range = {
            let source = arena.source(node.source())?.syntax();
            let record = arena.node(node)?;
            SourceMapRange::new(
                node.source(),
                SourceRange::from_raw(record.pos, record.end, source.positions())?,
            )
        };
        let range = metadata
            .and_then(|metadata| metadata.source_map_range())
            .unwrap_or(default_range);
        self.record_map_range_side(transformation, boundary, range, writer)
    }

    /// The shared range-side record: resolves the effective side
    /// position (Before = skip-trivia'd start, After = raw end),
    /// converts to the source's UTF-16 line/character, and records
    /// against the range's own source (the `emitSourcePos` foreign
    /// lane collapses to the explicit per-record source).
    fn record_map_range_side(
        &self,
        transformation: &TransformationResult<'_>,
        boundary: MapBoundary,
        range: SourceMapRange,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let SourceRange::Original(range_value) = range.range() else {
            return Ok(());
        };
        let arena = transformation.arena();
        let mapped_source = arena.source(range.source())?.syntax();
        let raw = match boundary {
            MapBoundary::Before => u32::try_from(skip_trivia(
                mapped_source.text(),
                range_value.start().value() as usize,
            ))
            .expect("source trivia position exceeds u32"),
            MapBoundary::After => range_value.end().value(),
        };
        let byte = SourceBytePosition::new(raw, mapped_source.positions())?;
        let location = SourceUtf16Location::from_byte(byte, mapped_source.positions())?;
        let file_name = mapped_source.file_name.clone();
        writer.record_source_map_position_for(
            range.source(),
            &file_name,
            location.line(),
            location.column(),
        );
        Ok(())
    }

    /// tsc-port: emitTokenWithSourceMap @6.0.3
    /// tsc-hash: 1f4c5a048470151a92b7a92ff32a976744e5222fb62cfa7c9b3e3964bde39732
    /// tsc-span: _tsc.js:121333-121351
    ///
    /// The token map side (h2-6a-m-2 §4 route table): the
    /// `token_source_map_ranges` override is consulted BEFORE any
    /// synthetic test (review F6 — a synthetic-cursor token with a real
    /// override records); without an override the caller-supplied
    /// default range decides, and `None` records nothing.
    fn record_token_map_side(
        &self,
        transformation: &TransformationResult<'_>,
        boundary: MapBoundary,
        owner: TransformNode,
        token_kind: SyntaxKind,
        default_range: Option<SourceMapRange>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if !writer.has_source_map_recording() {
            return Ok(());
        }
        let arena = transformation.arena();
        let metadata = arena.metadata(owner);
        let flags = metadata.map_or(EmitFlags::NONE, |metadata| metadata.flags());
        if boundary == MapBoundary::Before
            && flags.intersects(EmitFlags::NO_TOKEN_LEADING_SOURCE_MAPS)
            || boundary == MapBoundary::After
                && flags.intersects(EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS)
        {
            return Ok(());
        }
        let range = metadata
            .and_then(|metadata| metadata.token_source_map_ranges().get(&token_kind))
            .copied()
            .or(default_range);
        let Some(range) = range else {
            return Ok(());
        };
        self.record_map_range_side(transformation, boundary, range, writer)
    }

    /// A one-token default range anchored at a raw source position: the
    /// span from the raw anchor to one byte past its skip-trivia'd token
    /// start (upstream `writeToken`'s returned continuation). `None` when
    /// the anchor cannot form a valid range.
    fn token_map_range_at(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        raw_anchor: u32,
        writer: &TextWriter,
    ) -> Result<Option<SourceMapRange>, PrinterError> {
        self.token_map_range_spanning(transformation, source, raw_anchor, 1, writer)
    }

    /// As `token_map_range_at` for a token of a known spelling length
    /// (upstream `writeToken`'s `pos + tokenString.length` continuation).
    fn token_map_range_spanning(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        raw_anchor: u32,
        spelling_len: usize,
        writer: &TextWriter,
    ) -> Result<Option<SourceMapRange>, PrinterError> {
        if !writer.has_source_map_recording() {
            return Ok(None);
        }
        let syntax = transformation.arena().source(source)?.syntax();
        let token_start = skip_trivia(syntax.text(), raw_anchor as usize);
        let Ok(end_raw) = u32::try_from(token_start + spelling_len) else {
            return Ok(None);
        };
        Ok(
            SourceRange::from_raw(raw_anchor, end_raw, syntax.positions())
                .ok()
                .map(|range| SourceMapRange::new(source, range)),
        )
    }

    /// The Block/CaseBlock brace lane of the §4 route table: bracket one
    /// written brace with its token map pair (upstream
    /// `emitTokenWithComment` → `writeToken` for braces), honoring the
    /// owner's `token_source_map_ranges` override.
    #[allow(clippy::too_many_arguments)]
    fn record_brace_write(
        &self,
        transformation: &TransformationResult<'_>,
        owner: TransformNode,
        token_kind: SyntaxKind,
        default_range: Option<SourceMapRange>,
        spelling: &'static str,
        classify: fn(&mut TextWriter, &str),
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.record_token_map_side(
            transformation,
            MapBoundary::Before,
            owner,
            token_kind,
            default_range,
            writer,
        )?;
        classify(writer, spelling);
        self.record_token_map_side(
            transformation,
            MapBoundary::After,
            owner,
            token_kind,
            default_range,
            writer,
        )?;
        Ok(())
    }
}

/// tsc-port: iterateCommentRanges @6.0.3
/// tsc-hash: 764a21ef657f07522ea01c6a257f918cacb811f74ae092784ce2729ac33b42ae
/// tsc-span: _tsc.js:8491-8585
/// tsc-port: emitNodeList @6.0.3 (empty branch)
/// tsc-hash: 75b9b75e2d5a4b53a93745abf7bc2026a6ae04f9b7c566f3d1e839c2a2a01516
/// tsc-span: _tsc.js:120029-120066
fn emit_empty_node_array_boundary_comments(
    source: &str,
    trailing_position: usize,
    leading_position: usize,
    suppress_same_line_trailing: bool,
    writer: &mut TextWriter,
) -> bool {
    let trailing = collect_source_comment_ranges(source, trailing_position, true);
    let mut emitted = BTreeSet::new();
    let mut wrote_comment = false;
    for comment in trailing {
        if suppress_same_line_trailing
            && !source[trailing_position..comment.start]
                .chars()
                .any(is_line_break)
        {
            emitted.insert((comment.start, comment.end));
            continue;
        }
        if !writer.is_at_start_of_line() {
            writer.write_space(" ");
        }
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        }
        emitted.insert((comment.start, comment.end));
        wrote_comment = true;
    }

    wrote_comment |= collect_source_comment_ranges(source, leading_position, false)
        .into_iter()
        .any(|comment| !emitted.contains(&(comment.start, comment.end)));
    emit_source_leading_comments_of_position(source, leading_position, &emitted, writer);
    wrote_comment
}

/// `emitBlockStatements` gives an empty multiline block two distinct comment
/// owners: `emitTokenWithComment(OpenBraceToken)` writes same-line comments
/// before the list's line break, while `emitTokenWithComment(CloseBraceToken)`
/// writes the remaining leading comments at the block indentation. Keep the
/// emitted ranges explicit because this printer deliberately has no global
/// emitted-comment map.
///
/// tsc-port: emitBlockStatements @6.0.3
/// tsc-hash: 9e607e908515d4e8c7076d8a5faf460a6c605a7b377ee5e5b2bfc9d62e4f0a2c
/// tsc-span: _tsc.js:118586-118601
/// tsc-port: emitNodeList @6.0.3 (empty branch)
/// tsc-hash: 75b9b75e2d5a4b53a93745abf7bc2026a6ae04f9b7c566f3d1e839c2a2a01516
/// tsc-span: _tsc.js:120029-120066
fn emit_empty_multiline_block_boundary_comments(
    source: &str,
    trailing_position: usize,
    leading_position: usize,
    suppress_same_line_trailing: bool,
    writer: &mut TextWriter,
) {
    let trailing = collect_source_comment_ranges(source, trailing_position, true);
    let mut emitted = BTreeSet::new();
    for comment in trailing {
        if suppress_same_line_trailing
            && !source[trailing_position..comment.start]
                .chars()
                .any(is_line_break)
        {
            emitted.insert((comment.start, comment.end));
            continue;
        }
        if !writer.is_at_start_of_line() {
            writer.write_space(" ");
        }
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        }
        emitted.insert((comment.start, comment.end));
    }

    writer.write_line(false);
    writer.increase_indent();
    emit_source_leading_comments_of_position(source, leading_position, &emitted, writer);
    writer.decrease_indent();
}

fn empty_node_array_boundary_has_comments(
    source: &str,
    trailing_position: usize,
    leading_position: usize,
    suppress_same_line_trailing: bool,
) -> bool {
    let trailing = collect_source_comment_ranges(source, trailing_position, true);
    let suppressed = trailing
        .iter()
        .filter(|comment| {
            suppress_same_line_trailing
                && !source[trailing_position..comment.start]
                    .chars()
                    .any(is_line_break)
        })
        .map(|comment| (comment.start, comment.end))
        .collect::<BTreeSet<_>>();
    trailing
        .iter()
        .any(|comment| !suppressed.contains(&(comment.start, comment.end)))
        || collect_source_comment_ranges(source, leading_position, false)
            .iter()
            .any(|comment| !suppressed.contains(&(comment.start, comment.end)))
}

fn emit_source_leading_comments_of_position(
    source: &str,
    position: usize,
    excluded: &BTreeSet<(usize, usize)>,
    writer: &mut TextWriter,
) {
    let mut wrote_comment = false;
    for comment in collect_source_comment_ranges(source, position, false) {
        if excluded.contains(&(comment.start, comment.end)) {
            continue;
        }
        if !wrote_comment
            && position != comment.start
            && source[position..comment.start].chars().any(is_line_break)
        {
            writer.write_line(false);
        }
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        } else if comment.kind == SourceCommentKind::Block {
            writer.write_space(" ");
        }
        wrote_comment = true;
    }
}

fn emit_source_trailing_comments_of_position(
    source: &str,
    position: usize,
    writer: &mut TextWriter,
) {
    for comment in collect_source_comment_ranges(source, position, true) {
        if !writer.is_at_start_of_line() && !writer.has_trailing_whitespace() {
            writer.write_space(" ");
        }
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        }
    }
}

/// `emitNodeListItems` uses the non-prefixing trailing-comment callback for
/// comments between an opening delimiter and its first child. Unlike token
/// trailing comments, a same-line block comment receives a following space.
fn emit_source_intervening_comments_of_position(
    source: &str,
    position: usize,
    writer: &mut TextWriter,
) {
    for comment in collect_source_comment_ranges(source, position, true) {
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        } else {
            writer.write_space(" ");
        }
    }
}

fn emit_source_jsx_trailing_comments_of_position(
    source: &str,
    position: usize,
    writer: &mut TextWriter,
) -> BTreeSet<(usize, usize)> {
    let mut emitted = BTreeSet::new();
    for comment in collect_source_comment_ranges(source, position, true) {
        write_source_comment(source, comment.start, comment.end, writer);
        if comment.kind == SourceCommentKind::Line {
            writer.write_line(false);
        }
        emitted.insert((comment.start, comment.end));
    }
    emitted
}

fn source_comment_will_emit_new_line(comment: &SourceCommentRange) -> bool {
    comment.kind == SourceCommentKind::Line || comment.has_trailing_new_line
}

fn synthetic_comment_will_emit_new_line(comment: &SyntheticComment) -> bool {
    comment.kind() == SyntheticCommentKind::SingleLine || comment.has_trailing_new_line()
}

fn collect_source_comment_ranges(
    source: &str,
    position: usize,
    trailing: bool,
) -> Vec<SourceCommentRange> {
    if position >= source.len() || !source.is_char_boundary(position) {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    // tsc's `iterateCommentRanges` treats a source-file shebang as trivia:
    // leading comment collection at position zero resumes immediately after
    // it. The shebang itself is emitted separately by the source-file writer.
    let mut cursor = if position == 0 {
        source_shebang(source).map_or(position, str::len)
    } else {
        position
    };
    let mut pending = None::<SourceCommentRange>;
    let mut collecting = trailing || position == 0;

    while cursor < source.len() {
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor below source length is a character boundary");
        if is_line_break(character) {
            cursor += character.len_utf8();
            if character == '\r' && source.as_bytes().get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
            if trailing {
                break;
            }
            collecting = true;
            if let Some(comment) = pending.as_mut() {
                comment.has_trailing_new_line = true;
            }
            continue;
        }
        if is_whitespace_like(character) {
            cursor += character.len_utf8();
            continue;
        }
        if source.as_bytes().get(cursor..cursor + 2) == Some(b"//") {
            let start = cursor;
            cursor += 2;
            let mut has_trailing_new_line = false;
            while cursor < source.len() {
                let character = source[cursor..]
                    .chars()
                    .next()
                    .expect("line-comment cursor is a character boundary");
                if is_line_break(character) {
                    has_trailing_new_line = true;
                    break;
                }
                cursor += character.len_utf8();
            }
            if collecting {
                if let Some(comment) = pending.replace(SourceCommentRange {
                    start,
                    end: cursor,
                    kind: SourceCommentKind::Line,
                    has_trailing_new_line,
                }) {
                    ranges.push(comment);
                }
            }
            continue;
        }
        if source.as_bytes().get(cursor..cursor + 2) == Some(b"/*") {
            let start = cursor;
            cursor += 2;
            if let Some(relative_end) = source[cursor..].find("*/") {
                cursor += relative_end + 2;
            } else {
                cursor = source.len();
            }
            if collecting {
                if let Some(comment) = pending.replace(SourceCommentRange {
                    start,
                    end: cursor,
                    kind: SourceCommentKind::Block,
                    has_trailing_new_line: false,
                }) {
                    ranges.push(comment);
                }
            }
            continue;
        }
        break;
    }

    if let Some(comment) = pending {
        ranges.push(comment);
    }
    #[cfg(test)]
    crate::token_cursor::record_cursor_source_work(cursor.saturating_sub(position));
    ranges
}

fn emit_same_line_trailing_comments(
    rest: SourceTrivia<'_>,
    writer: &mut TextWriter,
) -> Option<usize> {
    let mut cursor = rest.start;
    let mut last_comment_end = None;
    loop {
        while cursor < rest.end {
            let character = rest.source[cursor..rest.end]
                .chars()
                .next()
                .expect("trivia cursor is a character boundary");
            if is_line_break(character) {
                return last_comment_end;
            }
            if !is_whitespace_like(character) {
                break;
            }
            cursor += character.len_utf8();
        }
        if cursor >= rest.end {
            return last_comment_end;
        }

        let comment = if rest.source.as_bytes().get(cursor..cursor + 2) == Some(b"//") {
            let mut end = cursor + 2;
            let mut has_trailing_new_line = false;
            while end < rest.end {
                let character = rest.source[end..rest.end]
                    .chars()
                    .next()
                    .expect("line-comment cursor is a character boundary");
                if is_line_break(character) {
                    has_trailing_new_line = true;
                    break;
                }
                end += character.len_utf8();
            }
            SourceCommentRange {
                start: cursor,
                end,
                kind: SourceCommentKind::Line,
                has_trailing_new_line,
            }
        } else if rest.source.as_bytes().get(cursor..cursor + 2) == Some(b"/*") {
            let mut end = cursor + 2;
            while end + 1 < rest.end && &rest.source.as_bytes()[end..end + 2] != b"*/" {
                end += 1;
            }
            SourceCommentRange {
                start: cursor,
                end: (end + 2).min(rest.end),
                kind: SourceCommentKind::Block,
                has_trailing_new_line: false,
            }
        } else {
            return last_comment_end;
        };
        if !writer.has_trailing_whitespace() {
            writer.write_space(" ");
        }
        write_source_comment(rest.source, comment.start, comment.end, writer);
        cursor = comment.end;
        last_comment_end = Some(comment.end);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        }
        if comment.kind == SourceCommentKind::Line {
            return last_comment_end;
        }
    }
}

fn emit_same_line_trailing_block_comments(rest: SourceTrivia<'_>, writer: &mut TextWriter) {
    let text = rest.text();
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    loop {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if bytes.get(cursor..cursor + 2) != Some(b"/*") {
            return;
        }
        let start = cursor;
        cursor += 2;
        while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
            cursor += 1;
        }
        cursor = (cursor + 2).min(bytes.len());
        if !writer.has_trailing_whitespace() {
            writer.write_space(" ");
        }
        write_source_comment(rest.source, rest.start + start, rest.start + cursor, writer);
    }
}

fn strip_same_line_comment_prefix(trivia: SourceTrivia<'_>) -> SourceTrivia<'_> {
    let text = trivia.text();
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    let mut found = false;
    loop {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"//") {
            while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'\r' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'\n' {
                cursor += 1;
            }
            return trivia.advance(cursor);
        }
        if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            found = true;
            cursor += 2;
            while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                cursor += 1;
            }
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        return if found {
            trivia.advance(cursor)
        } else {
            trivia
        };
    }
}

fn detached_leading_trivia(trivia: &str) -> Option<&str> {
    let bytes = trivia.as_bytes();
    let mut cursor = 0usize;
    let mut saw_comment = false;
    while cursor < bytes.len() {
        let mut line_breaks = 0usize;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            if bytes[cursor] == b'\r' {
                line_breaks += 1;
                cursor += 1;
                if bytes.get(cursor) == Some(&b'\n') {
                    cursor += 1;
                }
            } else if bytes[cursor] == b'\n' {
                line_breaks += 1;
                cursor += 1;
            } else {
                cursor += 1;
            }
        }
        if saw_comment && line_breaks >= 2 {
            return Some(&trivia[..cursor]);
        }
        if bytes.get(cursor..cursor + 2) == Some(b"//") {
            saw_comment = true;
            cursor += 2;
            while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
                cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            saw_comment = true;
            cursor += 2;
            while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if cursor < bytes.len() {
            cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// `removeComments` retains one deliberately narrow exception: pinned
/// (`/*!`) comments in the detached group at source position zero. This is
/// the source-file ownership rule from tsc's `emitDetachedComments`, rather
/// than a general permission for pinned comments to survive elsewhere.
fn detached_pinned_comment_end(source: &str, position: usize, code_start: usize) -> Option<usize> {
    if position != 0 {
        return None;
    }
    let mut pinned = collect_source_comment_ranges(source, position, false)
        .into_iter()
        .filter(|comment| is_pinned_source_comment(source, *comment));
    let mut last = pinned.next()?;
    for comment in pinned {
        if contains_two_line_breaks(source, last.end, comment.start) {
            break;
        }
        last = comment;
    }
    contains_two_line_breaks(source, last.end, code_start).then_some(last.end)
}

fn is_pinned_source_comment(source: &str, comment: SourceCommentRange) -> bool {
    source.as_bytes().get(comment.start..comment.start + 3) == Some(b"/*!")
}

fn contains_two_line_breaks(source: &str, start: usize, end: usize) -> bool {
    let Some(text) = source.get(start..end) else {
        return false;
    };
    let mut count = 0usize;
    let mut previous_was_carriage_return = false;
    for character in text.chars() {
        match character {
            '\r' => {
                count += 1;
                previous_was_carriage_return = true;
            }
            '\n' if previous_was_carriage_return => {
                previous_was_carriage_return = false;
            }
            character if is_line_break(character) => {
                count += 1;
                previous_was_carriage_return = false;
            }
            _ => previous_was_carriage_return = false,
        }
        if count >= 2 {
            return true;
        }
    }
    false
}

/// tsc's `emitShebangIfNeeded` writes the source-file shebang before helpers,
/// prologue directives, and comments. It is not governed by removeComments.
fn source_shebang(text: &str) -> Option<&str> {
    text.strip_prefix("#!").map(|rest| {
        let end = rest
            .find(['\r', '\n', '\u{2028}', '\u{2029}'])
            .unwrap_or(rest.len());
        &text[..end + 2]
    })
}

/// tsc `isRecognizedTripleSlashComment` @6.0.3. This is intentionally
/// narrower than a generic `///` test: only pragmas understood by the parser
/// survive when their owning TypeScript statement becomes NotEmitted.
fn is_recognized_triple_slash_comment(comment: &str) -> bool {
    fn quoted_attribute(rest: &str, name: &str, allow_trailing_attributes: bool) -> bool {
        let Some(rest) = rest.strip_prefix(name) else {
            return false;
        };
        let rest = rest.trim_start_matches(is_js_whitespace);
        let Some(rest) = rest.strip_prefix('=') else {
            return false;
        };
        let rest = rest.trim_start_matches(is_js_whitespace);
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|quote| matches!(quote, '\'' | '"'))
        else {
            return false;
        };
        let rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            return false;
        };
        let trailing = &rest[end + quote.len_utf8()..];
        if allow_trailing_attributes {
            trailing.contains("/>")
        } else {
            trailing
                .trim_start_matches(is_js_whitespace)
                .starts_with("/>")
        }
    }

    let Some(rest) = comment.strip_prefix("///") else {
        return false;
    };
    let rest = rest.trim_start_matches(is_js_whitespace);
    if let Some(rest) = rest.strip_prefix("<reference") {
        if !rest.chars().next().is_some_and(is_js_whitespace) {
            return false;
        }
        let rest = rest.trim_start_matches(is_js_whitespace);
        return quoted_attribute(rest, "path", true)
            || quoted_attribute(rest, "types", true)
            || quoted_attribute(rest, "lib", true)
            || quoted_attribute(rest, "no-default-lib", false);
    }
    if let Some(rest) = rest.strip_prefix("<amd-dependency") {
        return rest.chars().next().is_some_and(is_js_whitespace)
            && quoted_attribute(rest.trim_start_matches(is_js_whitespace), "path", true);
    }
    rest.strip_prefix("<amd-module").is_some_and(|rest| {
        rest.chars().next().is_some_and(is_js_whitespace)
            && rest.trim_start_matches(is_js_whitespace).contains("/>")
    })
}

/// The NotEmittedStatement comment mode: scan the leading trivia but write
/// only recognized triple-slash line comments. Ordinary comments remain
/// owned by erased syntax and must not migrate to the next JavaScript node.
fn emit_triple_slash_leading_comments(trivia: &str, writer: &mut TextWriter) {
    let bytes = trivia.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"//") {
            let mut end = cursor + 2;
            while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
                end += trivia[end..].chars().next().map_or(1, char::len_utf8);
            }
            let comment = &trivia[cursor..end];
            if is_recognized_triple_slash_comment(comment) {
                write_comment_with_normalized_newlines(comment, writer);
                writer.write_line(false);
            }
            cursor = end;
            continue;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            cursor += 2;
            while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if cursor < bytes.len() {
            cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
}

fn emit_leading_comments(
    trivia: SourceTrivia<'_>,
    writer: &mut TextWriter,
    trailing_separator: bool,
) {
    let text = trivia.text();
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let whitespace_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let comment_follows = bytes.get(cursor..cursor + 2) == Some(b"//")
            || bytes.get(cursor..cursor + 2) == Some(b"/*");
        if comment_follows
            && (text[whitespace_start..cursor].contains('\r')
                || text[whitespace_start..cursor].contains('\n'))
            && !writer.is_at_start_of_line()
        {
            writer.write_line(false);
        }

        let (comment_end, line_comment) = if bytes.get(cursor..cursor + 2) == Some(b"//") {
            let mut end = cursor + 2;
            while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
                end += 1;
            }
            (end, true)
        } else if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && &bytes[end..end + 2] != b"*/" {
                end += 1;
            }
            end = (end + 2).min(bytes.len());
            (end, false)
        } else {
            if cursor < bytes.len() {
                cursor += text[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        };

        write_source_comment(
            trivia.source,
            trivia.start + cursor,
            trivia.start + comment_end,
            writer,
        );
        cursor = comment_end;

        let gap_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let gap_has_line_break =
            text[gap_start..cursor].contains('\r') || text[gap_start..cursor].contains('\n');
        if line_comment || gap_has_line_break {
            writer.write_line(false);
        } else if gap_start < cursor || cursor < bytes.len() || trailing_separator {
            writer.write_space(" ");
        }
    }
}

fn emit_pinned_leading_comments(trivia: SourceTrivia<'_>, writer: &mut TextWriter) {
    let mut needs_separator = false;
    for comment in collect_source_comment_ranges(trivia.source, trivia.start, false)
        .into_iter()
        .take_while(|comment| comment.end <= trivia.end)
        .filter(|comment| is_pinned_source_comment(trivia.source, *comment))
    {
        if needs_separator {
            writer.write_space(" ");
            needs_separator = false;
        }
        write_source_comment(trivia.source, comment.start, comment.end, writer);
        if comment.has_trailing_new_line {
            writer.write_line(false);
        } else {
            needs_separator = true;
        }
    }
    if needs_separator {
        writer.write_space(" ");
    }
}

/// The comment-lane source position: byte offset → UTF-16 line/character
/// over the current file's text, with the same line-start semantics as
/// the writer's generated tracking (`compute_line_starts`). Comments
/// always record against the CURRENT print source (upstream
/// `forEachLeadingCommentRange`/`forEachTrailingCommentRange` walk
/// `currentSourceFile.text`), so the text at hand is authoritative.
fn source_comment_utf16_location(source: &str, byte: usize) -> (u32, u32) {
    let prefix = &source[..byte];
    let starts = compute_line_starts(prefix);
    let line = u32::try_from(starts.len().saturating_sub(1)).expect("comment line exceeds u32");
    let line_start = starts.last().copied().unwrap_or(0) as usize;
    let character = u32::try_from(prefix[line_start..].encode_utf16().count())
        .expect("comment character exceeds u32");
    (line, character)
}

/// tsc-port: writeCommentRange @6.0.3
/// tsc-hash: 38bb9a8ad12c162ca638f01b60e4d17574d6bcfb54ffe941f257f40f02fc7d8b
/// tsc-span: _tsc.js:16867-16919
///
/// h2-6a-m-2 §4 comment phase: every source-comment byte funnels
/// through this writer (the triple-slash normalized-newlines path
/// included), and upstream brackets each written comment with
/// `emitPos(commentPos)`/`emitPos(commentEnd)` (_tsc.js:121151-121273)
/// — recorded here against the current print source.
fn write_source_comment(
    source: &str,
    comment_start: usize,
    comment_end: usize,
    writer: &mut TextWriter,
) {
    debug_assert!(comment_start <= comment_end);
    debug_assert!(source.is_char_boundary(comment_start));
    debug_assert!(source.is_char_boundary(comment_end));

    if writer.has_source_map_recording() {
        let (line, character) = source_comment_utf16_location(source, comment_start);
        writer.record_source_map_position(line, character);
    }
    write_source_comment_text(source, comment_start, comment_end, writer);
    if writer.has_source_map_recording() {
        let (line, character) = source_comment_utf16_location(source, comment_end);
        writer.record_source_map_position(line, character);
    }
}

fn write_source_comment_text(
    source: &str,
    comment_start: usize,
    comment_end: usize,
    writer: &mut TextWriter,
) {
    if source.as_bytes().get(comment_start + 1) != Some(&b'*') {
        writer.write_comment(&source[comment_start..comment_end]);
        return;
    }

    let first_line_start = source[..comment_start]
        .char_indices()
        .rev()
        .find(|(_, character)| is_line_break(*character))
        .map_or(0, |(position, character)| position + character.len_utf8());
    let first_comment_line_indent =
        calculate_source_indent(source, first_line_start, comment_start);
    let mut position = comment_start;

    while position < comment_end {
        let (line_end, next_line_start) = source_line_boundary(source, position, comment_end);
        if position != comment_start {
            let writer_indent_spacing = writer.indent() as usize * TextWriter::indent_size();
            let current_source_indent = calculate_source_indent(source, position, next_line_start);
            let relative_indent = writer_indent_spacing.saturating_add(current_source_indent);
            if relative_indent > first_comment_line_indent {
                writer.raw_write(&" ".repeat(relative_indent - first_comment_line_indent));
            } else {
                // An empty raw write intentionally suppresses the writer's
                // automatic indentation, just as tsc does for non-positive
                // relative indentation.
                writer.raw_write("");
            }
        }

        let current_line = source[position..line_end].trim_matches(is_js_whitespace);
        if current_line.is_empty() {
            writer.write_line(true);
        } else {
            writer.write_comment(current_line);
            if line_end != comment_end {
                writer.write_line(false);
            }
        }

        if next_line_start >= comment_end {
            break;
        }
        position = next_line_start;
    }
}

fn source_line_boundary(source: &str, start: usize, end: usize) -> (usize, usize) {
    for (relative, character) in source[start..end].char_indices() {
        if !is_line_break(character) {
            continue;
        }
        let line_end = start + relative;
        let mut next_line_start = line_end + character.len_utf8();
        if character == '\r' && source.as_bytes().get(next_line_start) == Some(&b'\n') {
            next_line_start += 1;
        }
        return (line_end, next_line_start);
    }
    (end, end)
}

fn calculate_source_indent(source: &str, start: usize, end: usize) -> usize {
    let mut indent = 0usize;
    for character in source[start..end].chars() {
        if is_line_break(character) || !is_whitespace_like(character) {
            break;
        }
        if character == '\t' {
            indent += TextWriter::indent_size() - indent % TextWriter::indent_size();
        } else {
            indent += 1;
        }
    }
    indent
}

fn write_comment_with_normalized_newlines(comment: &str, writer: &mut TextWriter) {
    let normalized = comment.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.split('\n').peekable();
    while let Some(line) = lines.next() {
        writer.write_comment(line);
        if lines.peek().is_some() {
            writer.write_line(true);
        }
    }
}

fn normalize_new_lines(text: &str, new_line: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if new_line == "\n" {
        normalized
    } else {
        normalized.replace('\n', new_line)
    }
}

fn quote_string_literal(text: &str, single_quote: bool, no_ascii_escaping: bool) -> String {
    quote_javascript_string(
        &text.encode_utf16().collect::<Vec<_>>(),
        single_quote,
        no_ascii_escaping,
    )
}

fn quote_javascript_string(units: &[u16], single_quote: bool, no_ascii_escaping: bool) -> String {
    let quote = if single_quote { '\'' } else { '"' };
    let mut quoted = String::with_capacity(units.len() + 2);
    quoted.push(quote);
    let mut index = 0usize;
    while index < units.len() {
        let unit = units[index];
        if no_ascii_escaping
            && (0xd800..=0xdbff).contains(&unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
        {
            let next = units[index + 1];
            let scalar = 0x10000 + (((unit - 0xd800) as u32) << 10) + (next - 0xdc00) as u32;
            if let Some(character) = char::from_u32(scalar) {
                push_quoted_character(&mut quoted, character, quote, false);
            }
            index += 2;
            continue;
        }
        if !no_ascii_escaping && unit > 0x7f || (0xd800..=0xdfff).contains(&unit) {
            use std::fmt::Write;
            let _ = write!(quoted, "\\u{unit:04X}");
            index += 1;
            continue;
        }
        if unit == 0 {
            // `getReplacement` (`_tsc.js:16301-16310`): NUL prints as `\0`
            // unless a decimal digit follows (which would form a legacy
            // octal escape) — then `\x00`.
            let digit_follows = units
                .get(index + 1)
                .is_some_and(|next| (0x30..=0x39).contains(next));
            quoted.push_str(if digit_follows { "\\x00" } else { "\\0" });
            index += 1;
            continue;
        }
        if let Some(character) = char::from_u32(unit as u32) {
            push_quoted_character(&mut quoted, character, quote, !no_ascii_escaping);
        }
        index += 1;
    }
    quoted.push(quote);
    quoted
}

fn push_quoted_character(
    quoted: &mut String,
    character: char,
    quote: char,
    escape_non_ascii: bool,
) {
    match character {
        character if character == quote => {
            quoted.push('\\');
            quoted.push(character);
        }
        '\\' => quoted.push_str("\\\\"),
        '\n' => quoted.push_str("\\n"),
        '\r' => quoted.push_str("\\r"),
        '\t' => quoted.push_str("\\t"),
        '\u{0008}' => quoted.push_str("\\b"),
        '\u{000c}' => quoted.push_str("\\f"),
        '\u{2028}' => quoted.push_str("\\u2028"),
        '\u{2029}' => quoted.push_str("\\u2029"),
        character if character < '\u{0020}' => {
            use std::fmt::Write;
            let _ = write!(quoted, "\\u{:04X}", character as u32);
        }
        character if escape_non_ascii && !character.is_ascii() => {
            for unit in character.encode_utf16(&mut [0; 2]) {
                use std::fmt::Write;
                let _ = write!(quoted, "\\u{unit:04X}");
            }
        }
        character => quoted.push(character),
    }
}

fn write_synthetic_comment(comment: &SyntheticComment, writer: &mut TextWriter) {
    match comment.kind() {
        SyntheticCommentKind::SingleLine => {
            writer.write_comment("//");
            writer.write_comment(comment.text());
        }
        SyntheticCommentKind::MultiLine => {
            writer.write_comment("/*");
            writer.write_comment(comment.text());
            writer.write_comment("*/");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OriginalNodeText<'a> {
    range: SourceByteRange,
    text: &'a str,
}

fn raw_write_range(
    writer: &mut TextWriter,
    text: &str,
    start: u32,
    end: u32,
) -> Result<(), PrinterError> {
    if start == end {
        return Ok(());
    }
    if start > end || end as usize > text.len() {
        return Err(PrinterError::InvalidTextSlice { start, end });
    }
    let slice = text
        .get(start as usize..end as usize)
        .ok_or(PrinterError::InvalidTextSlice { start, end })?;
    writer.raw_write(slice);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrinterError {
    Unsupported(UnsupportedEmitFeature),
    OptionUnavailable(&'static str),
    Transform(TransformError),
    Position(SourcePositionError),
    SourceIsNotATransformedRoot(TransformSourceId),
    RootIsNotSourceFile(TransformNode),
    UnknownStatement(u32),
    SyntheticNodeWorkerUnavailable(TransformNode),
    TransformedNodeWorkerUnavailable(TransformNode),
    UnsupportedTransformedSyntax {
        node: TransformNode,
        kind: SyntaxKind,
    },
    MissingTransformedChild {
        parent: SyntaxKind,
        field: &'static str,
    },
    OverlappingSourceRange {
        previous_end: u32,
        start: u32,
    },
    InvalidTextSlice {
        start: u32,
        end: u32,
    },
    TokenPositionNotScalarBoundary {
        position: u32,
    },
    TokenCursorSourceMismatch {
        cursor: TransformSourceId,
        owner: TransformSourceId,
    },
    CommentCursorSourceMismatch {
        cursor: TransformSourceId,
        owner: TransformSourceId,
    },
    CommentResumeBeforeOwner {
        source: TransformSourceId,
        owner_start: u32,
        next: u32,
    },
    CommentResumeOwnerMismatch {
        source: TransformSourceId,
        left_start: u32,
        right_start: u32,
    },
    TokenPositionOverflow {
        position: u32,
        token: SyntaxKind,
    },
    RetainedArrowTokenPipelineHooks {
        token: TransformNode,
        substitution: bool,
        notification: bool,
    },
    EmitHelperTextUnavailable(Box<str>),
}

impl From<TransformError> for PrinterError {
    fn from(value: TransformError) -> Self {
        Self::Transform(value)
    }
}

impl From<SourcePositionError> for PrinterError {
    fn from(value: SourcePositionError) -> Self {
        Self::Position(value)
    }
}

impl fmt::Display for PrinterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => {
                write!(formatter, "unsupported printer request: {}", feature.name())
            }
            Self::OptionUnavailable(option) => {
                write!(formatter, "printer option {option} is not active in H1.2")
            }
            Self::Transform(error) => error.fmt(formatter),
            Self::Position(error) => error.fmt(formatter),
            Self::SourceIsNotATransformedRoot(source) => write!(
                formatter,
                "transform source {} is not a completed root",
                source.raw()
            ),
            Self::RootIsNotSourceFile(node) => write!(
                formatter,
                "transform root {}:{} is not a SourceFile",
                node.source().raw(),
                node.node().0
            ),
            Self::UnknownStatement(node) => {
                write!(formatter, "source-file statement node {node} is unknown")
            }
            Self::SyntheticNodeWorkerUnavailable(node) => write!(
                formatter,
                "synthetic node {}:{} requires the H1.3 node worker",
                node.source().raw(),
                node.node().0
            ),
            Self::TransformedNodeWorkerUnavailable(node) => write!(
                formatter,
                "transformed node {}:{} requires the H1.3 node worker",
                node.source().raw(),
                node.node().0
            ),
            Self::UnsupportedTransformedSyntax { node, kind } => write!(
                formatter,
                "transformed {kind:?} node {}:{} has no active H1 printer worker",
                node.source().raw(),
                node.node().0
            ),
            Self::MissingTransformedChild { parent, field } => {
                write!(formatter, "transformed {parent:?} is missing child {field}")
            }
            Self::OverlappingSourceRange {
                previous_end,
                start,
            } => write!(
                formatter,
                "source statement range starts at {start} before prior end {previous_end}"
            ),
            Self::InvalidTextSlice { start, end } => {
                write!(formatter, "invalid source text slice {start}..{end}")
            }
            Self::TokenPositionNotScalarBoundary { position } => write!(
                formatter,
                "UTF-16 token position {position} does not map to a source scalar boundary"
            ),
            Self::TokenCursorSourceMismatch { cursor, owner } => write!(
                formatter,
                "token cursor source {} does not match comment owner source {}",
                cursor.raw(),
                owner.raw()
            ),
            Self::CommentCursorSourceMismatch { cursor, owner } => write!(
                formatter,
                "comment cursor source {} does not match comment owner source {}",
                cursor.raw(),
                owner.raw()
            ),
            Self::CommentResumeBeforeOwner {
                source,
                owner_start,
                next,
            } => write!(
                formatter,
                "comment resume position {next} precedes owner start {owner_start} in source {}",
                source.raw()
            ),
            Self::CommentResumeOwnerMismatch {
                source,
                left_start,
                right_start,
            } => write!(
                formatter,
                "comment resumes have different owner starts {left_start} and {right_start} in source {}",
                source.raw()
            ),
            Self::TokenPositionOverflow { position, token } => write!(
                formatter,
                "token {token:?} overflows source position {position}"
            ),
            Self::RetainedArrowTokenPipelineHooks {
                token,
                substitution,
                notification,
            } => write!(
                formatter,
                "retained arrow token {}:{} cannot use the arrow comment adapter with emit pipeline hooks (substitution={substitution}, notification={notification})",
                token.source().raw(),
                token.node().0,
            ),
            Self::EmitHelperTextUnavailable(helper) => {
                write!(formatter, "emit helper {helper} has no printable text")
            }
        }
    }
}

impl Error for PrinterError {}

#[cfg(test)]
#[path = "../tests/unit/comment_scope_predicate/tests.rs"]
mod comment_scope_predicate_tests;
