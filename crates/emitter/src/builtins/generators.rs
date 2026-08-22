//! H2.5h-b B-3: the Generators state machine.
//!
//! Function-per-function port of tsc's `transformGenerators` as bundled at
//! `_tsc.js:108119-110087` (the `transform-generators` owner frozen in
//! `ratchets/h2-5h-a-owner-graph.v1.json`, 129 pinned local functions),
//! plus the owner-adjacent addenda pinned in the packet
//! (`docs/design/greenfield/slices/h2-5h-b-b-3.md` §4.2). The module is
//! DORMANT: `transform_generators` is registered by no pipeline until the
//! B-5 runtime flip; until then the only callers are the focused
//! projection suite below, which drives this real transformer on parsed
//! fixtures and byte-compares against fresh-process oracle emits.
//!
//! Consumer-first per the owner graph's pinned `yield-star-synthesis`
//! composition edge: the machine consumes `yield*` (including the
//! `EmitFlags::ITERATOR` skip for B-4's synthesized iterators) before the
//! ES2015 producer lands.

use std::collections::BTreeMap;

use tsc_program::SourceFileId;
use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::ScriptTarget;

use crate::{
    factory::EmitHelperName, resolver::EmitResolver, CommentRange, EmitFlags, EmitHint,
    LexicalEnvironment, NodeFactory, SourceMapRange, SourceRange, SyntheticComment,
    SyntheticCommentKind, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
};

use super::{
    flags_after_update,
    generated_bindings::{AncestorBindingPolicy, GeneratedBindingScopes},
    helpers, initialize_transform_flags,
    target_bindings::{
        collect_untagged_identifier_texts, finalize_generated_binding_names, TargetBinding,
    },
};

// ---------------------------------------------------------------------------
// Alphabets
// ---------------------------------------------------------------------------

/// The recording alphabet (`OpCode`, bundler-inlined at every emit site:
/// `0 /* Nop */` .. `10 /* Endfinally */`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpCode {
    Nop,
    Statement,
    Assign,
    Break,
    BreakWhenTrue,
    BreakWhenFalse,
    Yield,
    YieldStar,
    Return,
    Throw,
    Endfinally,
}

/// The EMITTED alphabet inside `return [n, …]` arrays (`Instruction`):
/// `Next=0, Throw=1, Return=2, Break=3, Yield=4, YieldStar=5, Catch=6,
/// Endfinally=7`. Only Return/Break/Yield/YieldStar/Endfinally carry the
/// synthetic `/*name*/` trailing comment (`getInstructionName`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Instruction {
    Return,
    Break,
    Yield,
    YieldStar,
    Endfinally,
}

impl Instruction {
    const fn code(self) -> u32 {
        match self {
            Self::Return => 2,
            Self::Break => 3,
            Self::Yield => 4,
            Self::YieldStar => 5,
            Self::Endfinally => 7,
        }
    }

    /// tsc-port: getInstructionName @6.0.3
    /// tsc-hash: eb0ca6bd5545a7d3e3a5122748e1ddaa3d2fe6b95d183300c01411904e967e10
    /// tsc-span: _tsc.js:108103-108118
    const fn comment_text(self) -> &'static str {
        match self {
            Self::Return => "return",
            Self::Break => "break",
            Self::Yield => "yield",
            Self::YieldStar => "yield*",
            Self::Endfinally => "endfinally",
        }
    }
}

/// `CodeBlockKind` actions: `0 /* Open */`, `1 /* Close */`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockAction {
    Open,
    Close,
}

/// `ExceptionBlockState`: `Try=0 < Catch=1 < Finally=2 < Done=3` (the
/// begin/end asserts compare these ordinals).
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
enum ExceptionBlockState {
    Try,
    Catch,
    Finally,
    Done,
}

/// Index into the visitor's block arena (upstream keeps ONE object identity
/// across `beginBlock`/`endBlock`; the arena index is that identity).
type BlockId = usize;

/// Labels are `usize` ids allocated from 1 (`nextLabelId`); 0 is the shared
/// "no target" sentinel `findBreakTarget`/`findContinueTarget` return and
/// the script blocks' `breakLabel: -1` collapses onto it (no valid emitted
/// label is 0).
type Label = usize;

/// The code-block records (`beginBlock` object literals; kinds
/// `Exception=0, With=1, Switch=2, Loop=3, Labeled=4`). Exception blocks
/// mutate in place across begin/end (state transitions, catch/finally
/// labels), so the arena entry is the single owner.
#[derive(Debug)]
enum CodeBlock {
    Exception {
        state: ExceptionBlockState,
        start_label: Label,
        end_label: Label,
        catch_variable: Option<TransformNode>,
        catch_label: Option<Label>,
        finally_label: Option<Label>,
    },
    With {
        expression: TransformNode,
        /// Recorded for the upstream record shape; only `end_label` is
        /// read back (`endWithBlock`).
        #[allow(dead_code)]
        start_label: Label,
        end_label: Label,
    },
    Switch {
        is_script: bool,
        break_label: Label,
    },
    Loop {
        is_script: bool,
        break_label: Label,
        continue_label: Label,
    },
    Labeled {
        is_script: bool,
        label_text: String,
        break_label: Label,
    },
}

impl CodeBlock {
    /// tsc-port: supportsUnlabeledBreak @6.0.3
    /// tsc-hash: 338a47b0a2efcc800983f5f92fbdb44d5206e4983a6a0226e7619a2b87746b36
    /// tsc-span: _tsc.js:109517-109519
    const fn supports_unlabeled_break(&self) -> bool {
        matches!(self, Self::Switch { .. } | Self::Loop { .. })
    }

    /// tsc-port: supportsLabeledBreakOrContinue @6.0.3
    /// tsc-hash: 8843ad57c6e6a83f6335c4b8a78e2e45948af398c3bc9e97b285a1a3e638cfe8
    /// tsc-span: _tsc.js:109520-109522
    const fn supports_labeled_break_or_continue(&self) -> bool {
        matches!(self, Self::Labeled { .. })
    }

    /// tsc-port: supportsUnlabeledContinue @6.0.3
    /// tsc-hash: 81ce98f906a0701e222a278d50cc44fc88e120879a2ca0202cf69ebf1e5a7f31
    /// tsc-span: _tsc.js:109523-109525
    const fn supports_unlabeled_continue(&self) -> bool {
        matches!(self, Self::Loop { .. })
    }
}

/// One recorded operation (`operations` + `operationArguments` +
/// `operationLocations` share the index).
#[derive(Debug)]
struct Operation {
    code: OpCode,
    args: Vec<TransformNode>,
    labels: Vec<Label>,
    location: Option<TransformNode>,
}

// ---------------------------------------------------------------------------
// Transformer seam
// ---------------------------------------------------------------------------

/// Catch-variable rename state. Upstream keeps these at the
/// `transformGenerators` closure level because `onSubstituteNode` runs at
/// PRINT time, after every generator body has been transformed; the Rust
/// carrier is therefore the `Transformer` (not the per-root visitor).
#[derive(Default)]
struct RenameState {
    /// `renamedCatchVariables` — source texts with at least one rename.
    renamed_catch_variables: BTreeMap<String, ()>,
    /// `renamedCatchVariableDeclarations[getOriginalNodeId(decl)] = name`,
    /// keyed by the parse-tree identity the resolver reports.
    renamed_catch_variable_declarations: BTreeMap<(SourceFileId, NodeId), TargetBinding>,
}

/// tsc-port: transformGenerators @6.0.3
/// tsc-hash: a7c6256b82433a63dc01b3887370845fab05df1c00083366114fa045437f8e95
/// tsc-span: _tsc.js:108119-110087
///
/// The registration seam the B-5 runtime flip wires as the second entry of
/// the joint `[transformES2015, transformGenerators]` pass list
/// (`languageVersion < ES2015`, owner-graph `upstream_registration`).
#[allow(dead_code)] // the production registration arrives with the B-5 owner
pub(super) fn transform_generators<'resolver>(
    language_version: ScriptTarget,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(GeneratorsTransformer {
        resolver,
        language_version,
        renames: RenameState::default(),
    })
}

struct GeneratorsTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    language_version: ScriptTarget,
    renames: RenameState,
}

impl Transformer for GeneratorsTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformGenerators"
    }

    /// tsc-port: transformSourceFile @6.0.3
    /// tsc-hash: 73ead7f02f3e34dd8ea4ebd6677fe6ad02975986387bf7e3070c8038afbf4866
    /// tsc-span: _tsc.js:108159-108166
    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(root);
        }
        initialize_transform_flags(context.arena_mut()?, source)?;
        let current_root = context.arena().root(source)?;
        if !context
            .arena()
            .transform_flags(current_root)
            .contains(TransformFlags::CONTAINS_GENERATOR)
        {
            return Ok(root);
        }
        let mut visitor = GeneratorsVisitor::new(
            context,
            source,
            self.language_version,
            &mut self.renames,
            current_root,
        )?;
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.node(transformed);
        finalize_generated_binding_names(visitor.context, source, transformed)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }

    /// tsc-port: onSubstituteNode @6.0.3
    /// tsc-hash: 95db4fbcaba10e96d12184b62ac54dd102ddb3c2a854723902e633ab92aa8693
    /// tsc-span: _tsc.js:109260-109266
    ///
    /// Previous-first delegation is the harness's job (the B-1 hook-chain
    /// order contracts); this body is the owner's own arm.
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if hint == EmitHint::Expression {
            return self.substitute_expression(context, node);
        }
        Ok(node)
    }
}

impl GeneratorsTransformer<'_> {
    /// tsc-port: substituteExpression @6.0.3
    /// tsc-hash: ab512e435fe64c869361c8729c3d87b75cb965d3e702b6de4f82cc1b74ceb921
    /// tsc-span: _tsc.js:109267-109272
    fn substitute_expression(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(context.arena().node(node)?.data, NodeData::Identifier(_)) {
            return self.substitute_expression_identifier(context, node);
        }
        Ok(node)
    }

    /// tsc-port: substituteExpressionIdentifier @6.0.3
    /// tsc-hash: 9ad55adb46c4f1e904041b50939e84e422ac31d9ea5483001ca44050e80611d4
    /// tsc-span: _tsc.js:109273-109290
    fn substitute_expression_identifier(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let is_generated = context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_id().is_some());
        if is_generated || self.renames.renamed_catch_variables.is_empty() {
            return Ok(node);
        }
        let text = match &context.arena().node(node)?.data {
            NodeData::Identifier(data) => data.text.clone(),
            _ => return Ok(node),
        };
        if !self.renames.renamed_catch_variables.contains_key(&text) {
            return Ok(node);
        }
        let original = context.arena().get_original_node(node);
        if !matches!(
            context.arena().node(original)?.data,
            NodeData::Identifier(_)
        ) {
            return Ok(node);
        }
        // `original.parent` gates the walk upstream: only identifiers that
        // exist in the parse tree resolve. `parse_tree_resolver_node`
        // reports exactly that membership.
        let Some(reference) = context.arena().parse_tree_resolver_node(original)? else {
            return Ok(node);
        };
        let Some(declaration) = self.resolver.get_referenced_value_declaration(reference)? else {
            return Ok(node);
        };
        let Some(binding) = self
            .renames
            .renamed_catch_variable_declarations
            .get(&(declaration.source(), declaration.node()))
            .cloned()
        else {
            return Ok(node);
        };
        // `setParent(setTextRange(factory2.cloneNode(name), name), name.parent)`
        // + source-map/comment ranges of the substituted node. The clone is
        // a fresh generated identifier carrying the binding metadata; parent
        // linkage does not exist in the arena (byte-inert).
        let clone = {
            let mut factory = context.substitution_factory()?;
            create_identifier_raw(&mut factory, node.source(), binding.provisional_name())?
        };
        binding.write_generated_metadata(context.arena_mut()?, clone);
        let (source_map_range, comment_range) = substituted_ranges(context, node)?;
        let metadata = context.arena_mut()?.metadata_mut(clone);
        if let Some(range) = source_map_range {
            metadata.set_source_map_range(range);
        }
        if let Some(range) = comment_range {
            metadata.set_comment_range(range);
        }
        Ok(clone)
    }
}

// ---------------------------------------------------------------------------
// The visitor
// ---------------------------------------------------------------------------

/// The per-source-file machine. Upstream keeps this state in
/// `transformGenerators`-level `let`s; every field that
/// `transformGeneratorFunctionBody` saves/restores lives here, and nested
/// generator bodies stack through the save/restore protocol exactly as
/// upstream. The catch-rename maps live on the TRANSFORMER (print-time
/// substitution consumes them after this visitor is gone).
struct GeneratorsVisitor<'context, 'renames> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    language_version: ScriptTarget,
    renames: &'renames mut RenameState,
    generated_bindings: GeneratedBindingScopes,

    in_generator_function_body: bool,
    in_statement_containing_yield: bool,

    // Recording state (`operations`/`operationArguments`/`operationLocations`).
    operations: Vec<Operation>,
    // Block state.
    blocks: Vec<CodeBlock>,
    block_actions: Vec<(BlockAction, usize, BlockId)>,
    block_stack: Vec<BlockId>,
    // Label state. `label_offsets[label]` is `None` until `mark_label`
    // (upstream initializes to -1); index 0 is never allocated.
    label_offsets: Vec<Option<usize>>,
    next_label_id: Label,
    // The state temp for the CURRENT generator body.
    state: Option<TargetBinding>,

    // Build state (reset by `build`).
    block_index: usize,
    label_number: usize,
    label_numbers: Vec<Option<Vec<Label>>>,
    last_operation_was_abrupt: bool,
    last_operation_was_completion: bool,
    clauses: Option<Vec<TransformNode>>,
    statements: Option<Vec<TransformNode>>,
    exception_block_stack: Vec<Option<BlockId>>,
    current_exception_block: Option<BlockId>,
    with_block_stack: Vec<BlockId>,
    // `labelExpressions[label]` — the placeholder literals the final
    // `updateLabelExpressions` pass finalizes (the arena text-finalization
    // API; the `set_generated_identifier_text` precedent).
    label_expressions: Vec<Vec<TransformNode>>,
}

/// The state `transformGeneratorFunctionBody` saves and restores
/// (`_tsc.js:108342-108366` / `:108383-108395`).
struct SavedBodyState {
    in_generator_function_body: bool,
    in_statement_containing_yield: bool,
    operations: Vec<Operation>,
    blocks: Vec<CodeBlock>,
    block_actions: Vec<(BlockAction, usize, BlockId)>,
    block_stack: Vec<BlockId>,
    label_offsets: Vec<Option<usize>>,
    label_expressions: Vec<Vec<TransformNode>>,
    next_label_id: Label,
    state: Option<TargetBinding>,
}

impl<'context, 'renames> GeneratorsVisitor<'context, 'renames> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        language_version: ScriptTarget,
        renames: &'renames mut RenameState,
        root: TransformNode,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_untagged_identifier_texts(context.arena(), source, root)?,
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            language_version,
            renames,
            in_generator_function_body: false,
            in_statement_containing_yield: false,
            operations: Vec::new(),
            blocks: Vec::new(),
            block_actions: Vec::new(),
            block_stack: Vec::new(),
            label_offsets: Vec::new(),
            next_label_id: 1,
            state: None,
            block_index: 0,
            label_number: 0,
            label_numbers: Vec::new(),
            last_operation_was_abrupt: false,
            last_operation_was_completion: false,
            clauses: None,
            statements: None,
            exception_block_stack: Vec::new(),
            current_exception_block: None,
            with_block_stack: Vec::new(),
            label_expressions: Vec::new(),
        })
    }

    fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    fn transform_flags(&self, id: NodeId) -> TransformFlags {
        self.context.arena().transform_flags(self.node(id))
    }

    fn kind(&self, id: NodeId) -> Result<SyntaxKind, TransformError> {
        Ok(self.context.arena().node(self.node(id))?.kind)
    }

    fn data(&self, id: NodeId) -> Result<NodeData, TransformError> {
        Ok(self.context.arena().node(self.node(id))?.data.clone())
    }

    // -----------------------------------------------------------------------
    // Dispatch
    // -----------------------------------------------------------------------

    /// tsc-port: visitor @6.0.3
    /// tsc-hash: 308e1dfaad03d12139fe34a12a955137b22ba52a29451647a48a09280b11b0de
    /// tsc-span: _tsc.js:108167-108180
    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let transform_flags = self.transform_flags(id);
        if self.in_statement_containing_yield {
            self.visit_java_script_in_statement_containing_yield(id)
        } else if self.in_generator_function_body {
            self.visit_java_script_in_generator_function_body(id)
        } else if self.is_function_like_declaration_with_asterisk(id)? {
            self.visit_generator(id)
        } else if transform_flags.contains(TransformFlags::CONTAINS_GENERATOR) {
            self.visit_each_child(id)
        } else {
            Ok(Some(id))
        }
    }

    /// `isFunctionLikeDeclaration(node) && node.asteriskToken` — the
    /// generator-capable function-like kinds.
    fn is_function_like_declaration_with_asterisk(
        &self,
        id: NodeId,
    ) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(self.node(id))?.data {
            NodeData::FunctionDeclaration(data) => data.asterisk_token.is_some(),
            NodeData::FunctionExpression(data) => data.asterisk_token.is_some(),
            NodeData::MethodDeclaration(data) => data.asterisk_token.is_some(),
            _ => false,
        })
    }

    /// tsc-port: visitJavaScriptInStatementContainingYield @6.0.3
    /// tsc-hash: bad26cf40a6fce206348a2af61015cd15a85bde8d05429d1d73e91765a98ffc6
    /// tsc-span: _tsc.js:108181-108194
    fn visit_java_script_in_statement_containing_yield(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        match self.kind(id)? {
            SyntaxKind::DoStatement => self.visit_do_statement(id),
            SyntaxKind::WhileStatement => self.visit_while_statement(id),
            SyntaxKind::SwitchStatement => self.visit_switch_statement(id),
            SyntaxKind::LabeledStatement => self.visit_labeled_statement(id),
            _ => self.visit_java_script_in_generator_function_body(id),
        }
    }

    /// tsc-port: visitJavaScriptInGeneratorFunctionBody @6.0.3
    /// tsc-hash: 63e11dc87d7f1e0449c3c5c8c41a4108253cc6b27f0acd7850ee53f9c8997981
    /// tsc-span: _tsc.js:108195-108225
    fn visit_java_script_in_generator_function_body(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        match self.kind(id)? {
            SyntaxKind::FunctionDeclaration => self.visit_function_declaration(id),
            SyntaxKind::FunctionExpression => self.visit_function_expression(id),
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                self.visit_accessor_declaration(id)
            }
            SyntaxKind::VariableStatement => self.visit_variable_statement(id),
            SyntaxKind::ForStatement => self.visit_for_statement(id),
            SyntaxKind::ForInStatement => self.visit_for_in_statement(id),
            SyntaxKind::BreakStatement => self.visit_break_statement(id),
            SyntaxKind::ContinueStatement => self.visit_continue_statement(id),
            SyntaxKind::ReturnStatement => self.visit_return_statement(id),
            _ => {
                let flags = self.transform_flags(id);
                if flags.contains(TransformFlags::CONTAINS_YIELD) {
                    self.visit_java_script_containing_yield(id)
                } else if flags.contains(TransformFlags::CONTAINS_GENERATOR)
                    || flags.contains(TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION)
                {
                    self.visit_each_child(id)
                } else {
                    Ok(Some(id))
                }
            }
        }
    }

    /// tsc-port: visitJavaScriptContainingYield @6.0.3
    /// tsc-hash: 34bf6f04ea57f28683c14c543b122a4bb14d82235224140b1c96c1a6972929c1
    /// tsc-span: _tsc.js:108226-108249
    fn visit_java_script_containing_yield(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        match self.kind(id)? {
            SyntaxKind::BinaryExpression => self.visit_binary_expression(id),
            SyntaxKind::CommaListExpression => self.visit_comma_list_expression(id),
            SyntaxKind::ConditionalExpression => self.visit_conditional_expression(id),
            SyntaxKind::YieldExpression => self.visit_yield_expression(id).map(Some),
            SyntaxKind::ArrayLiteralExpression => self.visit_array_literal_expression(id).map(Some),
            SyntaxKind::ObjectLiteralExpression => {
                self.visit_object_literal_expression(id).map(Some)
            }
            SyntaxKind::ElementAccessExpression => self.visit_element_access_expression(id),
            SyntaxKind::CallExpression => self.visit_call_expression(id),
            SyntaxKind::NewExpression => self.visit_new_expression(id),
            _ => self.visit_each_child(id),
        }
    }

    /// tsc-port: visitGenerator @6.0.3
    /// tsc-hash: 5e9a4a42b8122ef4c7b815998e6cb62b74c0f91ba32d00e3bf68ad644784d07b
    /// tsc-span: _tsc.js:108250-108259
    fn visit_generator(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        match self.kind(id)? {
            SyntaxKind::FunctionDeclaration => self.visit_function_declaration(id),
            SyntaxKind::FunctionExpression => self.visit_function_expression(id),
            // `Debug.failBadSyntaxKind(node)`: a generator method/accessor
            // reaching the machine un-lowered (transformES2015 converts
            // object/class generator methods before this pass runs).
            other => Err(TransformError::UnexpectedChildKind {
                parent: other,
                field: "visitGenerator function-like declaration",
                actual: other,
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------

    /// tsc-port: visitFunctionDeclaration @6.0.3
    /// tsc-hash: 40d0dbd048205fe90ac87a5e4c2f2662f28a9079b78fe3ade306ed9118a2f6e0
    /// tsc-span: _tsc.js:108260-108296
    fn visit_function_declaration(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let mut node = self.node(id);
        let NodeData::FunctionDeclaration(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "function declaration data",
            });
        };
        if data.asterisk_token.is_some() {
            let parameters = self.visit_parameter_list(data.parameters)?;
            let body = data.body.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "body",
            })?;
            let body = self.transform_generator_function_body(body)?;
            // Fresh creation (`factory2.createFunctionDeclaration`): child
            // aggregation only — `create_node` adds the per-kind facets
            // (no asterisk => no generator facet; the EA-GAP-FLAGS
            // discipline bans inheriting the stale yield/generator bits).
            let mut flags = self.optional_array_flags(data.modifiers)
                | self.optional_array_flags(parameters)
                | self.context.arena().propagate_child_flags(body)?;
            if let Some(name) = data.name {
                flags |= self
                    .context
                    .arena()
                    .propagate_child_flags(self.node(name))?;
            }
            let replaced =
                NodeData::FunctionDeclaration(tsc_syntax::nodes::FunctionDeclarationData {
                    name: data.name,
                    type_parameters: None,
                    parameters,
                    r#type: None,
                    asterisk_token: None,
                    body: Some(body.node()),
                    modifiers: data.modifiers,
                });
            let created = self
                .context
                .factory()?
                .create_node(self.source, replaced, flags)?;
            let created = self.context.factory()?.set_text_range(created, node)?;
            self.context
                .arena_mut()?
                .set_original_node(created, Some(node))?;
            node = created;
        } else {
            let saved_in_generator_function_body = self.in_generator_function_body;
            let saved_in_statement_containing_yield = self.in_statement_containing_yield;
            self.in_generator_function_body = false;
            self.in_statement_containing_yield = false;
            let visited = self.visit_each_child(id)?;
            self.in_generator_function_body = saved_in_generator_function_body;
            self.in_statement_containing_yield = saved_in_statement_containing_yield;
            node = self.node(visited.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "function declaration",
            })?);
        }
        if self.in_generator_function_body {
            self.context.hoist_function_declaration(node)?;
            Ok(None)
        } else {
            Ok(Some(node.node()))
        }
    }

    /// tsc-port: visitFunctionExpression @6.0.3
    /// tsc-hash: 2b3f71007b119b5c39c87413cb1212f275c3391dbeff9611596dcd3923215792
    /// tsc-span: _tsc.js:108297-108329
    fn visit_function_expression(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::FunctionExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionExpression,
                field: "function expression data",
            });
        };
        if data.asterisk_token.is_some() {
            let parameters = self.visit_parameter_list(data.parameters)?;
            let body = data.body.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionExpression,
                field: "body",
            })?;
            let body = self.transform_generator_function_body(body)?;
            let mut flags = self.optional_array_flags(parameters)
                | self.context.arena().propagate_child_flags(body)?;
            if let Some(name) = data.name {
                flags |= self
                    .context
                    .arena()
                    .propagate_child_flags(self.node(name))?;
            }
            let replaced =
                NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                    name: data.name,
                    type_parameters: None,
                    parameters,
                    r#type: None,
                    asterisk_token: None,
                    body: Some(body.node()),
                    modifiers: None,
                });
            let created = self
                .context
                .factory()?
                .create_node(self.source, replaced, flags)?;
            let created = self.context.factory()?.set_text_range(created, node)?;
            self.context
                .arena_mut()?
                .set_original_node(created, Some(node))?;
            Ok(Some(created.node()))
        } else {
            let saved_in_generator_function_body = self.in_generator_function_body;
            let saved_in_statement_containing_yield = self.in_statement_containing_yield;
            self.in_generator_function_body = false;
            self.in_statement_containing_yield = false;
            let visited = self.visit_each_child(id)?;
            self.in_generator_function_body = saved_in_generator_function_body;
            self.in_statement_containing_yield = saved_in_statement_containing_yield;
            Ok(visited)
        }
    }

    /// tsc-port: visitAccessorDeclaration @6.0.3
    /// tsc-hash: 894de52c8603a49d3cf839366ec506ec76a5b9e065923cc73214029af55e2aa2
    /// tsc-span: _tsc.js:108330-108339
    fn visit_accessor_declaration(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let saved_in_generator_function_body = self.in_generator_function_body;
        let saved_in_statement_containing_yield = self.in_statement_containing_yield;
        self.in_generator_function_body = false;
        self.in_statement_containing_yield = false;
        let visited = self.visit_each_child(id)?;
        self.in_generator_function_body = saved_in_generator_function_body;
        self.in_statement_containing_yield = saved_in_statement_containing_yield;
        Ok(visited)
    }

    /// tsc-port: transformGeneratorFunctionBody @6.0.3
    /// tsc-hash: b5c1b42385ee0a311c638406213c2f61ddc245bf21ba1c206259ad02dab08b26
    /// tsc-span: _tsc.js:108340-108397
    fn transform_generator_function_body(
        &mut self,
        body: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let body_node = self.node(body);
        let NodeData::Block(block) = self.data(body)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Block,
                field: "generator body block",
            });
        };
        let mut statements: Vec<TransformNode> = Vec::new();
        let saved = SavedBodyState {
            in_generator_function_body: self.in_generator_function_body,
            in_statement_containing_yield: self.in_statement_containing_yield,
            operations: std::mem::take(&mut self.operations),
            blocks: std::mem::take(&mut self.blocks),
            block_actions: std::mem::take(&mut self.block_actions),
            block_stack: std::mem::take(&mut self.block_stack),
            label_offsets: std::mem::take(&mut self.label_offsets),
            label_expressions: std::mem::take(&mut self.label_expressions),
            next_label_id: self.next_label_id,
            state: self.state.take(),
        };
        self.in_generator_function_body = true;
        self.in_statement_containing_yield = false;
        self.next_label_id = 1;
        // `state = factory2.createTempVariable(/*recordTempVariable*/ void 0)`
        // — allocated, never hoisted; the sole `__generator` callback
        // parameter. Naming defers to the doc-order finalize walk under
        // `ReuseTempVariableScope`.
        let state = self.allocate_temp_binding()?;
        self.state = Some(state);
        self.context.resume_lexical_environment()?;
        let source_statements = self.array_nodes_of(block.statements)?;
        let statement_offset = self.copy_prologue(&source_statements, &mut statements, false)?;
        self.transform_and_emit_statements(&source_statements, statement_offset)?;
        let build_result = self.build()?;
        let lexical_environment = self.context.end_lexical_environment()?;
        self.insert_statements_after_standard_prologue(&mut statements, lexical_environment)?;
        let return_statement = self.create_return_statement(Some(build_result))?;
        statements.push(return_statement);
        self.in_generator_function_body = saved.in_generator_function_body;
        self.in_statement_containing_yield = saved.in_statement_containing_yield;
        self.operations = saved.operations;
        self.blocks = saved.blocks;
        self.block_actions = saved.block_actions;
        self.block_stack = saved.block_stack;
        self.label_offsets = saved.label_offsets;
        self.label_expressions = saved.label_expressions;
        self.next_label_id = saved.next_label_id;
        self.state = saved.state;
        // `createBlock(statements2, body.multiLine)` + `setTextRange(…, body)`.
        let body_multi_line = self.node_is_multi_line(body_node)?;
        let block = self.create_block_multi_line(statements, body_multi_line)?;
        let block = self.context.factory()?.set_text_range(block, body_node)?;
        Ok(block)
    }
}

// ---------------------------------------------------------------------------
// Statements: the transformAndEmit family + the script visit family
// ---------------------------------------------------------------------------

impl GeneratorsVisitor<'_, '_> {
    /// tsc-port: visitVariableStatement @6.0.3
    /// tsc-hash: 2cd0a6aa7d6d020abc07018c73d8ba5b7d190557d42f5e87e723e0dba75eef4c
    /// tsc-span: _tsc.js:108398-108422
    fn visit_variable_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::VariableStatement(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableStatement,
                field: "variable statement data",
            });
        };
        if self
            .transform_flags(id)
            .contains(TransformFlags::CONTAINS_YIELD)
        {
            let list = data
                .declaration_list
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declarationList",
                })?;
            self.transform_and_emit_variable_declaration_list(list)?;
            Ok(None)
        } else {
            // `getEmitFlags(node) & CustomPrologue` — B-4-synthesized hoist
            // carriers pass through untouched (dormant arm today).
            if self.emit_flags(node).contains(EmitFlags::CUSTOM_PROLOGUE) {
                return Ok(Some(id));
            }
            let list = data
                .declaration_list
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declarationList",
                })?;
            for declaration in self.declarations_of(list)? {
                let name = self.declaration_name(declaration)?;
                self.context.hoist_variable_declaration(name)?;
            }
            let variables = self.initialized_variables(list)?;
            if variables.is_empty() {
                return Ok(None);
            }
            let mut expressions = Vec::with_capacity(variables.len());
            for variable in variables {
                expressions.push(self.transform_initialized_variable(variable)?);
            }
            let inlined = self.inline_expressions(expressions)?;
            let statement = self.create_expression_statement(inlined)?;
            self.set_source_map_range_from(statement, node)?;
            Ok(Some(statement.node()))
        }
    }

    /// tsc-port: transformAndEmitStatements @6.0.3
    /// tsc-hash: 9f018585be7cc8c2cd3c7b87d0ecc0b1ba4ef42ac117a658e14ea2a19b76b4da
    /// tsc-span: _tsc.js:108743-108748
    fn transform_and_emit_statements(
        &mut self,
        statements: &[TransformNode],
        start: usize,
    ) -> Result<(), TransformError> {
        for statement in statements.iter().skip(start) {
            self.transform_and_emit_statement(*statement)?;
        }
        Ok(())
    }

    /// tsc-port: transformAndEmitEmbeddedStatement @6.0.3
    /// tsc-hash: 2881aa8b507d5888781fa634ab97b8f18f0072bc343e7f6156a2cfe17483c458
    /// tsc-span: _tsc.js:108749-108755
    fn transform_and_emit_embedded_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if let NodeData::Block(data) = self.context.arena().node(node)?.data.clone() {
            let statements = self.array_nodes_of(data.statements)?;
            self.transform_and_emit_statements(&statements, 0)
        } else {
            self.transform_and_emit_statement(node)
        }
    }

    /// tsc-port: transformAndEmitStatement @6.0.3
    /// tsc-hash: 7c8230a8640cc176775bf9b14164fa985eb9040cfe9c2394d21b6c8a25ef471a
    /// tsc-span: _tsc.js:108756-108763
    fn transform_and_emit_statement(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let saved_in_statement_containing_yield = self.in_statement_containing_yield;
        if !self.in_statement_containing_yield {
            self.in_statement_containing_yield = self.contains_yield(Some(node));
        }
        self.transform_and_emit_statement_worker(node)?;
        self.in_statement_containing_yield = saved_in_statement_containing_yield;
        Ok(())
    }

    /// tsc-port: transformAndEmitStatementWorker @6.0.3
    /// tsc-hash: 8aff072cd4d9b5d5e4f25b2a269fc128f8f9e72501d6fef327c56da4c422f482
    /// tsc-span: _tsc.js:108764-108799
    fn transform_and_emit_statement_worker(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(node)?.kind {
            SyntaxKind::Block => self.transform_and_emit_block(node),
            SyntaxKind::ExpressionStatement => self.transform_and_emit_expression_statement(node),
            SyntaxKind::IfStatement => self.transform_and_emit_if_statement(node),
            SyntaxKind::DoStatement => self.transform_and_emit_do_statement(node),
            SyntaxKind::WhileStatement => self.transform_and_emit_while_statement(node),
            SyntaxKind::ForStatement => self.transform_and_emit_for_statement(node),
            SyntaxKind::ForInStatement => self.transform_and_emit_for_in_statement(node),
            SyntaxKind::ContinueStatement => self.transform_and_emit_continue_statement(node),
            SyntaxKind::BreakStatement => self.transform_and_emit_break_statement(node),
            SyntaxKind::ReturnStatement => self.transform_and_emit_return_statement(node),
            SyntaxKind::WithStatement => self.transform_and_emit_with_statement(node),
            SyntaxKind::SwitchStatement => self.transform_and_emit_switch_statement(node),
            SyntaxKind::LabeledStatement => self.transform_and_emit_labeled_statement(node),
            SyntaxKind::ThrowStatement => self.transform_and_emit_throw_statement(node),
            SyntaxKind::TryStatement => self.transform_and_emit_try_statement(node),
            _ => {
                let visited = self.visit_statement_node(node)?;
                self.emit_statement_opt(visited)
            }
        }
    }

    /// tsc-port: transformAndEmitBlock @6.0.3
    /// tsc-hash: b1f5978ac132ca11e0e8cca03631b341ba82720ebf621fc075c045fb95303021
    /// tsc-span: _tsc.js:108800-108806
    fn transform_and_emit_block(&mut self, node: TransformNode) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::Block(data) = self.context.arena().node(node)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Block,
                    field: "block data",
                });
            };
            let statements = self.array_nodes_of(data.statements)?;
            self.transform_and_emit_statements(&statements, 0)
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: transformAndEmitExpressionStatement @6.0.3
    /// tsc-hash: a61dd0047076cf7eff8560d2fbf2f9fbf5ac76d958b53e49004b56e69e98a3e9
    /// tsc-span: _tsc.js:108807-108809
    fn transform_and_emit_expression_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let visited = self.visit_statement_node(node)?;
        self.emit_statement_opt(visited)
    }

    /// tsc-port: transformAndEmitVariableDeclarationList @6.0.3
    /// tsc-hash: 6cdc756131383449ef11bc1c638880dd14f387e6c7b22df16f440273339dcea3
    /// tsc-span: _tsc.js:108810-108835
    fn transform_and_emit_variable_declaration_list(
        &mut self,
        list: NodeId,
    ) -> Result<(), TransformError> {
        for declaration in self.declarations_of_id(list)? {
            let name = self.declaration_name(declaration)?;
            let clone = self.context.factory()?.clone_node(name)?;
            self.set_comment_range_from(clone, name)?;
            self.context.hoist_variable_declaration(clone)?;
        }
        let variables = self.initialized_variables_of_id(list)?;
        let num_variables = variables.len();
        let mut variables_written = 0;
        let mut pending_expressions: Vec<TransformNode> = Vec::new();
        while variables_written < num_variables {
            for variable in variables
                .iter()
                .skip(variables_written)
                .take(num_variables - variables_written)
            {
                let initializer = self.declaration_initializer(*variable)?;
                if self.contains_yield(initializer) && !pending_expressions.is_empty() {
                    break;
                }
                pending_expressions.push(self.transform_initialized_variable(*variable)?);
            }
            if !pending_expressions.is_empty() {
                let count = pending_expressions.len();
                let inlined = self.inline_expressions(std::mem::take(&mut pending_expressions))?;
                let statement = self.create_expression_statement(inlined)?;
                self.emit_statement(statement)?;
                variables_written += count;
            }
        }
        Ok(())
    }

    /// tsc-port: transformInitializedVariable @6.0.3
    /// tsc-hash: 8f41ee5a89a8c5a5f518518266db37752edb2ea60fa14b87deb56e9fac029b3e
    /// tsc-span: _tsc.js:108836-108844
    fn transform_initialized_variable(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.declaration_name(node)?;
        let initializer =
            self.declaration_initializer(node)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclaration,
                    field: "initializer",
                })?;
        let name_clone = self.context.factory()?.clone_node(name)?;
        self.set_source_map_range_from(name_clone, name)?;
        let visited = self.visit_required_expression(initializer)?;
        let assignment = self.create_assignment(name_clone, visited)?;
        self.set_source_map_range_from(assignment, node)?;
        Ok(assignment)
    }

    /// tsc-port: transformAndEmitIfStatement @6.0.3
    /// tsc-hash: 500b8cc6c6b336e94155a5f3f94294a351cbc31c121d8ca9f35bec60c0d2dd76
    /// tsc-span: _tsc.js:108845-108869
    fn transform_and_emit_if_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::IfStatement(data) = self.context.arena().node(node)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::IfStatement,
                    field: "if statement data",
                });
            };
            let then_statement = data.then_statement.map(|id| self.node(id));
            let else_statement = data.else_statement.map(|id| self.node(id));
            if self.contains_yield(then_statement) || self.contains_yield(else_statement) {
                let end_label = self.define_label();
                let else_label = if else_statement.is_some() {
                    Some(self.define_label())
                } else {
                    None
                };
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::IfStatement,
                        field: "expression",
                    })?;
                let condition = self.visit_required_expression(self.node(expression))?;
                self.emit_break_when_false(
                    else_label.unwrap_or(end_label),
                    condition,
                    Some(self.node(expression)),
                )?;
                let then_statement =
                    then_statement.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::IfStatement,
                        field: "thenStatement",
                    })?;
                self.transform_and_emit_embedded_statement(then_statement)?;
                if let Some(else_statement) = else_statement {
                    self.emit_break(end_label, None)?;
                    self.mark_label(else_label.expect("else label allocated above"))?;
                    self.transform_and_emit_embedded_statement(else_statement)?;
                }
                self.mark_label(end_label)?;
            } else {
                let visited = self.visit_statement_node(node)?;
                self.emit_statement_opt(visited)?;
            }
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: transformAndEmitDoStatement @6.0.3
    /// tsc-hash: faad717ba42b42cd664ed72d71b49931ee8310f711a2c55522e4ccd7564a116c
    /// tsc-span: _tsc.js:108870-108886
    fn transform_and_emit_do_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::DoStatement(data) = self.context.arena().node(node)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::DoStatement,
                    field: "do statement data",
                });
            };
            let condition_label = self.define_label();
            let loop_label = self.define_label();
            self.begin_loop_block(condition_label);
            self.mark_label(loop_label)?;
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::DoStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.mark_label(condition_label)?;
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::DoStatement,
                    field: "expression",
                })?;
            let condition = self.visit_required_expression(self.node(expression))?;
            self.emit_break_when_true(loop_label, condition, None)?;
            self.end_loop_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitDoStatement @6.0.3
    /// tsc-hash: 1e34e04e121b22424121927ab9f109afb9b73292547f43f32e8a0d3e40f5e41d
    /// tsc-span: _tsc.js:108887-108896
    fn visit_do_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            self.begin_script_loop_block();
            let visited = self.visit_each_child(id)?;
            self.end_loop_block()?;
            Ok(visited)
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: transformAndEmitWhileStatement @6.0.3
    /// tsc-hash: 2ff85356121b544b13e6a615b543609df19aff5380bfc9d6772548848434764c
    /// tsc-span: _tsc.js:108897-108909
    fn transform_and_emit_while_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::WhileStatement(data) = self.context.arena().node(node)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::WhileStatement,
                    field: "while statement data",
                });
            };
            let loop_label = self.define_label();
            let end_label = self.begin_loop_block(loop_label);
            self.mark_label(loop_label)?;
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::WhileStatement,
                    field: "expression",
                })?;
            let condition = self.visit_required_expression(self.node(expression))?;
            self.emit_break_when_false(end_label, condition, None)?;
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::WhileStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.emit_break(loop_label, None)?;
            self.end_loop_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitWhileStatement @6.0.3
    /// tsc-hash: 55f1ec91156d3be0ab32fcd25eae5b5bac82b2a38844784eb8cf39883b879f4b
    /// tsc-span: _tsc.js:108910-108919
    fn visit_while_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            self.begin_script_loop_block();
            let visited = self.visit_each_child(id)?;
            self.end_loop_block()?;
            Ok(visited)
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: transformAndEmitForStatement @6.0.3
    /// tsc-hash: e26d4be3d37b50edc9dcff728ed4108dff5bdafe2a8578bd4a2f61f008b6610e
    /// tsc-span: _tsc.js:108920-108961
    fn transform_and_emit_for_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::ForStatement(data) = self.context.arena().node(node)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForStatement,
                    field: "for statement data",
                });
            };
            let condition_label = self.define_label();
            let increment_label = self.define_label();
            let end_label = self.begin_loop_block(increment_label);
            if let Some(initializer) = data.initializer {
                if self.kind(initializer)? == SyntaxKind::VariableDeclarationList {
                    self.transform_and_emit_variable_declaration_list(initializer)?;
                } else {
                    let initializer_node = self.node(initializer);
                    let visited = self.visit_required_expression(initializer_node)?;
                    let statement = self.create_expression_statement(visited)?;
                    let statement = self
                        .context
                        .factory()?
                        .set_text_range(statement, initializer_node)?;
                    self.emit_statement(statement)?;
                }
            }
            self.mark_label(condition_label)?;
            if let Some(condition) = data.condition {
                let visited = self.visit_required_expression(self.node(condition))?;
                self.emit_break_when_false(end_label, visited, None)?;
            }
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.mark_label(increment_label)?;
            if let Some(incrementor) = data.incrementor {
                let incrementor_node = self.node(incrementor);
                let visited = self.visit_required_expression(incrementor_node)?;
                let statement = self.create_expression_statement(visited)?;
                let statement = self
                    .context
                    .factory()?
                    .set_text_range(statement, incrementor_node)?;
                self.emit_statement(statement)?;
            }
            self.emit_break(condition_label, None)?;
            self.end_loop_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitForStatement @6.0.3
    /// tsc-hash: f4d66caca57c2ffe08fa156e288cc79302a4638826425eba583127a3338ac90e
    /// tsc-span: _tsc.js:108962-108986
    fn visit_for_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            self.begin_script_loop_block();
        }
        let NodeData::ForStatement(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForStatement,
                field: "for statement data",
            });
        };
        let result = if let Some(initializer) = data.initializer.filter(|initializer| {
            self.kind(*initializer)
                .map(|kind| kind == SyntaxKind::VariableDeclarationList)
                .unwrap_or(false)
        }) {
            for declaration in self.declarations_of_id(initializer)? {
                let name = self.declaration_name(declaration)?;
                self.context.hoist_variable_declaration(name)?;
            }
            let variables = self.initialized_variables_of_id(initializer)?;
            let new_initializer = if variables.is_empty() {
                None
            } else {
                let mut expressions = Vec::with_capacity(variables.len());
                for variable in variables {
                    expressions.push(self.transform_initialized_variable(variable)?);
                }
                Some(self.inline_expressions(expressions)?)
            };
            let condition = data
                .condition
                .map(|condition| self.visit_expression_opt(self.node(condition)))
                .transpose()?
                .flatten();
            let incrementor = data
                .incrementor
                .map(|incrementor| self.visit_expression_opt(self.node(incrementor)))
                .transpose()?
                .flatten();
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForStatement,
                field: "statement",
            })?;
            let statement = self.visit_iteration_body(self.node(statement))?;
            let replaced = NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                statement: Some(statement.node()),
                initializer: new_initializer.map(|node| node.node()),
                condition: condition.map(TransformNode::node),
                incrementor: incrementor.map(TransformNode::node),
            });
            let original = self.node(id);
            let flags = flags_after_update(self.context.arena(), original, &replaced)?;
            let updated = self
                .context
                .factory()?
                .update_node(original, replaced, flags)?;
            Some(updated.node())
        } else {
            self.visit_each_child(id)?
        };
        if self.in_statement_containing_yield {
            self.end_loop_block()?;
        }
        Ok(result)
    }

    /// tsc-port: transformAndEmitForInStatement @6.0.3
    /// tsc-hash: ccd539ed18f0a978a441c4b1c43bf5068d0c6f2e94ff6a32002deee5823551dd
    /// tsc-span: _tsc.js:108987-109038
    fn transform_and_emit_for_in_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::ForInStatement(data) = self.context.arena().node(node)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForInStatement,
                    field: "for-in statement data",
                });
            };
            let obj = self.declare_local(None)?;
            let keys_array = self.declare_local(None)?;
            let key = self.declare_local(None)?;
            let keys_index = self.create_loop_variable()?;
            let initializer = data
                .initializer
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForInStatement,
                    field: "initializer",
                })?;
            self.context.hoist_variable_declaration(keys_index)?;
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForInStatement,
                    field: "expression",
                })?;
            let visited = self.visit_required_expression(self.node(expression))?;
            self.emit_assignment(obj, visited, None)?;
            let empty_array = self.create_array_literal(Vec::new())?;
            self.emit_assignment(keys_array, empty_array, None)?;
            let push_name = self.create_identifier("push")?;
            let push_access = self.create_property_access(keys_array, push_name)?;
            let push_call = self.create_call(push_access, vec![key])?;
            let push_statement = self.create_expression_statement(push_call)?;
            let for_in = self.create_for_in(key, obj, push_statement)?;
            self.emit_statement(for_in)?;
            let zero = self.create_numeric_literal("0")?;
            self.emit_assignment(keys_index, zero, None)?;
            let condition_label = self.define_label();
            let increment_label = self.define_label();
            let end_loop_label = self.begin_loop_block(increment_label);
            self.mark_label(condition_label)?;
            let length_name = self.create_identifier("length")?;
            let length_access = self.create_property_access(keys_array, length_name)?;
            let less_than = self.create_less_than(keys_index, length_access)?;
            self.emit_break_when_false(end_loop_label, less_than, None)?;
            let element = self.create_element_access(keys_array, keys_index)?;
            self.emit_assignment(key, element, None)?;
            let in_check = self.create_binary(key, SyntaxKind::InKeyword, obj)?;
            self.emit_break_when_false(increment_label, in_check, None)?;
            let variable = if self.kind(initializer)? == SyntaxKind::VariableDeclarationList {
                for declaration in self.declarations_of_id(initializer)? {
                    let name = self.declaration_name(declaration)?;
                    self.context.hoist_variable_declaration(name)?;
                }
                let first = self
                    .declarations_of_id(initializer)?
                    .first()
                    .copied()
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::VariableDeclarationList,
                        field: "declarations",
                    })?;
                let name = self.declaration_name(first)?;
                self.context.factory()?.clone_node(name)?
            } else {
                // `Debug.assert(isLeftHandSideExpression(variable))` — the
                // typed equivalent is the assignment sink below.
                self.visit_required_expression(self.node(initializer))?
            };
            self.emit_assignment(variable, key, None)?;
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForInStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.mark_label(increment_label)?;
            let increment = self.create_postfix_increment(keys_index)?;
            let increment_statement = self.create_expression_statement(increment)?;
            self.emit_statement(increment_statement)?;
            self.emit_break(condition_label, None)?;
            self.end_loop_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitForInStatement @6.0.3
    /// tsc-hash: 1479db3ac09b3fab28933d5d5fd8c11db834e9dc0136ca405bed91fa894e0efe
    /// tsc-span: _tsc.js:109039-109056
    fn visit_for_in_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            self.begin_script_loop_block();
        }
        let NodeData::ForInStatement(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForInStatement,
                field: "for-in statement data",
            });
        };
        let initializer = data
            .initializer
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForInStatement,
                field: "initializer",
            })?;
        let result = if self.kind(initializer)? == SyntaxKind::VariableDeclarationList {
            for declaration in self.declarations_of_id(initializer)? {
                let name = self.declaration_name(declaration)?;
                self.context.hoist_variable_declaration(name)?;
            }
            let first = self
                .declarations_of_id(initializer)?
                .first()
                .copied()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclarationList,
                    field: "declarations",
                })?;
            let name = self.declaration_name(first)?;
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForInStatement,
                    field: "expression",
                })?;
            let visited_expression = self.visit_required_expression(self.node(expression))?;
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForInStatement,
                field: "statement",
            })?;
            let visited_statement = self.visit_statement_lifted(self.node(statement))?;
            let replaced = NodeData::ForInStatement(tsc_syntax::nodes::ForInStatementData {
                statement: Some(visited_statement.node()),
                initializer: Some(name.node()),
                expression: Some(visited_expression.node()),
            });
            let original = self.node(id);
            let flags = flags_after_update(self.context.arena(), original, &replaced)?;
            let updated = self
                .context
                .factory()?
                .update_node(original, replaced, flags)?;
            Some(updated.node())
        } else {
            self.visit_each_child(id)?
        };
        if self.in_statement_containing_yield {
            self.end_loop_block()?;
        }
        Ok(result)
    }

    /// tsc-port: transformAndEmitContinueStatement @6.0.3
    /// tsc-hash: a37f3803e0ae7fae191f4a9dd6230e0368b370eac895ac0fb4a7357450549830
    /// tsc-span: _tsc.js:109057-109068
    fn transform_and_emit_continue_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let label_text = self.statement_label_text(node)?;
        let label = self.find_continue_target(label_text.as_deref());
        if label > 0 {
            self.emit_break(label, Some(node))?;
        } else {
            // Invalid-per-grammar input (a continue with no target survives
            // parse recovery only); emitted faithfully.
            self.emit_statement(node)?;
        }
        Ok(())
    }

    /// tsc-port: visitContinueStatement @6.0.3
    /// tsc-hash: 08ed3cdd08519b9fe7bcf61335d67c4d736d2e329f89198e73142291d382ceb9
    /// tsc-span: _tsc.js:109069-109081
    fn visit_continue_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            let label_text = self.statement_label_text(self.node(id))?;
            let label = self.find_continue_target(label_text.as_deref());
            if label > 0 {
                return self
                    .create_inline_break(label, Some(self.node(id)))
                    .map(|node| Some(node.node()));
            }
        }
        self.visit_each_child(id)
    }

    /// tsc-port: transformAndEmitBreakStatement @6.0.3
    /// tsc-hash: 848f1199536d426641932d064fe89be3442363d768c198ffcc563f1edc4b32d8
    /// tsc-span: _tsc.js:109082-109093
    fn transform_and_emit_break_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let label_text = self.statement_label_text(node)?;
        let label = self.find_break_target(label_text.as_deref());
        if label > 0 {
            self.emit_break(label, Some(node))?;
        } else {
            self.emit_statement(node)?;
        }
        Ok(())
    }

    /// tsc-port: visitBreakStatement @6.0.3
    /// tsc-hash: d2510c23505af99ff9401d8a429d0aed0c4570f7fab823f5e743a668f102bd31
    /// tsc-span: _tsc.js:109094-109106
    fn visit_break_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            let label_text = self.statement_label_text(self.node(id))?;
            let label = self.find_break_target(label_text.as_deref());
            if label > 0 {
                return self
                    .create_inline_break(label, Some(self.node(id)))
                    .map(|node| Some(node.node()));
            }
        }
        self.visit_each_child(id)
    }

    /// tsc-port: transformAndEmitReturnStatement @6.0.3
    /// tsc-hash: fc7ad6b2114c68f0a963399e2bbf4f7f8d8a762049675cd96e0f9be1df88d586
    /// tsc-span: _tsc.js:109107-109113
    fn transform_and_emit_return_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let expression = self.return_expression(node)?;
        let visited = expression
            .map(|expression| self.visit_expression_opt(expression))
            .transpose()?
            .flatten();
        self.emit_return(visited, Some(node))
    }

    /// tsc-port: visitReturnStatement @6.0.3
    /// tsc-hash: 384a635a720c3012aa363d354920f9cb7595555e152520f5d4a0e28442392291
    /// tsc-span: _tsc.js:109114-109120
    fn visit_return_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let expression = self.return_expression(node)?;
        let visited = expression
            .map(|expression| self.visit_expression_opt(expression))
            .transpose()?
            .flatten();
        self.create_inline_return(visited, Some(node))
            .map(|node| Some(node.node()))
    }

    /// tsc-port: transformAndEmitWithStatement @6.0.3
    /// tsc-hash: 9e589d24c92ff9f629eb94f3d070fb3f09f2bb9f95479281944a672e4965bc84
    /// tsc-span: _tsc.js:109121-109129
    fn transform_and_emit_with_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::WithStatement(data) = self.context.arena().node(node)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::WithStatement,
                    field: "with statement data",
                });
            };
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::WithStatement,
                    field: "expression",
                })?;
            let visited = self.visit_required_expression(self.node(expression))?;
            let cached = self.cache_expression(visited)?;
            self.begin_with_block(cached)?;
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::WithStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.end_with_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: transformAndEmitSwitchStatement @6.0.3
    /// tsc-hash: bb8675f09bac7919d9a7793188a3b212b16916e8222f7222861b0004f034b7ee
    /// tsc-span: _tsc.js:109130-109194
    fn transform_and_emit_switch_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let NodeData::SwitchStatement(data) = self.context.arena().node(node)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SwitchStatement,
                field: "switch statement data",
            });
        };
        let case_block = data
            .case_block
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SwitchStatement,
                field: "caseBlock",
            })?;
        if self.contains_yield(Some(self.node(case_block))) {
            let clauses = self.case_block_clauses(case_block)?;
            let num_clauses = clauses.len();
            let end_label = self.begin_switch_block();
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SwitchStatement,
                    field: "expression",
                })?;
            let visited = self.visit_required_expression(self.node(expression))?;
            let expression = self.cache_expression(visited)?;
            let mut clause_labels = Vec::with_capacity(num_clauses);
            let mut default_clause_index: Option<usize> = None;
            for (index, clause) in clauses.iter().enumerate() {
                clause_labels.push(self.define_label());
                if self.kind_of(*clause)? == SyntaxKind::DefaultClause
                    && default_clause_index.is_none()
                {
                    default_clause_index = Some(index);
                }
            }
            let mut clauses_written = 0;
            let mut pending_clauses: Vec<TransformNode> = Vec::new();
            while clauses_written < num_clauses {
                let mut default_clauses_skipped = 0;
                for i in clauses_written..num_clauses {
                    let clause = clauses[i];
                    if self.kind_of(clause)? == SyntaxKind::CaseClause {
                        let clause_expression = self.case_clause_expression(clause)?;
                        if self.contains_yield(Some(clause_expression))
                            && !pending_clauses.is_empty()
                        {
                            break;
                        }
                        let visited = self.visit_required_expression(clause_expression)?;
                        let inline_break =
                            self.create_inline_break(clause_labels[i], Some(clause_expression))?;
                        let case_clause = self.create_case_clause(visited, vec![inline_break])?;
                        pending_clauses.push(case_clause);
                    } else {
                        default_clauses_skipped += 1;
                    }
                }
                if !pending_clauses.is_empty() {
                    let count = pending_clauses.len();
                    let case_block_node =
                        self.create_case_block(std::mem::take(&mut pending_clauses))?;
                    let switch_statement =
                        self.create_switch_statement(expression, case_block_node)?;
                    self.emit_statement(switch_statement)?;
                    clauses_written += count;
                }
                if default_clauses_skipped > 0 {
                    clauses_written += default_clauses_skipped;
                }
            }
            if let Some(default_clause_index) = default_clause_index {
                self.emit_break(clause_labels[default_clause_index], None)?;
            } else {
                self.emit_break(end_label, None)?;
            }
            for (index, clause) in clauses.iter().enumerate() {
                self.mark_label(clause_labels[index])?;
                let statements = self.clause_statements(*clause)?;
                self.transform_and_emit_statements(&statements, 0)?;
            }
            self.end_switch_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitSwitchStatement @6.0.3
    /// tsc-hash: 401fa818af05e2c447fe19cadef81439924f359d29df80c633965abda4fa5602
    /// tsc-span: _tsc.js:109195-109204
    fn visit_switch_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            self.begin_script_switch_block();
        }
        let visited = self.visit_each_child(id)?;
        if self.in_statement_containing_yield {
            self.end_switch_block()?;
        }
        Ok(visited)
    }

    /// tsc-port: transformAndEmitLabeledStatement @6.0.3
    /// tsc-hash: ecea8dc0da489771425915e05b371b2699aa4be555d2aa3ed09cc7fbdac7b7b9
    /// tsc-span: _tsc.js:109205-109213
    fn transform_and_emit_labeled_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::LabeledStatement(data) = self.context.arena().node(node)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::LabeledStatement,
                    field: "labeled statement data",
                });
            };
            let label = data.label.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::LabeledStatement,
                field: "label",
            })?;
            let label_text = self.identifier_text(self.node(label))?;
            self.begin_labeled_block(label_text);
            let statement = data.statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::LabeledStatement,
                field: "statement",
            })?;
            self.transform_and_emit_embedded_statement(self.node(statement))?;
            self.end_labeled_block()?;
            Ok(())
        } else {
            let visited = self.visit_statement_node(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: visitLabeledStatement @6.0.3
    /// tsc-hash: c41c046ab364ef445626c9b8c54bb9e79fd0a71bc493db66a9ebef5633039369
    /// tsc-span: _tsc.js:109214-109223
    fn visit_labeled_statement(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if self.in_statement_containing_yield {
            let NodeData::LabeledStatement(data) = self.data(id)? else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::LabeledStatement,
                    field: "labeled statement data",
                });
            };
            let label = data.label.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::LabeledStatement,
                field: "label",
            })?;
            let label_text = self.identifier_text(self.node(label))?;
            self.begin_script_labeled_block(label_text);
        }
        let visited = self.visit_each_child(id)?;
        if self.in_statement_containing_yield {
            self.end_labeled_block()?;
        }
        Ok(visited)
    }

    /// tsc-port: transformAndEmitThrowStatement @6.0.3
    /// tsc-hash: e0892ff3c04ca7bc06b6ea744d40cf831d84b1ccf68d9d6e7ddd4894a46f5a76
    /// tsc-span: _tsc.js:109224-109230
    fn transform_and_emit_throw_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let NodeData::ThrowStatement(data) = self.context.arena().node(node)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ThrowStatement,
                field: "throw statement data",
            });
        };
        let expression = match data.expression {
            Some(expression) => self.visit_required_expression(self.node(expression))?,
            // `node.expression ?? factory2.createVoidZero()`
            None => self.create_void_zero()?,
        };
        self.emit_throw(expression, Some(node))
    }

    /// tsc-port: transformAndEmitTryStatement @6.0.3
    /// tsc-hash: d99e7fe5df70434d2ef8aff32395698a9b0625efa8f3457b5a08b377b6798d33
    /// tsc-span: _tsc.js:109231-109247
    fn transform_and_emit_try_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if self.contains_yield(Some(node)) {
            let NodeData::TryStatement(data) = self.context.arena().node(node)?.data.clone() else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "try statement data",
                });
            };
            self.begin_exception_block()?;
            let try_block = data.try_block.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "tryBlock",
            })?;
            self.transform_and_emit_embedded_statement(self.node(try_block))?;
            if let Some(catch_clause) = data.catch_clause {
                let NodeData::CatchClause(catch_data) = self.data(catch_clause)? else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CatchClause,
                        field: "catch clause data",
                    });
                };
                // ES2019 optional catch binding is lowered before this pass;
                // an absent declaration is the upstream undefined-deref.
                let variable = catch_data.variable_declaration.ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CatchClause,
                        field: "variableDeclaration",
                    },
                )?;
                self.begin_catch_block(self.node(variable))?;
                let block = catch_data
                    .block
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CatchClause,
                        field: "block",
                    })?;
                self.transform_and_emit_embedded_statement(self.node(block))?;
            }
            if let Some(finally_block) = data.finally_block {
                self.begin_finally_block()?;
                self.transform_and_emit_embedded_statement(self.node(finally_block))?;
            }
            self.end_exception_block()?;
            Ok(())
        } else {
            let visited = self.visit_each_child_statement(node)?;
            self.emit_statement_opt(visited)
        }
    }

    /// tsc-port: containsYield @6.0.3
    /// tsc-hash: bfd65ca7e124e65266ddc461c31883f9c271350dc67e9176a6ae91ba980b4ab3
    /// tsc-span: _tsc.js:109248-109250
    fn contains_yield(&self, node: Option<TransformNode>) -> bool {
        node.is_some_and(|node| {
            self.context
                .arena()
                .transform_flags(node)
                .contains(TransformFlags::CONTAINS_YIELD)
        })
    }

    /// tsc-port: countInitialNodesWithoutYield @6.0.3
    /// tsc-hash: 37a6f6f3e7e7d5aea92d146079cdd358976c3cf0cdd6913dd8a64b5fec07d35f
    /// tsc-span: _tsc.js:109251-109259
    ///
    /// Upstream returns -1 when no element contains yield; the sole callers
    /// guard with `> 0` / feed `reduceLeft` whose negative start clamps to
    /// 0 — the Rust port returns `None` for that arm.
    fn count_initial_nodes_without_yield(&self, nodes: &[TransformNode]) -> Option<usize> {
        nodes
            .iter()
            .position(|node| self.contains_yield(Some(*node)))
    }
}

// ---------------------------------------------------------------------------
// Expressions containing yield
// ---------------------------------------------------------------------------

impl GeneratorsVisitor<'_, '_> {
    /// tsc-port: visitBinaryExpression @6.0.3
    /// tsc-hash: c07fedbd5eac4aac0a1e65b84df6ded6a84c33f9938babb756fbc3ff48e8ab74
    /// tsc-span: _tsc.js:108423-108433
    fn visit_binary_expression(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        match self.expression_associativity(id)? {
            Associativity::Left => self.visit_left_associative_binary_expression(id),
            Associativity::Right => self.visit_right_associative_binary_expression(id),
        }
    }

    /// tsc-port: visitRightAssociativeBinaryExpression @6.0.3
    /// tsc-hash: a6fe47778d107b985e7b45adfe168f1fa53d16edab04ec23949c33a74fd1d10e
    /// tsc-span: _tsc.js:108434-108474
    fn visit_right_associative_binary_expression(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::BinaryExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "binary expression data",
            });
        };
        let left = data.left.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "left",
        })?;
        let right = data.right.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "right",
        })?;
        if self.contains_yield(Some(self.node(right))) {
            let target = match self.kind(left)? {
                SyntaxKind::PropertyAccessExpression => {
                    let NodeData::PropertyAccessExpression(left_data) = self.data(left)? else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyAccessExpression,
                            field: "property access data",
                        });
                    };
                    let receiver =
                        left_data
                            .expression
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::PropertyAccessExpression,
                                field: "expression",
                            })?;
                    let visited = self.visit_required_expression(self.node(receiver))?;
                    let cached = self.cache_expression(visited)?;
                    let replaced = NodeData::PropertyAccessExpression(
                        tsc_syntax::nodes::PropertyAccessExpressionData {
                            expression: Some(cached.node()),
                            question_dot_token: left_data.question_dot_token,
                            name: left_data.name,
                        },
                    );
                    let original = self.node(left);
                    let flags = flags_after_update(self.context.arena(), original, &replaced)?;
                    self.context
                        .factory()?
                        .update_node(original, replaced, flags)?
                }
                SyntaxKind::ElementAccessExpression => {
                    let NodeData::ElementAccessExpression(left_data) = self.data(left)? else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ElementAccessExpression,
                            field: "element access data",
                        });
                    };
                    let receiver =
                        left_data
                            .expression
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ElementAccessExpression,
                                field: "expression",
                            })?;
                    let argument = left_data.argument_expression.ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ElementAccessExpression,
                            field: "argumentExpression",
                        },
                    )?;
                    let visited_receiver = self.visit_required_expression(self.node(receiver))?;
                    let cached_receiver = self.cache_expression(visited_receiver)?;
                    let visited_argument = self.visit_required_expression(self.node(argument))?;
                    let cached_argument = self.cache_expression(visited_argument)?;
                    let replaced = NodeData::ElementAccessExpression(
                        tsc_syntax::nodes::ElementAccessExpressionData {
                            expression: Some(cached_receiver.node()),
                            question_dot_token: left_data.question_dot_token,
                            argument_expression: Some(cached_argument.node()),
                        },
                    );
                    let original = self.node(left);
                    let flags = flags_after_update(self.context.arena(), original, &replaced)?;
                    self.context
                        .factory()?
                        .update_node(original, replaced, flags)?
                }
                _ => self.visit_required_expression(self.node(left))?,
            };
            let operator = self.binary_operator_kind(id)?;
            if is_compound_assignment(operator) {
                let cached_target = self.cache_expression(target)?;
                let visited_right = self.visit_required_expression(self.node(right))?;
                let inner = self.create_binary(
                    cached_target,
                    non_assignment_operator_for_compound_assignment(operator),
                    visited_right,
                )?;
                let inner = self.context.factory()?.set_text_range(inner, node)?;
                let assignment = self.create_assignment(target, inner)?;
                let assignment = self.context.factory()?.set_text_range(assignment, node)?;
                Ok(Some(assignment.node()))
            } else {
                let visited_right = self.visit_required_expression(self.node(right))?;
                let replaced =
                    NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                        left: Some(target.node()),
                        operator_token: data.operator_token,
                        right: Some(visited_right.node()),
                    });
                let flags = flags_after_update(self.context.arena(), node, &replaced)?;
                let updated = self.context.factory()?.update_node(node, replaced, flags)?;
                Ok(Some(updated.node()))
            }
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: visitLeftAssociativeBinaryExpression @6.0.3
    /// tsc-hash: 739d608f180f4418856ec640d69f0d7b02483307789afb0be8a0708c51345dc5
    /// tsc-span: _tsc.js:108475-108485
    fn visit_left_associative_binary_expression(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::BinaryExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "binary expression data",
            });
        };
        let right = data.right.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "right",
        })?;
        if self.contains_yield(Some(self.node(right))) {
            let operator = self.binary_operator_kind(id)?;
            if is_logical_operator(operator) {
                return self
                    .visit_logical_binary_expression(id)
                    .map(|node| Some(node.node()));
            } else if operator == SyntaxKind::CommaToken {
                return self
                    .visit_comma_expression(id)
                    .map(|node| Some(node.node()));
            }
            let left = data.left.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "left",
            })?;
            let visited_left = self.visit_required_expression(self.node(left))?;
            let cached_left = self.cache_expression(visited_left)?;
            let visited_right = self.visit_required_expression(self.node(right))?;
            let replaced = NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(cached_left.node()),
                operator_token: data.operator_token,
                right: Some(visited_right.node()),
            });
            let flags = flags_after_update(self.context.arena(), node, &replaced)?;
            let updated = self.context.factory()?.update_node(node, replaced, flags)?;
            Ok(Some(updated.node()))
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: visitCommaExpression @6.0.3
    /// tsc-hash: 350c6a0c857f3227c98cdc140c7a34e7e66ddd9cdf6c9b98300f9c9c65da52b5
    /// tsc-span: _tsc.js:108486-108503
    fn visit_comma_expression(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let mut pending_expressions: Vec<TransformNode> = Vec::new();
        let NodeData::BinaryExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "comma expression data",
            });
        };
        let left = data.left.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "left",
        })?;
        let right = data.right.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "right",
        })?;
        self.visit_comma_operand(left, &mut pending_expressions)?;
        self.visit_comma_operand(right, &mut pending_expressions)?;
        self.inline_expressions(pending_expressions)
    }

    /// The inner `visit` closure of `visitCommaExpression`.
    /// tsc-port: visit @6.0.3
    /// tsc-hash: 39e7520089a7dfe7989ff77cd85034462b81fbee2115d2200eda7f55e3bb6976
    /// tsc-span: _tsc.js:108491-108502
    fn visit_comma_operand(
        &mut self,
        id: NodeId,
        pending_expressions: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let is_comma_binary = matches!(
            &self.context.arena().node(self.node(id))?.data,
            NodeData::BinaryExpression(_)
        ) && self.binary_operator_kind(id)? == SyntaxKind::CommaToken;
        if is_comma_binary {
            let NodeData::BinaryExpression(data) = self.data(id)? else {
                unreachable!("comma binary tested above");
            };
            let left = data.left.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "left",
            })?;
            let right = data.right.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "right",
            })?;
            self.visit_comma_operand(left, pending_expressions)?;
            self.visit_comma_operand(right, pending_expressions)?;
        } else {
            if self.contains_yield(Some(self.node(id))) && !pending_expressions.is_empty() {
                let inlined = self.inline_expressions(std::mem::take(pending_expressions))?;
                let statement = self.create_expression_statement(inlined)?;
                self.emit_worker(OpCode::Statement, vec![statement], Vec::new(), None);
            }
            let visited = self.visit_required_expression(self.node(id))?;
            pending_expressions.push(visited);
        }
        Ok(())
    }

    /// tsc-port: visitCommaListExpression @6.0.3
    /// tsc-hash: 0ba115d81a4a89f86fd5f0380efb42437af378330fbd3b4dd7880c104d5eec38
    /// tsc-span: _tsc.js:108504-108518
    ///
    /// CommaListExpression is synthesized-only (no parse production); the
    /// arm is reachable once B-4/B-5 owners synthesize comma lists that
    /// flow through the machine — ported faithfully, unit-driven.
    fn visit_comma_list_expression(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let NodeData::CommaListExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CommaListExpression,
                field: "comma list data",
            });
        };
        let elements = self.array_nodes_of(data.elements)?;
        let mut pending_expressions: Vec<TransformNode> = Vec::new();
        for element in elements {
            let is_comma_binary = matches!(
                &self.context.arena().node(element)?.data,
                NodeData::BinaryExpression(_)
            ) && self.binary_operator_kind(element.node())?
                == SyntaxKind::CommaToken;
            if is_comma_binary {
                let visited = self.visit_comma_expression(element.node())?;
                pending_expressions.push(visited);
            } else {
                if self.contains_yield(Some(element)) && !pending_expressions.is_empty() {
                    let inlined =
                        self.inline_expressions(std::mem::take(&mut pending_expressions))?;
                    let statement = self.create_expression_statement(inlined)?;
                    self.emit_worker(OpCode::Statement, vec![statement], Vec::new(), None);
                }
                let visited = self.visit_required_expression(element)?;
                pending_expressions.push(visited);
            }
        }
        self.inline_expressions(pending_expressions)
            .map(|node| Some(node.node()))
    }

    /// tsc-port: visitLogicalBinaryExpression @6.0.3
    /// tsc-hash: 25f76339ccad88ae3361cc39c35f639e208a24e491a9ca16bbb70dafa2f6583f
    /// tsc-span: _tsc.js:108519-108551
    fn visit_logical_binary_expression(
        &mut self,
        id: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::BinaryExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "logical binary data",
            });
        };
        let left = data.left.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "left",
        })?;
        let right = data.right.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "right",
        })?;
        let result_label = self.define_label();
        let result_local = self.declare_local(None)?;
        let visited_left = self.visit_required_expression(self.node(left))?;
        self.emit_assignment(result_local, visited_left, Some(self.node(left)))?;
        if self.binary_operator_kind(id)? == SyntaxKind::AmpersandAmpersandToken {
            self.emit_break_when_false(result_label, result_local, Some(self.node(left)))?;
        } else {
            self.emit_break_when_true(result_label, result_local, Some(self.node(left)))?;
        }
        let visited_right = self.visit_required_expression(self.node(right))?;
        self.emit_assignment(result_local, visited_right, Some(self.node(right)))?;
        self.mark_label(result_label)?;
        Ok(result_local)
    }

    /// tsc-port: visitConditionalExpression @6.0.3
    /// tsc-hash: c305eb643e4f7e0d98ab80cbb0760bae2ebde1fa1b884f48630eac21821d26ff
    /// tsc-span: _tsc.js:108552-108581
    fn visit_conditional_expression(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let NodeData::ConditionalExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ConditionalExpression,
                field: "conditional data",
            });
        };
        let when_true = data.when_true.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ConditionalExpression,
            field: "whenTrue",
        })?;
        let when_false = data
            .when_false
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ConditionalExpression,
                field: "whenFalse",
            })?;
        if self.contains_yield(Some(self.node(when_true)))
            || self.contains_yield(Some(self.node(when_false)))
        {
            let when_false_label = self.define_label();
            let result_label = self.define_label();
            let result_local = self.declare_local(None)?;
            let condition = data.condition.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ConditionalExpression,
                field: "condition",
            })?;
            let visited_condition = self.visit_required_expression(self.node(condition))?;
            self.emit_break_when_false(
                when_false_label,
                visited_condition,
                Some(self.node(condition)),
            )?;
            let visited_true = self.visit_required_expression(self.node(when_true))?;
            self.emit_assignment(result_local, visited_true, Some(self.node(when_true)))?;
            self.emit_break(result_label, None)?;
            self.mark_label(when_false_label)?;
            let visited_false = self.visit_required_expression(self.node(when_false))?;
            self.emit_assignment(result_local, visited_false, Some(self.node(when_false)))?;
            self.mark_label(result_label)?;
            Ok(Some(result_local.node()))
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: visitYieldExpression @6.0.3
    /// tsc-hash: 7f650448505e98a8735389dd31382932f8518c3c3d6a75f68d0e399a5b4ada46
    /// tsc-span: _tsc.js:108582-108604
    fn visit_yield_expression(&mut self, id: NodeId) -> Result<NodeId, TransformError> {
        let node = self.node(id);
        let NodeData::YieldExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::YieldExpression,
                field: "yield data",
            });
        };
        let resume_label = self.define_label();
        let expression = data
            .expression
            .map(|expression| self.visit_expression_opt(self.node(expression)))
            .transpose()?
            .flatten();
        if data.asterisk_token.is_some() {
            let expression = expression.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::YieldExpression,
                field: "expression",
            })?;
            // `(getEmitFlags(node.expression) & EmitFlags.Iterator) === 0`
            // — the yield-star-synthesis consumer edge: B-4's loop
            // conversion stamps Iterator on its synthesized iterator and
            // the machine forwards it unwrapped.
            let original_expression = data.expression.expect("asterisk yields carry expressions");
            let iterator = if self
                .emit_flags(self.node(original_expression))
                .contains(EmitFlags::ITERATOR)
            {
                expression
            } else {
                let values_call = self.create_values_helper_call(expression)?;
                self.context.factory()?.set_text_range(values_call, node)?
            };
            self.emit_yield_star(iterator, Some(node))?;
        } else {
            self.emit_yield(expression, Some(node))?;
        }
        self.mark_label(resume_label)?;
        let resume = self.create_generator_resume(Some(node))?;
        Ok(resume.node())
    }

    /// tsc-port: visitArrayLiteralExpression @6.0.3
    /// tsc-hash: 373615dbd7adce68e4d93ed3d8527fb73eb668fbdbcac638b56c6520c918adbc
    /// tsc-span: _tsc.js:108605-108614
    fn visit_array_literal_expression(&mut self, id: NodeId) -> Result<NodeId, TransformError> {
        let NodeData::ArrayLiteralExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ArrayLiteralExpression,
                field: "array literal data",
            });
        };
        let elements = self.array_nodes_of(data.elements)?;
        let multi_line = self.node_is_multi_line(self.node(id))?;
        self.visit_elements(&elements, None, None, multi_line)
            .map(|node| node.node())
    }

    /// tsc-port: visitElements @6.0.3
    /// tsc-hash: 27c5033bced1782855dddb2f3df36908ae590e07619b65375f521837df5aae69
    /// tsc-span: _tsc.js:108615-108656
    fn visit_elements(
        &mut self,
        elements: &[TransformNode],
        mut leading_element: Option<TransformNode>,
        location: Option<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let num_initial_elements = self.count_initial_nodes_without_yield(elements);
        let mut temp: Option<TransformNode> = None;
        if let Some(count) = num_initial_elements.filter(|count| *count > 0) {
            let local = self.declare_local(None)?;
            temp = Some(local);
            let mut initial_elements = Vec::with_capacity(count + 1);
            if let Some(leading) = leading_element {
                initial_elements.push(leading);
            }
            for element in elements.iter().take(count) {
                initial_elements.push(self.visit_required_expression(*element)?);
            }
            let array = self.create_array_literal(initial_elements)?;
            self.emit_assignment(local, array, None)?;
            leading_element = None;
        }
        // `reduceLeft(elements, reduceElement, [], numInitialElements)` —
        // a negative start clamps to 0.
        let start = num_initial_elements.unwrap_or(0);
        let mut expressions: Vec<TransformNode> = Vec::new();
        for element in elements.iter().skip(start) {
            // tsc-port: reduceElement @6.0.3
            // tsc-hash: 141664afe0e73989abfeed5ca406229b61f93956e08969989911c65215827fbd
            // tsc-span: _tsc.js:108634-108655
            if self.contains_yield(Some(*element)) && !expressions.is_empty() {
                let has_assigned_temp = temp.is_some();
                let local = match temp {
                    Some(local) => local,
                    None => {
                        let local = self.declare_local(None)?;
                        temp = Some(local);
                        local
                    }
                };
                let chunk = std::mem::take(&mut expressions);
                let assigned = if has_assigned_temp {
                    let literal = self.create_array_literal_multi_line(chunk, multi_line)?;
                    self.create_array_concat_call(local, vec![literal])?
                } else {
                    let mut with_leading = Vec::new();
                    if let Some(leading) = leading_element {
                        with_leading.push(leading);
                    }
                    with_leading.extend(chunk);
                    self.create_array_literal_multi_line(with_leading, multi_line)?
                };
                self.emit_assignment(local, assigned, None)?;
                leading_element = None;
            }
            expressions.push(self.visit_required_expression(*element)?);
        }
        match temp {
            Some(local) => {
                let literal = self.create_array_literal_multi_line(expressions, multi_line)?;
                self.create_array_concat_call(local, vec![literal])
            }
            None => {
                let mut with_leading = Vec::new();
                if let Some(leading) = leading_element {
                    with_leading.push(leading);
                }
                with_leading.extend(expressions);
                let literal = self.create_array_literal_multi_line(with_leading, multi_line)?;
                match location {
                    Some(location) => self.context.factory()?.set_text_range(literal, location),
                    None => Ok(literal),
                }
            }
        }
    }

    /// tsc-port: visitObjectLiteralExpression @6.0.3
    /// tsc-hash: c314e8e8c0217868cc4d8b5c2e1cd7d5033ea20b4939cf04a2f90d421255d1be
    /// tsc-span: _tsc.js:108657-108687
    fn visit_object_literal_expression(&mut self, id: NodeId) -> Result<NodeId, TransformError> {
        let node = self.node(id);
        let NodeData::ObjectLiteralExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ObjectLiteralExpression,
                field: "object literal data",
            });
        };
        let properties = self.array_nodes_of(data.properties)?;
        let multi_line = self.node_is_multi_line(node)?;
        let num_initial_properties = self.count_initial_nodes_without_yield(&properties);
        let temp = self.declare_local(None)?;
        let initial_count = num_initial_properties.unwrap_or(properties.len());
        let mut initial_properties = Vec::with_capacity(initial_count);
        for property in properties.iter().take(initial_count) {
            initial_properties.push(self.visit_object_literal_element(*property)?);
        }
        let initial_literal =
            self.create_object_literal_multi_line(initial_properties, multi_line)?;
        self.emit_assignment(temp, initial_literal, None)?;
        // tsc-port: reduceProperty @6.0.3
        // tsc-hash: e95a0bc9862f52d34e861168144c502309325213cb564e2f9a06ec0ad02e7f73
        // tsc-span: _tsc.js:108672-108686
        let mut expressions: Vec<TransformNode> = Vec::new();
        for property in properties.iter().skip(initial_count) {
            if self.contains_yield(Some(*property)) && !expressions.is_empty() {
                let chunk = std::mem::take(&mut expressions);
                let inlined = self.inline_expressions(chunk)?;
                let statement = self.create_expression_statement(inlined)?;
                self.emit_statement(statement)?;
            }
            let expression =
                self.create_expression_for_object_literal_element_like(node, *property, temp)?;
            if let Some(expression) = expression {
                let visited = self.visit_expression_opt(expression)?;
                if let Some(visited) = visited {
                    if multi_line {
                        self.start_on_new_line(visited)?;
                    }
                    expressions.push(visited);
                }
            }
        }
        let temp_clone = self.context.factory()?.clone_node(temp)?;
        let temp_clone = self.context.factory()?.set_text_range(temp_clone, temp)?;
        if multi_line {
            self.start_on_new_line(temp_clone)?;
        }
        expressions.push(temp_clone);
        self.inline_expressions(expressions).map(|node| node.node())
    }

    /// tsc-port: visitElementAccessExpression @6.0.3
    /// tsc-hash: 6b0e546d4bf1d61c58b3b64f90744a25b37179a049ec7eed301238251af73841
    /// tsc-span: _tsc.js:108688-108693
    fn visit_element_access_expression(
        &mut self,
        id: NodeId,
    ) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::ElementAccessExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ElementAccessExpression,
                field: "element access data",
            });
        };
        let argument = data
            .argument_expression
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ElementAccessExpression,
                field: "argumentExpression",
            })?;
        if self.contains_yield(Some(self.node(argument))) {
            let receiver = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ElementAccessExpression,
                    field: "expression",
                })?;
            let visited_receiver = self.visit_required_expression(self.node(receiver))?;
            let cached_receiver = self.cache_expression(visited_receiver)?;
            let visited_argument = self.visit_required_expression(self.node(argument))?;
            let replaced =
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(cached_receiver.node()),
                    question_dot_token: data.question_dot_token,
                    argument_expression: Some(visited_argument.node()),
                });
            let flags = flags_after_update(self.context.arena(), node, &replaced)?;
            let updated = self.context.factory()?.update_node(node, replaced, flags)?;
            Ok(Some(updated.node()))
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: visitCallExpression @6.0.3
    /// tsc-hash: 7c0d96a6a0e1226a6233687b1582904727947f358ea003f5a43d4e922c0971b9
    /// tsc-span: _tsc.js:108694-108716
    fn visit_call_expression(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::CallExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CallExpression,
                field: "call data",
            });
        };
        let arguments = self.array_nodes_of(data.arguments)?;
        let any_yield_argument = arguments
            .iter()
            .any(|argument| self.contains_yield(Some(*argument)));
        if !self.is_import_call(id)? && any_yield_argument {
            let callee = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "expression",
                })?;
            let (target, this_arg) = self.create_call_binding(
                self.node(callee),
                Some(self.language_version),
                /*cache_identifiers*/ true,
            )?;
            let visited_target = self.visit_required_expression(target)?;
            let cached_target = self.cache_expression(visited_target)?;
            let elements = self.visit_elements(&arguments, None, None, false)?;
            let apply = self.create_function_apply_call(cached_target, this_arg, elements)?;
            let apply = self.context.factory()?.set_text_range(apply, node)?;
            self.context
                .arena_mut()?
                .set_original_node(apply, Some(node))?;
            Ok(Some(apply.node()))
        } else {
            self.visit_each_child(id)
        }
    }

    /// tsc-port: visitNewExpression @6.0.3
    /// tsc-hash: c28eff415054d25b4582e5ce0ca9d0f82fa529a5ff31da715d2a303186d0d580
    /// tsc-span: _tsc.js:108717-108742
    fn visit_new_expression(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let node = self.node(id);
        let NodeData::NewExpression(data) = self.data(id)? else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::NewExpression,
                field: "new data",
            });
        };
        let arguments = self.array_nodes_of(data.arguments)?;
        let any_yield_argument = arguments
            .iter()
            .any(|argument| self.contains_yield(Some(*argument)));
        if any_yield_argument {
            let callee = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::NewExpression,
                    field: "expression",
                })?;
            let bind_name = self.create_identifier("bind")?;
            let bind_access = self.create_property_access(self.node(callee), bind_name)?;
            let (target, this_arg) = self.create_call_binding(bind_access, None, false)?;
            let visited_target = self.visit_required_expression(target)?;
            let cached_target = self.cache_expression(visited_target)?;
            let void_zero = self.create_void_zero()?;
            let elements = self.visit_elements(&arguments, Some(void_zero), None, false)?;
            let apply = self.create_function_apply_call(cached_target, this_arg, elements)?;
            let new_expression = self.create_new_expression(apply, Vec::new())?;
            let new_expression = self
                .context
                .factory()?
                .set_text_range(new_expression, node)?;
            self.context
                .arena_mut()?
                .set_original_node(new_expression, Some(node))?;
            Ok(Some(new_expression.node()))
        } else {
            self.visit_each_child(id)
        }
    }
}

// ---------------------------------------------------------------------------
// Temps, labels, and code blocks
// ---------------------------------------------------------------------------

impl GeneratorsVisitor<'_, '_> {
    /// tsc-port: cacheExpression @6.0.3
    /// tsc-hash: 9bf7b66845e2481b073404546d59dd3bb2d9271febd15d2974e5f6f45353d056
    /// tsc-span: _tsc.js:109291-109303
    fn cache_expression(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let is_generated = self
            .context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_id().is_some());
        if is_generated || self.emit_flags(node).contains(EmitFlags::HELPER_NAME) {
            return Ok(node);
        }
        // `createTempVariable(hoistVariableDeclaration)` — allocated AND
        // hoisted at creation.
        let temp = self.allocate_temp_binding()?;
        let identifier = self.create_generated_identifier(&temp)?;
        self.context.hoist_variable_declaration(identifier)?;
        let reference = self.create_generated_identifier(&temp)?;
        self.emit_assignment(reference, node, Some(node))?;
        Ok(reference)
    }

    /// tsc-port: declareLocal @6.0.3
    /// tsc-hash: 38e3b180101f2fd947737aa526b2ea6940e3530542c7a86da49ff279b32bab77
    /// tsc-span: _tsc.js:109304-109311
    fn declare_local(&mut self, name: Option<&str>) -> Result<TransformNode, TransformError> {
        Ok(self.declare_local_with_binding(name)?.0)
    }

    fn declare_local_with_binding(
        &mut self,
        name: Option<&str>,
    ) -> Result<(TransformNode, TargetBinding), TransformError> {
        let binding = match name {
            Some(name) => self.allocate_numbered_binding(name)?,
            None => self.allocate_temp_binding()?,
        };
        let identifier = self.create_generated_identifier(&binding)?;
        self.context.hoist_variable_declaration(identifier)?;
        let reference = self.create_generated_identifier(&binding)?;
        Ok((reference, binding))
    }

    /// `factory2.createLoopVariable()` (the `_i` name family).
    fn create_loop_variable(&mut self) -> Result<TransformNode, TransformError> {
        let binding = self.allocate_loop_variable_binding()?;
        self.create_generated_identifier(&binding)
    }

    /// tsc-port: defineLabel @6.0.3
    /// tsc-hash: 9c866533854e082d08103b062fef1cbb31edaacc749271a5fb72a7cf717ef017
    /// tsc-span: _tsc.js:109312-109320
    fn define_label(&mut self) -> Label {
        let label = self.next_label_id;
        self.next_label_id += 1;
        if self.label_offsets.len() <= label {
            self.label_offsets.resize(label + 1, None);
        }
        self.label_offsets[label] = None;
        label
    }

    /// tsc-port: markLabel @6.0.3
    /// tsc-hash: f562499ccf6af36ef341feb29f86d4b13fd33dff8d77f4d7062450e42e9389cf
    /// tsc-span: _tsc.js:109321-109324
    fn mark_label(&mut self, label: Label) -> Result<(), TransformError> {
        if self.label_offsets.len() <= label {
            // `Debug.assert(labelOffsets !== undefined, "No labels were defined.")`
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "generator label table",
            });
        }
        self.label_offsets[label] = Some(self.operations.len());
        Ok(())
    }

    /// tsc-port: beginBlock @6.0.3
    /// tsc-hash: a006f625b082280688e18ab9d688070b1065d21eb01d7664409d22f456425333
    /// tsc-span: _tsc.js:109325-109338
    fn begin_block(&mut self, block: CodeBlock) -> BlockId {
        let block_id = self.blocks.len();
        self.blocks.push(block);
        self.block_actions
            .push((BlockAction::Open, self.operations.len(), block_id));
        self.block_stack.push(block_id);
        block_id
    }

    /// tsc-port: endBlock @6.0.3
    /// tsc-hash: 33882e2938dcc1cae49666e4176befd58f599eebca3f71d50caec2212f0f009f
    /// tsc-span: _tsc.js:109339-109348
    fn end_block(&mut self) -> Result<BlockId, TransformError> {
        let block_id = self
            .block_stack
            .pop()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "generator block stack",
            })?;
        self.block_actions
            .push((BlockAction::Close, self.operations.len(), block_id));
        Ok(block_id)
    }

    /// tsc-port: peekBlock @6.0.3
    /// tsc-hash: ae3fc972654d70a55d0c051ce3184be5a410c97bd1ff55689b8d3317a4c0b828
    /// tsc-span: _tsc.js:109349-109351
    fn peek_block(&self) -> Option<BlockId> {
        self.block_stack.last().copied()
    }

    /// tsc-port: beginWithBlock @6.0.3
    /// tsc-hash: e9175b0ba2e70e441a513881f02cde3aff768e8c72593ce26845c94a21033e6c
    /// tsc-span: _tsc.js:109356-109366
    fn begin_with_block(&mut self, expression: TransformNode) -> Result<(), TransformError> {
        let start_label = self.define_label();
        let end_label = self.define_label();
        self.mark_label(start_label)?;
        self.begin_block(CodeBlock::With {
            expression,
            start_label,
            end_label,
        });
        Ok(())
    }

    /// tsc-port: endWithBlock @6.0.3
    /// tsc-hash: 8fc9b1a49d04fe9aaf31d43eaee0e3bde59e5e7cd1f77991f3385abf2eadcf54
    /// tsc-span: _tsc.js:109367-109371
    fn end_with_block(&mut self) -> Result<(), TransformError> {
        let block_id = self.end_block()?;
        match &self.blocks[block_id] {
            CodeBlock::With { end_label, .. } => {
                let end_label = *end_label;
                self.mark_label(end_label)
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::WithStatement,
                field: "with block at end",
            }),
        }
    }

    /// tsc-port: beginExceptionBlock @6.0.3
    /// tsc-hash: 42ade19da0a0a40b47d6cb42308c83e5a775c41dc28e22014adf4b66b4316b28
    /// tsc-span: _tsc.js:109372-109384
    fn begin_exception_block(&mut self) -> Result<Label, TransformError> {
        let start_label = self.define_label();
        let end_label = self.define_label();
        self.mark_label(start_label)?;
        self.begin_block(CodeBlock::Exception {
            state: ExceptionBlockState::Try,
            start_label,
            end_label,
            catch_variable: None,
            catch_label: None,
            finally_label: None,
        });
        self.emit_nop();
        Ok(end_label)
    }

    /// tsc-port: beginCatchBlock @6.0.3
    /// tsc-hash: d2fc338ccbc6cdacf53c9afe096110573e9e77b2a14a48e612409a37bc82ea1c
    /// tsc-span: _tsc.js:109385-109418
    fn begin_catch_block(&mut self, variable: TransformNode) -> Result<(), TransformError> {
        let NodeData::VariableDeclaration(declaration) =
            self.context.arena().node(variable)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "catch variable declaration",
            });
        };
        let name = declaration
            .name
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "name",
            })?;
        let name = self.node(name);
        let is_generated = self
            .context
            .arena()
            .metadata(name)
            .is_some_and(|metadata| metadata.generated_binding_id().is_some());
        let renamed = if is_generated {
            self.context.hoist_variable_declaration(name)?;
            name
        } else {
            let text = self.identifier_text(name)?;
            let (local, binding) = self.declare_local_with_binding(Some(&text))?;
            if self.renames.renamed_catch_variables.is_empty() {
                self.context.enable_substitution(SyntaxKind::Identifier)?;
            }
            self.renames.renamed_catch_variables.insert(text, ());
            // `renamedCatchVariableDeclarations[getOriginalNodeId(variable)] = name`
            let reference = self
                .context
                .arena()
                .require_parse_tree_resolver_node(variable)?;
            self.renames
                .renamed_catch_variable_declarations
                .insert((reference.source(), reference.node()), binding);
            local
        };
        let exception = self
            .peek_block()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CatchClause,
                field: "exception block",
            })?;
        let end_label = match &self.blocks[exception] {
            CodeBlock::Exception {
                state, end_label, ..
            } => {
                if *state >= ExceptionBlockState::Catch {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CatchClause,
                        field: "exception state below Catch",
                    });
                }
                *end_label
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CatchClause,
                    field: "enclosing exception block",
                })
            }
        };
        self.emit_break(end_label, None)?;
        let catch_label = self.define_label();
        self.mark_label(catch_label)?;
        if let CodeBlock::Exception {
            state,
            catch_variable,
            catch_label: block_catch_label,
            ..
        } = &mut self.blocks[exception]
        {
            *state = ExceptionBlockState::Catch;
            *catch_variable = Some(renamed);
            *block_catch_label = Some(catch_label);
        }
        let sent = self.create_state_sent_call(None)?;
        self.emit_assignment(renamed, sent, None)?;
        self.emit_nop();
        Ok(())
    }

    /// tsc-port: beginFinallyBlock @6.0.3
    /// tsc-hash: 5f19e31e2dd90f9a92496b6778e1a641ee5651e417fbdfe822104a812787cf01
    /// tsc-span: _tsc.js:109419-109429
    fn begin_finally_block(&mut self) -> Result<(), TransformError> {
        let exception = self
            .peek_block()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "exception block",
            })?;
        let end_label = match &self.blocks[exception] {
            CodeBlock::Exception {
                state, end_label, ..
            } => {
                if *state >= ExceptionBlockState::Finally {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TryStatement,
                        field: "exception state below Finally",
                    });
                }
                *end_label
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "enclosing exception block",
                })
            }
        };
        self.emit_break(end_label, None)?;
        let finally_label = self.define_label();
        self.mark_label(finally_label)?;
        if let CodeBlock::Exception {
            state,
            finally_label: block_finally_label,
            ..
        } = &mut self.blocks[exception]
        {
            *state = ExceptionBlockState::Finally;
            *block_finally_label = Some(finally_label);
        }
        Ok(())
    }

    /// tsc-port: endExceptionBlock @6.0.3
    /// tsc-hash: f8f26b14bea22ca58ec2eb938dac6634d932384bc59e0cdf094d474b58fece76
    /// tsc-span: _tsc.js:109430-109442
    fn end_exception_block(&mut self) -> Result<(), TransformError> {
        let block_id = self.end_block()?;
        let (state, end_label) = match &self.blocks[block_id] {
            CodeBlock::Exception {
                state, end_label, ..
            } => (*state, *end_label),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "exception block at end",
                })
            }
        };
        if state < ExceptionBlockState::Finally {
            self.emit_break(end_label, None)?;
        } else {
            self.emit_endfinally();
        }
        self.mark_label(end_label)?;
        self.emit_nop();
        if let CodeBlock::Exception { state, .. } = &mut self.blocks[block_id] {
            *state = ExceptionBlockState::Done;
        }
        Ok(())
    }

    /// tsc-port: beginScriptLoopBlock @6.0.3
    /// tsc-hash: d02cc5869859e3004627943929fc5100c8817dbefd77a3e721cab8147ee66520
    /// tsc-span: _tsc.js:109443-109450
    fn begin_script_loop_block(&mut self) {
        self.begin_block(CodeBlock::Loop {
            is_script: true,
            break_label: 0,
            continue_label: 0,
        });
    }

    /// tsc-port: beginLoopBlock @6.0.3
    /// tsc-hash: 3cd2a0e840c91099ebbc55813f48e1bc409642d12d9d7eecdb754ee99e29c7bf
    /// tsc-span: _tsc.js:109451-109460
    fn begin_loop_block(&mut self, continue_label: Label) -> Label {
        let break_label = self.define_label();
        self.begin_block(CodeBlock::Loop {
            is_script: false,
            break_label,
            continue_label,
        });
        break_label
    }

    /// tsc-port: endLoopBlock @6.0.3
    /// tsc-hash: 5c1060e4cb0a8df47bca2186f14e4a8acd1d6cb48cc77ed43e33c9feff0c0aaa
    /// tsc-span: _tsc.js:109461-109468
    fn end_loop_block(&mut self) -> Result<(), TransformError> {
        let block_id = self.end_block()?;
        match &self.blocks[block_id] {
            CodeBlock::Loop {
                is_script,
                break_label,
                ..
            } => {
                if !is_script {
                    let break_label = *break_label;
                    self.mark_label(break_label)?;
                }
                Ok(())
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "loop block at end",
            }),
        }
    }

    /// tsc-port: beginScriptSwitchBlock @6.0.3
    /// tsc-hash: 93817ddadcac9da1a07ec4356650e8e4f7cf928ee9f0f346023bb9dbe68ee94d
    /// tsc-span: _tsc.js:109469-109475
    fn begin_script_switch_block(&mut self) {
        self.begin_block(CodeBlock::Switch {
            is_script: true,
            break_label: 0,
        });
    }

    /// tsc-port: beginSwitchBlock @6.0.3
    /// tsc-hash: b879557ff0cc26b2bb70e8938b43d691cf07ee67914f1c4ea41c5c7a5af75882
    /// tsc-span: _tsc.js:109476-109484
    fn begin_switch_block(&mut self) -> Label {
        let break_label = self.define_label();
        self.begin_block(CodeBlock::Switch {
            is_script: false,
            break_label,
        });
        break_label
    }

    /// tsc-port: endSwitchBlock @6.0.3
    /// tsc-hash: 74dd506129e3c02e9f5c97144be2dfb411ea6d28580638dba55fcbf92a0f3026
    /// tsc-span: _tsc.js:109485-109492
    fn end_switch_block(&mut self) -> Result<(), TransformError> {
        let block_id = self.end_block()?;
        match &self.blocks[block_id] {
            CodeBlock::Switch {
                is_script,
                break_label,
            } => {
                if !is_script {
                    let break_label = *break_label;
                    self.mark_label(break_label)?;
                }
                Ok(())
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "switch block at end",
            }),
        }
    }

    /// tsc-port: beginScriptLabeledBlock @6.0.3
    /// tsc-hash: 2e5d8f46ca701d04a451ebe163ebf3b8f7a875dbd5048dc03bb1268018498d84
    /// tsc-span: _tsc.js:109493-109500
    fn begin_script_labeled_block(&mut self, label_text: String) {
        self.begin_block(CodeBlock::Labeled {
            is_script: true,
            label_text,
            break_label: 0,
        });
    }

    /// tsc-port: beginLabeledBlock @6.0.3
    /// tsc-hash: 8251d37e91f50aaad36de6848190322c18fc66509ae033b99b2c01330cde80e2
    /// tsc-span: _tsc.js:109501-109509
    fn begin_labeled_block(&mut self, label_text: String) {
        let break_label = self.define_label();
        self.begin_block(CodeBlock::Labeled {
            is_script: false,
            label_text,
            break_label,
        });
    }

    /// tsc-port: endLabeledBlock @6.0.3
    /// tsc-hash: 7fbc09cf024cc2aaabe489db29a65fecdc2b3fb12be129daae24a644eccffce7
    /// tsc-span: _tsc.js:109510-109516
    fn end_labeled_block(&mut self) -> Result<(), TransformError> {
        let block_id = self.end_block()?;
        match &self.blocks[block_id] {
            CodeBlock::Labeled {
                is_script,
                break_label,
                ..
            } => {
                if !is_script {
                    let break_label = *break_label;
                    self.mark_label(break_label)?;
                }
                Ok(())
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "labeled block at end",
            }),
        }
    }

    /// tsc-port: hasImmediateContainingLabeledBlock @6.0.3
    /// tsc-hash: ef67f25d295c8597261e63f8f9959939f21702d276450c6f7f88ec9818a170ec
    /// tsc-span: _tsc.js:109526-109538
    fn has_immediate_containing_labeled_block(&self, label_text: &str, start: isize) -> bool {
        let mut j = start;
        while j >= 0 {
            let containing = &self.blocks[self.block_stack[j as usize]];
            if let CodeBlock::Labeled {
                label_text: containing_text,
                ..
            } = containing
            {
                if containing_text == label_text {
                    return true;
                }
                j -= 1;
            } else {
                break;
            }
        }
        false
    }

    /// tsc-port: findBreakTarget @6.0.3
    /// tsc-hash: 7ff3d1fddb7e4f8f73cecfdda28e3bdbbc19f1e569e0ed7ece3f308188d44752
    /// tsc-span: _tsc.js:109539-109560
    fn find_break_target(&self, label_text: Option<&str>) -> Label {
        if let Some(label_text) = label_text {
            for i in (0..self.block_stack.len()).rev() {
                let block = &self.blocks[self.block_stack[i]];
                if block.supports_labeled_break_or_continue() {
                    if let CodeBlock::Labeled {
                        label_text: block_text,
                        break_label,
                        ..
                    } = block
                    {
                        if block_text == label_text {
                            return *break_label;
                        }
                    }
                } else if block.supports_unlabeled_break()
                    && self.has_immediate_containing_labeled_block(label_text, i as isize - 1)
                {
                    return match block {
                        CodeBlock::Switch { break_label, .. } => *break_label,
                        CodeBlock::Loop { break_label, .. } => *break_label,
                        _ => unreachable!("supports_unlabeled_break admits switch/loop"),
                    };
                }
            }
        } else {
            for i in (0..self.block_stack.len()).rev() {
                let block = &self.blocks[self.block_stack[i]];
                if block.supports_unlabeled_break() {
                    return match block {
                        CodeBlock::Switch { break_label, .. } => *break_label,
                        CodeBlock::Loop { break_label, .. } => *break_label,
                        _ => unreachable!("supports_unlabeled_break admits switch/loop"),
                    };
                }
            }
        }
        0
    }

    /// tsc-port: findContinueTarget @6.0.3
    /// tsc-hash: bc87cdf4e300b2991fa38baf61f54dee8c89b6d8881fb7da2005222fca0f15a2
    /// tsc-span: _tsc.js:109561-109580
    fn find_continue_target(&self, label_text: Option<&str>) -> Label {
        if let Some(label_text) = label_text {
            for i in (0..self.block_stack.len()).rev() {
                let block = &self.blocks[self.block_stack[i]];
                if block.supports_unlabeled_continue()
                    && self.has_immediate_containing_labeled_block(label_text, i as isize - 1)
                {
                    if let CodeBlock::Loop { continue_label, .. } = block {
                        return *continue_label;
                    }
                }
            }
        } else {
            for i in (0..self.block_stack.len()).rev() {
                let block = &self.blocks[self.block_stack[i]];
                if let CodeBlock::Loop { continue_label, .. } = block {
                    return *continue_label;
                }
            }
        }
        0
    }

    /// tsc-port: createLabel @6.0.3
    /// tsc-hash: 4e2a8ecb553028df68197008ed04343c91f28d511d2a9bc1debfa5043d12ee7b
    /// tsc-span: _tsc.js:109581-109595
    ///
    /// Round 1 emits `Number.MAX_SAFE_INTEGER` placeholders (recorded per
    /// label for the §12.4 two-round protocol); round 2 consults the
    /// resolved label→case map and mints final literals directly.
    fn create_label_expression(
        &mut self,
        label: Option<Label>,
    ) -> Result<TransformNode, TransformError> {
        if let Some(label) = label.filter(|label| *label > 0) {
            // `Number.MAX_SAFE_INTEGER` placeholder, recorded in
            // `labelExpressions[label]` and finalized by
            // `update_label_expressions`.
            let literal = self.create_numeric_literal("9007199254740991")?;
            self.record_label_expression(label, literal);
            return Ok(literal);
        }
        self.create_omitted_expression()
    }

    /// tsc-port: createInstruction @6.0.3
    /// tsc-hash: 803684eeae5ab66d21ce6f2344ac372b526c4752b809158e9aba9fca37946c4c
    /// tsc-span: _tsc.js:109596-109600
    fn create_instruction(
        &mut self,
        instruction: Instruction,
    ) -> Result<TransformNode, TransformError> {
        let literal = self.create_numeric_literal(&instruction.code().to_string())?;
        self.context
            .arena_mut()?
            .metadata_mut(literal)
            .add_trailing_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                instruction.comment_text(),
                false,
                false,
            ));
        Ok(literal)
    }

    /// tsc-port: createInlineBreak @6.0.3
    /// tsc-hash: 26170574408df556d6fecbf09f577cd386ec89aa0d2d899cb07a022179dc468c
    /// tsc-span: _tsc.js:109601-109612
    fn create_inline_break(
        &mut self,
        label: Label,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if label == 0 {
            // `Debug.assertLessThan(0, label, "Invalid label")`
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BreakStatement,
                field: "generator break label",
            });
        }
        let instruction = self.create_instruction(Instruction::Break)?;
        let label_expression = self.create_label_expression(Some(label))?;
        let array = self.create_array_literal(vec![instruction, label_expression])?;
        let statement = self.create_return_statement(Some(array))?;
        match location {
            Some(location) => self.context.factory()?.set_text_range(statement, location),
            None => Ok(statement),
        }
    }

    /// tsc-port: createInlineReturn @6.0.3
    /// tsc-hash: a3e0c29e7977e4e0ccfd57a3a75b4aaab55975fae52eb5729d784c30feb7f9d6
    /// tsc-span: _tsc.js:109613-109622
    fn create_inline_return(
        &mut self,
        expression: Option<TransformNode>,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let instruction = self.create_instruction(Instruction::Return)?;
        let elements = match expression {
            Some(expression) => vec![instruction, expression],
            None => vec![instruction],
        };
        let array = self.create_array_literal(elements)?;
        let statement = self.create_return_statement(Some(array))?;
        match location {
            Some(location) => self.context.factory()?.set_text_range(statement, location),
            None => Ok(statement),
        }
    }

    /// tsc-port: createGeneratorResume @6.0.3
    /// tsc-hash: 7950ba978df30ed6bd49d800d02e87091cf61ab64c44c7eac5735f3b61204f96
    /// tsc-span: _tsc.js:109623-109633
    fn create_generator_resume(
        &mut self,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_state_sent_call(location)
    }

    /// `state.sent()` — shared by `createGeneratorResume` and
    /// `beginCatchBlock`.
    fn create_state_sent_call(
        &mut self,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let state = self.state_reference()?;
        let sent = self.create_identifier("sent")?;
        let access = self.create_property_access(state, sent)?;
        let call = self.create_call(access, Vec::new())?;
        match location {
            Some(location) => self.context.factory()?.set_text_range(call, location),
            None => Ok(call),
        }
    }

    // -----------------------------------------------------------------------
    // Emit recorders
    // -----------------------------------------------------------------------

    /// tsc-port: emitNop @6.0.3
    /// tsc-hash: 171f3eb6a5435d8b0845a21f28e2ad2d3046c52fe7f85ebce64ccf0d59a4b227
    /// tsc-span: _tsc.js:109634-109636
    fn emit_nop(&mut self) {
        self.emit_worker(OpCode::Nop, Vec::new(), Vec::new(), None);
    }

    /// tsc-port: emitStatement @6.0.3
    /// tsc-hash: bd8f18daaa16c88d26fb7e7623fa5bafef0f41414db9b3f29730e7c6cf7b66e4
    /// tsc-span: _tsc.js:109637-109643
    fn emit_statement(&mut self, node: TransformNode) -> Result<(), TransformError> {
        self.emit_worker(OpCode::Statement, vec![node], Vec::new(), None);
        Ok(())
    }

    /// `emitStatement(visitNode(...))`'s undefined arm.
    fn emit_statement_opt(&mut self, node: Option<TransformNode>) -> Result<(), TransformError> {
        match node {
            Some(node) => self.emit_statement(node),
            None => {
                self.emit_nop();
                Ok(())
            }
        }
    }

    /// tsc-port: emitAssignment @6.0.3
    /// tsc-hash: c9dce9f7dbeae13b7a4c6b03da80d3d231758bfac5f1656f13287ce415080bf5
    /// tsc-span: _tsc.js:109644-109646
    fn emit_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(OpCode::Assign, vec![left, right], Vec::new(), location);
        Ok(())
    }

    /// tsc-port: emitBreak @6.0.3
    /// tsc-hash: a57648523065117b399b057715a831fb56b18710ba005c10abe35bfa77250260
    /// tsc-span: _tsc.js:109647-109649
    fn emit_break(
        &mut self,
        label: Label,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(OpCode::Break, Vec::new(), vec![label], location);
        Ok(())
    }

    /// tsc-port: emitBreakWhenTrue @6.0.3
    /// tsc-hash: 5e5f430ecde1fd9bf82e3d7e405868ca2693e7bfe3cd3b558b2b0ac2ae113e01
    /// tsc-span: _tsc.js:109650-109652
    fn emit_break_when_true(
        &mut self,
        label: Label,
        condition: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(
            OpCode::BreakWhenTrue,
            vec![condition],
            vec![label],
            location,
        );
        Ok(())
    }

    /// tsc-port: emitBreakWhenFalse @6.0.3
    /// tsc-hash: 813e987a2d88b2051bfa86aee01389128ae6d6e68622866f457f80e1ca05a217
    /// tsc-span: _tsc.js:109653-109655
    fn emit_break_when_false(
        &mut self,
        label: Label,
        condition: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(
            OpCode::BreakWhenFalse,
            vec![condition],
            vec![label],
            location,
        );
        Ok(())
    }

    /// tsc-port: emitYieldStar @6.0.3
    /// tsc-hash: c43daa1c4465d56d8813c657e69c73134bdde91b849ca839aba29c71e129eca6
    /// tsc-span: _tsc.js:109656-109658
    fn emit_yield_star(
        &mut self,
        expression: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(OpCode::YieldStar, vec![expression], Vec::new(), location);
        Ok(())
    }

    /// tsc-port: emitYield @6.0.3
    /// tsc-hash: 644e0ba4063953c1fef3f2bbd2e14abac5d7fc5c3a29ba4a3f5a953fe16fb05a
    /// tsc-span: _tsc.js:109659-109661
    fn emit_yield(
        &mut self,
        expression: Option<TransformNode>,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let args = expression.map(|node| vec![node]).unwrap_or_default();
        self.emit_worker(OpCode::Yield, args, Vec::new(), location);
        Ok(())
    }

    /// tsc-port: emitReturn @6.0.3
    /// tsc-hash: 38d1338d596867038cae20b4a4e4c146c99d5256a3d1f26232b9f84a35c6e239
    /// tsc-span: _tsc.js:109662-109664
    fn emit_return(
        &mut self,
        expression: Option<TransformNode>,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let args = expression.map(|node| vec![node]).unwrap_or_default();
        self.emit_worker(OpCode::Return, args, Vec::new(), location);
        Ok(())
    }

    /// tsc-port: emitThrow @6.0.3
    /// tsc-hash: ae8741de8b7365f889a3a9c8a4335bd1727b8f60a5fbe0d6b97e65e394fa55a5
    /// tsc-span: _tsc.js:109665-109667
    fn emit_throw(
        &mut self,
        expression: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.emit_worker(OpCode::Throw, vec![expression], Vec::new(), location);
        Ok(())
    }

    /// tsc-port: emitEndfinally @6.0.3
    /// tsc-hash: 6a5fa6863e7a958825293e5e29a78a5d3e26562128bbd249e71d57f1b7ed138d
    /// tsc-span: _tsc.js:109668-109670
    fn emit_endfinally(&mut self) {
        self.emit_worker(OpCode::Endfinally, Vec::new(), Vec::new(), None);
    }

    /// tsc-port: emitWorker @6.0.3
    /// tsc-hash: 54fb879d6878e8bfabb06122b2dc0d12c95e6254834f754ef93e711325c8cb26
    /// tsc-span: _tsc.js:109671-109684
    fn emit_worker(
        &mut self,
        code: OpCode,
        args: Vec<TransformNode>,
        labels: Vec<Label>,
        location: Option<TransformNode>,
    ) {
        if self.label_offsets.is_empty() {
            // `if (labelOffsets === void 0) markLabel(defineLabel())`
            let label = self.define_label();
            self.label_offsets[label] = Some(self.operations.len());
        }
        self.operations.push(Operation {
            code,
            args,
            labels,
            location,
        });
    }
}

// ---------------------------------------------------------------------------
// The build pipeline
// ---------------------------------------------------------------------------

impl GeneratorsVisitor<'_, '_> {
    /// tsc-port: build @6.0.3
    /// tsc-hash: 9310d5f9a876a3bbb7502359d284f69f3c97a566e4895ce12c875bec2d466fa2
    /// tsc-span: _tsc.js:109685-109726
    fn build(&mut self) -> Result<TransformNode, TransformError> {
        self.block_index = 0;
        self.label_number = 0;
        self.label_numbers = Vec::new();
        self.last_operation_was_abrupt = false;
        self.last_operation_was_completion = false;
        self.clauses = None;
        self.statements = None;
        self.exception_block_stack = Vec::new();
        self.current_exception_block = None;
        self.with_block_stack = Vec::new();
        let build_result = self.build_statements()?;
        let state_binding = self
            .state
            .clone()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionExpression,
                field: "generator state binding",
            })?;
        let state_parameter_name = self.create_generated_identifier(&state_binding)?;
        let parameter = self.create_parameter(state_parameter_name)?;
        let body = self.create_block_multi_line(build_result.clone(), !build_result.is_empty())?;
        let function = self.create_function_expression(vec![parameter], body)?;
        self.context
            .arena_mut()?
            .metadata_mut(function)
            .add_flags(EmitFlags::REUSE_TEMP_VARIABLE_SCOPE);
        self.create_generator_helper_call(function)
    }

    /// tsc-port: buildStatements @6.0.3
    /// tsc-hash: 55db5a17f4d457720e5ea7b84d5a2373215c8826a347db363623a1292ac9998e
    /// tsc-span: _tsc.js:109727-109745
    fn build_statements(&mut self) -> Result<Vec<TransformNode>, TransformError> {
        if !self.operations.is_empty() {
            for operation_index in 0..self.operations.len() {
                self.write_operation(operation_index)?;
            }
            self.flush_final_label(self.operations.len())?;
        } else {
            self.flush_final_label(0)?;
        }
        if let Some(clauses) = self.clauses.take() {
            let state = self.state_reference()?;
            let label_name = self.create_identifier("label")?;
            let label_access = self.create_property_access(state, label_name)?;
            let case_block = self.create_case_block(clauses)?;
            let switch_statement = self.create_switch_statement(label_access, case_block)?;
            self.start_on_new_line(switch_statement)?;
            return Ok(vec![switch_statement]);
        }
        if let Some(statements) = self.statements.take() {
            return Ok(statements);
        }
        Ok(Vec::new())
    }

    /// tsc-port: flushLabel @6.0.3
    /// tsc-hash: bbb88d6572897b9910bc6e387164a9495898592f7665282fbb282816bb5df9e8
    /// tsc-span: _tsc.js:109746-109757
    fn flush_label(&mut self) -> Result<(), TransformError> {
        if self.statements.is_none() {
            return Ok(());
        }
        self.append_label(!self.last_operation_was_abrupt)?;
        self.last_operation_was_abrupt = false;
        self.last_operation_was_completion = false;
        self.label_number += 1;
        Ok(())
    }

    /// tsc-port: flushFinalLabel @6.0.3
    /// tsc-hash: 9ebbd4e75c178bedef4db085fa4e878e53c9bab1d38c600e29becfa20605990d
    /// tsc-span: _tsc.js:109758-109776
    fn flush_final_label(&mut self, operation_index: usize) -> Result<(), TransformError> {
        if self.is_final_label_reachable(operation_index) {
            self.try_enter_label(operation_index)?;
            self.with_block_stack = Vec::new();
            self.write_return(None, None)?;
        }
        if self.statements.is_some() && self.clauses.is_some() {
            self.append_label(false)?;
        }
        self.update_label_expressions()
    }

    /// tsc-port: isFinalLabelReachable @6.0.3
    /// tsc-hash: bea697744daa1ce6e02578d978bd492f5cd0af3b0d663453ecb5282ce10477c6
    /// tsc-span: _tsc.js:109777-109790
    fn is_final_label_reachable(&self, operation_index: usize) -> bool {
        if !self.last_operation_was_completion {
            return true;
        }
        if self.label_offsets.is_empty() || self.label_expressions.is_empty() {
            return false;
        }
        for label in 0..self.label_offsets.len() {
            if self.label_offsets[label] == Some(operation_index)
                && self
                    .label_expressions
                    .get(label)
                    .is_some_and(|expressions| !expressions.is_empty())
            {
                return true;
            }
        }
        false
    }

    /// tsc-port: appendLabel @6.0.3
    /// tsc-hash: fc91b657fa18527229fca3a40b031dc97de4dd82e668d7a10d45fe7430e0d8c7
    /// tsc-span: _tsc.js:109791-109841
    fn append_label(&mut self, mark_label_end: bool) -> Result<(), TransformError> {
        if self.clauses.is_none() {
            self.clauses = Some(Vec::new());
        }
        if let Some(mut statements) = self.statements.take() {
            if !self.with_block_stack.is_empty() {
                for block_id in self.with_block_stack.clone().into_iter().rev() {
                    let expression = match &self.blocks[block_id] {
                        CodeBlock::With { expression, .. } => *expression,
                        _ => {
                            return Err(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::WithStatement,
                                field: "with block record",
                            })
                        }
                    };
                    let block = self.create_block(statements)?;
                    let with_statement = self.create_with_statement(expression, block)?;
                    statements = vec![with_statement];
                }
            }
            if let Some(exception_block) = self.current_exception_block.take() {
                let (start_label, catch_label, finally_label, end_label) =
                    match &self.blocks[exception_block] {
                        CodeBlock::Exception {
                            start_label,
                            catch_label,
                            finally_label,
                            end_label,
                            ..
                        } => (*start_label, *catch_label, *finally_label, *end_label),
                        _ => {
                            return Err(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::TryStatement,
                                field: "exception block record",
                            })
                        }
                    };
                let start = self.create_label_expression(Some(start_label))?;
                let catch = self.create_label_expression(catch_label)?;
                let finally = self.create_label_expression(finally_label)?;
                let end = self.create_label_expression(Some(end_label))?;
                let array = self.create_array_literal(vec![start, catch, finally, end])?;
                let state = self.state_reference()?;
                let trys_name = self.create_identifier("trys")?;
                let trys_access = self.create_property_access(state, trys_name)?;
                let push_name = self.create_identifier("push")?;
                let push_access = self.create_property_access(trys_access, push_name)?;
                let push_call = self.create_call(push_access, vec![array])?;
                let push_statement = self.create_expression_statement(push_call)?;
                statements.insert(0, push_statement);
            }
            if mark_label_end {
                let state = self.state_reference()?;
                let label_name = self.create_identifier("label")?;
                let label_access = self.create_property_access(state, label_name)?;
                let next = self.create_numeric_literal(&(self.label_number + 1).to_string())?;
                let assignment = self.create_assignment(label_access, next)?;
                let statement = self.create_expression_statement(assignment)?;
                statements.push(statement);
            }
            let case_expression = self.create_numeric_literal(&self.label_number.to_string())?;
            let clause = self.create_case_clause(case_expression, statements)?;
            self.clauses
                .as_mut()
                .expect("clauses initialized above")
                .push(clause);
        } else {
            let case_expression = self.create_numeric_literal(&self.label_number.to_string())?;
            let clause = self.create_case_clause(case_expression, Vec::new())?;
            self.clauses
                .as_mut()
                .expect("clauses initialized above")
                .push(clause);
        }
        Ok(())
    }

    /// tsc-port: tryEnterLabel @6.0.3
    /// tsc-hash: f127b6a29348b36ca6f5bd91f78f2962d8ffdc9dccee44bf89c2c29dc4fc3fa8
    /// tsc-span: _tsc.js:109842-109859
    fn try_enter_label(&mut self, operation_index: usize) -> Result<(), TransformError> {
        if self.label_offsets.is_empty() {
            return Ok(());
        }
        for label in 0..self.label_offsets.len() {
            if self.label_offsets[label] == Some(operation_index) {
                self.flush_label()?;
                if self.label_numbers.len() <= self.label_number {
                    self.label_numbers.resize(self.label_number + 1, None);
                }
                match &mut self.label_numbers[self.label_number] {
                    Some(labels) => labels.push(label),
                    slot @ None => *slot = Some(vec![label]),
                }
            }
        }
        Ok(())
    }

    /// tsc-port: updateLabelExpressions @6.0.3
    /// tsc-hash: 5b44b108fb2fceda2c2ac85a3b522df22fc3c93bf937700aabe3b8d32a993e4d
    /// tsc-span: _tsc.js:109860-109876
    ///
    /// Upstream mutates `expression.text = String(labelNumber)` on every
    /// recorded placeholder literal; the Rust equivalent is the sanctioned
    /// arena text-finalization API (the `set_generated_identifier_text`
    /// precedent the name finalizer uses).
    fn update_label_expressions(&mut self) -> Result<(), TransformError> {
        if self.label_expressions.is_empty() || self.label_numbers.is_empty() {
            return Ok(());
        }
        for label_number in 0..self.label_numbers.len() {
            let Some(labels) = self.label_numbers[label_number].clone() else {
                continue;
            };
            for label in labels {
                let Some(expressions) = self.label_expressions.get(label).cloned() else {
                    continue;
                };
                for expression in expressions {
                    self.context
                        .arena_mut()?
                        .set_numeric_literal_text(expression, &label_number.to_string())?;
                }
            }
        }
        Ok(())
    }

    /// tsc-port: tryEnterOrLeaveBlock @6.0.3
    /// tsc-hash: 5807b6973c63cd20ef5b5b6bae0c1d9284c05046b6172376e1f26f6078473c7c
    /// tsc-span: _tsc.js:109877-109910
    fn try_enter_or_leave_block(&mut self, operation_index: usize) -> Result<(), TransformError> {
        while self.block_index < self.block_actions.len()
            && self.block_actions[self.block_index].1 <= operation_index
        {
            let (action, _offset, block_id) = self.block_actions[self.block_index];
            self.block_index += 1;
            match &self.blocks[block_id] {
                CodeBlock::Exception { .. } => match action {
                    BlockAction::Open => {
                        if self.statements.is_none() {
                            self.statements = Some(Vec::new());
                        }
                        self.exception_block_stack
                            .push(self.current_exception_block);
                        self.current_exception_block = Some(block_id);
                    }
                    BlockAction::Close => {
                        self.current_exception_block = self.exception_block_stack.pop().flatten();
                    }
                },
                CodeBlock::With { .. } => match action {
                    BlockAction::Open => {
                        self.with_block_stack.push(block_id);
                    }
                    BlockAction::Close => {
                        self.with_block_stack.pop();
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    /// tsc-port: writeOperation @6.0.3
    /// tsc-hash: b7e584811b23bbac18f9c0d087934d1a5665370f5bd1f9b902e1c53714967dfa
    /// tsc-span: _tsc.js:109911-109948
    fn write_operation(&mut self, operation_index: usize) -> Result<(), TransformError> {
        self.try_enter_label(operation_index)?;
        self.try_enter_or_leave_block(operation_index)?;
        if self.last_operation_was_abrupt {
            return Ok(());
        }
        self.last_operation_was_abrupt = false;
        self.last_operation_was_completion = false;
        let code = self.operations[operation_index].code;
        if code == OpCode::Nop {
            return Ok(());
        } else if code == OpCode::Endfinally {
            return self.write_endfinally();
        }
        let args = self.operations[operation_index].args.clone();
        let labels = self.operations[operation_index].labels.clone();
        if code == OpCode::Statement {
            return self.write_statement_node(args.first().copied());
        }
        let location = self.operations[operation_index].location;
        match code {
            OpCode::Assign => self.write_assign(args[0], args[1], location),
            OpCode::Break => self.write_break(labels[0], location),
            OpCode::BreakWhenTrue => self.write_break_when_true(labels[0], args[0], location),
            OpCode::BreakWhenFalse => self.write_break_when_false(labels[0], args[0], location),
            OpCode::Yield => self.write_yield(args.first().copied(), location),
            OpCode::YieldStar => self.write_yield_star(args[0], location),
            OpCode::Return => self.write_return(args.first().copied(), location),
            OpCode::Throw => self.write_throw(args[0], location),
            OpCode::Nop | OpCode::Statement | OpCode::Endfinally => unreachable!("handled above"),
        }
    }

    /// tsc-port: writeStatement @6.0.3
    /// tsc-hash: 92fac0cb7f8243b3daa39c7346d6307d3874fa4ff31e2210689a7e9b4984a77e
    /// tsc-span: _tsc.js:109949-109957
    fn write_statement_node(
        &mut self,
        statement: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        if let Some(statement) = statement {
            match &mut self.statements {
                Some(statements) => statements.push(statement),
                None => self.statements = Some(vec![statement]),
            }
        }
        Ok(())
    }

    /// tsc-port: writeAssign @6.0.3
    /// tsc-hash: 253ed241d024a65f599ad717c22ca620b00bb1cf775816c4b0ba129ca62dac94
    /// tsc-span: _tsc.js:109958-109960
    fn write_assign(
        &mut self,
        left: TransformNode,
        right: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let assignment = self.create_assignment(left, right)?;
        let statement = self.create_expression_statement(assignment)?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeThrow @6.0.3
    /// tsc-hash: 459416d429d3a071b50839b6dfc40232f75e837563a517b3b63ea10c2ce24cbb
    /// tsc-span: _tsc.js:109961-109965
    fn write_throw(
        &mut self,
        expression: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        self.last_operation_was_completion = true;
        let statement = self.create_throw_statement(expression)?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeReturn @6.0.3
    /// tsc-hash: eec0a59991adf9a21b0ffd04c86e234a61eb678ccf8c98fcb61cc004e4b79a8c
    /// tsc-span: _tsc.js:109966-109982
    fn write_return(
        &mut self,
        expression: Option<TransformNode>,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        self.last_operation_was_completion = true;
        let instruction = self.create_instruction(Instruction::Return)?;
        let elements = match expression {
            Some(expression) => vec![instruction, expression],
            None => vec![instruction],
        };
        let array = self.create_array_literal(elements)?;
        let statement = self.create_return_statement(Some(array))?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeBreak @6.0.3
    /// tsc-hash: d2d8b5e612e7562ac6d269966e1923f2c4858b9b1e190d09c2e7ab3a7cbbc988
    /// tsc-span: _tsc.js:109983-109999
    fn write_break(
        &mut self,
        label: Label,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        let instruction = self.create_instruction(Instruction::Break)?;
        let label_expression = self.create_label_expression(Some(label))?;
        let array = self.create_array_literal(vec![instruction, label_expression])?;
        let statement = self.create_return_statement(Some(array))?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeBreakWhenTrue @6.0.3
    /// tsc-hash: 68c2d1f827d300062d24ea6dbe5ed243416cd8dbff556ac97a327af8b5f103e9
    /// tsc-span: _tsc.js:110000-110021
    fn write_break_when_true(
        &mut self,
        label: Label,
        condition: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let instruction = self.create_instruction(Instruction::Break)?;
        let label_expression = self.create_label_expression(Some(label))?;
        let array = self.create_array_literal(vec![instruction, label_expression])?;
        let return_statement = self.create_return_statement(Some(array))?;
        let return_statement = self.set_text_range_opt(return_statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(return_statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        let if_statement = self.create_if_statement(condition, return_statement)?;
        self.context
            .arena_mut()?
            .metadata_mut(if_statement)
            .add_flags(EmitFlags::SINGLE_LINE);
        self.write_statement_node(Some(if_statement))
    }

    /// tsc-port: writeBreakWhenFalse @6.0.3
    /// tsc-hash: 4868ca165f53461bc3fbfa6f5ef241b4fcc194f6accacc30869445aecc78938d
    /// tsc-span: _tsc.js:110022-110043
    fn write_break_when_false(
        &mut self,
        label: Label,
        condition: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let instruction = self.create_instruction(Instruction::Break)?;
        let label_expression = self.create_label_expression(Some(label))?;
        let array = self.create_array_literal(vec![instruction, label_expression])?;
        let return_statement = self.create_return_statement(Some(array))?;
        let return_statement = self.set_text_range_opt(return_statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(return_statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        let negated = self.create_logical_not(condition)?;
        let if_statement = self.create_if_statement(negated, return_statement)?;
        self.context
            .arena_mut()?
            .metadata_mut(if_statement)
            .add_flags(EmitFlags::SINGLE_LINE);
        self.write_statement_node(Some(if_statement))
    }

    /// tsc-port: writeYield @6.0.3
    /// tsc-hash: ea2181b9bfd87b8deeb48ed7d593551f3edcca4aaabe1b30187fee1c42c011e1
    /// tsc-span: _tsc.js:110044-110059
    fn write_yield(
        &mut self,
        expression: Option<TransformNode>,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        let instruction = self.create_instruction(Instruction::Yield)?;
        let elements = match expression {
            Some(expression) => vec![instruction, expression],
            None => vec![instruction],
        };
        let array = self.create_array_literal(elements)?;
        let statement = self.create_return_statement(Some(array))?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeYieldStar @6.0.3
    /// tsc-hash: 413b341e49c5a376b92142b6272ca376f8e1f9e25e4fe7f8d50d3599dc7f5270
    /// tsc-span: _tsc.js:110060-110076
    fn write_yield_star(
        &mut self,
        expression: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        let instruction = self.create_instruction(Instruction::YieldStar)?;
        let array = self.create_array_literal(vec![instruction, expression])?;
        let statement = self.create_return_statement(Some(array))?;
        let statement = self.set_text_range_opt(statement, location)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
        self.write_statement_node(Some(statement))
    }

    /// tsc-port: writeEndfinally @6.0.3
    /// tsc-hash: ed16caf003c7e373dc4a655a284831c4cf3b8ca78843174ad4a5c34d76245efa
    /// tsc-span: _tsc.js:110077-110086
    fn write_endfinally(&mut self) -> Result<(), TransformError> {
        self.last_operation_was_abrupt = true;
        let instruction = self.create_instruction(Instruction::Endfinally)?;
        let array = self.create_array_literal(vec![instruction])?;
        let statement = self.create_return_statement(Some(array))?;
        self.write_statement_node(Some(statement))
    }

    /// `labelExpressions[label].push(expression)` — the placeholder ledger
    /// `updateLabelExpressions` finalizes.
    fn record_label_expression(&mut self, label: Label, expression: TransformNode) {
        if self.label_expressions.len() <= label {
            self.label_expressions.resize(label + 1, Vec::new());
        }
        self.label_expressions[label].push(expression);
    }

    /// tsc-port: createValuesHelper @6.0.3
    /// tsc-hash: 032848f776556d9246f0c0edb4f976921c507f9b47ad5fdcee88c17dc1b06688
    /// tsc-span: _tsc.js:25897-25905
    fn create_values_helper_call(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::values())?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Values)?;
        self.create_call(helper, vec![expression])
    }

    /// tsc-port: createGeneratorHelper @6.0.3
    /// tsc-hash: 08b82812765a67b6725d6639fd1251deeff5667d74dbfbf5debbcdb1a509454f
    /// tsc-span: _tsc.js:25915-25923
    fn create_generator_helper_call(
        &mut self,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::generator())?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Generator)?;
        let this_token = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        self.create_call(helper, vec![this_token, body])
    }
}

// ---------------------------------------------------------------------------
// Visit plumbing
// ---------------------------------------------------------------------------

impl NodeDataChildVisitor for GeneratorsVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.kind(id).unwrap_or(SyntaxKind::Unknown)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        self.visit_node_array(id)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl GeneratorsVisitor<'_, '_> {
    /// `visitEachChild(node, visitor, context)` — the generic descent. The
    /// generators visitor is STATEFUL (mode flags select dispatch), so
    /// there is deliberately NO per-node memoization (the es2018 memo map
    /// is not replicated).
    fn visit_each_child(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        let original = self.node(id);
        let mut data = self.context.arena().node(original)?.data.clone();
        try_visit_each_child(&mut data, self)?;
        if self.context.arena().node(original)?.data == data {
            return Ok(Some(id));
        }
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(Some(
            self.context
                .factory()?
                .update_node(original, data, flags)?
                .node(),
        ))
    }

    /// `visitEachChild` in statement position, returning the node handle.
    fn visit_each_child_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(self.visit_each_child(node.node())?.map(|id| self.node(id)))
    }

    fn visit_node_array(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
        let original = tsc_syntax_array(self.source, id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        Ok(Some(updated.array()))
    }

    /// `Debug.checkDefined(visitNode(node, visitor, isExpression))`.
    fn visit_required_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit(node.node())?.map(|id| self.node(id)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "visited expression",
            },
        )
    }

    /// `visitNode(node, visitor, isExpression)` — optional position.
    fn visit_expression_opt(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(self.visit(node.node())?.map(|id| self.node(id)))
    }

    /// `visitNode(node, visitor, isStatement)` — optional position.
    fn visit_statement_node(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(self.visit(node.node())?.map(|id| self.node(id)))
    }

    /// `visitNode(node, visitor, isStatement, factory2.liftToBlock)` — the
    /// single-result visitor makes the lift the identity.
    fn visit_statement_lifted(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit(node.node())?.map(|id| self.node(id)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForInStatement,
                field: "lifted statement",
            },
        )
    }

    /// tsc-port: visitIterationBody @6.0.3
    /// tsc-hash: b03d1c5c697121a89f1eb971763c4207e9dedffd27c6ac9545ac74a32d82f9bc
    /// tsc-span: _tsc.js:91291-91305
    ///
    /// The block-scope collection arms are inert on post-ES2015 input (no
    /// uncaptured block-scoped declarations reach the machine) and the
    /// single-result visitor makes `liftToBlock` the identity; the visit
    /// itself is the surviving semantics.
    fn visit_iteration_body(
        &mut self,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_statement_lifted(body)
    }

    /// tsc-port: visitParameterList @6.0.3
    /// tsc-hash: 75f4e96e0f53dac4523f71d86dc9a4216465c88b670afeb6202b7853fb27d8fa
    /// tsc-span: _tsc.js:91168-91181
    ///
    /// `startLexicalEnvironment` → visit parameters → `suspendLexicalEnvironment`;
    /// the default-value arm is gated `target >= ES2015` upstream and the
    /// machine's post-ES2015 inputs carry no parameter initializers.
    fn visit_parameter_list(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.context.start_lexical_environment()?;
        let updated = match parameters {
            Some(parameters) => self.visit_node_array(parameters)?,
            None => None,
        };
        self.context.suspend_lexical_environment()?;
        Ok(updated)
    }

    // -----------------------------------------------------------------------
    // Prologue
    // -----------------------------------------------------------------------

    /// tsc-port: copyPrologue @6.0.3
    /// tsc-hash: 9c233c2771a89af4d6e7767d7a30253c9b999d5c25ac9ed62be08aba4149dfdb
    /// tsc-span: _tsc.js:24827-24830
    fn copy_prologue(
        &mut self,
        source: &[TransformNode],
        target: &mut Vec<TransformNode>,
        ensure_use_strict: bool,
    ) -> Result<usize, TransformError> {
        let offset = self.copy_standard_prologue(source, target, 0, ensure_use_strict)?;
        self.copy_custom_prologue(source, target, offset)
    }

    /// tsc-port: copyStandardPrologue @6.0.3
    /// tsc-hash: 7a83f5b2d0bfada432bb729b16e41de52a8cb69e13f5bdb19f627d23e06607f4
    /// tsc-span: _tsc.js:24837-24857
    ///
    /// `ensureUseStrict` is `false` at the sole generator-body call site;
    /// the arm ports fail-closed dormant.
    fn copy_standard_prologue(
        &mut self,
        source: &[TransformNode],
        target: &mut Vec<TransformNode>,
        statement_offset: usize,
        ensure_use_strict: bool,
    ) -> Result<usize, TransformError> {
        if ensure_use_strict {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "ensureUseStrict prologue arm (dormant at the generator body)",
            });
        }
        let mut offset = statement_offset;
        while offset < source.len() {
            let statement = source[offset];
            if self.is_prologue_directive(statement)? {
                target.push(statement);
                offset += 1;
            } else {
                break;
            }
        }
        Ok(offset)
    }

    /// tsc-port: copyCustomPrologue @6.0.3
    /// tsc-hash: 31ebe86b6ab3451c7d9470915ce9e462a96be87c7b4aad217af7b41d2a2df664
    /// tsc-span: _tsc.js:24858-24870
    fn copy_custom_prologue(
        &mut self,
        source: &[TransformNode],
        target: &mut Vec<TransformNode>,
        statement_offset: usize,
    ) -> Result<usize, TransformError> {
        let mut offset = statement_offset;
        while offset < source.len() {
            let statement = source[offset];
            if self
                .emit_flags(statement)
                .contains(EmitFlags::CUSTOM_PROLOGUE)
            {
                let visited = self.visit_statement_node(statement)?;
                if let Some(visited) = visited {
                    target.push(visited);
                }
                offset += 1;
            } else {
                break;
            }
        }
        Ok(offset)
    }

    /// `insertStatementsAfterStandardPrologue(statements, declarations)` —
    /// the hoisted function/variable set lands after the directive
    /// prologue (the es2017/es2018 `prologue_end` cursor idiom).
    fn insert_statements_after_standard_prologue(
        &mut self,
        statements: &mut Vec<TransformNode>,
        environment: LexicalEnvironment,
    ) -> Result<(), TransformError> {
        let mut prologue_end = 0;
        while prologue_end < statements.len()
            && self.is_prologue_directive(statements[prologue_end])?
        {
            prologue_end += 1;
        }
        // The es2017/es2018 environment conversion: hoisted functions
        // first, then initialization statements, then ONE variable
        // statement of the hoisted names (CustomPrologue-flagged).
        if !environment.variable_declarations().is_empty() {
            let mut declarations = Vec::new();
            for name in environment.variable_declarations().iter().copied() {
                declarations.push(self.create_variable_declaration(name, None)?);
            }
            let list = self.create_variable_declaration_list(declarations)?;
            let statement = self.create_variable_statement_from_list(list)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.insert(prologue_end, statement);
        }
        if !environment.initialization_statements().is_empty() {
            statements.splice(
                prologue_end..prologue_end,
                environment.initialization_statements().iter().copied(),
            );
        }
        if !environment.function_declarations().is_empty() {
            statements.splice(
                prologue_end..prologue_end,
                environment.function_declarations().iter().copied(),
            );
        }
        Ok(())
    }

    fn create_variable_declaration(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut children = vec![name];
        children.extend(initializer);
        let flags = self.child_flags(&children)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
            }),
            flags,
        )
    }

    fn create_variable_declaration_list(
        &mut self,
        declarations: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let transform_flags = self.context.arena().array_transform_flags(declarations)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            transform_flags,
        )
    }

    fn create_variable_statement_from_list(
        &mut self,
        list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(list)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            flags,
        )
    }

    /// `isPrologueDirective` — an expression statement whose expression is
    /// a string literal.
    fn is_prologue_directive(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(false);
        };
        Ok(matches!(
            self.context.arena().node(self.node(expression))?.data,
            NodeData::StringLiteral(_)
        ))
    }

    // -----------------------------------------------------------------------
    // Call bindings
    // -----------------------------------------------------------------------

    /// tsc-port: createCallBinding @6.0.3
    /// tsc-hash: 445f6a3542132e1adf49e01683e039e6fa034bd127cd15ab5447db84951b41bc
    /// tsc-span: _tsc.js:24691-24753
    ///
    /// The two super arms are post-ES2015-unreachable at the machine's
    /// pipeline position (transformES2015 rewrites super positions first)
    /// and port fail-closed.
    fn create_call_binding(
        &mut self,
        expression: TransformNode,
        language_version: Option<ScriptTarget>,
        cache_identifiers: bool,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        let _ = language_version;
        let callee = self.skip_outer_expressions(expression)?;
        let callee_kind = self.context.arena().node(callee)?.kind;
        if matches!(callee_kind, SyntaxKind::SuperKeyword) || self.is_super_property(callee)? {
            return Err(TransformError::RequiredChildRemoved {
                parent: callee_kind,
                field: "super call binding (post-ES2015-unreachable)",
            });
        }
        if self.emit_flags(callee).contains(EmitFlags::HELPER_NAME) {
            let this_arg = self.create_void_zero()?;
            // `parenthesizeLeftSideOfAccess` is factory-automatic at the
            // consuming `createPropertyAccessExpression` (`.apply`).
            return Ok((callee, this_arg));
        }
        match self.context.arena().node(callee)?.data.clone() {
            NodeData::PropertyAccessExpression(data) => {
                let receiver = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "expression",
                    })?;
                let receiver = self.node(receiver);
                if self.should_be_captured_in_temp_variable(receiver, cache_identifiers)? {
                    let this_arg = self.create_hoisted_temp_reference()?;
                    let assignment = self.create_assignment(this_arg, receiver)?;
                    let assignment = self
                        .context
                        .factory()?
                        .set_text_range(assignment, receiver)?;
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "name",
                    })?;
                    let target = self.create_property_access(assignment, self.node(name))?;
                    let target = self.context.factory()?.set_text_range(target, callee)?;
                    Ok((target, this_arg))
                } else {
                    Ok((callee, receiver))
                }
            }
            NodeData::ElementAccessExpression(data) => {
                let receiver = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ElementAccessExpression,
                        field: "expression",
                    })?;
                let receiver = self.node(receiver);
                if self.should_be_captured_in_temp_variable(receiver, cache_identifiers)? {
                    let this_arg = self.create_hoisted_temp_reference()?;
                    let assignment = self.create_assignment(this_arg, receiver)?;
                    let assignment = self
                        .context
                        .factory()?
                        .set_text_range(assignment, receiver)?;
                    let argument =
                        data.argument_expression
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ElementAccessExpression,
                                field: "argumentExpression",
                            })?;
                    let target = self.create_element_access(assignment, self.node(argument))?;
                    let target = self.context.factory()?.set_text_range(target, callee)?;
                    Ok((target, this_arg))
                } else {
                    Ok((callee, receiver))
                }
            }
            _ => {
                let this_arg = self.create_void_zero()?;
                Ok((expression, this_arg))
            }
        }
    }

    /// `createTempVariable(recordTempVariable)` where the recorder is
    /// `hoistVariableDeclaration` — the callBinding temp arms.
    fn create_hoisted_temp_reference(&mut self) -> Result<TransformNode, TransformError> {
        let binding = self.allocate_temp_binding()?;
        let hoist_target = self.create_generated_identifier(&binding)?;
        self.context.hoist_variable_declaration(hoist_target)?;
        self.create_generated_identifier(&binding)
    }

    /// tsc-port: shouldBeCapturedInTempVariable @6.0.3
    /// tsc-hash: 930638d4e30da0491d0c7e2612bf2920f6280413e771880f72c3c18f6712baf0
    /// tsc-span: _tsc.js:24669-24690
    fn should_be_captured_in_temp_variable(
        &self,
        node: TransformNode,
        cache_identifiers: bool,
    ) -> Result<bool, TransformError> {
        let target = self.skip_parentheses(node)?;
        Ok(match &self.context.arena().node(target)?.data {
            NodeData::Identifier(_) => cache_identifiers,
            NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::StringLiteral(_) => false,
            _ if self.context.arena().node(target)?.kind == SyntaxKind::ThisKeyword => false,
            NodeData::ArrayLiteralExpression(data) => {
                let elements = self.array_nodes_of(data.elements)?;
                !elements.is_empty()
            }
            NodeData::ObjectLiteralExpression(data) => {
                let properties = self.array_nodes_of(data.properties)?;
                !properties.is_empty()
            }
            _ => true,
        })
    }

    /// tsc-port: skipOuterExpressions @6.0.3
    /// tsc-hash: 8b1eff7c004dde6bbe6b5940ba064195f1aea6668ca5d8b1f4a69bf9cec4dec1
    /// tsc-span: _tsc.js:27582-27587
    ///
    /// `OuterExpressionKinds.All` over the machine's JS-shaped inputs:
    /// parenthesized expressions and partially-emitted expressions (the
    /// TypeScript assertion kinds are stripped before this pass).
    fn skip_outer_expressions(&self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let mut node = node;
        loop {
            match &self.context.arena().node(node)?.data {
                NodeData::ParenthesizedExpression(data) => {
                    let Some(expression) = data.expression else {
                        return Ok(node);
                    };
                    node = self.node(expression);
                }
                NodeData::PartiallyEmittedExpression(data) => {
                    let Some(expression) = data.expression else {
                        return Ok(node);
                    };
                    node = self.node(expression);
                }
                _ => return Ok(node),
            }
        }
    }

    /// `skipParentheses` — the `shouldBeCapturedInTempVariable` unwrap.
    fn skip_parentheses(&self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let mut node = node;
        while let NodeData::ParenthesizedExpression(data) = &self.context.arena().node(node)?.data {
            let Some(expression) = data.expression else {
                return Ok(node);
            };
            node = self.node(expression);
        }
        Ok(node)
    }

    fn is_super_property(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::PropertyAccessExpression(data) => data.expression.is_some_and(|expression| {
                self.kind(expression)
                    .map(|kind| kind == SyntaxKind::SuperKeyword)
                    .unwrap_or(false)
            }),
            NodeData::ElementAccessExpression(data) => data.expression.is_some_and(|expression| {
                self.kind(expression)
                    .map(|kind| kind == SyntaxKind::SuperKeyword)
                    .unwrap_or(false)
            }),
            _ => false,
        })
    }

    /// tsc-port: isImportCall @6.0.3
    /// tsc-hash: 74cfad37d8ed5b905210a6398b89c2c9f89f42600024f1749a0c92ab8c6c11f1
    /// tsc-span: _tsc.js:14150-14154
    fn is_import_call(&self, id: NodeId) -> Result<bool, TransformError> {
        let NodeData::CallExpression(data) = self.data(id)? else {
            return Ok(false);
        };
        Ok(data
            .expression
            .map(|expression| self.kind(expression))
            .transpose()?
            == Some(SyntaxKind::ImportKeyword))
    }

    // -----------------------------------------------------------------------
    // Object-literal element expressions
    // -----------------------------------------------------------------------

    /// tsc-port: createExpressionForObjectLiteralElementLike @6.0.3
    /// tsc-hash: fa28bb1dbba197796435533109e6e363d16c2e025051702a2212f763301e34b3
    /// tsc-span: _tsc.js:27483-27498
    fn create_expression_for_object_literal_element_like(
        &mut self,
        object: TransformNode,
        property: TransformNode,
        receiver: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        // `property.name && isPrivateIdentifier(property.name)` is the
        // upstream `Debug.failBadSyntaxKind` fault.
        if let Some(name) = self.property_name_of(property)? {
            if matches!(
                self.context.arena().node(self.node(name))?.data,
                NodeData::PrivateIdentifier(_)
            ) {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PrivateIdentifier,
                    field: "object-literal private identifier",
                });
            }
        }
        match self.context.arena().node(property)?.kind {
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                self.create_expression_for_accessor_declaration(object, property, receiver)
            }
            SyntaxKind::PropertyAssignment => self
                .create_expression_for_property_assignment(property, receiver)
                .map(Some),
            SyntaxKind::ShorthandPropertyAssignment => self
                .create_expression_for_shorthand_property_assignment(property, receiver)
                .map(Some),
            SyntaxKind::MethodDeclaration => self
                .create_expression_for_method_declaration(property, receiver)
                .map(Some),
            other => Err(TransformError::RequiredChildRemoved {
                parent: other,
                field: "object literal element",
            }),
        }
    }

    /// tsc-port: createExpressionForAccessorDeclaration @6.0.3
    /// tsc-hash: f7a4fc78ae9810764bc7643a09f2468c93573afa4573451dceda8b4adbce250b
    /// tsc-span: _tsc.js:27348-27404
    fn create_expression_for_accessor_declaration(
        &mut self,
        object: TransformNode,
        accessor: TransformNode,
        receiver: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let multi_line = self.node_is_multi_line(object)?;
        let pair = self.get_all_accessor_declarations(object, accessor)?;
        // Only the FIRST accessor position materializes the pair.
        if pair.first_accessor != accessor {
            return Ok(None);
        }
        // `createPropertyDescriptor({enumerable, configurable, get, set},
        // !multiLine)` — row order fixed; the descriptor literal's
        // multi-line-ness follows the containing object literal.
        let mut properties: Vec<TransformNode> = Vec::new();
        let enumerable_name = self.create_identifier("enumerable")?;
        let enumerable_value = self.create_false()?;
        properties.push(self.create_property_assignment(enumerable_name, enumerable_value)?);
        let configurable_name = self.create_identifier("configurable")?;
        let configurable_value = self.create_true()?;
        properties.push(self.create_property_assignment(configurable_name, configurable_value)?);
        if let Some(get_accessor) = pair.get_accessor {
            let function = self.accessor_to_function_expression(get_accessor)?;
            let get_name = self.create_identifier("get")?;
            properties.push(self.create_property_assignment(get_name, function)?);
        }
        if let Some(set_accessor) = pair.set_accessor {
            let function = self.accessor_to_function_expression(set_accessor)?;
            let set_name = self.create_identifier("set")?;
            properties.push(self.create_property_assignment(set_name, function)?);
        }
        let descriptor = self.create_object_literal_multi_line(properties, multi_line)?;
        let name =
            self.property_name_of(accessor)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::GetAccessor,
                    field: "accessor name",
                })?;
        let name_expression = self.expression_for_property_name(self.node(name))?;
        // `createObjectDefinePropertyCall` — the global-method call.
        let object_name = self.create_identifier("Object")?;
        let define_property_name = self.create_identifier("defineProperty")?;
        let define_property = self.create_property_access(object_name, define_property_name)?;
        let call =
            self.create_call(define_property, vec![receiver, name_expression, descriptor])?;
        // `setTextRange(call, firstAccessor)` — range only, no original.
        let call = self
            .context
            .factory()?
            .set_text_range(call, pair.first_accessor)?;
        Ok(Some(call))
    }

    /// tsc-port: createExpressionForPropertyAssignment @6.0.3
    /// tsc-hash: d875847e6dfe5a88cdd180d4c2d247f9e3065648496c2ee42cdb8d2ec08e27db
    /// tsc-span: _tsc.js:27405-27422
    fn create_expression_for_property_assignment(
        &mut self,
        property: TransformNode,
        receiver: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::PropertyAssignment(data) = self.context.arena().node(property)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAssignment,
                field: "property assignment data",
            });
        };
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyAssignment,
            field: "name",
        })?;
        let initializer = data
            .initializer
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAssignment,
                field: "initializer",
            })?;
        let access = self.create_member_access_for_property_name(
            receiver,
            self.node(name),
            Some(self.node(name)),
        )?;
        let assignment = self.create_assignment(access, self.node(initializer))?;
        let assignment = self
            .context
            .factory()?
            .set_text_range(assignment, property)?;
        self.context
            .arena_mut()?
            .set_original_node(assignment, Some(property))?;
        Ok(assignment)
    }

    /// tsc-port: createExpressionForShorthandPropertyAssignment @6.0.3
    /// tsc-hash: 177d276035c1a4c120f6d4ca82554cfc7af50a06c72125186664c9bf0ead0c4a
    /// tsc-span: _tsc.js:27423-27442
    ///
    /// Post-ES2015-dormant (shorthand is lowered before the machine);
    /// ported faithfully and unit-driven.
    fn create_expression_for_shorthand_property_assignment(
        &mut self,
        property: TransformNode,
        receiver: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ShorthandPropertyAssignment(data) =
            self.context.arena().node(property)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ShorthandPropertyAssignment,
                field: "shorthand data",
            });
        };
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ShorthandPropertyAssignment,
            field: "name",
        })?;
        let access = self.create_member_access_for_property_name(
            receiver,
            self.node(name),
            Some(self.node(name)),
        )?;
        let name_node = self.node(name);
        let clone = self.context.factory()?.clone_node(name_node)?;
        let assignment = self.create_assignment(access, clone)?;
        let assignment = self
            .context
            .factory()?
            .set_text_range(assignment, property)?;
        self.context
            .arena_mut()?
            .set_original_node(assignment, Some(property))?;
        Ok(assignment)
    }

    /// tsc-port: createExpressionForMethodDeclaration @6.0.3
    /// tsc-hash: bd3aa684f7597a8b6f7df3c0671f980501e141759c75a5a4c79dcc9ccb281fb5
    /// tsc-span: _tsc.js:27443-27482
    ///
    /// Post-ES2015-dormant (object methods are lowered before the
    /// machine); ported faithfully and unit-driven.
    fn create_expression_for_method_declaration(
        &mut self,
        method: TransformNode,
        receiver: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::MethodDeclaration(data) = self.context.arena().node(method)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::MethodDeclaration,
                field: "method data",
            });
        };
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::MethodDeclaration,
            field: "name",
        })?;
        let parameters = self.array_nodes_of(data.parameters)?;
        let body = data.body.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::MethodDeclaration,
            field: "body",
        })?;
        let function = self.create_function_expression_full(
            data.modifiers,
            data.asterisk_token.map(|token| self.node(token)),
            None,
            parameters,
            self.node(body),
        )?;
        let function = self.context.factory()?.set_text_range(function, method)?;
        self.context
            .arena_mut()?
            .set_original_node(function, Some(method))?;
        let access = self.create_member_access_for_property_name(
            receiver,
            self.node(name),
            Some(self.node(name)),
        )?;
        let assignment = self.create_assignment(access, function)?;
        let assignment = self.context.factory()?.set_text_range(assignment, method)?;
        self.context
            .arena_mut()?
            .set_original_node(assignment, Some(method))?;
        Ok(assignment)
    }

    /// tsc-port: createMemberAccessForPropertyName @6.0.3
    /// tsc-hash: 88b490bf2cd47503f62314d8fc5fb1c7bca83df86aae8890df643915162ce392
    /// tsc-span: _tsc.js:27206-27217
    fn create_member_access_for_property_name(
        &mut self,
        target: TransformNode,
        member_name: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if let NodeData::ComputedPropertyName(data) = &self.context.arena().node(member_name)?.data
        {
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let expression = self.node(expression);
            let access = self.create_element_access(target, expression)?;
            return self.set_text_range_opt(access, location);
        }
        // Non-computed names are REUSED DIRECTLY (no clone); the resulting
        // ACCESS is ranged to the member name and takes NoNestedSourceMaps.
        let access = match &self.context.arena().node(member_name)?.data {
            NodeData::Identifier(_) => self.create_property_access(target, member_name)?,
            NodeData::StringLiteral(_) | NodeData::NumericLiteral(_) => {
                self.create_element_access(target, member_name)?
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(member_name)?.kind,
                    field: "member property name",
                })
            }
        };
        let access = self
            .context
            .factory()?
            .set_text_range(access, member_name)?;
        self.context
            .arena_mut()?
            .metadata_mut(access)
            .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
        Ok(access)
    }
}

// ---------------------------------------------------------------------------
// Accessor pairs
// ---------------------------------------------------------------------------

struct AccessorPair {
    first_accessor: TransformNode,
    get_accessor: Option<TransformNode>,
    set_accessor: Option<TransformNode>,
}

impl GeneratorsVisitor<'_, '_> {
    /// tsc-port: getAllAccessorDeclarations @6.0.3
    /// tsc-hash: 8e23b58d85c286c6344992bac81b90a2c92285508dcf40a9c80d316dca13286a
    /// tsc-span: _tsc.js:16719-16760
    ///
    /// Object-literal accessors only (`isStatic` is uniformly false there);
    /// the class arms arrive with their owners.
    fn get_all_accessor_declarations(
        &self,
        object: TransformNode,
        accessor: TransformNode,
    ) -> Result<AccessorPair, TransformError> {
        let accessor_kind = self.context.arena().node(accessor)?.kind;
        if self.has_dynamic_name(accessor)? {
            return Ok(AccessorPair {
                first_accessor: accessor,
                get_accessor: (accessor_kind == SyntaxKind::GetAccessor).then_some(accessor),
                set_accessor: (accessor_kind == SyntaxKind::SetAccessor).then_some(accessor),
            });
        }
        let NodeData::ObjectLiteralExpression(data) =
            self.context.arena().node(object)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ObjectLiteralExpression,
                field: "accessor container",
            });
        };
        let accessor_name =
            self.property_name_text(accessor)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: accessor_kind,
                    field: "accessor property name",
                })?;
        let mut first_accessor: Option<TransformNode> = None;
        let mut get_accessor: Option<TransformNode> = None;
        let mut set_accessor: Option<TransformNode> = None;
        for member in self.array_nodes_of(data.properties)? {
            let member_kind = self.context.arena().node(member)?.kind;
            if !matches!(
                member_kind,
                SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
            ) {
                continue;
            }
            let Some(member_name) = self.property_name_text(member)? else {
                continue;
            };
            if member_name != accessor_name {
                continue;
            }
            if first_accessor.is_none() {
                first_accessor = Some(member);
            }
            if member_kind == SyntaxKind::GetAccessor && get_accessor.is_none() {
                get_accessor = Some(member);
            }
            if member_kind == SyntaxKind::SetAccessor && set_accessor.is_none() {
                set_accessor = Some(member);
            }
        }
        Ok(AccessorPair {
            first_accessor: first_accessor.ok_or(TransformError::RequiredChildRemoved {
                parent: accessor_kind,
                field: "first accessor",
            })?,
            get_accessor,
            set_accessor,
        })
    }

    /// tsc-port: hasDynamicName @6.0.3
    /// tsc-hash: d126787bc1b36621098ed5255c26d1e27abe5bf6dbc55570657aa03f95a588bb
    /// tsc-span: _tsc.js:15850-15853
    fn has_dynamic_name(&self, declaration: TransformNode) -> Result<bool, TransformError> {
        let Some(name) = self.property_name_of(declaration)? else {
            return Ok(false);
        };
        let NodeData::ComputedPropertyName(data) =
            &self.context.arena().node(self.node(name))?.data
        else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(true);
        };
        let expression = self.skip_parentheses(self.node(expression))?;
        Ok(!self.is_string_or_numeric_literal_like(expression)?
            && !self.is_signed_numeric_literal(expression)?)
    }

    /// tsc-port: getPropertyNameForPropertyNameNode @6.0.3
    /// tsc-hash: 5770eff9fe2f071f83fce9a7aaff9c54fa6f09141154c33c0f7f3e5dc86ee117
    /// tsc-span: _tsc.js:15861-15887
    fn property_name_text(&self, member: TransformNode) -> Result<Option<String>, TransformError> {
        let Some(name) = self.property_name_of(member)? else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(self.node(name))?.data {
            NodeData::Identifier(data) => Some(data.escaped_text.clone()),
            NodeData::StringLiteral(data) => Some(escape_leading_underscores_owned(&data.text)),
            NodeData::NumericLiteral(data) => Some(escape_leading_underscores_owned(&data.text)),
            NodeData::BigIntLiteral(data) => Some(escape_leading_underscores_owned(&data.text)),
            NodeData::ComputedPropertyName(data) => {
                let Some(expression) = data.expression else {
                    return Ok(None);
                };
                let expression = self.node(expression);
                match &self.context.arena().node(expression)?.data {
                    NodeData::StringLiteral(data) => {
                        Some(escape_leading_underscores_owned(&data.text))
                    }
                    NodeData::NumericLiteral(data) => {
                        Some(escape_leading_underscores_owned(&data.text))
                    }
                    _ => None,
                }
            }
            _ => None,
        })
    }

    /// The declaration-name accessor shared by the accessor pair walk and
    /// the private-identifier fault gate.
    fn property_name_of(&self, member: TransformNode) -> Result<Option<NodeId>, TransformError> {
        Ok(match &self.context.arena().node(member)?.data {
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        })
    }

    /// tsc-port: createExpressionForPropertyName @6.0.3
    /// tsc-hash: fc486b593b709b18b266695eed3d95c48147033188cb4fc1c3b0f2a658b8a51d
    /// tsc-span: _tsc.js:27339-27347
    fn expression_for_property_name(
        &mut self,
        member_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(member_name)?.data.clone() {
            NodeData::Identifier(data) => {
                // `createStringLiteralFromNode` — the literal renders with
                // the identifier as its text source.
                let literal = self.create_string_literal(&data.text)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(literal)
                    .set_string_literal_text_source(member_name);
                Ok(literal)
            }
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })?;
                let expression = self.node(expression);
                let clone = self.context.factory()?.clone_node(expression)?;
                self.context.factory()?.set_text_range(clone, expression)
            }
            _ => {
                let clone = self.clone_property_name_literal(member_name)?;
                self.context.factory()?.set_text_range(clone, member_name)
            }
        }
    }

    /// The accessor bodies of `createExpressionForAccessorDeclaration`:
    /// `createFunctionExpression(getModifiers(acc), …, acc.parameters, …,
    /// acc.body)` + original/range.
    fn accessor_to_function_expression(
        &mut self,
        accessor: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (modifiers, parameters, body) = match self.context.arena().node(accessor)?.data.clone()
        {
            NodeData::GetAccessor(data) => (data.modifiers, data.parameters, data.body),
            NodeData::SetAccessor(data) => (data.modifiers, data.parameters, data.body),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(accessor)?.kind,
                    field: "accessor declaration",
                })
            }
        };
        let body = body.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::GetAccessor,
            field: "body",
        })?;
        let parameters = self.array_nodes_of(parameters)?;
        let function = self.create_function_expression_full(
            modifiers,
            None,
            None,
            parameters,
            self.node(body),
        )?;
        let function = self.context.factory()?.set_text_range(function, accessor)?;
        self.context
            .arena_mut()?
            .set_original_node(function, Some(accessor))?;
        Ok(function)
    }

    // -----------------------------------------------------------------------
    // Binding allocation (the E-NAMES-H eager model)
    // -----------------------------------------------------------------------

    /// `createTempVariable` — ordinary `_a` family.
    fn allocate_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
        let provisional = self.generated_bindings.allocate_temp();
        TargetBinding::allocate(self.context, provisional)
    }

    /// `createUniqueName(text)` — the `text_1` family (`declareLocal` with
    /// a source name; catch renames).
    fn allocate_numbered_binding(&mut self, text: &str) -> Result<TargetBinding, TransformError> {
        let provisional = self.generated_bindings.allocate_numbered(text);
        TargetBinding::allocate_numbered(self.context, text.to_owned(), provisional)
    }

    /// `createLoopVariable` — the `_i` family (first production caller of
    /// the B-1-landed allocator).
    fn allocate_loop_variable_binding(&mut self) -> Result<TargetBinding, TransformError> {
        let provisional = self
            .generated_bindings
            .allocate_loop_variable(/*reserve_in_nested_scopes*/ false);
        // Planned-authoritative: the finalize walk keeps the `_i`-family
        // spelling verbatim (the B-4 collision lattice is its owner's
        // concern; no B-3 fixture occupies the family).
        TargetBinding::allocate_planned(self.context, provisional)
    }

    fn create_generated_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(binding.provisional_name())?;
        binding.write_generated_metadata(self.context.arena_mut()?, identifier);
        Ok(identifier)
    }

    /// The state temp's reference identifier.
    fn state_reference(&mut self) -> Result<TransformNode, TransformError> {
        let binding = self
            .state
            .clone()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionExpression,
                field: "generator state binding",
            })?;
        self.create_generated_identifier(&binding)
    }

    // -----------------------------------------------------------------------
    // Node constructors
    // -----------------------------------------------------------------------

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text: text.to_owned(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn clone_property_name_literal(
        &mut self,
        property_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let clone = self.context.factory()?.clone_node(property_name)?;
        if self.context.arena().node(property_name)?.kind == SyntaxKind::StringLiteral {
            self.context
                .arena_mut()?
                .metadata_mut(clone)
                .set_string_literal_text_source(property_name);
        }
        Ok(clone)
    }

    fn create_numeric_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_true(&mut self) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context
            .factory()?
            .create_token(source, SyntaxKind::TrueKeyword, TransformFlags::NONE)
    }

    fn create_false(&mut self) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context
            .factory()?
            .create_token(source, SyntaxKind::FalseKeyword, TransformFlags::NONE)
    }

    fn create_omitted_expression(&mut self) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::OmittedExpression(tsc_syntax::nodes::OmittedExpressionData {}),
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(zero)?;
        self.context.factory()?.create_node(
            source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            flags,
        )
    }

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_array_literal_multi_line(elements, false)
    }

    fn create_array_literal_multi_line(
        &mut self,
        elements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        // `createArrayLiteralExpression` adds the implicit trailing comma
        // when the LAST element is an omitted expression (`[,]`).
        let trailing_hole = match elements.last() {
            Some(last) => matches!(
                self.context.arena().node(*last)?.data,
                NodeData::OmittedExpression(_)
            ),
            None => false,
        };
        let elements = self
            .context
            .factory()?
            .create_node_array_with_trailing_comma(source, elements, trailing_hole)?;
        let flags = self.context.arena().array_transform_flags(elements);
        let literal = self.context.factory()?.create_node(
            source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            flags,
        )?;
        let literal = self.context.factory()?.set_multi_line(literal, multi_line)?;
        Ok(literal)
    }

    fn create_object_literal_multi_line(
        &mut self,
        properties: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let properties = self
            .context
            .factory()?
            .create_node_array(source, properties)?;
        let flags = self.context.arena().array_transform_flags(properties);
        let literal = self.context.factory()?.create_node(
            source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            flags,
        )?;
        let literal = self.context.factory()?.set_multi_line(literal, multi_line)?;
        Ok(literal)
    }

    fn create_property_assignment(
        &mut self,
        name: TransformNode,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[name, initializer])?;
        self.context.factory()?.create_node(
            source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(initializer.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            flags,
        )
    }

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression, name])?;
        self.context.factory()?.create_node(
            source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                name: Some(name.node()),
            }),
            flags,
        )
    }

    fn create_element_access(
        &mut self,
        expression: TransformNode,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression, argument])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                argument_expression: Some(argument.node()),
            }),
            flags,
        )
    }

    fn create_call(
        &mut self,
        callee: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let arguments = self
            .context
            .factory()?
            .create_node_array(source, arguments)?;
        let flags = self.context.arena().propagate_child_flags(callee)?
            | self.context.arena().array_transform_flags(arguments);
        self.context.factory()?.create_node(
            source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(callee.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            flags,
        )
    }

    fn create_new_expression(
        &mut self,
        callee: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let arguments = self
            .context
            .factory()?
            .create_node_array(source, arguments)?;
        let flags = self.context.arena().propagate_child_flags(callee)?
            | self.context.arena().array_transform_flags(arguments);
        self.context.factory()?.create_node(
            source,
            NodeData::NewExpression(tsc_syntax::nodes::NewExpressionData {
                expression: Some(callee.node()),
                type_arguments: None,
                arguments: Some(arguments.array()),
                question_dot_token: None,
            }),
            flags,
        )
    }

    /// `createFunctionApplyCall(target, thisArg, argumentsExpression)` —
    /// `target.apply(thisArg, args)`.
    fn create_function_apply_call(
        &mut self,
        target: TransformNode,
        this_arg: TransformNode,
        arguments_expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let apply = self.create_identifier("apply")?;
        let access = self.create_property_access(target, apply)?;
        self.create_call(access, vec![this_arg, arguments_expression])
    }

    /// `createArrayConcatCall(array, values)` — `array.concat(...)`.
    fn create_array_concat_call(
        &mut self,
        array: TransformNode,
        values: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let concat = self.create_identifier("concat")?;
        let access = self.create_property_access(array, concat)?;
        self.create_call(access, values)
    }

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let operator_token =
            self.context
                .factory()?
                .create_token(source, operator, TransformFlags::NONE)?;
        let flags = self.child_flags(&[left, operator_token, right])?;
        self.context.factory()?.create_node(
            source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator_token.node()),
                right: Some(right.node()),
            }),
            flags,
        )
    }

    /// `createLessThan(left, right)`.
    fn create_less_than(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::LessThanToken, right)
    }

    /// `createPostfixIncrement(operand)`.
    fn create_postfix_increment(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(operand)?;
        self.context.factory()?.create_node(
            source,
            NodeData::PostfixUnaryExpression(tsc_syntax::nodes::PostfixUnaryExpressionData {
                operand: Some(operand.node()),
                operator: SyntaxKind::PlusPlusToken,
            }),
            flags,
        )
    }

    /// `createLogicalNot(operand)`.
    fn create_logical_not(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(operand)?;
        self.context.factory()?.create_node(
            source,
            NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                operator: SyntaxKind::ExclamationToken,
                operand: Some(operand.node()),
            }),
            flags,
        )
    }

    /// `inlineExpressions(expressions)` — the left-fold comma chain.
    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut iterator = expressions.into_iter();
        let mut result = iterator
            .next()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "inlined expressions",
            })?;
        for expression in iterator {
            result = self.create_binary(result, SyntaxKind::CommaToken, expression)?;
        }
        Ok(result)
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_return_statement(
        &mut self,
        expression: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = match expression {
            Some(expression) => self.context.arena().propagate_child_flags(expression)?,
            None => TransformFlags::NONE,
        };
        self.context.factory()?.create_node(
            source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: expression.map(TransformNode::node),
            }),
            flags,
        )
    }

    fn create_throw_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            source,
            NodeData::ThrowStatement(tsc_syntax::nodes::ThrowStatementData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_if_statement(
        &mut self,
        expression: TransformNode,
        then_statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression, then_statement])?;
        self.context.factory()?.create_node(
            source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(expression.node()),
                then_statement: Some(then_statement.node()),
                else_statement: None,
            }),
            flags,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_block_multi_line(statements, false)
    }

    fn create_block_multi_line(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let statements = self
            .context
            .factory()?
            .create_node_array(source, statements)?;
        let flags = self.context.arena().array_transform_flags(statements);
        let block = self.context.factory()?.create_node(
            source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            flags,
        )?;
        let block = self.context.factory()?.set_multi_line(block, multi_line)?;
        Ok(block)
    }

    fn create_with_statement(
        &mut self,
        expression: TransformNode,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression, statement])?;
        self.context.factory()?.create_node(
            source,
            NodeData::WithStatement(tsc_syntax::nodes::WithStatementData {
                expression: Some(expression.node()),
                statement: Some(statement.node()),
            }),
            flags,
        )
    }

    fn create_for_in(
        &mut self,
        initializer: TransformNode,
        expression: TransformNode,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[initializer, expression, statement])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ForInStatement(tsc_syntax::nodes::ForInStatementData {
                statement: Some(statement.node()),
                initializer: Some(initializer.node()),
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_case_clause(
        &mut self,
        expression: TransformNode,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let statements = self
            .context
            .factory()?
            .create_node_array(source, statements)?;
        let flags = self.context.arena().propagate_child_flags(expression)?
            | self.context.arena().array_transform_flags(statements);
        self.context.factory()?.create_node(
            source,
            NodeData::CaseClause(tsc_syntax::nodes::CaseClauseData {
                expression: Some(expression.node()),
                statements: Some(statements.array()),
            }),
            flags,
        )
    }

    fn create_case_block(
        &mut self,
        clauses: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let clauses = self.context.factory()?.create_node_array(source, clauses)?;
        let flags = self.context.arena().array_transform_flags(clauses);
        self.context.factory()?.create_node(
            source,
            NodeData::CaseBlock(tsc_syntax::nodes::CaseBlockData {
                clauses: Some(clauses.array()),
            }),
            flags,
        )
    }

    fn create_switch_statement(
        &mut self,
        expression: TransformNode,
        case_block: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression, case_block])?;
        self.context.factory()?.create_node(
            source,
            NodeData::SwitchStatement(tsc_syntax::nodes::SwitchStatementData {
                expression: Some(expression.node()),
                case_block: Some(case_block.node()),
            }),
            flags,
        )
    }

    fn create_parameter(&mut self, name: TransformNode) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.context.arena().propagate_child_flags(name)?;
        self.context.factory()?.create_node(
            source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            flags,
        )
    }

    /// The `__generator` callback shell.
    fn create_function_expression(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let parameters = self
            .context
            .factory()?
            .create_node_array(source, parameters)?;
        let flags = self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )
    }

    /// The method/accessor conversion shell (modifiers/asterisk threaded).
    fn create_function_expression_full(
        &mut self,
        modifiers: Option<NodeArrayId>,
        asterisk_token: Option<TransformNode>,
        name: Option<TransformNode>,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let parameters = self
            .context
            .factory()?
            .create_node_array(source, parameters)?;
        let flags = self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: name.map(TransformNode::node),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: asterisk_token.map(TransformNode::node),
                body: Some(body.node()),
                modifiers,
            }),
            flags,
        )
    }

    // -----------------------------------------------------------------------
    // Small helpers
    // -----------------------------------------------------------------------

    fn optional_array_flags(&self, array: Option<NodeArrayId>) -> TransformFlags {
        array
            .and_then(|array| self.context.arena().node_array_ref(self.source, array))
            .map(|array| self.context.arena().array_transform_flags(array))
            .unwrap_or(TransformFlags::NONE)
    }

    fn kind_of(&self, node: TransformNode) -> Result<SyntaxKind, TransformError> {
        Ok(self.context.arena().node(node)?.kind)
    }

    /// `visitNodes2(properties, visitor, isObjectLiteralElementLike, 0, n)`
    /// — the initial-chunk property visit (undefined results drop).
    fn visit_object_literal_element(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit(node.node())?.map(|id| self.node(id)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ObjectLiteralExpression,
                field: "object literal element",
            },
        )
    }

    fn child_flags(&self, nodes: &[TransformNode]) -> Result<TransformFlags, TransformError> {
        let mut flags = TransformFlags::NONE;
        for node in nodes {
            flags |= self.context.arena().propagate_child_flags(*node)?;
        }
        Ok(flags)
    }

    fn emit_flags(&self, node: TransformNode) -> EmitFlags {
        self.context
            .arena()
            .metadata(node)
            .map(|metadata| metadata.flags())
            .unwrap_or(EmitFlags::NONE)
    }

    fn start_on_new_line(&mut self, node: TransformNode) -> Result<(), TransformError> {
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_starts_on_new_line(true);
        Ok(())
    }

    fn set_text_range_opt(
        &mut self,
        node: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        match location {
            Some(location) => self.context.factory()?.set_text_range(node, location),
            None => Ok(node),
        }
    }

    fn set_source_map_range_from(
        &mut self,
        node: TransformNode,
        range_source: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(range) = self.effective_source_range(range_source)? {
            self.context
                .arena_mut()?
                .metadata_mut(node)
                .set_source_map_range(SourceMapRange::new(range_source.source(), range));
        }
        Ok(())
    }

    fn set_comment_range_from(
        &mut self,
        node: TransformNode,
        range_source: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(range) = self.effective_source_range(range_source)? {
            self.context
                .arena_mut()?
                .metadata_mut(node)
                .set_comment_range(CommentRange::new(range_source.source(), range));
        }
        Ok(())
    }

    /// The raw source range of a node when it has one (synthesized nodes
    /// without ranges contribute nothing — the upstream ranges on those
    /// sites are the synthetic sentinels).
    fn effective_source_range(
        &self,
        node: TransformNode,
    ) -> Result<Option<SourceRange>, TransformError> {
        let record = self.context.arena().node(node)?;
        let source = self.context.arena().source(node.source())?.syntax();
        match SourceRange::from_raw(record.pos, record.end, source.positions()) {
            Ok(range @ SourceRange::Original(_)) => Ok(Some(range)),
            _ => Ok(None),
        }
    }

    /// `node.multiLine` — the parser/factory decision carried on the node
    /// record (the printer's own read, printer.rs block arm).
    fn node_is_multi_line(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(self.context.arena().node(node)?.multi_line == Some(true))
    }

    fn identifier_text(&self, node: TransformNode) -> Result<String, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Ok(data.text.clone()),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(node)?.kind,
                field: "identifier",
            }),
        }
    }

    fn statement_label_text(&self, node: TransformNode) -> Result<Option<String>, TransformError> {
        let label = match &self.context.arena().node(node)?.data {
            NodeData::BreakStatement(data) => data.label,
            NodeData::ContinueStatement(data) => data.label,
            _ => None,
        };
        label
            .map(|label| self.identifier_text(self.node(label)))
            .transpose()
    }

    fn return_expression(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::ReturnStatement(data) = &self.context.arena().node(node)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ReturnStatement,
                field: "return statement",
            });
        };
        Ok(data.expression.map(|expression| self.node(expression)))
    }

    fn declaration_name(&self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "variable declaration",
            });
        };
        data.name
            .map(|name| self.node(name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "name",
            })
    }

    fn declaration_initializer(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "variable declaration",
            });
        };
        Ok(data.initializer.map(|initializer| self.node(initializer)))
    }

    fn declarations_of(&self, list: NodeId) -> Result<Vec<TransformNode>, TransformError> {
        self.declarations_of_id(list)
    }

    fn declarations_of_id(&self, list: NodeId) -> Result<Vec<TransformNode>, TransformError> {
        let NodeData::VariableDeclarationList(data) =
            &self.context.arena().node(self.node(list))?.data
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclarationList,
                field: "variable declaration list",
            });
        };
        self.array_nodes_of(data.declarations)
    }

    /// tsc-port: getInitializedVariables @6.0.3
    /// tsc-hash: c8fe6eddb970f82b98bea9d71039c1b05f0ed2f3d794db496b853a12e77a7498
    /// tsc-span: _tsc.js:17421-17423
    fn initialized_variables(&self, list: NodeId) -> Result<Vec<TransformNode>, TransformError> {
        self.initialized_variables_of_id(list)
    }

    fn initialized_variables_of_id(
        &self,
        list: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        Ok(self
            .declarations_of_id(list)?
            .into_iter()
            .filter(|declaration| {
                self.declaration_initializer(*declaration)
                    .map(|initializer| initializer.is_some())
                    .unwrap_or(false)
            })
            .collect())
    }

    fn case_block_clauses(&self, case_block: NodeId) -> Result<Vec<TransformNode>, TransformError> {
        let NodeData::CaseBlock(data) = &self.context.arena().node(self.node(case_block))?.data
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CaseBlock,
                field: "case block",
            });
        };
        self.array_nodes_of(data.clauses)
    }

    fn case_clause_expression(
        &self,
        clause: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::CaseClause(data) = &self.context.arena().node(clause)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CaseClause,
                field: "case clause",
            });
        };
        data.expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CaseClause,
                field: "expression",
            })
    }

    fn clause_statements(
        &self,
        clause: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        match &self.context.arena().node(clause)?.data {
            NodeData::CaseClause(data) => self.array_nodes_of(data.statements),
            NodeData::DefaultClause(data) => self.array_nodes_of(data.statements),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(clause)?.kind,
                field: "switch clause",
            }),
        }
    }

    fn array_nodes_of(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) =
            array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        else {
            return Ok(Vec::new());
        };
        Ok(self
            .context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .map(|node| self.node(*node))
            .collect())
    }

    /// tsc-port: getExpressionAssociativity @6.0.3
    /// tsc-hash: 305a13c1344f1bf932c36db1bd830f5c27b1a81b610181add2f7b327303cb386
    /// tsc-span: _tsc.js:16003-16007
    ///
    /// The binary-expression projection of the operator table (the sole
    /// generator call site dispatches binary expressions only).
    fn expression_associativity(&self, id: NodeId) -> Result<Associativity, TransformError> {
        let operator = self.binary_operator_kind(id)?;
        Ok(match operator {
            SyntaxKind::AsteriskAsteriskToken
            | SyntaxKind::EqualsToken
            | SyntaxKind::PlusEqualsToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::AmpersandEqualsToken
            | SyntaxKind::CaretEqualsToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken => Associativity::Right,
            _ => Associativity::Left,
        })
    }

    fn binary_operator_kind(&self, id: NodeId) -> Result<SyntaxKind, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(self.node(id))?.data
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "binary expression",
            });
        };
        let operator = data
            .operator_token
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "operatorToken",
            })?;
        self.kind(operator)
    }

    fn is_string_or_numeric_literal_like(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(node)?.data,
            NodeData::StringLiteral(_)
                | NodeData::NoSubstitutionTemplateLiteral(_)
                | NodeData::NumericLiteral(_)
        ))
    }

    fn is_signed_numeric_literal(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::PrefixUnaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        if !matches!(
            data.operator,
            SyntaxKind::PlusToken | SyntaxKind::MinusToken
        ) {
            return Ok(false);
        }
        Ok(data.operand.is_some_and(|operand| {
            self.kind(operand)
                .map(|kind| kind == SyntaxKind::NumericLiteral)
                .unwrap_or(false)
        }))
    }
}

/// tsc-port: getOperatorAssociativity @6.0.3 (binary projection)
/// tsc-hash: eb5fcb3da6d283ff2bb685355345d612ced69c8e299c81ab48ead1fa8691cf51
/// tsc-span: _tsc.js:16008-16043
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Associativity {
    Left,
    Right,
}

/// tsc-port: isCompoundAssignment @6.0.3
/// tsc-hash: cf363727b517ac8079c5b9f484d3874e50114346987a6065d811ac34416fc940
/// tsc-span: _tsc.js:93033-93035
const fn is_compound_assignment(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PlusEqualsToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::AmpersandEqualsToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken
            | SyntaxKind::CaretEqualsToken
    )
}

/// tsc-port: getNonAssignmentOperatorForCompoundAssignment @6.0.3
/// tsc-hash: 92244f9073469f47d35d385e7aac910055f3863bb6192feacb69a8c31d6272d7
/// tsc-span: _tsc.js:93036-93069
const fn non_assignment_operator_for_compound_assignment(kind: SyntaxKind) -> SyntaxKind {
    match kind {
        SyntaxKind::PlusEqualsToken => SyntaxKind::PlusToken,
        SyntaxKind::MinusEqualsToken => SyntaxKind::MinusToken,
        SyntaxKind::AsteriskEqualsToken => SyntaxKind::AsteriskToken,
        SyntaxKind::AsteriskAsteriskEqualsToken => SyntaxKind::AsteriskAsteriskToken,
        SyntaxKind::SlashEqualsToken => SyntaxKind::SlashToken,
        SyntaxKind::PercentEqualsToken => SyntaxKind::PercentToken,
        SyntaxKind::LessThanLessThanEqualsToken => SyntaxKind::LessThanLessThanToken,
        SyntaxKind::GreaterThanGreaterThanEqualsToken => SyntaxKind::GreaterThanGreaterThanToken,
        SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken => {
            SyntaxKind::GreaterThanGreaterThanGreaterThanToken
        }
        SyntaxKind::AmpersandEqualsToken => SyntaxKind::AmpersandToken,
        SyntaxKind::BarEqualsToken => SyntaxKind::BarToken,
        SyntaxKind::CaretEqualsToken => SyntaxKind::CaretToken,
        SyntaxKind::BarBarEqualsToken => SyntaxKind::BarBarToken,
        SyntaxKind::AmpersandAmpersandEqualsToken => SyntaxKind::AmpersandAmpersandToken,
        SyntaxKind::QuestionQuestionEqualsToken => SyntaxKind::QuestionQuestionToken,
        other => other,
    }
}

/// tsc-port: isLogicalOperator @6.0.3
/// tsc-hash: b27722cefafa158e12d3a292e3145161aa29ba491af543906ab8ee77c924c7bb
/// tsc-span: _tsc.js:17075-17077
///
/// `isBinaryLogicalOperator(token) || token === ExclamationToken`; the
/// exclamation arm is unreachable from a binary operator token.
const fn is_logical_operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BarBarToken
            | SyntaxKind::AmpersandAmpersandToken
            | SyntaxKind::ExclamationToken
    )
}

fn escape_leading_underscores_owned(text: &str) -> String {
    tsc_syntax::escape_leading_underscores(text)
}

fn tsc_syntax_array(source: TransformSourceId, array: NodeArrayId) -> TransformNodeArray {
    TransformNodeArray::new(source, array)
}

/// The substitution clone's identifier constructor (transformer-side; the
/// visitor is gone at print time).
fn create_identifier_raw(
    factory: &mut NodeFactory<'_>,
    source: TransformSourceId,
    text: &str,
) -> Result<TransformNode, TransformError> {
    factory.create_node(
        source,
        NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
            escaped_text: tsc_syntax::escape_leading_underscores(text),
            text: text.to_owned(),
        }),
        TransformFlags::NONE,
    )
}

/// `getSourceMapRange(node)` / `getCommentRange(node)` for the
/// substitution clone: stored metadata range, else the node's own range.
fn substituted_ranges(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<(Option<SourceMapRange>, Option<CommentRange>), TransformError> {
    let metadata = context.arena().metadata(node);
    let stored_source_map = metadata
        .as_ref()
        .and_then(|metadata| metadata.source_map_range());
    let stored_comment = metadata
        .as_ref()
        .and_then(|metadata| metadata.comment_range());
    let raw = {
        let record = context.arena().node(node)?;
        let source = context.arena().source(node.source())?.syntax();
        match SourceRange::from_raw(record.pos, record.end, source.positions()) {
            Ok(range @ SourceRange::Original(_)) => Some(range),
            _ => None,
        }
    };
    let source_map =
        stored_source_map.or_else(|| raw.map(|range| SourceMapRange::new(node.source(), range)));
    let comment =
        stored_comment.or_else(|| raw.map(|range| CommentRange::new(node.source(), range)));
    Ok((source_map, comment))
}

#[cfg(test)]
#[path = "../../tests/unit/generators/tests.rs"]
mod tests;
