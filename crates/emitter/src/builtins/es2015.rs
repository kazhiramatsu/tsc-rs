//! H2.5h-b B-4: the ES2015 visitors.
//!
//! Function-per-function port of tsc's `transformES2015` as bundled at
//! `_tsc.js:104740-108100` (the `transform-es2015` owner frozen in
//! `ratchets/h2-5h-a-owner-graph.v1.json`, 171 pinned local functions),
//! plus the owner-adjacent addenda pinned in the packet
//! (`docs/design/greenfield/slices/h2-5h-b-b-4.md` §4.2). The module is
//! DORMANT: `transform_es2015` is registered by no pipeline until the
//! B-5 runtime flip; until then the only callers are the focused
//! projection suite below, which drives the real
//! `[transform_es2015, transform_generators]` chain (the upstream
//! registration order) on parsed fixtures and byte-compares against
//! fresh-process oracle emits.
//!
//! Producer side of the owner graph's pinned `yield-star-synthesis`
//! composition edge: both converted-loop call sites re-emit
//! `yield* call` with `EmitFlags::ITERATOR` stamped on the call, which
//! B-3's `visitYieldExpression` consumer-skips (no `__values` wrap).
//! Tagged-template lowering is the B-5 shared module; the owner's
//! `visitTaggedTemplateExpression` arm is a typed fail-closed seam here.

use std::collections::BTreeMap;

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags};

use crate::{
    factory::EmitHelperName, resolver::EmitResolver, CommentRange, EmitFlags, EmitHint,
    InternalEmitFlags, LexicalEnvironment, SourceMapRange, SourceRange, SyntheticComment,
    SyntheticCommentKind, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
};

use super::{
    flags_after_update,
    flatten_destructuring::{
        flatten_destructuring_assignment, flatten_destructuring_binding, FlattenHost, FlattenLevel,
    },
    generated_bindings::{AncestorBindingPolicy, GeneratedBindingScopes},
    helpers, initialize_transform_flags,
    tagged_template::{self, ProcessLevel},
    target_bindings::{
        collect_untagged_identifier_texts, finalize_generated_binding_names,
        ParsedSourceIdentifierNames, TargetBinding,
    },
};

// ---------------------------------------------------------------------------
// Alphabets
// ---------------------------------------------------------------------------

/// `HierarchyFacts` (bundler-inlined at every `enterSubtree`/`exitSubtree`
/// site; the exact numeric alphabet is pinned in the packet §4.3).
/// Ancestor facts occupy bits 0..14 (`AncestorFactsMask = 32767`);
/// subtree facts occupy bits 15..17 and are masked OFF on entry and kept
/// on exit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HierarchyFacts(u32);

#[allow(dead_code)] // the full pinned alphabet (§4.3) stays declared
impl HierarchyFacts {
    const NONE: Self = Self(0);
    const FUNCTION: Self = Self(1);
    const ARROW_FUNCTION: Self = Self(2);
    const ASYNC_FUNCTION_BODY: Self = Self(4);
    const NON_STATIC_CLASS_ELEMENT: Self = Self(8);
    const CAPTURES_THIS: Self = Self(16);
    const EXPORTED_VARIABLE_STATEMENT: Self = Self(32);
    const TOP_LEVEL: Self = Self(64);
    const BLOCK: Self = Self(128);
    const ITERATION_STATEMENT: Self = Self(256);
    const ITERATION_STATEMENT_BLOCK: Self = Self(512);
    const ITERATION_CONTAINER: Self = Self(1024);
    const FOR_STATEMENT: Self = Self(2048);
    const FOR_IN_OR_FOR_OF_STATEMENT: Self = Self(4096);
    const CONSTRUCTOR_WITH_SUPER_CALL: Self = Self(8192);
    const STATIC_INITIALIZER: Self = Self(16384);
    const ANCESTOR_FACTS_MASK: Self = Self(32767);

    const NEW_TARGET: Self = Self(32768);
    const LEXICAL_THIS: Self = Self(65536);
    const CAPTURED_LEXICAL_THIS: Self = Self(131072);
    const SUBTREE_FACTS_MASK: Self = Self(!32767);

    // (exclude, include) pairs exactly as they appear at the call sites.
    const SOURCE_FILE_EXCLUDES: Self = Self(8064);
    const SOURCE_FILE_INCLUDES: Self = Self(64);
    const FUNCTION_EXCLUDES: Self = Self(32670);
    const FUNCTION_INCLUDES: Self = Self(65);
    const CONSTRUCTOR_EXCLUDES: Self = Self(32662);
    const CONSTRUCTOR_INCLUDES: Self = Self(73);
    const ASYNC_FUNCTION_BODY_EXCLUDES: Self = Self(32662);
    const ASYNC_FUNCTION_BODY_INCLUDES: Self = Self(69);
    const ARROW_FUNCTION_EXCLUDES: Self = Self(15232);
    const ARROW_FUNCTION_INCLUDES: Self = Self(66);
    const ARROW_FUNCTION_SUBTREE_EXCLUDES: Self = Self(0);
    const STATIC_INITIALIZER_EXCLUDES: Self = Self(32670);
    const STATIC_INITIALIZER_INCLUDES: Self = Self(16449);
    const BLOCK_SCOPE_EXCLUDES: Self = Self(7104);
    const BLOCK_SCOPE_INCLUDES: Self = Self(0);
    const ITERATION_STATEMENT_BLOCK_EXCLUDES: Self = Self(7104);
    const ITERATION_STATEMENT_BLOCK_INCLUDES: Self = Self(512);
    const BLOCK_EXCLUDES: Self = Self(6976);
    const BLOCK_INCLUDES: Self = Self(128);
    const DO_OR_WHILE_STATEMENT_EXCLUDES: Self = Self(0);
    const DO_OR_WHILE_STATEMENT_INCLUDES: Self = Self(1280);
    const FOR_STATEMENT_EXCLUDES: Self = Self(5056);
    const FOR_STATEMENT_INCLUDES: Self = Self(3328);
    const FOR_IN_OR_FOR_OF_STATEMENT_EXCLUDES: Self = Self(3008);
    const FOR_IN_OR_FOR_OF_STATEMENT_INCLUDES: Self = Self(5376);
    const FUNCTION_SUBTREE_EXCLUDES: Self = Self(229376);

    const fn bits(self) -> u32 {
        self.0
    }

    const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// `ES2015SubstitutionFlags`: `CapturedThis = 1`, `BlockScopedBindings = 2`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Es2015SubstitutionFlags(u32);

impl Es2015SubstitutionFlags {
    const CAPTURED_THIS: Self = Self(1);
    const BLOCK_SCOPED_BINDINGS: Self = Self(2);

    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `Jump`: `Break = 2`, `Continue = 4`, `Return = 8`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Jump(u32);

#[allow(dead_code)] // the full pinned alphabet (§4.3) stays declared
impl Jump {
    const NONE: Self = Self(0);
    const BREAK: Self = Self(2);
    const CONTINUE: Self = Self(4);
    const RETURN: Self = Self(8);

    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// `LoopOutParameterFlags`: `Body = 1`, `Initializer = 2`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LoopOutParameterFlags(u32);

impl LoopOutParameterFlags {
    const BODY: Self = Self(1);
    const INITIALIZER: Self = Self(2);

    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `CopyDirection`: `ToOriginal = 0`, `ToOutParameter = 1`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CopyDirection {
    ToOriginal,
    ToOutParameter,
}

/// `SpreadSegmentKind`: `None = 0`, `UnpackedSpread = 1`, `PackedSpread = 2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpreadSegmentKind {
    None,
    UnpackedSpread,
    PackedSpread,
}

/// tsc-port: createSpreadSegment @6.0.3
/// tsc-hash: a04413ca6e352516d65787da81e6c23deca95098fefaa252b3d37f77865480c8
/// tsc-span: _tsc.js:104737-104739
struct SpreadSegment {
    kind: SpreadSegmentKind,
    expression: TransformNode,
}

/// One converted-loop out parameter (`{ flags, originalName, outParamName }`).
struct LoopOutParameter {
    flags: LoopOutParameterFlags,
    /// The ORIGINAL parse-tree identifier (upstream threads the node).
    original_name: TransformNode,
    out_param_name: TargetBinding,
}

/// `ConvertedLoopState` — the per-converted-loop record
/// (`createConvertedLoopState` builds it; inner loops copy the
/// arguments/this/hoisted slots from the outer state and propagate them
/// back through `addExtraDeclarationsForConvertedLoop`).
#[derive(Default)]
struct ConvertedLoopState {
    /// `labels` — label text -> currently-active flag.
    labels: BTreeMap<String, bool>,
    /// `labeledNonLocalBreaks` / `labeledNonLocalContinues` — insertion
    /// ordered (upstream Map iteration order).
    labeled_non_local_breaks: Vec<(String, String)>,
    labeled_non_local_continues: Vec<(String, String)>,
    non_local_jumps: Jump,
    allowed_non_labeled_jumps: Jump,
    arguments_name: Option<TargetBinding>,
    this_name: Option<TargetBinding>,
    contains_lexical_this: bool,
    hoisted_local_variables: Vec<TransformNode>,
    condition_variable: Option<TargetBinding>,
    loop_parameters: Vec<TransformNode>,
    loop_out_parameters: Vec<LoopOutParameter>,
}

// ---------------------------------------------------------------------------
// The transformer
// ---------------------------------------------------------------------------

/// Print-time state shared between the visitation pass and the
/// `onEmitNode`/`onSubstituteNode` hooks. Upstream keeps these in
/// `transformES2015`-level `let`s that OUTLIVE `transformSourceFile`
/// (the B-3 rename-map precedent): `hierarchyFacts` is re-entered per
/// emitted function by `onEmitNode`, and `enabledSubstitutions` latches
/// which substitutions fire at print time.
#[derive(Default)]
struct Es2015PrintState {
    hierarchy_facts: HierarchyFacts,
    enabled_substitutions: Es2015SubstitutionFlags,
    /// The saved ancestor facts for the `onEmitNode` enter/exit pair
    /// (`before_emit_node` pushes an entry per emitted node — `None` when
    /// the enter predicate did not fire, so the paired exit stays
    /// balanced; the printer's error-preserving pairing keeps this LIFO).
    emit_facts_stack: Vec<Option<HierarchyFacts>>,
    /// The shared file-level `_this`/`_newTarget`/`_super` bindings (§5:
    /// upstream mints fresh optimistic instances per site and the name
    /// generator converges same-text file-level-optimistic instances to
    /// one spelling; ONE binding per source file reproduces the measured
    /// spellings, incl. the collision fixtures).
    captured_this: Option<TargetBinding>,
    new_target: Option<TargetBinding>,
    synthetic_super: Option<TargetBinding>,
    /// `getGeneratedNameForNode` cache keyed by the parse-tree node
    /// (upstream `generateNameCached`); shared between the visitation
    /// pass and PRINT-time substitution. Print-time lookups never
    /// allocate: every colliding declaration's binding is pre-allocated
    /// before the finalize walk (see `preallocate_colliding_declaration_names`),
    /// so a print-time miss is a typed error, not a silent divergence.
    generated_names_for_nodes: BTreeMap<(TransformSourceId, NodeId), TargetBinding>,
}

/// The FIRST entry of the joint `[transformES2015, transformGenerators]`
/// list at `languageVersion < ES2015` (upstream registration
/// `_tsc.js:115942-115945`; live since the B-5 runtime flip).
pub(super) fn transform_es2015<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(Es2015Transformer {
        resolver,
        downlevel_iteration: options.downlevel_iteration.unwrap_or(false),
        use_define_for_class_fields: options.use_define_for_class_fields_effective(),
        print_state: Es2015PrintState::default(),
    })
}

struct Es2015Transformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    downlevel_iteration: bool,
    use_define_for_class_fields: bool,
    print_state: Es2015PrintState,
}

impl Transformer for Es2015Transformer<'_> {
    fn name(&self) -> &'static str {
        "transformES2015"
    }

    /// tsc-port: transformSourceFile @6.0.3
    /// tsc-hash: 5259f401bfa4a730f0520aae96d9c7cdf636ce79f6ac7e515a4495d555c16e72
    /// tsc-span: _tsc.js:104768-104781
    ///
    /// The source-file gate is DECLARATION FILES ONLY (upstream has no
    /// transform-flag gate here; the per-node `shouldVisitNode` owns every
    /// flag/forced arm and `visitSourceFile` always runs).
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
        let mut visitor = Es2015Visitor::new(
            context,
            source,
            self.resolver,
            self.downlevel_iteration,
            self.use_define_for_class_fields,
            &mut self.print_state,
            current_root,
        )?;
        let transformed = visitor.visit_source_file(current_root)?;
        visitor.preallocate_colliding_declaration_names()?;
        visitor.renumber_state_bindings()?;
        finalize_generated_binding_names(visitor.context, source, transformed)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }

    /// tsc-port: onSubstituteNode @6.0.3
    /// tsc-hash: b394c52c1aa81fe0824beb6fe6e136215fca7c67804c0c4dcf30d9ae352f2147
    /// tsc-span: _tsc.js:108001-108010
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
        if matches!(context.arena().node(node)?.data, NodeData::Identifier(_)) {
            return self.substitute_identifier(context, node);
        }
        // The harness hint model passes `Unspecified` for non-identifier
        // expression children (printer emit_node_id_with_context); every
        // ThisKeyword token the ES5 printer emits IS an expression, so the
        // upstream `hint === Expression` routing for `this` maps to the
        // token shape here.
        if context.arena().node(node)?.kind == SyntaxKind::ThisKeyword {
            return self.substitute_this_keyword(context, node);
        }
        Ok(node)
    }

    /// tsc-port: onEmitNode @6.0.3 (the enter half)
    /// tsc-hash: d40d38f5984c152e785e53bf787aecc395b7447a7406a1aa44a67d00cf0b33c0
    /// tsc-span: _tsc.js:107970-107981
    ///
    /// Upstream wraps `emitCallback` between `enterSubtree` and
    /// `exitSubtree`; the harness decomposes the wrap into this
    /// before/after pair (transform.rs:749-765) and the printer's
    /// error-preserving pairing keeps the explicit stack LIFO.
    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let fires = self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::CAPTURED_THIS)
            && is_function_like_kind(context.arena().node(node)?.kind);
        if !fires {
            self.print_state.emit_facts_stack.push(None);
            return Ok(());
        }
        let captures_this = context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CAPTURES_THIS));
        let include = if captures_this {
            HierarchyFacts::FUNCTION_INCLUDES.union(HierarchyFacts::CAPTURES_THIS)
        } else {
            HierarchyFacts::FUNCTION_INCLUDES
        };
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::FUNCTION_EXCLUDES,
            include,
        );
        self.print_state.emit_facts_stack.push(Some(ancestor));
        Ok(())
    }

    /// tsc-port: onEmitNode @6.0.3 (the exit half)
    /// tsc-hash: d40d38f5984c152e785e53bf787aecc395b7447a7406a1aa44a67d00cf0b33c0
    /// tsc-span: _tsc.js:107970-107981
    fn after_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(Some(ancestor)) = self.print_state.emit_facts_stack.pop() {
            exit_subtree(
                &mut self.print_state.hierarchy_facts,
                ancestor,
                HierarchyFacts::NONE,
                HierarchyFacts::NONE,
            );
        }
        Ok(())
    }
}

impl Es2015Transformer<'_> {
    /// tsc-port: substituteIdentifier @6.0.3
    /// tsc-hash: 013c4617e93fc50e00b9b10b58a0e71fe12e2279482711828178120972f71291
    /// tsc-span: _tsc.js:108011-108019
    fn substitute_identifier(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS)
            || is_internal_name(context, node)
        {
            return Ok(node);
        }
        let Some(original) = parse_tree_identifier(context, node)? else {
            return Ok(node);
        };
        if !self.is_name_of_declaration_with_colliding_name(context, original)? {
            return Ok(node);
        }
        let binding = self.print_state_generated_name(context, original)?;
        substitution_identifier_clone(context, node, &binding)
    }

    /// tsc-port: isNameOfDeclarationWithCollidingName @6.0.3
    /// tsc-hash: 99833a0d93e796be19158f565367ca8bd1977344d1d60615bd0b339d86a26e6b
    /// tsc-span: _tsc.js:108020-108029
    fn is_name_of_declaration_with_colliding_name(
        &self,
        context: &TransformationContext,
        original: TransformNode,
    ) -> Result<bool, TransformError> {
        let arena = context.arena();
        let record = arena.node(original)?;
        let Some(parent) = record.parent else {
            return Ok(false);
        };
        let parent = TransformNode::new(original.source(), parent);
        let name_matches = match &arena.node(parent)?.data {
            NodeData::BindingElement(data) => data.name == Some(original.node()),
            NodeData::ClassDeclaration(data) => data.name == Some(original.node()),
            NodeData::EnumDeclaration(data) => data.name == Some(original.node()),
            NodeData::VariableDeclaration(data) => data.name == Some(original.node()),
            _ => false,
        };
        if !name_matches {
            return Ok(false);
        }
        let Some(reference) = arena.parse_tree_resolver_node(parent)? else {
            return Ok(false);
        };
        Ok(self
            .resolver
            .is_declaration_with_colliding_name(reference)?)
    }

    /// tsc-port: substituteExpression @6.0.3
    /// tsc-hash: 24124bc8ea7bd7cd3d994f6ba1b134eb70d59d2831dce4e6fdd3baf10dc8508d
    /// tsc-span: _tsc.js:108030-108038
    fn substitute_expression(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match context.arena().node(node)?.data {
            NodeData::Identifier(_) => self.substitute_expression_identifier(context, node),
            NodeData::Token if context.arena().node(node)?.kind == SyntaxKind::ThisKeyword => {
                self.substitute_this_keyword(context, node)
            }
            _ => Ok(node),
        }
    }

    /// tsc-port: substituteExpressionIdentifier @6.0.3
    /// tsc-hash: 46f51cc58b34c3da1fc518e5eda36f005120dfb5bdc34bbf29499ecfd2777deb
    /// tsc-span: _tsc.js:108039-108047
    fn substitute_expression_identifier(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS)
            || is_internal_name(context, node)
        {
            return Ok(node);
        }
        let Some(original) = parse_tree_identifier(context, node)? else {
            return Ok(node);
        };
        let Some(reference) = context.arena().parse_tree_resolver_node(original)? else {
            return Ok(node);
        };
        let Some(declaration) = self
            .resolver
            .get_referenced_declaration_with_colliding_name(reference)?
        else {
            return Ok(node);
        };
        let declaration_node = TransformNode::new(node.source(), declaration.node());
        let is_class_like = matches!(
            context.arena().node(declaration_node)?.data,
            NodeData::ClassDeclaration(_) | NodeData::ClassExpression(_)
        );
        if is_class_like && is_part_of_class_body(context, declaration_node, original)? {
            return Ok(node);
        }
        let name = get_name_of_declaration(context, declaration_node)?;
        let binding = self.print_state_generated_name(context, name)?;
        substitution_identifier_clone(context, node, &binding)
    }

    /// tsc-port: substituteThisKeyword @6.0.3
    /// tsc-hash: 5b11bd13c32beab2d6c24f4fa50ba8c8872804d6bcd1b5781594a0504153bac7
    /// tsc-span: _tsc.js:108065-108070
    fn substitute_this_keyword(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::CAPTURED_THIS)
            || !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::CAPTURES_THIS)
        {
            return Ok(node);
        }
        let binding =
            self.print_state
                .captured_this
                .clone()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ThisKeyword,
                    field: "captured-this binding (allocated at insertCaptureThisForNode)",
                })?;
        substitution_identifier_clone(context, node, &binding)
    }

    /// PRINT-time `getGeneratedNameForNode` — LOOKUP ONLY (the visitation
    /// pass pre-allocates every colliding declaration's binding before the
    /// finalize walk; a miss here is a typed error, never an allocation
    /// after finalize).
    fn print_state_generated_name(
        &self,
        _context: &TransformationContext,
        name: TransformNode,
    ) -> Result<TargetBinding, TransformError> {
        self.print_state
            .generated_names_for_nodes
            .get(&(name.source(), name.node()))
            .cloned()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Identifier,
                field: "pre-allocated generated name for colliding declaration",
            })
    }
}

/// tsc-port: enterSubtree @6.0.3
/// tsc-hash: cf4c612dbd59e62b72d2c4c597760cb9fa8f73023a77cb2d466673d064852ee8
/// tsc-span: _tsc.js:104782-104786
fn enter_subtree(
    facts: &mut HierarchyFacts,
    exclude: HierarchyFacts,
    include: HierarchyFacts,
) -> HierarchyFacts {
    let ancestor = *facts;
    *facts = HierarchyFacts(
        (facts.bits() & !exclude.bits() | include.bits())
            & HierarchyFacts::ANCESTOR_FACTS_MASK.bits(),
    );
    ancestor
}

/// tsc-port: exitSubtree @6.0.3
/// tsc-hash: 4792b8f9d72b075de5dd753a51fda08df77e9fcec865c9ae5a06869ad6b1b5c6
/// tsc-span: _tsc.js:104787-104789
fn exit_subtree(
    facts: &mut HierarchyFacts,
    ancestor: HierarchyFacts,
    exclude: HierarchyFacts,
    include: HierarchyFacts,
) {
    *facts = HierarchyFacts(
        ((facts.bits() & !exclude.bits() | include.bits())
            & HierarchyFacts::SUBTREE_FACTS_MASK.bits())
            | ancestor.bits(),
    );
}

fn is_function_like_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::Constructor
    )
}

/// `isInternalName(node)` — the `EmitFlags::INTERNAL_NAME` read.
fn is_internal_name(context: &TransformationContext, node: TransformNode) -> bool {
    context
        .arena()
        .metadata(node)
        .is_some_and(|metadata| metadata.flags().contains(EmitFlags::INTERNAL_NAME))
}

/// `getParseTreeNode(node, isIdentifier)` over the arena original chain,
/// skipping generated identifiers exactly as the substitution callers do
/// (`!isGeneratedIdentifier(nodeIn)` / `isInternalName` are the callers'
/// own gates).
fn parse_tree_identifier(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let is_generated = context
        .arena()
        .metadata(node)
        .is_some_and(|metadata| metadata.generated_binding_id().is_some());
    if is_generated {
        return Ok(None);
    }
    let original = context.arena().get_original_node(node);
    if !matches!(
        context.arena().node(original)?.data,
        NodeData::Identifier(_)
    ) {
        return Ok(None);
    }
    if context
        .arena()
        .parse_tree_resolver_node(original)?
        .is_none()
    {
        return Ok(None);
    }
    Ok(Some(original))
}

/// tsc-port: isPartOfClassBody @6.0.3
/// tsc-hash: e74c699f696ee9a55e13a0d193a41e8a34e1314678776a5801a0a58fb3a2c22f
/// tsc-span: _tsc.js:108048-108064
fn is_part_of_class_body(
    context: &TransformationContext,
    declaration: TransformNode,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let arena = context.arena();
    let declaration_record = arena.node(declaration)?;
    let (decl_pos, decl_end) = (declaration_record.pos, declaration_record.end);
    let mut current_node = arena.get_original_node(node);
    {
        let record = arena.node(current_node)?;
        if current_node == declaration || record.end <= decl_pos || record.pos >= decl_end {
            return Ok(false);
        }
    }
    let block_scope = enclosing_block_scope_container(context, declaration)?;
    let mut current = Some(current_node.node());
    while let Some(id) = current {
        current_node = TransformNode::new(node.source(), id);
        if Some(id) == block_scope.map(|scope| scope.node()) || current_node == declaration {
            return Ok(false);
        }
        let record = arena.node(current_node)?;
        if is_class_element_kind(record.kind) && record.parent == Some(declaration.node()) {
            return Ok(true);
        }
        current = record.parent;
    }
    Ok(false)
}

/// tsc-port: getEnclosingBlockScopeContainer @6.0.3
/// tsc-hash: 50444054506d87acb188cbcd3ed441a6c57e41352eda843ae6f0840bbbb1cc07
/// tsc-span: _tsc.js:13844-13846
fn enclosing_block_scope_container(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let arena = context.arena();
    let mut current = arena.node(node)?.parent;
    while let Some(id) = current {
        let candidate = TransformNode::new(node.source(), id);
        let record = arena.node(candidate)?;
        let parent_is_function_like = record
            .parent
            .map(|parent| {
                arena
                    .node(TransformNode::new(node.source(), parent))
                    .map(|record| is_function_like_kind(record.kind))
            })
            .transpose()?
            .unwrap_or(false);
        if is_block_scope_kind(record.kind, parent_is_function_like) {
            return Ok(Some(candidate));
        }
        current = record.parent;
    }
    Ok(None)
}

/// `isBlockScope(node, parent)` — the kind classification.
fn is_block_scope_kind(kind: SyntaxKind, parent_is_function_like: bool) -> bool {
    match kind {
        SyntaxKind::SourceFile
        | SyntaxKind::CatchClause
        | SyntaxKind::ModuleDeclaration
        | SyntaxKind::ForStatement
        | SyntaxKind::ForInStatement
        | SyntaxKind::ForOfStatement
        | SyntaxKind::CaseBlock => true,
        SyntaxKind::Block => !parent_is_function_like,
        _ => is_function_like_kind(kind),
    }
}

fn is_class_element_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::SemicolonClassElement
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::IndexSignature
    )
}

/// tsc-port: getNameOfDeclaration @6.0.3 (restricted)
/// tsc-hash: 5d3aafbdab871f0fe6f088a4904cd11e6b44e467e0cca8ad0c215b3f899b570b
/// tsc-span: _tsc.js:11562-11565
///
/// The substitution path reaches only declarations the resolver's
/// colliding-name family returns (BindingElement / ClassDeclaration /
/// EnumDeclaration / VariableDeclaration and class expressions); the
/// assigned-name arms are outside that family and port fail-closed.
fn get_name_of_declaration(
    context: &TransformationContext,
    declaration: TransformNode,
) -> Result<TransformNode, TransformError> {
    let name = match &context.arena().node(declaration)?.data {
        NodeData::VariableDeclaration(data) => data.name,
        NodeData::BindingElement(data) => data.name,
        NodeData::ClassDeclaration(data) => data.name,
        NodeData::ClassExpression(data) => data.name,
        NodeData::EnumDeclaration(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::FunctionExpression(data) => data.name,
        _ => None,
    };
    name.map(|name| TransformNode::new(declaration.source(), name))
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::Identifier,
            field: "declaration name (colliding-name substitution family)",
        })
}

/// The substitution clone: `setTextRange(generatedName, node)` — a fresh
/// generated identifier carrying the binding metadata plus the substituted
/// node's source-map/comment ranges (the generators.rs:6222 precedent).
fn substitution_identifier_clone(
    context: &mut TransformationContext,
    node: TransformNode,
    binding: &TargetBinding,
) -> Result<TransformNode, TransformError> {
    let clone = {
        let mut factory = context.substitution_factory()?;
        let created = factory.create_node(
            node.source(),
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: binding.provisional_name().to_owned(),
                text: binding.provisional_name().to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        created
    };
    binding.write_generated_metadata(context.arena_mut()?, clone);
    let (source_map_range, comment_range) = {
        let arena = context.arena();
        let record = arena.node(node)?;
        let source = arena.source(node.source())?.syntax();
        match SourceRange::from_raw(record.pos, record.end, source.positions()) {
            Ok(range @ SourceRange::Original(_)) => (
                Some(SourceMapRange::new(node.source(), range)),
                Some(CommentRange::new(node.source(), range)),
            ),
            _ => (None, None),
        }
    };
    let metadata = context.arena_mut()?.metadata_mut(clone);
    if let Some(range) = source_map_range {
        metadata.set_source_map_range(range);
    }
    if let Some(range) = comment_range {
        metadata.set_comment_range(range);
    }
    Ok(clone)
}

// ---------------------------------------------------------------------------
// The visitor
// ---------------------------------------------------------------------------

/// One-or-many visit result. Upstream visitors return `VisitResult`
/// (`Node | Node[] | undefined`): statement-position dispatchers
/// (`visitClassDeclaration`, converted loops) return arrays that statement
/// LIST walkers splice and single-statement positions lift into a Block
/// (`factory2.liftToBlock` as the `visitNode` lift argument). `Many` only
/// ever carries STATEMENTS, so the kind-informed lift is exact.
enum VisitOutcome {
    Elided,
    One(TransformNode),
    Many(Vec<TransformNode>),
}

/// The per-source-file visitor. Upstream keeps this state in
/// `transformES2015`-level `let`s; `hierarchyFacts`,
/// `enabledSubstitutions`, the shared file-level bindings, and the
/// node-keyed generated-name cache OUTLIVE the visitation (print-time
/// hooks consume them) and therefore live in the transformer's
/// `Es2015PrintState`, borrowed here. The visitor is STATEFUL (facts,
/// converted-loop state, unused-expression-result flag select dispatch),
/// so there is deliberately NO per-node visit memoization (the B-3
/// generators precedent; the es2017 memo map is not replicated).
pub(super) struct Es2015Visitor<'context, 'resolver, 'state> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    downlevel_iteration: bool,
    use_define_for_class_fields: bool,
    print_state: &'state mut Es2015PrintState,

    /// `currentText` (`skipTrivia` for the class-wrapper end positions;
    /// same-line tests ride the parse `PositionIndex`).
    current_text: String,

    /// `convertedLoopState` (None outside converted-loop bodies).
    converted_loop_state: Option<Box<ConvertedLoopState>>,

    /// `taggedTemplateStringDeclarations` — dormant recorder (the B-5
    /// tagged-template module records into it; the source-file tail that
    /// emits the declarations list is live and ports faithfully).
    tagged_template_string_declarations: Vec<TransformNode>,

    generated_bindings: GeneratedBindingScopes,
    /// The parsed-source name snapshot backing the file-level-optimistic
    /// planning (`ParsedSourceIdentifierNames::optimistic_candidate`).
    parsed_names: ParsedSourceIdentifierNames,
    /// tsc numbers `createUniqueName("state")` in the printer's per-scope
    /// name-generation pass (parent scope fully, then children), while the
    /// eager model allocates inner-first; the records below re-plan the
    /// family in scope-pass order before the finalize walk (§5).
    function_scope_path: Vec<u32>,
    function_scope_child_counters: Vec<u32>,
    state_binding_records: Vec<StateBindingRecord>,
}

/// One converted-loop `state` binding with its containing-scope path and
/// every identifier minted for it.
struct StateBindingRecord {
    scope_path: Vec<u32>,
    sequence: u32,
    #[allow(dead_code)] // the owning binding identity (diagnostics)
    binding: TargetBinding,
    identifiers: Vec<TransformNode>,
}

impl<'context, 'resolver, 'state> Es2015Visitor<'context, 'resolver, 'state> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        downlevel_iteration: bool,
        use_define_for_class_fields: bool,
        print_state: &'state mut Es2015PrintState,
        root: TransformNode,
    ) -> Result<Self, TransformError> {
        let current_text = context.arena().source(source)?.syntax().text().to_owned();
        let reserved = collect_untagged_identifier_texts(context.arena(), source, root)?;
        let parsed_names = ParsedSourceIdentifierNames::collect(context.arena(), source)?;
        Ok(Self {
            context,
            source,
            resolver,
            downlevel_iteration,
            use_define_for_class_fields,
            print_state,
            current_text,
            converted_loop_state: None,
            tagged_template_string_declarations: Vec::new(),
            generated_bindings: GeneratedBindingScopes::new(
                reserved,
                AncestorBindingPolicy::Reserve,
            ),
            parsed_names,
            function_scope_path: Vec::new(),
            function_scope_child_counters: vec![0],
            state_binding_records: Vec::new(),
        })
    }

    // --- core accessors (the generators.rs idiom) ---------------------

    pub(super) fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    fn kind(&self, node: TransformNode) -> Result<SyntaxKind, TransformError> {
        Ok(self.context.arena().node(node)?.kind)
    }

    fn transform_flags(&self, node: TransformNode) -> TransformFlags {
        self.context.arena().transform_flags(node)
    }

    fn emit_flags(&self, node: TransformNode) -> EmitFlags {
        self.context
            .arena()
            .metadata(node)
            .map(|metadata| metadata.flags())
            .unwrap_or(EmitFlags::NONE)
    }

    fn internal_emit_flags(&self, node: TransformNode) -> InternalEmitFlags {
        self.context
            .arena()
            .metadata(node)
            .map(|metadata| metadata.internal_flags())
            .unwrap_or_default()
    }

    fn add_emit_flags(
        &mut self,
        node: TransformNode,
        flags: EmitFlags,
    ) -> Result<(), TransformError> {
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .add_flags(flags);
        Ok(())
    }

    fn child_flags(&self, nodes: &[TransformNode]) -> Result<TransformFlags, TransformError> {
        let mut flags = TransformFlags::NONE;
        for node in nodes {
            flags |= self.context.arena().propagate_child_flags(*node)?;
        }
        Ok(flags)
    }

    pub(super) fn set_text_range(
        &mut self,
        node: TransformNode,
        location: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, location)
    }

    fn set_original(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<(), TransformError> {
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(())
    }

    fn start_on_new_line(&mut self, node: TransformNode) -> Result<(), TransformError> {
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_starts_on_new_line(true);
        Ok(())
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

    pub(super) fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        match array {
            Some(array) => {
                let array = tsc_syntax_array(self.source, array);
                Ok(self
                    .context
                    .arena()
                    .node_array(array)?
                    .nodes
                    .iter()
                    .map(|id| self.node(*id))
                    .collect())
            }
            None => Ok(Vec::new()),
        }
    }

    fn clone_node(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.context.factory()?.clone_node(node)
    }

    /// `nodeIsSynthesized` — position-based, the B-2 pin.
    fn node_is_synthesized(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(self.effective_source_range(node)?.is_none())
    }

    // --- binding allocation (E-NAMES eager model) ----------------------

    fn allocate_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
        let provisional = self.generated_bindings.allocate_temp();
        TargetBinding::allocate(self.context, provisional)
    }

    /// `createUniqueName(text)` — the `text_1` numbered family.
    pub(super) fn allocate_numbered_binding(
        &mut self,
        text: &str,
    ) -> Result<TargetBinding, TransformError> {
        let provisional = self.generated_bindings.allocate_numbered(text);
        TargetBinding::allocate_numbered(self.context, text.to_owned(), provisional)
    }

    /// `createLoopVariable()` — the `_i` family (planned-authoritative:
    /// the finalize walk keeps the family spelling, the B-3 precedent).
    fn allocate_loop_variable_binding(&mut self) -> Result<TargetBinding, TransformError> {
        let provisional = self
            .generated_bindings
            .allocate_loop_variable(/*reserve_in_nested_scopes*/ false);
        TargetBinding::allocate_planned(self.context, provisional)
    }

    pub(super) fn create_generated_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(binding.provisional_name())?;
        binding.write_generated_metadata(self.context.arena_mut()?, identifier);
        Ok(identifier)
    }

    /// `getGeneratedNameForNode(node)` — `generateNameCached`: ONE binding
    /// per parse-tree node, cached in the print state (print-time
    /// substitution reads the same cache). Identifier-named nodes take the
    /// numbered `text_1` family; pattern-named parameters and other
    /// nameless nodes take the temp family (upstream `generateNameForNode`
    /// falls to `makeTempVariableName`).
    fn get_generated_name_for_node(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let key = (node.source(), node.node());
        if let Some(binding) = self
            .print_state
            .generated_names_for_nodes
            .get(&key)
            .cloned()
        {
            return self.create_generated_identifier(&binding);
        }
        // generateNameForNode arms (`_tsc.js:120876-120933`): identifier →
        // text-numbered; class expression → the "class" family; named
        // class/function declarations recurse on the name, unnamed →
        // "default"; everything else (pattern parameters, computed names)
        // → the temp family.
        let name_text = match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        };
        let kind_base = match &self.context.arena().node(node)?.data {
            NodeData::ClassExpression(_) => Some("class".to_owned()),
            NodeData::ClassDeclaration(data) => match data.name {
                Some(name) => match &self.context.arena().node(self.node(name))?.data {
                    NodeData::Identifier(identifier) => Some(identifier.text.clone()),
                    _ => Some("default".to_owned()),
                },
                None => Some("default".to_owned()),
            },
            NodeData::FunctionDeclaration(data) => match data.name {
                Some(name) => match &self.context.arena().node(self.node(name))?.data {
                    NodeData::Identifier(identifier) => Some(identifier.text.clone()),
                    _ => Some("default".to_owned()),
                },
                None => Some("default".to_owned()),
            },
            _ => None,
        };
        let binding = match name_text.or(kind_base) {
            Some(text) => self.allocate_numbered_binding(&text)?,
            None => self.allocate_temp_binding()?,
        };
        self.print_state
            .generated_names_for_nodes
            .insert(key, binding.clone());
        self.create_generated_identifier(&binding)
    }

    /// Pre-allocate the generated name of every colliding declaration so
    /// PRINT-time substitution only ever looks up (§5; runs before the
    /// finalize walk). Extensional equality: the reference-side query
    /// (`getReferencedDeclarationWithCollidingName`) answers through the
    /// SAME `isSymbolOfDeclarationWithCollidingName` predicate the
    /// declaration-side query uses, so enumerating declaration sites
    /// covers both substitution paths.
    fn preallocate_colliding_declaration_names(&mut self) -> Result<(), TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS)
        {
            return Ok(());
        }
        let candidates: Vec<TransformNode> = {
            let arena = self.context.arena();
            let syntax = arena.source(self.source)?.syntax();
            let node_base = syntax.arena.node_base();
            let mut out = Vec::new();
            for (offset, record) in syntax.arena.nodes().iter().enumerate() {
                let name = match &record.data {
                    NodeData::VariableDeclaration(data) => data.name,
                    NodeData::BindingElement(data) => data.name,
                    NodeData::ClassDeclaration(data) => data.name,
                    NodeData::ClassExpression(data) => data.name,
                    NodeData::EnumDeclaration(data) => data.name,
                    _ => None,
                };
                let Some(name) = name else { continue };
                let id = NodeId(node_base + u32::try_from(offset).expect("node count fits u32"));
                let declaration = self.node(id);
                let name = self.node(name);
                if !matches!(arena.node(name)?.data, NodeData::Identifier(_)) {
                    continue;
                }
                let Some(reference) = arena.parse_tree_resolver_node(declaration)? else {
                    continue;
                };
                if self
                    .resolver
                    .is_declaration_with_colliding_name(reference)?
                {
                    out.push(name);
                }
            }
            out
        };
        for name in candidates {
            let _ = self.get_generated_name_for_node(name)?;
        }
        Ok(())
    }
}

fn tsc_syntax_array(source: TransformSourceId, array: NodeArrayId) -> TransformNodeArray {
    TransformNodeArray::new(source, array)
}

// ---------------------------------------------------------------------------
// Node constructors (module-internal `create_*` wrappers over
// `factory().create_node` with EA-GAP-FLAGS child folds; parenthesization
// is factory-automatic)
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.to_owned(),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    pub(super) fn create_string_literal(
        &mut self,
        text: &str,
    ) -> Result<TransformNode, TransformError> {
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

    fn create_token_node(
        &mut self,
        kind: SyntaxKind,
        flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_token(source, kind, flags)
    }

    fn create_this_token(&mut self) -> Result<TransformNode, TransformError> {
        self.create_token_node(
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )
    }

    fn create_true(&mut self) -> Result<TransformNode, TransformError> {
        self.create_token_node(SyntaxKind::TrueKeyword, TransformFlags::NONE)
    }

    fn create_false(&mut self) -> Result<TransformNode, TransformError> {
        self.create_token_node(SyntaxKind::FalseKeyword, TransformFlags::NONE)
    }

    fn create_null(&mut self) -> Result<TransformNode, TransformError> {
        self.create_token_node(SyntaxKind::NullKeyword, TransformFlags::NONE)
    }

    /// `createVoidZero()` — `void 0`.
    pub(super) fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        let source = self.source;
        let flags = self.child_flags(&[zero])?;
        self.context.factory()?.create_node(
            source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
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

    fn create_property_access_text(
        &mut self,
        expression: TransformNode,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.create_property_access(expression, name)
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

    pub(super) fn create_call(
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
            | self.context.arena().array_transform_flags(arguments)
            | TransformFlags::CONTAINS_ES_2020;
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

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let operator_token = self.create_token_node(operator, TransformFlags::NONE)?;
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

    pub(super) fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    fn create_logical_and(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::AmpersandAmpersandToken, right)
    }

    pub(super) fn create_logical_or(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::BarBarToken, right)
    }

    fn create_strict_equality(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsEqualsEqualsToken, right)
    }

    fn create_strict_inequality(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::ExclamationEqualsEqualsToken, right)
    }

    fn create_less_than(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::LessThanToken, right)
    }

    fn create_subtract(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::MinusToken, right)
    }

    fn create_comma(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::CommaToken, right)
    }

    fn create_postfix_increment(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[operand])?;
        self.context.factory()?.create_node(
            source,
            NodeData::PostfixUnaryExpression(tsc_syntax::nodes::PostfixUnaryExpressionData {
                operand: Some(operand.node()),
                operator: SyntaxKind::PlusPlusToken,
            }),
            flags,
        )
    }

    fn create_logical_not(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[operand])?;
        self.context.factory()?.create_node(
            source,
            NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                operator: SyntaxKind::ExclamationToken,
                operand: Some(operand.node()),
            }),
            flags,
        )
    }

    fn create_conditional(
        &mut self,
        condition: TransformNode,
        when_true: TransformNode,
        when_false: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let question = self.create_token_node(SyntaxKind::QuestionToken, TransformFlags::NONE)?;
        let colon = self.create_token_node(SyntaxKind::ColonToken, TransformFlags::NONE)?;
        let flags = self.child_flags(&[condition, when_true, when_false])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            flags,
        )
    }

    fn create_paren(&mut self, expression: TransformNode) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_partially_emitted_expression(
        &mut self,
        expression: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression])?;
        let created = self.context.factory()?.create_node(
            source,
            NodeData::PartiallyEmittedExpression(
                tsc_syntax::nodes::PartiallyEmittedExpressionData {
                    expression: Some(expression.node()),
                },
            ),
            flags,
        )?;
        self.set_original(created, original)?;
        self.set_text_range(created, original)?;
        Ok(created)
    }

    pub(super) fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_array_literal_full(elements, false, false)
    }

    /// `createArrayLiteralExpression` — the trailing-comma auto-add when
    /// the LAST element is omitted rides the explicit flag (the B-3
    /// `_tsc.js:22441-22449` lesson).
    fn create_array_literal_full(
        &mut self,
        elements: Vec<TransformNode>,
        multi_line: bool,
        has_trailing_comma: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let last_is_omitted = match elements.last() {
            Some(last) => self.kind(*last)? == SyntaxKind::OmittedExpression,
            None => false,
        };
        let array = self
            .context
            .factory()?
            .create_node_array_with_trailing_comma(
                source,
                elements,
                has_trailing_comma || last_is_omitted,
            )?;
        let flags = self.context.arena().array_transform_flags(array);
        let created = self.context.factory()?.create_node(
            source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(array.array()),
            }),
            flags,
        )?;
        if multi_line {
            self.context.factory()?.set_multi_line(created, true)?;
        }
        Ok(created)
    }

    fn create_object_literal(
        &mut self,
        properties: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let array = self
            .context
            .factory()?
            .create_node_array(source, properties)?;
        let flags = self.context.arena().array_transform_flags(array);
        let created = self.context.factory()?.create_node(
            source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(array.array()),
            }),
            flags,
        )?;
        if multi_line {
            self.context.factory()?.set_multi_line(created, true)?;
        }
        Ok(created)
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

    fn create_property_assignment_text(
        &mut self,
        name: &str,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.create_property_assignment(name, initializer)
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression])?;
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
            Some(expression) => self.child_flags(&[expression])?,
            None => TransformFlags::NONE,
        } | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: expression.map(|expression| expression.node()),
            }),
            flags,
        )
    }

    fn create_if_statement(
        &mut self,
        expression: TransformNode,
        then_statement: TransformNode,
        else_statement: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let mut nodes = vec![expression, then_statement];
        if let Some(else_statement) = else_statement {
            nodes.push(else_statement);
        }
        let flags = self.child_flags(&nodes)?;
        self.context.factory()?.create_node(
            source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(expression.node()),
                then_statement: Some(then_statement.node()),
                else_statement: else_statement.map(|node| node.node()),
            }),
            flags,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let array = self
            .context
            .factory()?
            .create_node_array(source, statements)?;
        let flags = self.context.arena().array_transform_flags(array);
        let created = self.context.factory()?.create_node(
            source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(array.array()),
            }),
            flags,
        )?;
        if multi_line {
            self.context.factory()?.set_multi_line(created, true)?;
        }
        Ok(created)
    }

    fn create_empty_statement(&mut self) -> Result<TransformNode, TransformError> {
        let source = self.source;
        self.context.factory()?.create_node(
            source,
            NodeData::EmptyStatement(tsc_syntax::nodes::EmptyStatementData {}),
            TransformFlags::NONE,
        )
    }

    fn create_variable_declaration_plain(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let mut nodes = vec![name];
        if let Some(initializer) = initializer {
            nodes.push(initializer);
        }
        let flags = self.child_flags(&nodes)?;
        self.context.factory()?.create_node(
            source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(|node| node.node()),
            }),
            flags,
        )
    }

    fn create_variable_declaration_list(
        &mut self,
        declarations: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let array = self
            .context
            .factory()?
            .create_node_array(source, declarations)?;
        let flags = self.context.arena().array_transform_flags(array)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(array.array()),
            }),
            flags,
        )
    }

    fn create_variable_statement_from_list(
        &mut self,
        list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[list])?;
        self.context.factory()?.create_node(
            source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            flags,
        )
    }

    fn create_variable_statement_single(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let declaration = self.create_variable_declaration_plain(name, initializer)?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        self.create_variable_statement_from_list(list)
    }

    fn create_parameter_declaration(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[name])?;
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

    /// `createFunctionExpression(modifiers?, asterisk?, name?, typeParams?,
    /// parameters, type?, body)` with the factory `function_facets` fold.
    fn create_function_expression_full(
        &mut self,
        asterisk: bool,
        name: Option<TransformNode>,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let asterisk_token = if asterisk {
            Some(self.create_token_node(SyntaxKind::AsteriskToken, TransformFlags::NONE)?)
        } else {
            None
        };
        let parameters_array = self
            .context
            .factory()?
            .create_node_array(source, parameters)?;
        let mut flags = self.context.arena().array_transform_flags(parameters_array)
            | self.child_flags(&[body])?
            | TransformFlags::CONTAINS_ES_2015;
        if let Some(name) = name {
            flags |= self.child_flags(&[name])?;
        }
        if asterisk {
            flags |= TransformFlags::CONTAINS_GENERATOR;
        }
        self.context.factory()?.create_node(
            source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: name.map(|name| name.node()),
                type_parameters: None,
                parameters: Some(parameters_array.array()),
                r#type: None,
                asterisk_token: asterisk_token.map(|token| token.node()),
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )
    }

    fn create_yield_star(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let asterisk = self.create_token_node(SyntaxKind::AsteriskToken, TransformFlags::NONE)?;
        let flags = self.child_flags(&[expression])?
            | TransformFlags::CONTAINS_ES_2015
            | TransformFlags::CONTAINS_ES_2018
            | TransformFlags::CONTAINS_YIELD;
        self.context.factory()?.create_node(
            source,
            NodeData::YieldExpression(tsc_syntax::nodes::YieldExpressionData {
                asterisk_token: Some(asterisk.node()),
                expression: Some(expression.node()),
            }),
            flags,
        )
    }
}

// ---------------------------------------------------------------------------
// Visit plumbing + dispatch
// ---------------------------------------------------------------------------

impl NodeDataChildVisitor for Es2015Visitor<'_, '_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.kind(self.node(id)).unwrap_or(SyntaxKind::Unknown)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        match self.visit(self.node(id))? {
            VisitOutcome::Elided => Ok(None),
            VisitOutcome::One(node) => Ok(Some(node.node())),
            VisitOutcome::Many(statements) => {
                // Kind-informed lift: `Many` only arises from statement
                // visitors, and single-statement child positions take
                // `factory2.liftToBlock` upstream.
                Ok(Some(self.lift_to_block(statements)?.node()))
            }
        }
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let original = tsc_syntax_array(self.source, id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            match self.visit(self.node(node))? {
                VisitOutcome::Elided => {}
                VisitOutcome::One(node) => visited.push(node),
                VisitOutcome::Many(statements) => visited.extend(statements),
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        Ok(Some(updated.array()))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: liftToBlock @6.0.3
    /// tsc-hash: c96ac6375abe99aeb4b2779fc5d1a4b28d835df33d5198647cd888d1abd36a48
    /// tsc-span: _tsc.js:24878-24881
    fn lift_to_block(
        &mut self,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if statements.len() == 1 {
            return Ok(statements[0]);
        }
        self.create_block(statements, /*multi_line*/ true)
    }

    /// `visitEachChild(node, visitor, context)` — the generic descent with
    /// update-identity preservation.
    fn visit_each_child_id(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
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

    fn visit_each_child(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(self
            .visit_each_child_id(node.node())?
            .map(|id| self.node(id)))
    }

    pub(super) fn visit_each_child_required(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_each_child(node)?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "visited child",
            })
    }

    /// `Debug.checkDefined(visitNode(node, visitor, isExpression))`.
    pub(super) fn visit_required_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.visit(node)? {
            VisitOutcome::One(node) => Ok(node),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "visited expression",
            }),
        }
    }

    fn visit_expression_opt(
        &mut self,
        node: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        match node {
            Some(node) => match self.visit(node)? {
                VisitOutcome::One(node) => Ok(Some(node)),
                VisitOutcome::Elided => Ok(None),
                VisitOutcome::Many(_) => Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "expression position received a statement list",
                }),
            },
            None => Ok(None),
        }
    }

    /// `visitNode(node, visitor, isStatement, factory2.liftToBlock)`.
    fn visit_statement_lifted(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        match self.visit(node)? {
            VisitOutcome::Elided => Ok(None),
            VisitOutcome::One(node) => Ok(Some(node)),
            VisitOutcome::Many(statements) => Ok(Some(self.lift_to_block(statements)?)),
        }
    }

    /// `visitNodes2(statements, visitor, isStatement, offset?)` with the
    /// statement-splice protocol, returning a plain Vec.
    fn visit_statements_into(
        &mut self,
        statements: &[TransformNode],
        start: usize,
        target: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        for statement in statements.iter().skip(start) {
            match self.visit(*statement)? {
                VisitOutcome::Elided => {}
                VisitOutcome::One(node) => target.push(node),
                VisitOutcome::Many(nodes) => target.extend(nodes),
            }
        }
        Ok(())
    }

    /// tsc-port: shouldVisitNode @6.0.3
    /// tsc-hash: daaa7226e6ca8115617cc655f69f95e1523fc50596bb4cfef2f5e1c7c0e9c174
    /// tsc-span: _tsc.js:104800-104806
    fn should_visit_node(&self, node: TransformNode) -> Result<bool, TransformError> {
        if self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_ES_2015)
        {
            return Ok(true);
        }
        if self.converted_loop_state.is_some() {
            return Ok(true);
        }
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::CONSTRUCTOR_WITH_SUPER_CALL)
            && self.is_or_may_contain_return_completion(node)?
        {
            return Ok(true);
        }
        if is_iteration_statement_kind(self.kind(node)?)
            && self.should_convert_iteration_statement(node)?
        {
            return Ok(true);
        }
        Ok(self
            .internal_emit_flags(node)
            .contains(InternalEmitFlags::TYPE_SCRIPT_CLASS_WRAPPER))
    }

    /// tsc-port: isOrMayContainReturnCompletion @6.0.3
    /// tsc-hash: 512f73e1544830b532901dc633c1b58620ebe5934e65c05a4e9aa2a0f3fefa29
    /// tsc-span: _tsc.js:104793-104799
    fn is_or_may_contain_return_completion(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if !self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION)
        {
            return Ok(false);
        }
        Ok(matches!(
            self.kind(node)?,
            SyntaxKind::ReturnStatement
                | SyntaxKind::IfStatement
                | SyntaxKind::WithStatement
                | SyntaxKind::SwitchStatement
                | SyntaxKind::CaseBlock
                | SyntaxKind::CaseClause
                | SyntaxKind::DefaultClause
                | SyntaxKind::TryStatement
                | SyntaxKind::CatchClause
                | SyntaxKind::LabeledStatement
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::DoStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::Block
        ))
    }

    /// tsc-port: visitor @6.0.3
    /// tsc-hash: 6c81599cbeb4c7d8a540cdad1ae6e36165af188b04160c0211a0385bf404702d
    /// tsc-span: _tsc.js:104807-104813
    fn visit(&mut self, node: TransformNode) -> Result<VisitOutcome, TransformError> {
        if self.should_visit_node(node)? {
            self.visitor_worker(node, false)
        } else {
            Ok(VisitOutcome::One(node))
        }
    }

    /// tsc-port: visitorWithUnusedExpressionResult @6.0.3
    /// tsc-hash: d96bb20713207bddd4bf48a0bd4d91aaa27a8c1fa596beaabbe7460b17ab4cc8
    /// tsc-span: _tsc.js:104814-104820
    fn visit_with_unused_expression_result(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        if self.should_visit_node(node)? {
            self.visitor_worker(node, true)
        } else {
            Ok(VisitOutcome::One(node))
        }
    }

    /// tsc-port: callExpressionVisitor @6.0.3
    /// tsc-hash: 1cd164c791adeb5549edcc6ac319f12b1e66e87123ebf9a9ee2fc0213437807b
    /// tsc-span: _tsc.js:104845-104854
    fn call_expression_visitor(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.kind(node)? == SyntaxKind::SuperKeyword {
            return self.visit_super_keyword(node, /*is_expression_of_call*/ true);
        }
        self.visit_required_expression(node)
    }

    /// tsc-port: visitorWorker @6.0.3
    /// tsc-hash: 6c45f345b7b1911b12899874ce7b2816c9459e0e8bac37d9c1b44dd2176e19e0
    /// tsc-span: _tsc.js:104855-104981
    fn visitor_worker(
        &mut self,
        node: TransformNode,
        expression_result_is_unused: bool,
    ) -> Result<VisitOutcome, TransformError> {
        let kind = self.kind(node)?;
        Ok(match kind {
            SyntaxKind::StaticKeyword => VisitOutcome::Elided,
            SyntaxKind::ClassDeclaration => self.visit_class_declaration(node)?,
            SyntaxKind::ClassExpression => VisitOutcome::One(self.visit_class_expression(node)?),
            SyntaxKind::Parameter => match self.visit_parameter(node)? {
                Some(parameter) => VisitOutcome::One(parameter),
                None => VisitOutcome::Elided,
            },
            SyntaxKind::FunctionDeclaration => {
                VisitOutcome::One(self.visit_function_declaration(node)?)
            }
            SyntaxKind::ArrowFunction => VisitOutcome::One(self.visit_arrow_function(node)?),
            SyntaxKind::FunctionExpression => {
                VisitOutcome::One(self.visit_function_expression(node)?)
            }
            SyntaxKind::VariableDeclaration => self.visit_variable_declaration(node)?,
            SyntaxKind::Identifier => VisitOutcome::One(self.visit_identifier(node)?),
            SyntaxKind::VariableDeclarationList => {
                VisitOutcome::One(self.visit_variable_declaration_list(node)?)
            }
            SyntaxKind::SwitchStatement => VisitOutcome::One(self.visit_switch_statement(node)?),
            SyntaxKind::CaseBlock => VisitOutcome::One(self.visit_case_block(node)?),
            SyntaxKind::Block => {
                VisitOutcome::One(self.visit_block(node, /*is_function_body*/ false)?)
            }
            SyntaxKind::BreakStatement | SyntaxKind::ContinueStatement => {
                VisitOutcome::One(self.visit_break_or_continue_statement(node)?)
            }
            SyntaxKind::LabeledStatement => self.visit_labeled_statement(node)?,
            SyntaxKind::DoStatement | SyntaxKind::WhileStatement => {
                self.visit_do_or_while_statement(node, None)?
            }
            SyntaxKind::ForStatement => self.visit_for_statement(node, None)?,
            SyntaxKind::ForInStatement => self.visit_for_in_statement(node, None)?,
            SyntaxKind::ForOfStatement => self.visit_for_of_statement(node, None)?,
            SyntaxKind::ExpressionStatement => {
                VisitOutcome::One(self.visit_expression_statement(node)?)
            }
            SyntaxKind::ObjectLiteralExpression => {
                VisitOutcome::One(self.visit_object_literal_expression(node)?)
            }
            SyntaxKind::CatchClause => VisitOutcome::One(self.visit_catch_clause(node)?),
            SyntaxKind::ShorthandPropertyAssignment => {
                VisitOutcome::One(self.visit_shorthand_property_assignment(node)?)
            }
            SyntaxKind::ComputedPropertyName => {
                VisitOutcome::One(self.visit_computed_property_name(node)?)
            }
            SyntaxKind::ArrayLiteralExpression => {
                VisitOutcome::One(self.visit_array_literal_expression(node)?)
            }
            SyntaxKind::CallExpression => VisitOutcome::One(self.visit_call_expression(node)?),
            SyntaxKind::NewExpression => VisitOutcome::One(self.visit_new_expression(node)?),
            SyntaxKind::ParenthesizedExpression => VisitOutcome::One(
                self.visit_parenthesized_expression(node, expression_result_is_unused)?,
            ),
            SyntaxKind::BinaryExpression => {
                VisitOutcome::One(self.visit_binary_expression(node, expression_result_is_unused)?)
            }
            SyntaxKind::CommaListExpression => VisitOutcome::One(
                self.visit_comma_list_expression(node, expression_result_is_unused)?,
            ),
            SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead
            | SyntaxKind::TemplateMiddle
            | SyntaxKind::TemplateTail => VisitOutcome::One(self.visit_template_literal(node)?),
            SyntaxKind::StringLiteral => VisitOutcome::One(self.visit_string_literal(node)?),
            SyntaxKind::NumericLiteral => VisitOutcome::One(self.visit_numeric_literal(node)?),
            SyntaxKind::TaggedTemplateExpression => {
                VisitOutcome::One(self.visit_tagged_template_expression(node)?)
            }
            SyntaxKind::TemplateExpression => {
                VisitOutcome::One(self.visit_template_expression(node)?)
            }
            SyntaxKind::YieldExpression => VisitOutcome::One(self.visit_yield_expression(node)?),
            SyntaxKind::SpreadElement => VisitOutcome::One(self.visit_spread_element(node)?),
            SyntaxKind::SuperKeyword => {
                VisitOutcome::One(self.visit_super_keyword(node, /*is_expression_of_call*/ false)?)
            }
            SyntaxKind::ThisKeyword => VisitOutcome::One(self.visit_this_keyword(node)?),
            SyntaxKind::MetaProperty => VisitOutcome::One(self.visit_meta_property(node)?),
            SyntaxKind::MethodDeclaration => {
                VisitOutcome::One(self.visit_method_declaration(node)?)
            }
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                VisitOutcome::One(self.visit_accessor_declaration(node)?)
            }
            SyntaxKind::VariableStatement => match self.visit_variable_statement(node)? {
                Some(statement) => VisitOutcome::One(statement),
                None => VisitOutcome::Elided,
            },
            SyntaxKind::ReturnStatement => VisitOutcome::One(self.visit_return_statement(node)?),
            SyntaxKind::VoidExpression => VisitOutcome::One(self.visit_void_expression(node)?),
            _ => match self.visit_each_child(node)? {
                Some(updated) => VisitOutcome::One(updated),
                None => VisitOutcome::Elided,
            },
        })
    }
}

impl Es2015Visitor<'_, '_, '_> {
    fn enter_function_scope_path(&mut self) {
        let child = self
            .function_scope_child_counters
            .last_mut()
            .expect("scope counter present");
        let index = *child;
        *child += 1;
        self.function_scope_path.push(index);
        self.function_scope_child_counters.push(0);
    }

    fn exit_function_scope_path(&mut self) {
        self.function_scope_path.pop();
        self.function_scope_child_counters.pop();
    }

    /// Re-plan the `state` family in tsc scope-pass order and re-stamp the
    /// identifiers' planned metadata before the finalize walk.
    fn renumber_state_bindings(&mut self) -> Result<(), TransformError> {
        if self.state_binding_records.len() < 2 {
            return Ok(());
        }
        let mut order: Vec<usize> = (0..self.state_binding_records.len()).collect();
        order.sort_by(|a, b| {
            let ra = &self.state_binding_records[*a];
            let rb = &self.state_binding_records[*b];
            ra.scope_path
                .cmp(&rb.scope_path)
                .then(ra.sequence.cmp(&rb.sequence))
        });
        for (position, index) in order.into_iter().enumerate() {
            let planned = format!("state_{}", position + 1);
            let identifiers = self.state_binding_records[index].identifiers.clone();
            for identifier in identifiers {
                // The finalize walk reads each identifier's TEXT as the
                // planned spelling; the sanctioned re-plan writes through
                // the same arena surface the E-NAMES finalizer uses.
                self.context
                    .arena_mut()?
                    .set_generated_identifier_text(identifier, &planned)?;
            }
        }
        Ok(())
    }

    /// The guarded converted-loop-state accessors (every caller sits
    /// inside an `is_some` gate; the panic is the unreachable arm).
    fn loop_state(&self) -> &ConvertedLoopState {
        self.converted_loop_state
            .as_deref()
            .expect("converted loop state")
    }

    fn loop_state_mut(&mut self) -> &mut ConvertedLoopState {
        self.converted_loop_state
            .as_deref_mut()
            .expect("converted loop state")
    }
}

// ---------------------------------------------------------------------------
// Source file + simple statement/expression lanes
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitSourceFile @6.0.3
    /// tsc-hash: edf8f67d8fff1b6f3f66819bfeae6f9748e2f96c81c55763ef24917630ca5917
    /// tsc-span: _tsc.js:104982-105011
    fn visit_source_file(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::SOURCE_FILE_EXCLUDES,
            HierarchyFacts::SOURCE_FILE_INCLUDES,
        );
        let mut prologue: Vec<TransformNode> = Vec::new();
        let mut statements: Vec<TransformNode> = Vec::new();
        self.context.start_lexical_environment()?;
        let source_statements = {
            let NodeData::SourceFile(data) = &self.context.arena().node(node)?.data else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "source file",
                });
            };
            self.array_nodes(data.statements)?
        };
        let statement_offset = self.copy_prologue(
            &source_statements,
            &mut prologue,
            /*ensure_use_strict*/ false,
        )?;
        self.visit_statements_into(&source_statements, statement_offset, &mut statements)?;
        if !self.tagged_template_string_declarations.is_empty() {
            let declarations = std::mem::take(&mut self.tagged_template_string_declarations);
            let list = self.create_variable_declaration_list(declarations)?;
            let statement = self.create_variable_statement_from_list(list)?;
            statements.push(statement);
        }
        let environment = self.context.end_lexical_environment()?;
        self.merge_lexical_environment(&mut prologue, environment)?;
        self.insert_capture_this_for_node_if_needed(&mut prologue, node)?;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        let mut combined = prologue;
        combined.append(&mut statements);
        let statements_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, combined)?
        };
        let updated_data = {
            let NodeData::SourceFile(data) = &self.context.arena().node(node)?.data else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "source file",
                });
            };
            let mut data = data.clone();
            data.statements = Some(statements_array.array());
            NodeData::SourceFile(data)
        };
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: visitSwitchStatement @6.0.3
    /// tsc-hash: 190046ed895022aa398d84951c951655ea5e774dec6b84c5ada2a4858b31bd35
    /// tsc-span: _tsc.js:105012-105021
    fn visit_switch_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.converted_loop_state.is_some() {
            let saved = self.loop_state().allowed_non_labeled_jumps;
            self.loop_state_mut().allowed_non_labeled_jumps = saved.union(Jump::BREAK);
            let result = self.visit_each_child_required(node)?;
            self.loop_state_mut().allowed_non_labeled_jumps = saved;
            return Ok(result);
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: visitCaseBlock @6.0.3
    /// tsc-hash: c2af15bfefb2b24dfa120095ad3328610bd9de65e1eb3fd9d7c02048102e26a0
    /// tsc-span: _tsc.js:105022-105027
    fn visit_case_block(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::BLOCK_SCOPE_EXCLUDES,
            HierarchyFacts::BLOCK_SCOPE_INCLUDES,
        );
        let updated = self.visit_each_child_required(node)?;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitBlock @6.0.3
    /// tsc-hash: f92d9420e434cf356bc214f53100e12d8623c1dcf9eba976a9e0c7c049968ddc
    /// tsc-span: _tsc.js:106330-106338
    fn visit_block(
        &mut self,
        node: TransformNode,
        is_function_body: bool,
    ) -> Result<TransformNode, TransformError> {
        if is_function_body {
            return self.visit_each_child_required(node);
        }
        let (exclude, include) = if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::ITERATION_STATEMENT)
        {
            (
                HierarchyFacts::ITERATION_STATEMENT_BLOCK_EXCLUDES,
                HierarchyFacts::ITERATION_STATEMENT_BLOCK_INCLUDES,
            )
        } else {
            (
                HierarchyFacts::BLOCK_EXCLUDES,
                HierarchyFacts::BLOCK_INCLUDES,
            )
        };
        let ancestor = enter_subtree(&mut self.print_state.hierarchy_facts, exclude, include);
        let updated = self.visit_each_child_required(node)?;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitExpressionStatement @6.0.3
    /// tsc-hash: f18e77397af1d0ea9eba2f5bb4a868fd41b30dd61489f9fdf22573fbcd36d5d9
    /// tsc-span: _tsc.js:106339-106341
    fn visit_expression_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_each_child_with_unused_expression_result(node)
    }

    /// tsc-port: visitVoidExpression @6.0.3
    /// tsc-hash: 54c16b398fbb5ab4b9a9ce27217d3ca320b10e59515e9f089288b3388d511605
    /// tsc-span: _tsc.js:105069-105071
    fn visit_void_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_each_child_with_unused_expression_result(node)
    }

    /// tsc-port: visitParenthesizedExpression @6.0.3
    /// tsc-hash: 94058b819191804cd2cc9b65a6d3d0dceaff730f38adcbfeb381ef6b989713c0
    /// tsc-span: _tsc.js:106342-106344
    fn visit_parenthesized_expression(
        &mut self,
        node: TransformNode,
        expression_result_is_unused: bool,
    ) -> Result<TransformNode, TransformError> {
        if expression_result_is_unused {
            self.visit_each_child_with_unused_expression_result(node)
        } else {
            self.visit_each_child_required(node)
        }
    }

    /// `visitEachChild(node, visitorWithUnusedExpressionResult, context)` —
    /// the unused-result descent shared by expression statements, void
    /// expressions, parenthesized-unused positions, and comma folds. The
    /// only children these kinds carry are expressions.
    fn visit_each_child_with_unused_expression_result(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original = node;
        let data = self.context.arena().node(original)?.data.clone();
        let updated_data = match data {
            NodeData::ExpressionStatement(mut data) => {
                if let Some(expression) = data.expression {
                    let visited =
                        self.visit_with_unused_expression_result(self.node(expression))?;
                    let VisitOutcome::One(visited) = visited else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ExpressionStatement,
                            field: "expression",
                        });
                    };
                    data.expression = Some(visited.node());
                }
                NodeData::ExpressionStatement(data)
            }
            NodeData::VoidExpression(mut data) => {
                if let Some(expression) = data.expression {
                    let visited =
                        self.visit_with_unused_expression_result(self.node(expression))?;
                    let VisitOutcome::One(visited) = visited else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::VoidExpression,
                            field: "expression",
                        });
                    };
                    data.expression = Some(visited.node());
                }
                NodeData::VoidExpression(data)
            }
            NodeData::ParenthesizedExpression(mut data) => {
                if let Some(expression) = data.expression {
                    let visited =
                        self.visit_with_unused_expression_result(self.node(expression))?;
                    let VisitOutcome::One(visited) = visited else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ParenthesizedExpression,
                            field: "expression",
                        });
                    };
                    data.expression = Some(visited.node());
                }
                NodeData::ParenthesizedExpression(data)
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.kind(original)?,
                    field: "unused-expression-result descent",
                })
            }
        };
        if self.context.arena().node(original)?.data == updated_data {
            return Ok(original);
        }
        let flags = flags_after_update(self.context.arena(), original, &updated_data)?;
        self.context
            .factory()?
            .update_node(original, updated_data, flags)
    }

    /// tsc-port: visitIdentifier @6.0.3
    /// tsc-hash: 3f9c650aa2a11cdb26e226e22fbc8255a36e911c4c9e4f62f02f5e338cafbfd8
    /// tsc-span: _tsc.js:105072-105088
    fn visit_identifier(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        if self.converted_loop_state.is_some() {
            let is_arguments = {
                match self.context.arena().parse_tree_resolver_node(node)? {
                    Some(reference) => self.resolver.is_arguments_local_binding(reference)?,
                    None => false,
                }
            };
            if is_arguments {
                let binding = match &self.loop_state().arguments_name {
                    Some(binding) => binding.clone(),
                    None => {
                        let binding = self.allocate_numbered_binding("arguments")?;
                        self.loop_state_mut().arguments_name = Some(binding.clone());
                        binding
                    }
                };
                return self.create_generated_identifier(&binding);
            }
        }
        let has_extended_escape = {
            // NodeFlags 256 (IdentifierHasExtendedUnicodeEscape) has no
            // parse-side writer; the spelling channel is the source slice
            // (the parsed-tree facet arm derives the same way).
            let record = self.context.arena().node(node)?;
            let start = record.pos as usize;
            let end = (record.end as usize).min(self.current_text.len());
            start < end && self.current_text[start..end].contains("\\u{")
        };
        if has_extended_escape {
            let text = {
                let NodeData::Identifier(data) = &self.context.arena().node(node)?.data else {
                    return Ok(node);
                };
                unescape_leading_underscores(&data.escaped_text).to_owned()
            };
            let created = self.create_identifier(&text)?;
            // `setTextRange(...)` — position-threading would re-open the
            // printer's parsed-spelling channel (upstream prints the fresh
            // node's TEXT because the clone has no parent); the range rides
            // the map/comment channels (the get_name adaptation).
            self.set_source_map_range_from(created, node)?;
            self.set_comment_range_from(created, node)?;
            self.set_original(created, node)?;
            return Ok(created);
        }
        Ok(node)
    }

    /// tsc-port: visitTemplateLiteral @6.0.3
    /// tsc-hash: fde517b62bb80b6d1d22dd2d9dcf537b52c27350c8865d8037e4e9b105c88fe0
    /// tsc-span: _tsc.js:107912-107914
    fn visit_template_literal(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let text = match &self.context.arena().node(node)?.data {
            NodeData::NoSubstitutionTemplateLiteral(data) => data.text.clone(),
            NodeData::TemplateHead(data) => data.text.clone(),
            NodeData::TemplateMiddle(data) => data.text.clone(),
            NodeData::TemplateTail(data) => data.text.clone(),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.kind(node)?,
                    field: "template literal",
                })
            }
        };
        let created = self.create_string_literal(&text)?;
        self.set_text_range(created, node)?;
        Ok(created)
    }

    /// tsc-port: visitStringLiteral @6.0.3
    /// tsc-hash: 681f6439ed00ed909b262b464c9792b22130db1231779524d766d3414928d92e
    /// tsc-span: _tsc.js:107915-107920
    fn visit_string_literal(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (has_escape, text) = match &self.context.arena().node(node)?.data {
            NodeData::StringLiteral(data) => (
                data.has_extended_unicode_escape == Some(true),
                data.text.clone(),
            ),
            _ => (false, String::new()),
        };
        if has_escape {
            let created = self.create_string_literal(&text)?;
            self.set_text_range(created, node)?;
            return Ok(created);
        }
        Ok(node)
    }

    /// tsc-port: visitNumericLiteral @6.0.3
    /// tsc-hash: 4caf4d932461774f5450b679677ebf901915403b0c7650ac0fe3e010d027996b
    /// tsc-span: _tsc.js:107921-107926
    fn visit_numeric_literal(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let record = self.context.arena().node(node)?;
        let is_binary_or_octal = record.numeric_literal_flags & 384 != 0;
        let text = match &record.data {
            NodeData::NumericLiteral(data) => data.text.clone(),
            _ => String::new(),
        };
        if is_binary_or_octal {
            // `createNumericLiteral(node.text)` — the parse record's text
            // is the COOKED decimal value; the fresh literal drops the
            // source spelling channel, printing decimal.
            let created = self.create_numeric_literal(&text)?;
            self.set_text_range(created, node)?;
            return Ok(created);
        }
        Ok(node)
    }

    /// tsc-port: visitComputedPropertyName @6.0.3
    /// tsc-hash: 0e8ce1579d4516671fdca0e0a617143696f43dd041b6bc04b56d10b967905938
    /// tsc-span: _tsc.js:107648-107650
    fn visit_computed_property_name(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_each_child_required(node)
    }

    /// tsc-port: visitYieldExpression @6.0.3
    /// tsc-hash: 0cbeb41776ccdeeb9f17e80b64a30c6ae7f8e8f7c2bea8708036f8419518029d
    /// tsc-span: _tsc.js:107651-107653
    fn visit_yield_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_each_child_required(node)
    }

    /// tsc-port: visitSpreadElement @6.0.3
    /// tsc-hash: 4739a46ea49b2710d8998664d780ed2343a4b97cb404cef87c4a779d2ff8761f
    /// tsc-span: _tsc.js:107909-107911
    fn visit_spread_element(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let expression = match &self.context.arena().node(node)?.data {
            NodeData::SpreadElement(data) => data.expression,
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::SpreadElement,
            field: "expression",
        })?;
        self.visit_required_expression(self.node(expression))
    }

    /// tsc-port: visitTaggedTemplateExpression @6.0.3
    /// tsc-hash: 311c98a5b65cc44b160bbf1a221a3950679691e0fca83a65b8449b4ef4a8a919
    /// tsc-span: _tsc.js:107927-107936
    fn visit_tagged_template_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        tagged_template::process_tagged_template_expression(self, node, ProcessLevel::All)
    }

    /// tsc-port: visitReturnStatement @6.0.3
    /// tsc-hash: 1a1668d953062e8ab8d142a5c8c81e784c5d24bc6084257524e4ea141e6f8cde
    /// tsc-span: _tsc.js:105034-105054
    fn visit_return_statement(
        &mut self,
        mut node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.converted_loop_state.is_some() {
            self.loop_state_mut().non_local_jumps =
                self.loop_state().non_local_jumps.union(Jump::RETURN);
            if self.is_return_void_statement_in_constructor_with_captured_super(node)? {
                node = self.return_captured_this(node)?;
            }
            let expression = match &self.context.arena().node(node)?.data {
                NodeData::ReturnStatement(data) => data.expression.map(|id| self.node(id)),
                _ => None,
            };
            let value = match expression {
                Some(expression) => self.visit_required_expression(expression)?,
                None => self.create_void_zero()?,
            };
            let property = self.create_property_assignment_text("value", value)?;
            let object = self.create_object_literal(vec![property], /*multi_line*/ false)?;
            return self.create_return_statement(Some(object));
        }
        if self.is_return_void_statement_in_constructor_with_captured_super(node)? {
            return self.return_captured_this(node);
        }
        self.visit_each_child_required(node)
    }
}

/// tsc-port: unescapeLeadingUnderscores @6.0.3
/// tsc-hash: e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271
/// tsc-span: _tsc.js:11441-11444
fn unescape_leading_underscores(identifier: &str) -> &str {
    let bytes = identifier.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'_' && bytes[1] == b'_' && bytes[2] == b'_' {
        &identifier[1..]
    } else {
        identifier
    }
}

fn is_iteration_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::WhileStatement
            | SyntaxKind::DoStatement
    )
}
// ---------------------------------------------------------------------------
// this / new.target capture + super (§7 step 2)
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: createCapturedThis @6.0.3
    /// tsc-hash: b34b092a136f191e4afe1cadca97a3d20b422f35611287f7fa7b53ffbd81c3fd
    /// tsc-span: _tsc.js:105031-105033
    ///
    /// `createUniqueName("_this", Optimistic | FileLevel)` — ONE shared
    /// file-level-optimistic binding per source file (§5: upstream mints a
    /// fresh instance per site and the printer converges same-text
    /// optimistic instances; the shared binding reproduces the measured
    /// spellings incl. the collision fixtures).
    fn create_captured_this(&mut self) -> Result<TransformNode, TransformError> {
        let binding = match &self.print_state.captured_this {
            Some(binding) => binding.clone(),
            None => {
                let binding = self.allocate_file_level_optimistic_binding("_this")?;
                self.print_state.captured_this = Some(binding.clone());
                binding
            }
        };
        self.create_generated_identifier(&binding)
    }

    /// The `_newTarget` shared binding (`createUniqueName("_newTarget",
    /// Optimistic | FileLevel)` at both mint sites :105992/:107966).
    fn create_new_target_identifier(&mut self) -> Result<TransformNode, TransformError> {
        let binding = match &self.print_state.new_target {
            Some(binding) => binding.clone(),
            None => {
                let binding = self.allocate_file_level_optimistic_binding("_newTarget")?;
                self.print_state.new_target = Some(binding.clone());
                binding
            }
        };
        self.create_generated_identifier(&binding)
    }

    /// tsc-port: createSyntheticSuper @6.0.3
    /// tsc-hash: 32fcaf9fa900d7c23e9a318e0917e585cf489bd790e4d5e63760e11e2a5fd652
    /// tsc-span: _tsc.js:107953-107955
    fn create_synthetic_super(&mut self) -> Result<TransformNode, TransformError> {
        let binding = match &self.print_state.synthetic_super {
            Some(binding) => binding.clone(),
            None => {
                let binding = self.allocate_file_level_optimistic_binding("_super")?;
                self.print_state.synthetic_super = Some(binding.clone());
                binding
            }
        };
        self.create_generated_identifier(&binding)
    }

    /// tsc-port: returnCapturedThis @6.0.3
    /// tsc-hash: a63cb5a00ee2252a88554fd93bd82cf3c590e67baedfe430bf9ac9580820b681
    /// tsc-span: _tsc.js:105028-105030
    fn return_captured_this(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let captured = self.create_captured_this()?;
        let statement = self.create_return_statement(Some(captured))?;
        self.set_original(statement, node)?;
        Ok(statement)
    }

    /// tsc-port: isReturnVoidStatementInConstructorWithCapturedSuper @6.0.3
    /// tsc-hash: f6c8c6170bc4f560ac839a8f039fb2171d7795a3b3f550150f68a0faabe0d3ce
    /// tsc-span: _tsc.js:104790-104792
    fn is_return_void_statement_in_constructor_with_captured_super(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if !self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::CONSTRUCTOR_WITH_SUPER_CALL)
        {
            return Ok(false);
        }
        match &self.context.arena().node(node)?.data {
            NodeData::ReturnStatement(data) => Ok(data.expression.is_none()),
            _ => Ok(false),
        }
    }

    /// tsc-port: visitThisKeyword @6.0.3
    /// tsc-hash: 04811031550ee1ad94085e3b8fa9e441793913e58aeb63e40ed07a21739908a8
    /// tsc-span: _tsc.js:105055-105068
    fn visit_this_keyword(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.print_state.hierarchy_facts = self
            .print_state
            .hierarchy_facts
            .union(HierarchyFacts::LEXICAL_THIS);
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::ARROW_FUNCTION)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::STATIC_INITIALIZER)
        {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        if self.converted_loop_state.is_some() {
            if self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::ARROW_FUNCTION)
            {
                self.loop_state_mut().contains_lexical_this = true;
                return Ok(node);
            }
            // `convertedLoopState.thisName ||= createUniqueName("this")`
            let binding = match &self.loop_state().this_name {
                Some(binding) => binding.clone(),
                None => {
                    let binding = self.allocate_numbered_binding("this")?;
                    self.loop_state_mut().this_name = Some(binding.clone());
                    binding
                }
            };
            return self.create_generated_identifier(&binding);
        }
        Ok(node)
    }

    /// tsc-port: visitSuperKeyword @6.0.3
    /// tsc-hash: 6d19da8b75d9f4d7234044542bdbfff72f391935c4dc1f7273270c6484192a19
    /// tsc-span: _tsc.js:107956-107962
    fn visit_super_keyword(
        &mut self,
        node: TransformNode,
        is_expression_of_call: bool,
    ) -> Result<TransformNode, TransformError> {
        let expression = if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::NON_STATIC_CLASS_ELEMENT)
            && !is_expression_of_call
        {
            let synthetic = self.create_synthetic_super()?;
            self.set_original(synthetic, node)?;
            let prototype = self.create_identifier("prototype")?;
            self.create_property_access(synthetic, prototype)?
        } else {
            self.create_synthetic_super()?
        };
        self.set_original(expression, node)?;
        self.set_comment_range_from(expression, node)?;
        self.set_source_map_range_from(expression, node)?;
        Ok(expression)
    }

    /// tsc-port: visitMetaProperty @6.0.3
    /// tsc-hash: d21802d0b4b5f3394247c5b071c3a3a78e4c847d6b72d70e1b950ed1de9f090b
    /// tsc-span: _tsc.js:107963-107969
    fn visit_meta_property(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let is_new_target = {
            let NodeData::MetaProperty(data) = &self.context.arena().node(node)?.data else {
                return Ok(node);
            };
            data.keyword_token == SyntaxKind::NewKeyword
                && match data.name {
                    Some(name) => matches!(
                        &self.context.arena().node(self.node(name))?.data,
                        NodeData::Identifier(id) if id.escaped_text == "target"
                    ),
                    None => false,
                }
        };
        if is_new_target {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::NEW_TARGET);
            return self.create_new_target_identifier();
        }
        Ok(node)
    }

    /// tsc-port: insertCaptureThisForNodeIfNeeded @6.0.3
    /// tsc-hash: 45639599b45b90b5d3d35c4ec5549e1867983f1535774671f0a08c0231f219e4
    /// tsc-span: _tsc.js:105918-105924
    fn insert_capture_this_for_node_if_needed(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::CAPTURED_LEXICAL_THIS)
            && self.kind(node)? != SyntaxKind::ArrowFunction
        {
            let this = self.create_this_token()?;
            self.insert_capture_this_for_node(statements, node, this)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: insertCaptureThisForNode @6.0.3
    /// tsc-hash: 97a38bc8c3789af8530cfa22114dea94aea9c5c38df6c66977819cdc509a1c1e
    /// tsc-span: _tsc.js:105925-105944
    fn insert_capture_this_for_node(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
        initializer: TransformNode,
    ) -> Result<(), TransformError> {
        self.enable_substitutions_for_captured_this()?;
        let captured = self.create_captured_this()?;
        let declaration = self.create_variable_declaration_plain(captured, Some(initializer))?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        let statement = self.create_variable_statement_from_list(list)?;
        self.add_emit_flags(
            statement,
            EmitFlags::NO_COMMENTS | EmitFlags::CUSTOM_PROLOGUE,
        )?;
        self.set_source_map_range_from(statement, node)?;
        self.insert_statement_after_custom_prologue(statements, statement)?;
        Ok(())
    }

    /// tsc-port: insertCaptureNewTargetIfNeeded @6.0.3
    /// tsc-hash: 820984ac3bd8d30b44abadfe06e1257f60177a6f672c5acaa9a89549796dc117
    /// tsc-span: _tsc.js:105945-106005
    fn insert_capture_new_target_if_needed(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if !self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::NEW_TARGET)
        {
            return Ok(());
        }
        let kind = self.kind(node)?;
        let new_target = match kind {
            SyntaxKind::ArrowFunction => return Ok(()),
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                self.create_void_zero()?
            }
            SyntaxKind::Constructor => {
                let this = self.create_this_no_substitution()?;
                let constructor = self.create_identifier("constructor")?;
                self.create_property_access(this, constructor)?
            }
            SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression => {
                // this && this instanceof LocalName ? this.constructor : void 0
                let this_a = self.create_this_no_substitution()?;
                let this_b = self.create_this_no_substitution()?;
                let local = self.get_local_name(node, /*allow_comments*/ false)?;
                let instance_of =
                    self.create_binary(this_b, SyntaxKind::InstanceOfKeyword, local)?;
                let condition = self.create_logical_and(this_a, instance_of)?;
                let this_c = self.create_this_no_substitution()?;
                let constructor = self.create_identifier("constructor")?;
                let when_true = self.create_property_access(this_c, constructor)?;
                let when_false = self.create_void_zero()?;
                self.create_conditional(condition, when_true, when_false)?
            }
            _ => {
                return Err(assembly_kind_error(kind, "insertCaptureNewTargetIfNeeded"));
            }
        };
        let target = self.create_new_target_identifier()?;
        let declaration = self.create_variable_declaration_plain(target, Some(new_target))?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        let statement = self.create_variable_statement_from_list(list)?;
        self.add_emit_flags(
            statement,
            EmitFlags::NO_COMMENTS | EmitFlags::CUSTOM_PROLOGUE,
        )?;
        self.insert_statement_after_custom_prologue(statements, statement)?;
        Ok(())
    }

    /// `createActualThis()` — `setEmitFlags(createThis(), NoSubstitution)`
    /// (:105653-105655).
    fn create_this_no_substitution(&mut self) -> Result<TransformNode, TransformError> {
        let this = self.create_this_token()?;
        self.add_emit_flags(this, EmitFlags::NO_SUBSTITUTION)?;
        Ok(this)
    }
}

impl Es2015Visitor<'_, '_, '_> {
    /// `createUniqueName(text, Optimistic | FileLevel)` — the shared
    /// file-level-optimistic family.
    fn allocate_file_level_optimistic_binding(
        &mut self,
        text: &str,
    ) -> Result<TargetBinding, TransformError> {
        // FileLevel-optimistic planning resolves SOURCE-name collisions at
        // plan time (`isFileLevelUniqueName` ignores generated peers); the
        // finalizer keeps the planned spelling verbatim.
        let planned = self.parsed_names.optimistic_candidate(text);
        let provisional = self
            .generated_bindings
            .reserve_planned_file_level_optimistic_with_policy(
                planned, /*reserve_in_nested_scopes*/ true,
            );
        TargetBinding::allocate_file_level_optimistic_reserved_in_nested_scopes(
            self.context,
            text.to_owned(),
            provisional,
        )
    }
}

fn assembly_kind_error(kind: SyntaxKind, site: &'static str) -> TransformError {
    TransformError::RequiredChildRemoved {
        parent: kind,
        field: site,
    }
}
// ---------------------------------------------------------------------------
// Prologue + lexical-environment machinery (§4.2 pins)
// ---------------------------------------------------------------------------
// NOTE: these are Es2015Visitor methods (self = the visitor).

impl Es2015Visitor<'_, '_, '_> {
    /// `isPrologueDirective` — ExpressionStatement over a StringLiteral.
    fn is_prologue_directive(&self, node: TransformNode) -> Result<bool, TransformError> {
        let arena = self.context.arena();
        if let NodeData::ExpressionStatement(data) = &arena.node(node)?.data {
            if let Some(expression) = data.expression {
                let expression = self.node(expression);
                return Ok(matches!(
                    arena.node(expression)?.data,
                    NodeData::StringLiteral(_)
                ));
            }
        }
        Ok(false)
    }

    /// `isCustomPrologue` — `getEmitFlags(node) & EmitFlags.CustomPrologue`.
    fn is_custom_prologue(&self, node: TransformNode) -> bool {
        self.context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
    }

    /// tsc-port: isHoistedFunction @6.0.3
    /// tsc-hash: a43dffc56712a0f0a13148f4eca8cd05064849784894e44d65835c84e84b880a
    /// tsc-span: _tsc.js:14167-14169
    fn is_hoisted_function(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(self.is_custom_prologue(node)
            && matches!(
                self.context.arena().node(node)?.data,
                NodeData::FunctionDeclaration(_)
            ))
    }

    /// tsc-port: isHoistedVariableStatement @6.0.3
    /// tsc-hash: be4121319d7decd5d3087cc7fd9d2eb5510b17ae67b08bc236fb082f77b141d8
    /// tsc-span: _tsc.js:14173-14175
    fn is_hoisted_variable_statement(&self, node: TransformNode) -> Result<bool, TransformError> {
        if !self.is_custom_prologue(node) {
            return Ok(false);
        }
        let arena = self.context.arena();
        let NodeData::VariableStatement(statement) = &arena.node(node)?.data else {
            return Ok(false);
        };
        let Some(list) = statement.declaration_list else {
            return Ok(false);
        };
        let list = self.node(list);
        let NodeData::VariableDeclarationList(list_data) = &arena.node(list)?.data else {
            return Ok(false);
        };
        let Some(declarations) = list_data.declarations else {
            return Ok(true);
        };
        let declarations = arena.node_array(tsc_syntax_array(self.source, declarations))?;
        for declaration in declarations.nodes.clone() {
            let declaration = self.node(declaration);
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(declaration)?.data
            else {
                return Ok(false);
            };
            // `isHoistedVariable`: identifier name AND no initializer
            // (_tsc.js:14170-14172).
            if data.initializer.is_some() {
                return Ok(false);
            }
            let Some(name) = data.name else {
                return Ok(false);
            };
            let name = self.node(name);
            if !matches!(
                self.context.arena().node(name)?.data,
                NodeData::Identifier(_)
            ) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `findSpanEnd(array, test, start)`.
    fn find_span_end<F>(
        &self,
        statements: &[TransformNode],
        mut test: F,
        start: usize,
    ) -> Result<usize, TransformError>
    where
        F: FnMut(&Self, TransformNode) -> Result<bool, TransformError>,
    {
        let mut index = start;
        while index < statements.len() && test(self, statements[index])? {
            index += 1;
        }
        Ok(index)
    }

    /// tsc-port: mergeLexicalEnvironment @6.0.3
    /// tsc-hash: ac1f665ea3f8a127f7cb6dbd55b79a8e307e38359a9aef18a2f5dada71bcd2c2
    /// tsc-span: _tsc.js:24889-24932
    ///
    /// The Rust `LexicalEnvironment` record splits hoisted names /
    /// hoisted functions / initialization statements; this first
    /// materializes the upstream `endLexicalEnvironment` statements list
    /// (`_tsc.js:116163-116193` — functions, then ONE CustomPrologue var
    /// statement over the hoisted names, then initialization statements)
    /// and then runs the pinned three-span splice + directive dedup.
    fn merge_lexical_environment(
        &mut self,
        statements: &mut Vec<TransformNode>,
        environment: LexicalEnvironment,
    ) -> Result<(), TransformError> {
        let declarations = self.materialize_lexical_environment(environment)?;
        if declarations.is_empty() {
            return Ok(());
        }
        let left_standard_prologue_end =
            self.find_span_end(statements, |v, n| v.is_prologue_directive(n), 0)?;
        let left_hoisted_functions_end = self.find_span_end(
            statements,
            |v, n| v.is_hoisted_function(n),
            left_standard_prologue_end,
        )?;
        let left_hoisted_variables_end = self.find_span_end(
            statements,
            |v, n| v.is_hoisted_variable_statement(n),
            left_hoisted_functions_end,
        )?;
        let right_standard_prologue_end =
            self.find_span_end(&declarations, |v, n| v.is_prologue_directive(n), 0)?;
        let right_hoisted_functions_end = self.find_span_end(
            &declarations,
            |v, n| v.is_hoisted_function(n),
            right_standard_prologue_end,
        )?;
        let right_hoisted_variables_end = self.find_span_end(
            &declarations,
            |v, n| v.is_hoisted_variable_statement(n),
            right_hoisted_functions_end,
        )?;
        let right_custom_prologue_end = self.find_span_end(
            &declarations,
            |v, n| Ok(v.is_custom_prologue(n)),
            right_hoisted_variables_end,
        )?;
        if right_custom_prologue_end != declarations.len() {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "lexical environment declarations must be prologues",
            });
        }
        if right_custom_prologue_end > right_hoisted_variables_end {
            statements.splice(
                left_hoisted_variables_end..left_hoisted_variables_end,
                declarations[right_hoisted_variables_end..right_custom_prologue_end]
                    .iter()
                    .copied(),
            );
        }
        if right_hoisted_variables_end > right_hoisted_functions_end {
            statements.splice(
                left_hoisted_functions_end..left_hoisted_functions_end,
                declarations[right_hoisted_functions_end..right_hoisted_variables_end]
                    .iter()
                    .copied(),
            );
        }
        if right_hoisted_functions_end > right_standard_prologue_end {
            statements.splice(
                left_standard_prologue_end..left_standard_prologue_end,
                declarations[right_standard_prologue_end..right_hoisted_functions_end]
                    .iter()
                    .copied(),
            );
        }
        if right_standard_prologue_end > 0 {
            if left_standard_prologue_end == 0 {
                statements.splice(
                    0..0,
                    declarations[0..right_standard_prologue_end].iter().copied(),
                );
            } else {
                let mut left_prologues = std::collections::BTreeSet::new();
                for statement in statements[..left_standard_prologue_end].iter() {
                    left_prologues.insert(self.directive_text(*statement)?);
                }
                for declaration in declarations[..right_standard_prologue_end].iter().rev() {
                    if !left_prologues.contains(&self.directive_text(*declaration)?) {
                        statements.insert(0, *declaration);
                    }
                }
            }
        }
        Ok(())
    }

    /// Materialize `endLexicalEnvironment` statements
    /// (`_tsc.js:116163-116193`): hoisted functions, then one
    /// CustomPrologue var statement over the hoisted names, then the
    /// initialization statements.
    fn materialize_lexical_environment(
        &mut self,
        environment: LexicalEnvironment,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements = Vec::new();
        statements.extend_from_slice(environment.function_declarations());
        if !environment.variable_declarations().is_empty() {
            let declarations = environment
                .variable_declarations()
                .iter()
                .copied()
                .map(|name| self.create_variable_declaration_plain(name, None))
                .collect::<Result<Vec<_>, _>>()?;
            let statement = self.create_variable_statement_from_declarations(declarations)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.push(statement);
        }
        statements.extend_from_slice(environment.initialization_statements());
        Ok(statements)
    }
}

// ---------------------------------------------------------------------------
// Prologue insertion + small addenda
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: insertStatementsAfterCustomPrologue @6.0.3
    /// tsc-hash: d761e394849a886073c226027fd159cc65bbef99618c3957487240f411612c90
    /// tsc-span: _tsc.js:12947-12949
    fn insert_statements_after_custom_prologue(
        &mut self,
        to: &mut Vec<TransformNode>,
        from: &[TransformNode],
    ) -> Result<(), TransformError> {
        self.insert_statements_after_prologue_worker(to, from, /*is_custom*/ true)
    }

    /// tsc-port: insertStatementAfterCustomPrologue @6.0.3
    /// tsc-hash: 7b1a417fc2da425e75dac5bedf61ad6e0021e0b6e88543114a64e6942fc300b6
    /// tsc-span: _tsc.js:12950-12952
    fn insert_statement_after_custom_prologue(
        &mut self,
        to: &mut Vec<TransformNode>,
        statement: TransformNode,
    ) -> Result<(), TransformError> {
        self.insert_statements_after_custom_prologue(to, &[statement])
    }

    /// tsc-port: insertStatementsAfterStandardPrologue @6.0.3
    /// tsc-hash: b61c455c626f61f6b0dbf0329fe6550d7229aabb4f49642282f99725391fc54a
    /// tsc-span: _tsc.js:12941-12943
    ///
    /// The standard arm of the shared insertStatementsAfterPrologue
    /// worker (:12925-12940).
    fn insert_statements_after_standard_prologue(
        &mut self,
        to: &mut Vec<TransformNode>,
        from: &[TransformNode],
    ) -> Result<(), TransformError> {
        self.insert_statements_after_prologue_worker(to, from, /*is_custom*/ false)
    }

    fn insert_statements_after_prologue_worker(
        &mut self,
        to: &mut Vec<TransformNode>,
        from: &[TransformNode],
        is_custom: bool,
    ) -> Result<(), TransformError> {
        if from.is_empty() {
            return Ok(());
        }
        let mut index = 0;
        while index < to.len() {
            let statement = to[index];
            let keep = if is_custom {
                self.is_prologue_directive(statement)? || self.is_custom_prologue(statement)
            } else {
                self.is_prologue_directive(statement)?
            };
            if !keep {
                break;
            }
            index += 1;
        }
        to.splice(index..index, from.iter().copied());
        Ok(())
    }

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
        self.copy_custom_prologue(source, target, offset, CustomPrologueFilter::All)
    }

    /// tsc-port: copyStandardPrologue @6.0.3
    /// tsc-hash: 7a83f5b2d0bfada432bb729b16e41de52a8cb69e13f5bdb19f627d23e06607f4
    /// tsc-span: _tsc.js:24837-24857
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
                field: "ensureUseStrict prologue arm (alwaysStrict is out of the B-4 position)",
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
    ///
    /// The VISITED copy with the optional filter (`isHoistedFunction` /
    /// `isHoistedVariableStatement` — the two-phase `transformFunctionBody`
    /// copies).
    fn copy_custom_prologue(
        &mut self,
        source: &[TransformNode],
        target: &mut Vec<TransformNode>,
        statement_offset: usize,
        filter: CustomPrologueFilter,
    ) -> Result<usize, TransformError> {
        let mut offset = statement_offset;
        while offset < source.len() {
            let statement = source[offset];
            let is_custom = self.is_custom_prologue(statement);
            let passes = is_custom
                && match filter {
                    CustomPrologueFilter::All => true,
                    CustomPrologueFilter::HoistedFunctions => {
                        self.is_hoisted_function(statement)?
                    }
                    CustomPrologueFilter::HoistedVariableStatements => {
                        self.is_hoisted_variable_statement(statement)?
                    }
                };
            if passes {
                if let Some(visited) = self.visit_statement_lifted(statement)? {
                    target.push(visited);
                }
                offset += 1;
            } else {
                break;
            }
        }
        Ok(offset)
    }

    /// tsc-port: arrayIsEqualTo @6.0.3
    /// tsc-hash: dbabd399703753a41ee061112da610ed985667c1ba1797bd4c371924ebe47395
    /// tsc-span: _tsc.js:457-470
    fn array_is_equal_to(&self, left: &[TransformNode], right: &[TransformNode]) -> bool {
        left.len() == right.len() && left.iter().zip(right.iter()).all(|(a, b)| a == b)
    }

    /// tsc-port: unwrapInnermostStatementOfLabel @6.0.3
    /// tsc-hash: b2ed1607745b0f49fd60c8231a9a8d0c223f8eacaba7ceabbce4f801d009bf91
    /// tsc-span: _tsc.js:14393-14403
    fn unwrap_innermost_statement_of_label(
        &mut self,
        node: TransformNode,
        record_labels: bool,
    ) -> Result<TransformNode, TransformError> {
        let mut current = node;
        loop {
            if record_labels {
                self.record_label(current)?;
            }
            let (label_statement,) = {
                let NodeData::LabeledStatement(data) = &self.context.arena().node(current)?.data
                else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::LabeledStatement,
                        field: "labeled statement",
                    });
                };
                (data.statement.map(|id| self.node(id)),)
            };
            let statement = label_statement.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::LabeledStatement,
                field: "statement",
            })?;
            if self.kind(statement)? != SyntaxKind::LabeledStatement {
                return Ok(statement);
            }
            current = statement;
        }
    }

    /// tsc-port: recordLabel @6.0.3
    /// tsc-hash: 7766cbffd08668b0700407f95c14a9d112c7349b37a520b5f624d40fede08d26
    /// tsc-span: _tsc.js:106492-106494
    fn record_label(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let label_text = self.labeled_statement_label_text(node)?;
        if let Some(state) = self.converted_loop_state.as_mut() {
            state.labels.insert(label_text, true);
        }
        Ok(())
    }

    /// tsc-port: resetLabel @6.0.3
    /// tsc-hash: 2917d74d5fa128ee63b3c7a562a397c29a2d8336accc3985b953437b694df144
    /// tsc-span: _tsc.js:106495-106497
    fn reset_label(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let label_text = self.labeled_statement_label_text(node)?;
        if let Some(state) = self.converted_loop_state.as_mut() {
            state.labels.insert(label_text, false);
        }
        Ok(())
    }

    fn labeled_statement_label_text(&self, node: TransformNode) -> Result<String, TransformError> {
        let label = match &self.context.arena().node(node)?.data {
            NodeData::LabeledStatement(data) => data.label.map(|id| self.node(id)),
            _ => None,
        };
        let label = label.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::LabeledStatement,
            field: "label",
        })?;
        self.identifier_text(label)
    }

    /// tsc-port: restoreEnclosingLabel @6.0.3
    /// tsc-hash: fe151529af3462a6c56359506563a0b4173bcd3ea5c0605e9beb4ac6a2a8d298
    /// tsc-span: _tsc.js:24655-24668
    fn restore_enclosing_label(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        reset_labels: bool,
    ) -> Result<TransformNode, TransformError> {
        let Some(outermost) = outermost_labeled_statement else {
            return Ok(node);
        };
        let (label, inner) = {
            let NodeData::LabeledStatement(data) = &self.context.arena().node(outermost)?.data
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::LabeledStatement,
                    field: "labeled statement",
                });
            };
            (
                data.label.map(|id| self.node(id)),
                data.statement.map(|id| self.node(id)),
            )
        };
        let inner_updated = match inner {
            Some(inner) if self.kind(inner)? == SyntaxKind::LabeledStatement => {
                self.restore_enclosing_label(node, Some(inner), reset_labels)?
            }
            _ => node,
        };
        let updated_data = NodeData::LabeledStatement(tsc_syntax::nodes::LabeledStatementData {
            label: label.map(|label| label.node()),
            statement: Some(inner_updated.node()),
        });
        let updated = if self.context.arena().node(outermost)?.data == updated_data {
            outermost
        } else {
            let flags = flags_after_update(self.context.arena(), outermost, &updated_data)?;
            self.context
                .factory()?
                .update_node(outermost, updated_data, flags)?
        };
        if reset_labels && self.converted_loop_state.is_some() {
            self.reset_label(outermost)?;
        }
        Ok(updated)
    }

    /// tsc-port: skipOuterExpressions @6.0.3
    /// tsc-hash: 8b1eff7c004dde6bbe6b5940ba064195f1aea6668ca5d8b1f4a69bf9cec4dec1
    /// tsc-span: _tsc.js:27582-27587
    ///
    /// `OuterExpressionKinds.All = 63`.
    fn skip_outer_expressions(&self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let mut current = node;
        loop {
            let next = match &self.context.arena().node(current)?.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                _ => None,
            };
            match next {
                Some(next) => current = self.node(next),
                None => return Ok(current),
            }
        }
    }

    /// tsc-port: getSuperCallFromStatement @6.0.3
    /// tsc-hash: f777d5cf25bf07fb7171f609cfc81def662cbf6df522af76a81dc77e3f355287
    /// tsc-span: _tsc.js:93070-93076
    fn get_super_call_from_statement(
        &self,
        statement: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let expression = self.skip_outer_expressions(self.node(expression))?;
        if self.is_super_call(expression)? {
            Ok(Some(expression))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: isSuperCall @6.0.3
    /// tsc-hash: ed46d3b633bea556783f24654454e51a494901f4fde4c70b6c39bac09b32f806
    /// tsc-span: _tsc.js:14147-14149
    fn is_super_call(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        match data.expression {
            Some(expression) => Ok(self.kind(self.node(expression))? == SyntaxKind::SuperKeyword),
            None => Ok(false),
        }
    }

    /// tsc-port: isSuperProperty @6.0.3
    /// tsc-hash: d71f4915c785ca5e6a0642e8c3e85529c28ea19447923c8a337ab6ffa5c4f262
    /// tsc-span: _tsc.js:14608-14611
    fn is_super_property(&self, node: TransformNode) -> Result<bool, TransformError> {
        let expression = match &self.context.arena().node(node)?.data {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        match expression {
            Some(expression) => Ok(self.kind(self.node(expression))? == SyntaxKind::SuperKeyword),
            None => Ok(false),
        }
    }

    fn is_binding_pattern(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(node)?.data,
            NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_)
        ))
    }

    fn binding_pattern_has_elements(&self, node: TransformNode) -> Result<bool, TransformError> {
        let elements = match &self.context.arena().node(node)?.data {
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        match elements {
            Some(elements) => Ok(!self
                .context
                .arena()
                .node_array(tsc_syntax_array(self.source, elements))?
                .nodes
                .is_empty()),
            None => Ok(false),
        }
    }

    /// tsc-port: createTypeCheck @6.0.3
    /// tsc-hash: 545917e4fff60f1d07b445c7b41156183e8da6c608d5723705fe1edc1bf1f553
    /// tsc-span: _tsc.js:24548-24550
    fn create_type_check(
        &mut self,
        value: TransformNode,
        tag: &str,
    ) -> Result<TransformNode, TransformError> {
        if tag == "undefined" {
            // `value === void 0`
            let void_zero = self.create_void_zero()?;
            return self.create_strict_equality(value, void_zero);
        }
        let source = self.source;
        let type_of = {
            let flags = self.child_flags(&[value])?;
            self.context.factory()?.create_node(
                source,
                NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
                    expression: Some(value.node()),
                }),
                flags,
            )?
        };
        let literal = self.create_string_literal(tag)?;
        self.create_strict_equality(type_of, literal)
    }

    fn function_parameters(
        &self,
        node: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameters = match &self.context.arena().node(node)?.data {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        self.array_nodes(parameters)
    }

    fn function_body(&self, node: TransformNode) -> Result<Option<TransformNode>, TransformError> {
        let body = match &self.context.arena().node(node)?.data {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            NodeData::Constructor(data) => data.body,
            _ => None,
        };
        Ok(body.map(|id| self.node(id)))
    }
}

/// The `copyCustomPrologue` filter parameter.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CustomPrologueFilter {
    All,
    HoistedFunctions,
    HoistedVariableStatements,
}

// ---------------------------------------------------------------------------
// Parameters (visitParameter + default/rest prologue protocol)
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitParameterList @6.0.3
    /// tsc-hash: 75f4e96e0f53dac4523f71d86dc9a4216465c88b670afeb6202b7853fb27d8fa
    /// tsc-span: _tsc.js:91168-91181
    ///
    /// start env → set IN_PARAMETERS → visit parameters → clear → suspend
    /// env. The `addDefaultValueAssignmentsIfNeeded` arm is gated
    /// `getEmitScriptTarget(...) >= ES2015` and stays DORMANT at the ES5
    /// construction this packet makes.
    fn visit_parameter_list(
        &mut self,
        parameters: &[TransformNode],
    ) -> Result<Vec<TransformNode>, TransformError> {
        self.context.start_lexical_environment()?;
        self.context
            .set_lexical_environment_flags(crate::LexicalEnvironmentFlags::IN_PARAMETERS, true)?;
        let mut visited = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            match self.visit(*parameter)? {
                VisitOutcome::Elided => {}
                VisitOutcome::One(node) => visited.push(node),
                VisitOutcome::Many(_) => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::Parameter,
                        field: "parameter position received a statement list",
                    })
                }
            }
        }
        self.context
            .set_lexical_environment_flags(crate::LexicalEnvironmentFlags::IN_PARAMETERS, false)?;
        self.context.suspend_lexical_environment()?;
        Ok(visited)
    }

    /// tsc-port: visitParameter @6.0.3
    /// tsc-hash: a1848862a06c3fe5ac65f77b69d47da37b30ebaa5a076ca0bb5d75f7c90d4e93
    /// tsc-span: _tsc.js:105672-105722
    fn visit_parameter(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let (has_dot_dot_dot, name, has_initializer) = {
            let NodeData::Parameter(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Parameter, "parameter"));
            };
            (
                data.dot_dot_dot_token.is_some(),
                data.name.map(|id| self.node(id)),
                data.initializer.is_some(),
            )
        };
        if has_dot_dot_dot {
            return Ok(None);
        }
        if let Some(name_node) = name {
            if self.is_binding_pattern(name_node)? {
                let generated = self.get_generated_name_for_node(node)?;
                let parameter = self.create_parameter_declaration(generated)?;
                self.set_text_range(parameter, node)?;
                self.set_original(parameter, node)?;
                return Ok(Some(parameter));
            }
        }
        if has_initializer {
            let name_node =
                name.ok_or(assembly_kind_error(SyntaxKind::Parameter, "parameter name"))?;
            let parameter = self.create_parameter_declaration(name_node)?;
            self.set_text_range(parameter, node)?;
            self.set_original(parameter, node)?;
            return Ok(Some(parameter));
        }
        Ok(Some(node))
    }

    /// tsc-port: hasDefaultValueOrBindingPattern @6.0.3
    /// tsc-hash: 04e54a9ade403c9673531a0a4216d80ec619cdf99caeecd6da49eb921076c8c9
    /// tsc-span: _tsc.js:105723-105725
    fn has_default_value_or_binding_pattern(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::Parameter(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        if data.initializer.is_some() {
            return Ok(true);
        }
        match data.name {
            Some(name) => self.is_binding_pattern(self.node(name)),
            None => Ok(false),
        }
    }

    /// tsc-port: addDefaultValueAssignmentsIfNeeded @6.0.3 (bundled as
    /// addDefaultValueAssignmentsIfNeeded2 — the ES2015 owner copy)
    /// tsc-hash: 482860970a15e88cd23c88baabd2ecc2ec15cb13fe81c1d3920405baba547550
    /// tsc-span: _tsc.js:105726-105744
    fn add_default_value_assignments_if_needed(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let parameters = self.function_parameters(node)?;
        let mut any = false;
        for parameter in &parameters {
            if self.has_default_value_or_binding_pattern(*parameter)? {
                any = true;
                break;
            }
        }
        if !any {
            return Ok(false);
        }
        let mut added = false;
        for parameter in parameters {
            let (name, initializer, dot_dot_dot) = {
                let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                    continue;
                };
                (
                    data.name.map(|id| self.node(id)),
                    data.initializer.map(|id| self.node(id)),
                    data.dot_dot_dot_token.is_some(),
                )
            };
            if dot_dot_dot {
                continue;
            }
            let Some(name) = name else { continue };
            if self.is_binding_pattern(name)? {
                added = self.insert_default_value_assignment_for_binding_pattern(
                    statements,
                    parameter,
                    name,
                    initializer,
                )? || added;
            } else if let Some(initializer) = initializer {
                self.insert_default_value_assignment_for_initializer(
                    statements,
                    parameter,
                    name,
                    initializer,
                )?;
                added = true;
            }
        }
        Ok(added)
    }

    /// tsc-port: insertDefaultValueAssignmentForBindingPattern @6.0.3
    /// tsc-hash: 9ca1cca3cd788d1d5e68559aa6271b66235a7a203f811032168e86838c6f623f
    /// tsc-span: _tsc.js:105745-105783
    fn insert_default_value_assignment_for_binding_pattern(
        &mut self,
        statements: &mut Vec<TransformNode>,
        parameter: TransformNode,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<bool, TransformError> {
        if self.binding_pattern_has_elements(name)? {
            let rval = self.get_generated_name_for_node(parameter)?;
            let declarations = flatten_destructuring_binding(
                self,
                parameter,
                FlattenLevel::All,
                Some(rval),
                /*hoist_temp_variables*/ false,
                /*skip_initializer*/ false,
            )?;
            let list = self.create_variable_declaration_list(declarations)?;
            let statement = self.create_variable_statement_from_list(list)?;
            self.add_emit_flags(statement, EmitFlags::CUSTOM_PROLOGUE)?;
            self.insert_statement_after_custom_prologue(statements, statement)?;
            return Ok(true);
        }
        if let Some(initializer) = initializer {
            let generated = self.get_generated_name_for_node(parameter)?;
            let visited = self.visit_required_expression(initializer)?;
            let assignment = self.create_assignment(generated, visited)?;
            let statement = self.create_expression_statement(assignment)?;
            self.add_emit_flags(statement, EmitFlags::CUSTOM_PROLOGUE)?;
            self.insert_statement_after_custom_prologue(statements, statement)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: insertDefaultValueAssignmentForInitializer @6.0.3
    /// tsc-hash: 79c81a915aedc84b09ad6cb2155bbb5c3b4c7a2c76c325cdcf18e913cd3756c7
    /// tsc-span: _tsc.js:105784-105814
    fn insert_default_value_assignment_for_initializer(
        &mut self,
        statements: &mut Vec<TransformNode>,
        parameter: TransformNode,
        name: TransformNode,
        initializer: TransformNode,
    ) -> Result<(), TransformError> {
        let initializer = self.visit_required_expression(initializer)?;
        let check_name = self.clone_node(name)?;
        let type_check = self.create_type_check(check_name, "undefined")?;
        let assign_target = self.clone_node(name)?;
        self.set_text_range(assign_target, name)?;
        self.add_emit_flags(assign_target, EmitFlags::NO_SOURCE_MAP)?;
        let initializer_flags = self.emit_flags(initializer);
        self.add_emit_flags(
            initializer,
            EmitFlags::NO_SOURCE_MAP | initializer_flags | EmitFlags::NO_COMMENTS,
        )?;
        let assignment = self.create_assignment(assign_target, initializer)?;
        self.set_text_range(assignment, parameter)?;
        self.add_emit_flags(assignment, EmitFlags::NO_COMMENTS)?;
        let assignment_statement = self.create_expression_statement(assignment)?;
        let block = self.create_block(vec![assignment_statement], /*multi_line*/ false)?;
        self.set_text_range(block, parameter)?;
        self.add_emit_flags(
            block,
            EmitFlags::SINGLE_LINE
                | EmitFlags::NO_TRAILING_SOURCE_MAP
                | EmitFlags::NO_TOKEN_SOURCE_MAPS
                | EmitFlags::NO_COMMENTS,
        )?;
        let statement = self.create_if_statement(type_check, block, None)?;
        self.start_on_new_line(statement)?;
        self.set_text_range(statement, parameter)?;
        self.add_emit_flags(
            statement,
            EmitFlags::NO_TOKEN_SOURCE_MAPS
                | EmitFlags::NO_TRAILING_SOURCE_MAP
                | EmitFlags::CUSTOM_PROLOGUE
                | EmitFlags::NO_COMMENTS,
        )?;
        self.insert_statement_after_custom_prologue(statements, statement)?;
        Ok(())
    }

    /// tsc-port: shouldAddRestParameter @6.0.3
    /// tsc-hash: d8c077d222c6225fda5df9eff5adb0b2b0ed455c3791678edc570a2799f9f1f2
    /// tsc-span: _tsc.js:105815-105817
    fn should_add_rest_parameter(
        &self,
        node: Option<TransformNode>,
        in_constructor_with_synthesized_super: bool,
    ) -> Result<bool, TransformError> {
        let Some(node) = node else { return Ok(false) };
        if in_constructor_with_synthesized_super {
            return Ok(false);
        }
        let NodeData::Parameter(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        Ok(data.dot_dot_dot_token.is_some())
    }

    /// tsc-port: addRestParameterIfNeeded @6.0.3
    /// tsc-hash: 7d6931b3bcf385971c3f1bbb7034f9596b31941dd1c5ee4496a4dea642340e4a
    /// tsc-span: _tsc.js:105818-105917
    fn add_rest_parameter_if_needed(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
        in_constructor_with_synthesized_super: bool,
    ) -> Result<bool, TransformError> {
        let parameters = self.function_parameters(node)?;
        let parameter = parameters.last().copied();
        if !self.should_add_rest_parameter(parameter, in_constructor_with_synthesized_super)? {
            return Ok(false);
        }
        let parameter = parameter.expect("rest parameter present");
        let name = {
            let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Parameter, "rest parameter"));
            };
            data.name.map(|id| self.node(id))
        };
        let name_is_identifier = match name {
            Some(name) => matches!(
                self.context.arena().node(name)?.data,
                NodeData::Identifier(_)
            ),
            None => false,
        };
        let mut prologue_statements = Vec::new();
        // declarationName: identifier clone (parent+range threaded) or an
        // UNRECORDED temp; expressionName: fresh clone or the same temp.
        let declaration_name = if name_is_identifier {
            let name = name.expect("identifier name");
            let clone = self.clone_node(name)?;
            self.set_text_range(clone, name)?;
            clone
        } else {
            let binding = self.allocate_temp_binding()?;
            self.create_generated_identifier(&binding)?
        };
        self.add_emit_flags(declaration_name, EmitFlags::NO_SOURCE_MAP)?;
        let expression_name = if name_is_identifier {
            let name = name.expect("identifier name");
            self.clone_node(name)?
        } else {
            declaration_name
        };
        let rest_index = parameters.len() - 1;
        // var name = [];
        let empty_array = self.create_array_literal(vec![])?;
        let declaration =
            self.create_variable_declaration_plain(declaration_name, Some(empty_array))?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        let statement = self.create_variable_statement_from_list(list)?;
        self.set_text_range(statement, parameter)?;
        self.add_emit_flags(statement, EmitFlags::CUSTOM_PROLOGUE)?;
        prologue_statements.push(statement);
        // for (var _i = restIndex; _i < arguments.length; _i++) { ... }
        let loop_binding = self.allocate_loop_variable_binding()?;
        let temp_init = self.create_generated_identifier(&loop_binding)?;
        let rest_index_literal = self.create_numeric_literal(&rest_index.to_string())?;
        let init_declaration =
            self.create_variable_declaration_plain(temp_init, Some(rest_index_literal))?;
        let init_list = self.create_variable_declaration_list(vec![init_declaration])?;
        self.set_text_range(init_list, parameter)?;
        let temp_cond = self.create_generated_identifier(&loop_binding)?;
        let arguments_a = self.create_identifier("arguments")?;
        let arguments_length = self.create_property_access_text(arguments_a, "length")?;
        let condition = self.create_less_than(temp_cond, arguments_length)?;
        self.set_text_range(condition, parameter)?;
        let temp_incr = self.create_generated_identifier(&loop_binding)?;
        let incrementor = self.create_postfix_increment(temp_incr)?;
        self.set_text_range(incrementor, parameter)?;
        // name[_i - restIndex] = arguments[_i];
        let temp_index = self.create_generated_identifier(&loop_binding)?;
        let index_expression = if rest_index == 0 {
            temp_index
        } else {
            let rest_literal = self.create_numeric_literal(&rest_index.to_string())?;
            self.create_subtract(temp_index, rest_literal)?
        };
        let target_access = self.create_element_access(expression_name, index_expression)?;
        let arguments_b = self.create_identifier("arguments")?;
        let temp_read = self.create_generated_identifier(&loop_binding)?;
        let source_access = self.create_element_access(arguments_b, temp_read)?;
        let body_assignment = self.create_assignment(target_access, source_access)?;
        let body_statement = self.create_expression_statement(body_assignment)?;
        self.start_on_new_line(body_statement)?;
        self.set_text_range(body_statement, parameter)?;
        let body_block = self.create_block(vec![body_statement], /*multi_line*/ false)?;
        let for_statement = {
            let source = self.source;
            let flags = self.child_flags(&[init_list, condition, incrementor, body_block])?;
            self.context.factory()?.create_node(
                source,
                NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                    statement: Some(body_block.node()),
                    initializer: Some(init_list.node()),
                    condition: Some(condition.node()),
                    incrementor: Some(incrementor.node()),
                }),
                flags,
            )?
        };
        self.add_emit_flags(for_statement, EmitFlags::CUSTOM_PROLOGUE)?;
        self.start_on_new_line(for_statement)?;
        prologue_statements.push(for_statement);
        if !name_is_identifier {
            let declarations = flatten_destructuring_binding(
                self,
                parameter,
                FlattenLevel::All,
                Some(expression_name),
                /*hoist_temp_variables*/ false,
                /*skip_initializer*/ false,
            )?;
            let list = self.create_variable_declaration_list(declarations)?;
            let statement = self.create_variable_statement_from_list(list)?;
            self.set_text_range(statement, parameter)?;
            self.add_emit_flags(statement, EmitFlags::CUSTOM_PROLOGUE)?;
            prologue_statements.push(statement);
        }
        self.insert_statements_after_custom_prologue(statements, &prologue_statements)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// FlattenHost (the first production host; B-2 trait, owner-graph
// `destructuring-shared-module` edge)
// ---------------------------------------------------------------------------

impl FlattenHost for Es2015Visitor<'_, '_, '_> {
    fn context(&mut self) -> &mut TransformationContext {
        self.context
    }

    fn context_ref(&self) -> &TransformationContext {
        self.context
    }

    fn flatten_source(&self) -> TransformSourceId {
        self.source
    }

    fn downlevel_iteration(&self) -> bool {
        self.downlevel_iteration
    }

    fn generated_bindings(&mut self) -> &mut GeneratedBindingScopes {
        &mut self.generated_bindings
    }

    fn visit_expression(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.visit_required_expression(node)
    }

    fn visit_binding_or_assignment_element(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.visit_required_expression(node)
    }
}

// ---------------------------------------------------------------------------
// Functions and arrows
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitArrowFunction @6.0.3
    /// tsc-hash: 5aa1e3a6520e9abf96f800b0657391f09b4616fe55362903ac71d6a676f2de0e
    /// tsc-span: _tsc.js:106151-106178
    fn visit_arrow_function(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_LEXICAL_THIS)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::STATIC_INITIALIZER)
        {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        let saved_converted_loop_state = self.converted_loop_state.take();
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::ARROW_FUNCTION_EXCLUDES,
            HierarchyFacts::ARROW_FUNCTION_INCLUDES,
        );
        let parameters = {
            let source_parameters = self.function_parameters(node)?;
            self.visit_parameter_list(&source_parameters)?
        };
        let body = self.transform_function_body(node)?;
        let func = self.create_function_expression_full(
            /*asterisk*/ false, /*name*/ None, parameters, body,
        )?;
        self.set_text_range(func, node)?;
        self.set_original(func, node)?;
        self.add_emit_flags(func, EmitFlags::CAPTURES_THIS)?;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::ARROW_FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        Ok(func)
    }

    /// tsc-port: visitFunctionExpression @6.0.3
    /// tsc-hash: d282358038f6b8e9a21488951ec8505ed30e36de35066fb763dfec2e78283220
    /// tsc-span: _tsc.js:106179-106201
    fn visit_function_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let is_async_body = self
            .emit_flags(node)
            .contains(EmitFlags::ASYNC_FUNCTION_BODY);
        let ancestor = if is_async_body {
            enter_subtree(
                &mut self.print_state.hierarchy_facts,
                HierarchyFacts::ASYNC_FUNCTION_BODY_EXCLUDES,
                HierarchyFacts::ASYNC_FUNCTION_BODY_INCLUDES,
            )
        } else {
            enter_subtree(
                &mut self.print_state.hierarchy_facts,
                HierarchyFacts::FUNCTION_EXCLUDES,
                HierarchyFacts::FUNCTION_INCLUDES,
            )
        };
        let saved_converted_loop_state = self.converted_loop_state.take();
        let parameters = {
            let source_parameters = self.function_parameters(node)?;
            self.visit_parameter_list(&source_parameters)?
        };
        let body = self.transform_function_body(node)?;
        let name = if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::NEW_TARGET)
        {
            Some(self.get_local_name(node, /*allow_comments*/ false)?)
        } else {
            match &self.context.arena().node(node)?.data {
                NodeData::FunctionExpression(data) => data.name.map(|id| self.node(id)),
                _ => None,
            }
        };
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        let (asterisk,) = {
            let NodeData::FunctionExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::FunctionExpression,
                    "function expression",
                ));
            };
            (data.asterisk_token,)
        };
        let parameters_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, parameters)?
        };
        let updated_data =
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: name.map(|name| name.node()),
                type_parameters: None,
                parameters: Some(parameters_array.array()),
                r#type: None,
                asterisk_token: asterisk,
                body: Some(body.node()),
                modifiers: None,
            });
        if self.context.arena().node(node)?.data == updated_data {
            return Ok(node);
        }
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: visitFunctionDeclaration @6.0.3
    /// tsc-hash: fbb5a9f062f4bbf2352ecd3c56154c6b08b2b33a08a163bac32a03dd4bd90ce3
    /// tsc-span: _tsc.js:106202-106223
    fn visit_function_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let saved_converted_loop_state = self.converted_loop_state.take();
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::FUNCTION_EXCLUDES,
            HierarchyFacts::FUNCTION_INCLUDES,
        );
        let parameters = {
            let source_parameters = self.function_parameters(node)?;
            self.visit_parameter_list(&source_parameters)?
        };
        let body = self.transform_function_body(node)?;
        let name = if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::NEW_TARGET)
        {
            Some(self.get_local_name(node, /*allow_comments*/ false)?)
        } else {
            match &self.context.arena().node(node)?.data {
                NodeData::FunctionDeclaration(data) => data.name.map(|id| self.node(id)),
                _ => None,
            }
        };
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        let (asterisk, modifiers) = {
            let NodeData::FunctionDeclaration(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::FunctionDeclaration,
                    "function declaration",
                ));
            };
            (data.asterisk_token, data.modifiers)
        };
        // `visitNodes2(node.modifiers, visitor, isModifier)` — the only
        // ES2015-relevant modifier arm is the StaticKeyword elision, which
        // cannot appear on function declarations; visit generically.
        let modifiers = match modifiers {
            Some(modifiers) => self.visit_nodes(modifiers)?,
            None => None,
        };
        let parameters_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, parameters)?
        };
        let updated_data =
            NodeData::FunctionDeclaration(tsc_syntax::nodes::FunctionDeclarationData {
                name: name.map(|name| name.node()),
                type_parameters: None,
                parameters: Some(parameters_array.array()),
                r#type: None,
                asterisk_token: asterisk,
                body: Some(body.node()),
                modifiers,
            });
        if self.context.arena().node(node)?.data == updated_data {
            return Ok(node);
        }
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: transformFunctionLikeToExpression @6.0.3
    /// tsc-hash: a387fa1b4621bc39a9907377af4b532274faaaf20a565ddbbbb2e8f8dbeb33dc
    /// tsc-span: _tsc.js:106224-106254
    fn transform_function_like_to_expression(
        &mut self,
        node: TransformNode,
        location: Option<TransformNode>,
        mut name: Option<TransformNode>,
        container: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let saved_converted_loop_state = self.converted_loop_state.take();
        let container_is_class = match container {
            Some(container) => matches!(
                self.context.arena().node(container)?.data,
                NodeData::ClassDeclaration(_) | NodeData::ClassExpression(_)
            ),
            None => false,
        };
        let is_static_member = self.has_static_modifier(node)?;
        let ancestor = if container_is_class && !is_static_member {
            enter_subtree(
                &mut self.print_state.hierarchy_facts,
                HierarchyFacts::FUNCTION_EXCLUDES,
                HierarchyFacts::FUNCTION_INCLUDES.union(HierarchyFacts::NON_STATIC_CLASS_ELEMENT),
            )
        } else {
            enter_subtree(
                &mut self.print_state.hierarchy_facts,
                HierarchyFacts::FUNCTION_EXCLUDES,
                HierarchyFacts::FUNCTION_INCLUDES,
            )
        };
        let parameters = {
            let source_parameters = self.function_parameters(node)?;
            self.visit_parameter_list(&source_parameters)?
        };
        let body = self.transform_function_body(node)?;
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::NEW_TARGET)
            && name.is_none()
            && matches!(
                self.kind(node)?,
                SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression
            )
        {
            name = Some(self.get_generated_name_for_node(node)?);
        }
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        let asterisk = self.function_asterisk_token(node)?;
        let func = self.create_function_expression_full(asterisk, name, parameters, body)?;
        if let Some(location) = location {
            self.set_text_range(func, location)?;
        }
        self.set_original(func, node)?;
        Ok(func)
    }

    fn function_asterisk_token(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::FunctionDeclaration(data) => data.asterisk_token.is_some(),
            NodeData::FunctionExpression(data) => data.asterisk_token.is_some(),
            NodeData::MethodDeclaration(data) => data.asterisk_token.is_some(),
            _ => false,
        })
    }

    fn has_static_modifier(&self, node: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(node)?.data {
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::PropertyDeclaration(data) => data.modifiers,
            _ => None,
        };
        let Some(modifiers) = modifiers else {
            return Ok(false);
        };
        for modifier in self.array_nodes(Some(modifiers))? {
            if self.kind(modifier)? == SyntaxKind::StaticKeyword {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: transformFunctionBody @6.0.3
    /// tsc-hash: 3a3d99baf53b7ade96d462610aefdbf6855671e31375e93c5e5078e71d80d750
    /// tsc-span: _tsc.js:106255-106329
    fn transform_function_body(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut multi_line = false;
        let mut single_line = false;
        let mut close_brace_location: Option<TransformNode> = None;
        let mut prologue: Vec<TransformNode> = Vec::new();
        let mut statements: Vec<TransformNode> = Vec::new();
        let body = self
            .function_body(node)?
            .ok_or(assembly_kind_error(SyntaxKind::Block, "function body"))?;
        self.enter_function_scope_path();
        let body_is_block = self.kind(body)? == SyntaxKind::Block;
        let mut statement_offset = 0usize;
        self.context.resume_lexical_environment()?;
        let body_statements = if body_is_block {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "body block"));
            };
            self.array_nodes(data.statements)?
        } else {
            Vec::new()
        };
        if body_is_block {
            statement_offset = self.copy_standard_prologue(
                &body_statements,
                &mut prologue,
                0,
                /*ensure_use_strict*/ false,
            )?;
            statement_offset = self.copy_custom_prologue(
                &body_statements,
                &mut statements,
                statement_offset,
                CustomPrologueFilter::HoistedFunctions,
            )?;
            statement_offset = self.copy_custom_prologue(
                &body_statements,
                &mut statements,
                statement_offset,
                CustomPrologueFilter::HoistedVariableStatements,
            )?;
        }
        multi_line =
            self.add_default_value_assignments_if_needed(&mut statements, node)? || multi_line;
        multi_line = self.add_rest_parameter_if_needed(
            &mut statements,
            node,
            /*in_constructor_with_synthesized_super*/ false,
        )? || multi_line;
        if body_is_block {
            statement_offset = self.copy_custom_prologue(
                &body_statements,
                &mut statements,
                statement_offset,
                CustomPrologueFilter::All,
            )?;
            self.visit_statements_into(&body_statements, statement_offset, &mut statements)?;
            if !multi_line && self.node_is_multi_line(body)? {
                multi_line = true;
            }
        } else {
            // Debug.assert(node.kind === ArrowFunction)
            if self.kind(node)? != SyntaxKind::ArrowFunction {
                return Err(assembly_kind_error(
                    self.kind(node)?,
                    "expression body outside an arrow function",
                ));
            }
            let equals_greater_than = match &self.context.arena().node(node)?.data {
                NodeData::ArrowFunction(data) => {
                    data.equals_greater_than_token.map(|id| self.node(id))
                }
                _ => None,
            };
            let both_parsed = match equals_greater_than {
                Some(token) => {
                    !self.node_is_synthesized(token)? && !self.node_is_synthesized(body)?
                }
                None => false,
            };
            if both_parsed {
                let token = equals_greater_than.expect("token");
                if self.range_end_is_on_same_line_as_range_start(token, body)? {
                    single_line = true;
                } else {
                    multi_line = true;
                }
            }
            let expression = self.visit_required_expression(body)?;
            let return_statement = self.create_return_statement(Some(expression))?;
            self.set_text_range(return_statement, body)?;
            self.context
                .arena_mut()?
                .move_synthetic_comments(return_statement, body);
            self.add_emit_flags(
                return_statement,
                EmitFlags::NO_TOKEN_SOURCE_MAPS
                    | EmitFlags::NO_TRAILING_SOURCE_MAP
                    | EmitFlags::NO_TRAILING_COMMENTS,
            )?;
            statements.push(return_statement);
            close_brace_location = Some(body);
        }
        let environment = self.context.end_lexical_environment()?;
        self.merge_lexical_environment(&mut prologue, environment)?;
        self.insert_capture_new_target_if_needed(&mut prologue, node)?;
        self.insert_capture_this_for_node_if_needed(&mut prologue, node)?;
        if !prologue.is_empty() {
            multi_line = true;
        }
        let mut combined = prologue;
        combined.append(&mut statements);
        if body_is_block && self.array_is_equal_to(&combined, &body_statements) {
            self.exit_function_scope_path();
            return Ok(body);
        }
        // setTextRange(createNodeArray(statements), statementsLocation) —
        // the array range rides the block range (arrow bodies use
        // moveRangeEnd(body, -1); byte-inert without maps).
        let block = self.create_block(combined, multi_line)?;
        self.set_text_range(block, body)?;
        if !multi_line && single_line {
            self.add_emit_flags(block, EmitFlags::SINGLE_LINE)?;
        }
        if let Some(close_brace) = close_brace_location {
            if let Some(range) = self.effective_source_range(close_brace)? {
                self.context
                    .arena_mut()?
                    .metadata_mut(block)
                    .set_token_source_map_range(
                        SyntaxKind::CloseBraceToken,
                        SourceMapRange::new(close_brace.source(), range),
                    );
            }
        }
        self.set_original(block, body)?;
        self.exit_function_scope_path();
        Ok(block)
    }

    /// tsc-port: rangeEndIsOnSameLineAsRangeStart @6.0.3
    /// tsc-hash: dd99f6cad4d8b4ddb82e6dce98f287b16aa413c040a022706f3488b89ce7983a
    /// tsc-span: _tsc.js:17352-17359
    fn range_end_is_on_same_line_as_range_start(
        &self,
        range1: TransformNode,
        range2: TransformNode,
    ) -> Result<bool, TransformError> {
        let arena = self.context.arena();
        let end1 = arena.node(range1)?.end;
        let start2 = arena.node(range2)?.pos;
        let source = arena.source(self.source)?.syntax();
        let skipped = skip_trivia_bytes(&self.current_text, start2);
        let line1 = source
            .positions()
            .line_and_character_byte(end1)
            .map(|position| position.line);
        let line2 = source
            .positions()
            .line_and_character_byte(skipped)
            .map(|position| position.line);
        Ok(line1.is_some() && line1 == line2)
    }

    /// tsc-port: visitMethodDeclaration @6.0.3
    /// tsc-hash: d56b4681c7eceb5147d21466386bbe1363effc5ed2af0aed1e5f11b02c2db3a3
    /// tsc-span: _tsc.js:107600-107620
    fn visit_method_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(node)?.data {
            NodeData::MethodDeclaration(data) => data.name.map(|id| self.node(id)),
            _ => None,
        }
        .ok_or(assembly_kind_error(
            SyntaxKind::MethodDeclaration,
            "method name",
        ))?;
        if self.kind(name)? == SyntaxKind::ComputedPropertyName {
            return Err(assembly_kind_error(
                SyntaxKind::ComputedPropertyName,
                "object-literal method with computed name reaches visitMethodDeclaration",
            ));
        }
        let function_expression = self.transform_function_like_to_expression(
            node,
            /*location*/ Some(node),
            /*name*/ None,
            /*container*/ None,
        )?;
        let existing = self.emit_flags(function_expression);
        self.add_emit_flags(
            function_expression,
            EmitFlags::NO_LEADING_COMMENTS | existing,
        )?;
        let assignment = self.create_property_assignment(name, function_expression)?;
        self.set_text_range(assignment, node)?;
        Ok(assignment)
    }

    /// tsc-port: visitAccessorDeclaration @6.0.3
    /// tsc-hash: 74e578370a593301971ed3b86923d55e1bf4321bf3e2a65152f1b617043ed7fa
    /// tsc-span: _tsc.js:107621-107637
    fn visit_accessor_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(node)?.data {
            NodeData::GetAccessor(data) => data.name.map(|id| self.node(id)),
            NodeData::SetAccessor(data) => data.name.map(|id| self.node(id)),
            _ => None,
        }
        .ok_or(assembly_kind_error(
            SyntaxKind::GetAccessor,
            "accessor name",
        ))?;
        if self.kind(name)? == SyntaxKind::ComputedPropertyName {
            return Err(assembly_kind_error(
                SyntaxKind::ComputedPropertyName,
                "accessor with computed name reaches visitAccessorDeclaration",
            ));
        }
        let saved_converted_loop_state = self.converted_loop_state.take();
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::FUNCTION_EXCLUDES,
            HierarchyFacts::FUNCTION_INCLUDES,
        );
        let parameters = {
            let source_parameters = self.function_parameters(node)?;
            self.visit_parameter_list(&source_parameters)?
        };
        let body = self.transform_function_body(node)?;
        let parameters_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, parameters)?
        };
        let updated_data = match &self.context.arena().node(node)?.data {
            NodeData::GetAccessor(data) => {
                let mut data = data.clone();
                data.parameters = Some(parameters_array.array());
                data.body = Some(body.node());
                data.r#type = None;
                NodeData::GetAccessor(data)
            }
            NodeData::SetAccessor(data) => {
                let mut data = data.clone();
                data.parameters = Some(parameters_array.array());
                data.body = Some(body.node());
                NodeData::SetAccessor(data)
            }
            _ => {
                return Err(assembly_kind_error(
                    SyntaxKind::GetAccessor,
                    "accessor declaration",
                ))
            }
        };
        let updated = if self.context.arena().node(node)?.data == updated_data {
            node
        } else {
            let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
            self.context
                .factory()?
                .update_node(node, updated_data, flags)?
        };
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        Ok(updated)
    }

    /// tsc-port: getLocalName @6.0.3
    /// tsc-hash: db85ef71236480d7de1d2e131b01d6f8fed272ef41d0f5297ce7fb3485ee7979
    /// tsc-span: _tsc.js:24803-24805
    fn get_local_name(
        &mut self,
        node: TransformNode,
        allow_comments: bool,
    ) -> Result<TransformNode, TransformError> {
        self.get_name(node, allow_comments, EmitFlags::LOCAL_NAME)
    }

    /// tsc-port: getInternalName @6.0.3
    /// tsc-hash: cf6484de86856ef03a019d04862c731662b2aa5b215b22cb036feb31593f7778
    /// tsc-span: _tsc.js:24800-24802
    fn get_internal_name(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.get_name(
            node,
            /*allow_comments*/ false,
            EmitFlags::LOCAL_NAME | EmitFlags::INTERNAL_NAME,
        )
    }

    /// tsc-port: getName @6.0.3
    /// tsc-hash: 9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33
    /// tsc-span: _tsc.js:24788-24799
    fn get_name(
        &mut self,
        node: TransformNode,
        allow_comments: bool,
        extra_flags: EmitFlags,
    ) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(node)?.data {
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::FunctionExpression(data) => data.name,
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            NodeData::VariableDeclaration(data) => data.name,
            _ => None,
        };
        let name = name.map(|id| self.node(id));
        let name_is_plain_identifier = match name {
            Some(name) => {
                matches!(
                    self.context.arena().node(name)?.data,
                    NodeData::Identifier(_)
                ) && self
                    .context
                    .arena()
                    .metadata(name)
                    .and_then(|metadata| metadata.generated_binding_id())
                    .is_none()
            }
            None => false,
        };
        if name_is_plain_identifier {
            let name = name.expect("plain identifier name");
            let clone = self.clone_node(name)?;
            // `setTextRange(cloneNode(nodeName), nodeName)` — the range
            // rides the MAP/comment channels here: upstream suppresses the
            // member-access line break through the SYNTHESIZED dot token,
            // while this printer keys on the receiver's node positions, so
            // position-threading the clone would open a source-derived
            // line upstream never opens (byte-exact adaptation; maps are
            // byte-inert at this dormant position).
            self.set_source_map_range_from(clone, name)?;
            self.set_comment_range_from(clone, name)?;
            let mut flags = self.emit_flags(name) | extra_flags;
            flags |= EmitFlags::NO_SOURCE_MAP; // allowSourceMaps is never passed true here
            if !allow_comments {
                flags |= EmitFlags::NO_COMMENTS;
            }
            self.add_emit_flags(clone, flags)?;
            return Ok(clone);
        }
        self.get_generated_name_for_node(node)
    }
}

/// `skipTrivia(text, pos)` — the whitespace/comment scan used by the
/// same-line test and the class-wrapper positions (byte positions; the
/// §7 fixture surface has no comments in the scanned spans, so the
/// whitespace arm is the live one; comments scan faithfully).
pub(super) fn skip_trivia_bytes(text: &str, position: u32) -> u32 {
    let bytes = text.as_bytes();
    let mut index = position as usize;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => index += 1,
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'/' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'*' => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            _ => break,
        }
    }
    index as u32
}

// ---------------------------------------------------------------------------
// Variable statements/declarations + let rules + labels + catch
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    fn directive_text(&mut self, statement: TransformNode) -> Result<String, TransformError> {
        let expression = match &self.context.arena().node(statement)?.data {
            NodeData::ExpressionStatement(data) => data.expression,
            _ => None,
        }
        .ok_or(assembly_kind_error(
            SyntaxKind::ExpressionStatement,
            "directive",
        ))?;
        match &self.context.arena().node(self.node(expression))?.data {
            NodeData::StringLiteral(data) => Ok(data.text.clone()),
            _ => Err(assembly_kind_error(
                SyntaxKind::StringLiteral,
                "directive text",
            )),
        }
    }

    fn create_variable_statement_from_declarations(
        &mut self,
        declarations: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let list = self.create_variable_declaration_list(declarations)?;
        self.create_variable_statement_from_list(list)
    }

    /// tsc-port: enableSubstitutionsForBlockScopedBindings @6.0.3
    /// tsc-hash: 370d3cfeda365a23d8182f0904f69f14e9a1da986f804b19645022e3dc587366
    /// tsc-span: _tsc.js:107982-107987
    fn enable_substitutions_for_block_scoped_bindings(&mut self) -> Result<(), TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS)
        {
            self.print_state.enabled_substitutions = self
                .print_state
                .enabled_substitutions
                .union(Es2015SubstitutionFlags::BLOCK_SCOPED_BINDINGS);
            self.context.enable_substitution(SyntaxKind::Identifier)?;
        }
        Ok(())
    }

    /// tsc-port: enableSubstitutionsForCapturedThis @6.0.3
    /// tsc-hash: 1a495f7143adcc0d9d17a756942d319e3e45ea692c9c31a5e2923ffc643f02ea
    /// tsc-span: _tsc.js:107988-108000
    fn enable_substitutions_for_captured_this(&mut self) -> Result<(), TransformError> {
        if !self
            .print_state
            .enabled_substitutions
            .intersects(Es2015SubstitutionFlags::CAPTURED_THIS)
        {
            self.print_state.enabled_substitutions = self
                .print_state
                .enabled_substitutions
                .union(Es2015SubstitutionFlags::CAPTURED_THIS);
            self.context.enable_substitution(SyntaxKind::ThisKeyword)?;
            self.context
                .enable_emit_notification(SyntaxKind::Constructor)?;
            self.context
                .enable_emit_notification(SyntaxKind::MethodDeclaration)?;
            self.context
                .enable_emit_notification(SyntaxKind::GetAccessor)?;
            self.context
                .enable_emit_notification(SyntaxKind::SetAccessor)?;
            self.context
                .enable_emit_notification(SyntaxKind::ArrowFunction)?;
            self.context
                .enable_emit_notification(SyntaxKind::FunctionExpression)?;
            self.context
                .enable_emit_notification(SyntaxKind::FunctionDeclaration)?;
        }
        Ok(())
    }

    /// tsc-port: isVariableStatementOfTypeScriptClassWrapper @6.0.3
    /// tsc-hash: 31a18db369045519f75b5175bc55db48232f9ed7b1b1ce746ef638b72a0459da
    /// tsc-span: _tsc.js:106382-106384
    fn is_variable_statement_of_type_script_class_wrapper(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::VariableStatement(statement) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(list) = statement.declaration_list else {
            return Ok(false);
        };
        let NodeData::VariableDeclarationList(list_data) =
            &self.context.arena().node(self.node(list))?.data
        else {
            return Ok(false);
        };
        let declarations = self.array_nodes(list_data.declarations)?;
        if declarations.len() != 1 {
            return Ok(false);
        }
        let NodeData::VariableDeclaration(declaration) =
            &self.context.arena().node(declarations[0])?.data
        else {
            return Ok(false);
        };
        let Some(initializer) = declaration.initializer else {
            return Ok(false);
        };
        Ok(self
            .internal_emit_flags(self.node(initializer))
            .contains(InternalEmitFlags::TYPE_SCRIPT_CLASS_WRAPPER))
    }

    /// tsc-port: visitVariableStatement @6.0.3
    /// tsc-hash: d4ad2ab4e75c439dfd94062c1cc87cb9085fd84128230174ffd73126ebc697f2
    /// tsc-span: _tsc.js:106385-106418
    fn visit_variable_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let is_exported = self.has_syntactic_export_modifier(node)?;
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::NONE,
            if is_exported {
                HierarchyFacts::EXPORTED_VARIABLE_STATEMENT
            } else {
                HierarchyFacts::NONE
            },
        );
        let updated: Option<TransformNode>;
        let (list, list_is_block_scoped) = {
            let NodeData::VariableStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableStatement,
                    "variable statement",
                ));
            };
            let list = data.declaration_list.map(|id| self.node(id));
            let block_scoped = match list {
                Some(list) => self.context.arena().node(list)?.flags & 7 != 0,
                None => false,
            };
            (list, block_scoped)
        };
        if self.converted_loop_state.is_some()
            && !list_is_block_scoped
            && !self.is_variable_statement_of_type_script_class_wrapper(node)?
        {
            // hoist `var` declarations declared in the converted loop and
            // rewrite initializers as assignments.
            let mut assignments: Vec<TransformNode> = Vec::new();
            let list = list.ok_or(assembly_kind_error(
                SyntaxKind::VariableStatement,
                "declaration list",
            ))?;
            let declarations = {
                let NodeData::VariableDeclarationList(data) =
                    &self.context.arena().node(list)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::VariableDeclarationList,
                        "declaration list",
                    ));
                };
                self.array_nodes(data.declarations)?
            };
            for declaration in declarations {
                self.hoist_variable_declaration_declared_in_converted_loop(declaration)?;
                let (name, initializer) = {
                    let NodeData::VariableDeclaration(data) =
                        &self.context.arena().node(declaration)?.data
                    else {
                        continue;
                    };
                    (
                        data.name.map(|id| self.node(id)),
                        data.initializer.map(|id| self.node(id)),
                    )
                };
                if let Some(initializer) = initializer {
                    let name = name.ok_or(assembly_kind_error(
                        SyntaxKind::VariableDeclaration,
                        "declaration name",
                    ))?;
                    let assignment = if self.is_binding_pattern(name)? {
                        flatten_destructuring_assignment(
                            self,
                            declaration,
                            FlattenLevel::All,
                            /*needs_value*/ false,
                            /*use_assignment_completion*/ false,
                        )?
                    } else {
                        let visited = self.visit_required_expression(initializer)?;
                        let assignment =
                            self.create_binary(name, SyntaxKind::EqualsToken, visited)?;
                        self.set_text_range(assignment, declaration)?;
                        assignment
                    };
                    assignments.push(assignment);
                }
            }
            if assignments.is_empty() {
                updated = None;
            } else {
                let expression = self.inline_expressions(assignments)?;
                let statement = self.create_expression_statement(expression)?;
                self.set_text_range(statement, node)?;
                updated = Some(statement);
            }
        } else {
            updated = Some(self.visit_each_child_required(node)?);
        }
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    fn has_syntactic_export_modifier(&self, node: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(node)?.data {
            NodeData::VariableStatement(data) => data.modifiers,
            NodeData::ClassDeclaration(data) => data.modifiers,
            NodeData::FunctionDeclaration(data) => data.modifiers,
            _ => None,
        };
        let Some(modifiers) = modifiers else {
            return Ok(false);
        };
        for modifier in self.array_nodes(Some(modifiers))? {
            if self.kind(modifier)? == SyntaxKind::ExportKeyword {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn has_syntactic_default_modifier(&self, node: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(node)?.data {
            NodeData::ClassDeclaration(data) => data.modifiers,
            _ => None,
        };
        let Some(modifiers) = modifiers else {
            return Ok(false);
        };
        for modifier in self.array_nodes(Some(modifiers))? {
            if self.kind(modifier)? == SyntaxKind::DefaultKeyword {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: inlineExpressions @6.0.3
    /// tsc-hash: 0b804e265fda3151c49457cd1f8ca94580b01c04d161eed6103baadbec28db8a
    /// tsc-span: _tsc.js:24785-24787
    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut iterator = expressions.into_iter();
        let mut result = iterator.next().ok_or(assembly_kind_error(
            SyntaxKind::BinaryExpression,
            "inlineExpressions over an empty list",
        ))?;
        for expression in iterator {
            result = self.create_comma(result, expression)?;
        }
        Ok(result)
    }

    /// tsc-port: visitVariableDeclarationList @6.0.3
    /// tsc-hash: 4d6889b5ad683119291304054db2694d3d191227211d32ab8abd88a3265dedfc
    /// tsc-span: _tsc.js:106419-106439
    fn visit_variable_declaration_list(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let node_flags = self.context.arena().node(node)?.flags;
        let is_block_scoped = node_flags & 7 != 0;
        let contains_binding_pattern = self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_BINDING_PATTERN);
        if is_block_scoped || contains_binding_pattern {
            if is_block_scoped {
                self.enable_substitutions_for_block_scoped_bindings()?;
            }
            let is_let = node_flags & 1 != 0;
            let declarations = {
                let NodeData::VariableDeclarationList(data) =
                    &self.context.arena().node(node)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::VariableDeclarationList,
                        "declaration list",
                    ));
                };
                self.array_nodes(data.declarations)?
            };
            let mut visited: Vec<TransformNode> = Vec::new();
            for declaration in &declarations {
                let outcome = if is_let {
                    self.visit_variable_declaration_in_let_declaration_list(*declaration)?
                } else {
                    self.visit_variable_declaration(*declaration)?
                };
                match outcome {
                    VisitOutcome::Elided => {}
                    VisitOutcome::One(node) => visited.push(node),
                    VisitOutcome::Many(nodes) => visited.extend(nodes),
                }
            }
            let list = self.create_variable_declaration_list(visited.clone())?;
            self.set_original(list, node)?;
            self.set_text_range(list, node)?;
            self.set_comment_range_from(list, node)?;
            // setSourceMapRange(declarationList, getRangeUnion(declarations))
            // when a binding pattern headed or tailed the source list.
            if contains_binding_pattern && !declarations.is_empty() {
                let first_is_pattern = {
                    let NodeData::VariableDeclaration(data) =
                        &self.context.arena().node(declarations[0])?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::VariableDeclaration,
                            "first declaration",
                        ));
                    };
                    match data.name {
                        Some(name) => self.is_binding_pattern(self.node(name))?,
                        None => false,
                    }
                };
                let last = declarations[declarations.len() - 1];
                let last_is_pattern = {
                    let NodeData::VariableDeclaration(data) =
                        &self.context.arena().node(last)?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::VariableDeclaration,
                            "last declaration",
                        ));
                    };
                    match data.name {
                        Some(name) => self.is_binding_pattern(self.node(name))?,
                        None => false,
                    }
                };
                if first_is_pattern || last_is_pattern {
                    if let Some(range) = self.range_union(&visited)? {
                        self.context
                            .arena_mut()?
                            .metadata_mut(list)
                            .set_source_map_range(SourceMapRange::new(self.source, range));
                    }
                }
            }
            return Ok(list);
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: getRangeUnion @6.0.3
    /// tsc-hash: dc50f7cf51de23e7e3c3ec0dbeaa884f2e6b0d3cfbbb7fef1323d07a98064a88
    /// tsc-span: _tsc.js:106440-106447
    fn range_union(&self, nodes: &[TransformNode]) -> Result<Option<SourceRange>, TransformError> {
        let mut union: Option<(u32, u32)> = None;
        for node in nodes {
            if let Some(SourceRange::Original(range)) = self.effective_source_range(*node)? {
                let (pos, end) = (range.start().value(), range.end().value());
                union = Some(match union {
                    Some((current_pos, current_end)) => {
                        (current_pos.min(pos), current_end.max(end))
                    }
                    None => (pos, end),
                });
            }
        }
        let source = self.context.arena().source(self.source)?.syntax();
        Ok(union.and_then(|(pos, end)| SourceRange::from_raw(pos, end, source.positions()).ok()))
    }

    /// tsc-port: shouldEmitExplicitInitializerForLetDeclaration @6.0.3
    /// tsc-hash: 57764e4e2c565e7ea31404315d66ecd6fe9692c724433b66922d4140f7b86309
    /// tsc-span: _tsc.js:106448-106454
    fn should_emit_explicit_initializer_for_let_declaration(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(reference) = self.context.arena().parse_tree_resolver_node(node)? else {
            return Ok(false);
        };
        let is_captured_in_function = self.resolver.has_node_check_flag(
            reference,
            NodeCheckFlags::CAPTURED_BLOCK_SCOPED_BINDING.bits() as u32,
        )?;
        let is_declared_in_loop = self.resolver.has_node_check_flag(
            reference,
            NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP.bits() as u32,
        )?;
        let facts = self.print_state.hierarchy_facts;
        let emitted_as_top_level = facts.intersects(HierarchyFacts::TOP_LEVEL)
            || (is_captured_in_function
                && is_declared_in_loop
                && facts.intersects(HierarchyFacts::ITERATION_STATEMENT_BLOCK));
        let emit_explicit_initializer = !emitted_as_top_level
            && !facts.intersects(HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT)
            && (!self
                .resolver
                .is_declaration_with_colliding_name(reference)?
                || (is_declared_in_loop
                    && !is_captured_in_function
                    && !facts.intersects(
                        HierarchyFacts::FOR_STATEMENT
                            .union(HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT),
                    )));
        Ok(emit_explicit_initializer)
    }

    /// tsc-port: visitVariableDeclarationInLetDeclarationList @6.0.3
    /// tsc-hash: dd4a618cbe337593fb6db97a7f3a56c60b2378256e02f079570580a92fa74d18
    /// tsc-span: _tsc.js:106455-106472
    fn visit_variable_declaration_in_let_declaration_list(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        let (name, has_initializer) = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "variable declaration",
                ));
            };
            (
                data.name.map(|id| self.node(id)),
                data.initializer.is_some(),
            )
        };
        let name = name.ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?;
        if self.is_binding_pattern(name)? {
            return self.visit_variable_declaration(node);
        }
        if !has_initializer && self.should_emit_explicit_initializer_for_let_declaration(node)? {
            let void_zero = self.create_void_zero()?;
            let updated_data =
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: Some(void_zero.node()),
                });
            let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
            return Ok(VisitOutcome::One(self.context.factory()?.update_node(
                node,
                updated_data,
                flags,
            )?));
        }
        Ok(match self.visit_each_child(node)? {
            Some(updated) => VisitOutcome::One(updated),
            None => VisitOutcome::Elided,
        })
    }

    /// tsc-port: visitVariableDeclaration @6.0.3
    /// tsc-hash: e60df465c1dafe75387852d60a455f4b45ac1e8dbf10429cfd0a0b461dce0de2
    /// tsc-span: _tsc.js:106473-106491
    fn visit_variable_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::EXPORTED_VARIABLE_STATEMENT,
            HierarchyFacts::NONE,
        );
        let name = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "variable declaration",
                ));
            };
            data.name.map(|id| self.node(id))
        };
        let name = name.ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?;
        let updated = if self.is_binding_pattern(name)? {
            // `hoistTempVariables = (ancestorFacts & ExportedVariableStatement) !== 0`
            let exported = ancestor.intersects(HierarchyFacts::EXPORTED_VARIABLE_STATEMENT);
            let declarations = flatten_destructuring_binding(
                self,
                node,
                FlattenLevel::All,
                /*rval*/ None,
                /*hoist_temp_variables*/ exported,
                /*skip_initializer*/ false,
            )?;
            VisitOutcome::Many(declarations)
        } else {
            match self.visit_each_child(node)? {
                Some(node) => VisitOutcome::One(node),
                None => VisitOutcome::Elided,
            }
        };
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitLabeledStatement @6.0.3
    /// tsc-hash: 4d2f491656597174aa3adcd6665ce0d5ed8d6bd2d010cef26a6da0b9fccd5b72
    /// tsc-span: _tsc.js:106498-106512
    fn visit_labeled_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        let record = self.converted_loop_state.is_some();
        let statement = self.unwrap_innermost_statement_of_label(node, record)?;
        if is_iteration_statement_kind(self.kind(statement)?) {
            return self.visit_iteration_statement(statement, Some(node));
        }
        let visited = match self.visit_statement_lifted(statement)? {
            Some(visited) => visited,
            None => {
                let empty = self.create_empty_statement()?;
                self.set_text_range(empty, statement)?;
                empty
            }
        };
        let reset = self.converted_loop_state.is_some();
        Ok(VisitOutcome::One(self.restore_enclosing_label(
            visited,
            Some(node),
            reset,
        )?))
    }

    /// tsc-port: visitBreakOrContinueStatement @6.0.3
    /// tsc-hash: ef8efd157dd0f849a8cfed9fda4d5f6b4dc464c2134c2337740f188ef6749590
    /// tsc-span: _tsc.js:105089-105143
    fn visit_break_or_continue_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.converted_loop_state.is_some() {
            let kind = self.kind(node)?;
            let is_break = kind == SyntaxKind::BreakStatement;
            let jump = if is_break {
                Jump::BREAK
            } else {
                Jump::CONTINUE
            };
            let label = match &self.context.arena().node(node)?.data {
                NodeData::BreakStatement(data) => data.label.map(|id| self.node(id)),
                NodeData::ContinueStatement(data) => data.label.map(|id| self.node(id)),
                _ => None,
            };
            let label_text = label.map(|label| self.identifier_text(label)).transpose()?;
            let can_use_break_or_continue = match &label_text {
                Some(text) => self.loop_state().labels.get(text).copied().unwrap_or(false),
                None => self.loop_state().allowed_non_labeled_jumps.intersects(jump),
            };
            if !can_use_break_or_continue {
                let label_marker: String;
                match &label_text {
                    None => {
                        let state = self.loop_state_mut();
                        if is_break {
                            state.non_local_jumps = state.non_local_jumps.union(Jump::BREAK);
                            label_marker = "break".to_owned();
                        } else {
                            state.non_local_jumps = state.non_local_jumps.union(Jump::CONTINUE);
                            label_marker = "continue".to_owned();
                        }
                    }
                    Some(text) => {
                        if is_break {
                            label_marker = format!("break-{text}");
                        } else {
                            label_marker = format!("continue-{text}");
                        }
                        set_labeled_jump(
                            self.loop_state_mut(),
                            is_break,
                            text.clone(),
                            label_marker.clone(),
                        );
                    }
                }
                let mut return_expression = self.create_string_literal(&label_marker)?;
                let out_params: Vec<(TransformNode, TargetBinding)> = {
                    let state = self.loop_state();
                    state
                        .loop_out_parameters
                        .iter()
                        .map(|parameter| {
                            (parameter.original_name, parameter.out_param_name.clone())
                        })
                        .collect()
                };
                if !out_params.is_empty() {
                    let mut expr: Option<TransformNode> = None;
                    for (original_name, out_param) in &out_params {
                        let copy_expr = self.copy_out_parameter_pair(
                            *original_name,
                            out_param,
                            CopyDirection::ToOutParameter,
                        )?;
                        expr = Some(match expr {
                            Some(previous) => self.create_comma(previous, copy_expr)?,
                            None => copy_expr,
                        });
                    }
                    let expr = expr.expect("at least one out parameter");
                    return_expression = self.create_comma(expr, return_expression)?;
                }
                return self.create_return_statement(Some(return_expression));
            }
        }
        self.visit_each_child_required(node)
    }

    /// `copyOutParameter(outParam, copyDirection)` over the split
    /// (originalName, outParamName) pair.
    fn copy_out_parameter_pair(
        &mut self,
        original_name: TransformNode,
        out_param: &TargetBinding,
        direction: CopyDirection,
    ) -> Result<TransformNode, TransformError> {
        let out_param_identifier = self.create_generated_identifier(out_param)?;
        let original = self.clone_node(original_name)?;
        let (target, source) = match direction {
            CopyDirection::ToOriginal => (original, out_param_identifier),
            CopyDirection::ToOutParameter => (out_param_identifier, original),
        };
        self.create_binary(target, SyntaxKind::EqualsToken, source)
    }

    /// tsc-port: visitCatchClause @6.0.3
    /// tsc-hash: a07efc99cc6405347314776be164ca516462bcede9e0c73b579d1c11057f7b93
    /// tsc-span: _tsc.js:107564-107595
    fn visit_catch_clause(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::BLOCK_SCOPE_EXCLUDES,
            HierarchyFacts::BLOCK_SCOPE_INCLUDES,
        );
        let (variable_declaration, block) = {
            let NodeData::CatchClause(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::CatchClause, "catch clause"));
            };
            (
                data.variable_declaration.map(|id| self.node(id)),
                data.block.map(|id| self.node(id)),
            )
        };
        // Debug.assert(!!node.variableDeclaration, "Catch clause variable should always be present when downleveling ES2015.")
        let variable_declaration = variable_declaration.ok_or(assembly_kind_error(
            SyntaxKind::CatchClause,
            "catch clause variable (optional catch binding is ES2019 input)",
        ))?;
        let block = block.ok_or(assembly_kind_error(SyntaxKind::CatchClause, "catch block"))?;
        let name = {
            let NodeData::VariableDeclaration(data) =
                &self.context.arena().node(variable_declaration)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "catch variable",
                ));
            };
            data.name.map(|id| self.node(id))
        };
        let name = name.ok_or(assembly_kind_error(
            SyntaxKind::VariableDeclaration,
            "catch variable name",
        ))?;
        let updated: TransformNode = if self.is_binding_pattern(name)? {
            let temp_binding = self.allocate_temp_binding()?;
            let temp = self.create_generated_identifier(&temp_binding)?;
            let new_variable_declaration = self.create_variable_declaration_plain(temp, None)?;
            self.set_text_range(new_variable_declaration, variable_declaration)?;
            let temp_reference = self.create_generated_identifier(&temp_binding)?;
            let vars = flatten_destructuring_binding(
                self,
                variable_declaration,
                FlattenLevel::All,
                Some(temp_reference),
                /*hoist_temp_variables*/ false,
                /*skip_initializer*/ false,
            )?;
            let list = self.create_variable_declaration_list(vars)?;
            self.set_text_range(list, variable_declaration)?;
            let destructure = self.create_variable_statement_from_list(list)?;
            let new_block = self.add_statement_to_start_of_block(block, destructure)?;
            let updated_data = NodeData::CatchClause(tsc_syntax::nodes::CatchClauseData {
                variable_declaration: Some(new_variable_declaration.node()),
                block: Some(new_block.node()),
            });
            let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
            self.context
                .factory()?
                .update_node(node, updated_data, flags)?
        } else {
            self.visit_each_child_required(node)?
        };
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: addStatementToStartOfBlock @6.0.3
    /// tsc-hash: ff982635f12dfb32fc1e44767af664eac088f99a07aa8ee4c0533ea9fe3d9cc1
    /// tsc-span: _tsc.js:107596-107599
    fn add_statement_to_start_of_block(
        &mut self,
        block: TransformNode,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(block)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "block"));
            };
            self.array_nodes(data.statements)?
        };
        let mut transformed: Vec<TransformNode> = vec![statement];
        self.visit_statements_into(&statements, 0, &mut transformed)?;
        let array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, transformed)?
        };
        let updated_data = NodeData::Block(tsc_syntax::nodes::BlockData {
            statements: Some(array.array()),
        });
        let flags = flags_after_update(self.context.arena(), block, &updated_data)?;
        self.context
            .factory()?
            .update_node(block, updated_data, flags)
    }

    /// tsc-port: visitShorthandPropertyAssignment @6.0.3
    /// tsc-hash: 8c948a1b8b24003593c33efc4fbdcd9b65a0a2042e5c1fbc3e3250b84fdb0796
    /// tsc-span: _tsc.js:107638-107647
    fn visit_shorthand_property_assignment(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(node)?.data {
            NodeData::ShorthandPropertyAssignment(data) => data.name.map(|id| self.node(id)),
            _ => None,
        }
        .ok_or(assembly_kind_error(
            SyntaxKind::ShorthandPropertyAssignment,
            "name",
        ))?;
        let clone = self.clone_node(name)?;
        let value = self.visit_identifier(clone)?;
        let assignment = self.create_property_assignment(name, value)?;
        self.set_text_range(assignment, node)?;
        Ok(assignment)
    }

    /// tsc-port: visitBinaryExpression @6.0.3
    /// tsc-hash: e0f6bfb2444d83cca54f4b8284d4e96b0e44fd648efd77c86788396ac456a65a
    /// tsc-span: _tsc.js:106345-106364
    fn visit_binary_expression(
        &mut self,
        node: TransformNode,
        expression_result_is_unused: bool,
    ) -> Result<TransformNode, TransformError> {
        if self.is_destructuring_assignment(node)? {
            return flatten_destructuring_assignment(
                self,
                node,
                FlattenLevel::All,
                /*needs_value*/ !expression_result_is_unused,
                /*use_assignment_completion*/ false,
            );
        }
        let (left, operator_token, right, operator_kind) = {
            let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::BinaryExpression,
                    "binary expression",
                ));
            };
            let operator =
                data.operator_token
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "operator",
                    ))?;
            (
                data.left.map(|id| self.node(id)),
                operator,
                data.right.map(|id| self.node(id)),
                self.kind(operator)?,
            )
        };
        if operator_kind == SyntaxKind::CommaToken {
            let left = left.ok_or(assembly_kind_error(SyntaxKind::BinaryExpression, "left"))?;
            let right = right.ok_or(assembly_kind_error(SyntaxKind::BinaryExpression, "right"))?;
            let visited_left = {
                let VisitOutcome::One(node) = self.visit_with_unused_expression_result(left)?
                else {
                    return Err(assembly_kind_error(SyntaxKind::BinaryExpression, "left"));
                };
                node
            };
            let visited_right = if expression_result_is_unused {
                let VisitOutcome::One(node) = self.visit_with_unused_expression_result(right)?
                else {
                    return Err(assembly_kind_error(SyntaxKind::BinaryExpression, "right"));
                };
                node
            } else {
                self.visit_required_expression(right)?
            };
            let updated_data =
                NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                    left: Some(visited_left.node()),
                    operator_token: Some(operator_token.node()),
                    right: Some(visited_right.node()),
                });
            if self.context.arena().node(node)?.data == updated_data {
                return Ok(node);
            }
            let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
            return self
                .context
                .factory()?
                .update_node(node, updated_data, flags);
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: isDestructuringAssignment @6.0.3
    /// tsc-hash: 57f11978bed7f73705f836f943b584fbe39823ae01178fff5a5b6b046b44268b
    /// tsc-span: _tsc.js:17114-17124
    fn is_destructuring_assignment(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(operator) = data.operator_token else {
            return Ok(false);
        };
        if self.kind(self.node(operator))? != SyntaxKind::EqualsToken {
            return Ok(false);
        }
        let Some(left) = data.left else {
            return Ok(false);
        };
        Ok(matches!(
            self.context.arena().node(self.node(left))?.data,
            NodeData::ObjectLiteralExpression(_) | NodeData::ArrayLiteralExpression(_)
        ))
    }

    /// tsc-port: visitCommaListExpression @6.0.3
    /// tsc-hash: 228ee52cee142abb49c94501f55f0f27bdc556ce15530e2dd3d29291c919cbde
    /// tsc-span: _tsc.js:106365-106381
    fn visit_comma_list_expression(
        &mut self,
        node: TransformNode,
        expression_result_is_unused: bool,
    ) -> Result<TransformNode, TransformError> {
        let elements = {
            let NodeData::CommaListExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::CommaListExpression,
                    "comma list",
                ));
            };
            self.array_nodes(data.elements)?
        };
        let mut visited = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let is_last = index == elements.len() - 1;
            let node = if !is_last || expression_result_is_unused {
                let VisitOutcome::One(node) = self.visit_with_unused_expression_result(*element)?
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::CommaListExpression,
                        "element",
                    ));
                };
                node
            } else {
                self.visit_required_expression(*element)?
            };
            visited.push(node);
        }
        let array = {
            let source = self.source;
            self.context.factory()?.create_node_array(source, visited)?
        };
        let updated_data =
            NodeData::CommaListExpression(tsc_syntax::nodes::CommaListExpressionData {
                elements: Some(array.array()),
            });
        if self.context.arena().node(node)?.data == updated_data {
            return Ok(node);
        }
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }
}

/// tsc-port: setLabeledJump @6.0.3
/// tsc-hash: 6fe2c8c909ed47acea83f4d4a959a2c812131fb756c1d928696f9f82c46f22a1
/// tsc-span: _tsc.js:107420-107432
fn set_labeled_jump(
    state: &mut ConvertedLoopState,
    is_break: bool,
    label_text: String,
    label_marker: String,
) {
    let table = if is_break {
        &mut state.labeled_non_local_breaks
    } else {
        &mut state.labeled_non_local_continues
    };
    match table.iter_mut().find(|(text, _)| *text == label_text) {
        Some((_, marker)) => *marker = label_marker,
        None => table.push((label_text, label_marker)),
    }
}

// ---------------------------------------------------------------------------
// Template expressions + object literals
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitTemplateExpression @6.0.3
    /// tsc-hash: f5dfead2be93a40dc2a498913d2945edfa7d29c222bb2e642422251792d07c1d
    /// tsc-span: _tsc.js:107937-107952
    fn visit_template_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (head_text, spans) = {
            let NodeData::TemplateExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::TemplateExpression,
                    "template expression",
                ));
            };
            let head = data
                .head
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::TemplateExpression, "head"))?;
            let head_text = match &self.context.arena().node(head)?.data {
                NodeData::TemplateHead(head) => head.text.clone(),
                _ => {
                    return Err(assembly_kind_error(SyntaxKind::TemplateHead, "head"));
                }
            };
            (head_text, self.array_nodes(data.template_spans)?)
        };
        let mut expression = self.create_string_literal(&head_text)?;
        for span in spans {
            let (span_expression, literal_text) = {
                let NodeData::TemplateSpan(data) = &self.context.arena().node(span)?.data else {
                    return Err(assembly_kind_error(SyntaxKind::TemplateSpan, "span"));
                };
                let span_expression = data
                    .expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(SyntaxKind::TemplateSpan, "expression"))?;
                let literal = data
                    .literal
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(SyntaxKind::TemplateSpan, "literal"))?;
                let literal_text = match &self.context.arena().node(literal)?.data {
                    NodeData::TemplateMiddle(data) => data.text.clone(),
                    NodeData::TemplateTail(data) => data.text.clone(),
                    _ => {
                        return Err(assembly_kind_error(
                            SyntaxKind::TemplateMiddle,
                            "span literal",
                        ))
                    }
                };
                (span_expression, literal_text)
            };
            let mut arguments = vec![self.visit_required_expression(span_expression)?];
            if !literal_text.is_empty() {
                arguments.push(self.create_string_literal(&literal_text)?);
            }
            let concat = self.create_property_access_text(expression, "concat")?;
            expression = self.create_call(concat, arguments)?;
        }
        self.set_text_range(expression, node)?;
        Ok(expression)
    }

    /// tsc-port: visitObjectLiteralExpression @6.0.3
    /// tsc-hash: b459cf3d073d23b749e01b2e2ea12872454374ebd9d40138fad657bba005fd63
    /// tsc-span: _tsc.js:106867-106899
    fn visit_object_literal_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let properties = {
            let NodeData::ObjectLiteralExpression(data) = &self.context.arena().node(node)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::ObjectLiteralExpression,
                    "object literal",
                ));
            };
            self.array_nodes(data.properties)?
        };
        let mut num_initial_properties: Option<usize> = None;
        let mut has_computed = false;
        for (index, property) in properties.iter().enumerate() {
            let contains_yield_in_async = self
                .transform_flags(*property)
                .contains(TransformFlags::CONTAINS_YIELD)
                && self
                    .print_state
                    .hierarchy_facts
                    .intersects(HierarchyFacts::ASYNC_FUNCTION_BODY);
            let property_name = match &self.context.arena().node(*property)?.data {
                NodeData::PropertyAssignment(data) => data.name,
                NodeData::ShorthandPropertyAssignment(data) => data.name,
                NodeData::MethodDeclaration(data) => data.name,
                NodeData::GetAccessor(data) => data.name,
                NodeData::SetAccessor(data) => data.name,
                _ => None,
            };
            let is_computed = match property_name {
                Some(name) => self.kind(self.node(name))? == SyntaxKind::ComputedPropertyName,
                None => false,
            };
            if contains_yield_in_async || is_computed {
                has_computed = is_computed;
                num_initial_properties = Some(index);
                break;
            }
        }
        let Some(num_initial_properties) = num_initial_properties else {
            return self.visit_each_child_required(node);
        };
        let temp_binding = self.allocate_temp_binding()?;
        {
            let hoist_identifier = self.create_generated_identifier(&temp_binding)?;
            self.context.hoist_variable_declaration(hoist_identifier)?;
        }
        let temp = self.create_generated_identifier(&temp_binding)?;
        let mut expressions: Vec<TransformNode> = Vec::new();
        let node_multi_line = self.node_is_multi_line(node)?;
        let mut initial_visited: Vec<TransformNode> = Vec::new();
        for property in properties.iter().take(num_initial_properties) {
            match self.visit(*property)? {
                VisitOutcome::Elided => {}
                VisitOutcome::One(node) => initial_visited.push(node),
                VisitOutcome::Many(_) => {
                    return Err(assembly_kind_error(
                        SyntaxKind::ObjectLiteralExpression,
                        "object literal element position received a statement list",
                    ))
                }
            }
        }
        let initial_literal = self.create_object_literal(initial_visited, node_multi_line)?;
        if has_computed {
            self.add_emit_flags(initial_literal, EmitFlags::INDENTED)?;
        }
        let assignment = self.create_assignment(temp, initial_literal)?;
        if node_multi_line {
            self.start_on_new_line(assignment)?;
        }
        expressions.push(assignment);
        self.add_object_literal_members(
            &mut expressions,
            node,
            &properties,
            &temp_binding,
            num_initial_properties,
        )?;
        // The trailing temp reference (multiLine: startOnNewLine on a
        // range-threaded clone).
        let trailing = self.create_generated_identifier(&temp_binding)?;
        if node_multi_line {
            self.start_on_new_line(trailing)?;
        }
        expressions.push(trailing);
        self.inline_expressions(expressions)
    }

    /// tsc-port: addObjectLiteralMembers @6.0.3
    /// tsc-hash: a1cdb8896ce8298383f56fe3daec7ff246c2b38042292b0db0c4cc6a804f4df7
    /// tsc-span: _tsc.js:107484-107511
    fn add_object_literal_members(
        &mut self,
        expressions: &mut Vec<TransformNode>,
        node: TransformNode,
        properties: &[TransformNode],
        receiver: &TargetBinding,
        start: usize,
    ) -> Result<(), TransformError> {
        let node_multi_line = self.node_is_multi_line(node)?;
        for (index, property) in properties.iter().enumerate().skip(start) {
            let kind = self.kind(*property)?;
            match kind {
                SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                    let accessors = self.get_all_accessor_declarations(properties, *property)?;
                    if Some(*property) == accessors.first_accessor.into() {
                        let receiver_node = self.create_generated_identifier(receiver)?;
                        let expression = self.transform_accessors_to_expression(
                            receiver_node,
                            &accessors,
                            node,
                            node_multi_line,
                        )?;
                        expressions.push(expression);
                    }
                }
                SyntaxKind::MethodDeclaration => {
                    let receiver_node = self.create_generated_identifier(receiver)?;
                    let expression = self
                        .transform_object_literal_method_declaration_to_expression(
                            *property,
                            receiver_node,
                            node,
                            node_multi_line,
                        )?;
                    expressions.push(expression);
                }
                SyntaxKind::PropertyAssignment => {
                    let receiver_node = self.create_generated_identifier(receiver)?;
                    let expression = self.transform_property_assignment_to_expression(
                        *property,
                        receiver_node,
                        node_multi_line,
                    )?;
                    expressions.push(expression);
                }
                SyntaxKind::ShorthandPropertyAssignment => {
                    let receiver_node = self.create_generated_identifier(receiver)?;
                    let expression = self.transform_shorthand_property_assignment_to_expression(
                        *property,
                        receiver_node,
                        node_multi_line,
                    )?;
                    expressions.push(expression);
                }
                _ => {
                    let _ = index;
                    return Err(assembly_kind_error(
                        kind,
                        "addObjectLiteralMembers (Debug.failBadSyntaxKind)",
                    ));
                }
            }
        }
        Ok(())
    }

    /// tsc-port: transformPropertyAssignmentToExpression @6.0.3
    /// tsc-hash: 5b2000cdb58228a9f8836d1e0c0864293f953fb46e31e155f5451a410b79e18e
    /// tsc-span: _tsc.js:107512-107526
    fn transform_property_assignment_to_expression(
        &mut self,
        property: TransformNode,
        receiver: TransformNode,
        starts_on_new_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let (name, initializer) = {
            let NodeData::PropertyAssignment(data) = &self.context.arena().node(property)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::PropertyAssignment,
                    "property assignment",
                ));
            };
            (
                data.name.map(|id| self.node(id)),
                data.initializer.map(|id| self.node(id)),
            )
        };
        let name = name.ok_or(assembly_kind_error(SyntaxKind::PropertyAssignment, "name"))?;
        let initializer = initializer.ok_or(assembly_kind_error(
            SyntaxKind::PropertyAssignment,
            "initializer",
        ))?;
        let visited_name = self.visit_required_expression(name)?;
        let access = self.create_member_access_for_property_name(receiver, visited_name, None)?;
        let visited_initializer = self.visit_required_expression(initializer)?;
        let expression = self.create_assignment(access, visited_initializer)?;
        self.set_text_range(expression, property)?;
        if starts_on_new_line {
            self.start_on_new_line(expression)?;
        }
        Ok(expression)
    }

    /// tsc-port: transformShorthandPropertyAssignmentToExpression @6.0.3
    /// tsc-hash: 7a7be981ba5757884fb490e01196a675412feee29e9ea8128f44db7d18706fc3
    /// tsc-span: _tsc.js:107527-107541
    fn transform_shorthand_property_assignment_to_expression(
        &mut self,
        property: TransformNode,
        receiver: TransformNode,
        starts_on_new_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let name = {
            let NodeData::ShorthandPropertyAssignment(data) =
                &self.context.arena().node(property)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::ShorthandPropertyAssignment,
                    "shorthand assignment",
                ));
            };
            data.name.map(|id| self.node(id))
        };
        let name = name.ok_or(assembly_kind_error(
            SyntaxKind::ShorthandPropertyAssignment,
            "name",
        ))?;
        let visited_name = self.visit_required_expression(name)?;
        let access = self.create_member_access_for_property_name(receiver, visited_name, None)?;
        let clone = self.clone_node(name)?;
        let expression = self.create_assignment(access, clone)?;
        self.set_text_range(expression, property)?;
        if starts_on_new_line {
            self.start_on_new_line(expression)?;
        }
        Ok(expression)
    }

    /// tsc-port: transformObjectLiteralMethodDeclarationToExpression @6.0.3
    /// tsc-hash: 1dd122082b022ec70935a58b9f3b93a3d664d0d00ccd08829398649a2226d158
    /// tsc-span: _tsc.js:107542-107563
    fn transform_object_literal_method_declaration_to_expression(
        &mut self,
        method: TransformNode,
        receiver: TransformNode,
        container: TransformNode,
        starts_on_new_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let name = {
            let NodeData::MethodDeclaration(data) = &self.context.arena().node(method)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::MethodDeclaration,
                    "method declaration",
                ));
            };
            data.name.map(|id| self.node(id))
        };
        let name = name.ok_or(assembly_kind_error(SyntaxKind::MethodDeclaration, "name"))?;
        let visited_name = self.visit_required_expression(name)?;
        let access = self.create_member_access_for_property_name(receiver, visited_name, None)?;
        let function = self.transform_function_like_to_expression(
            method,
            /*location*/ Some(method),
            /*name*/ None,
            Some(container),
        )?;
        let expression = self.create_assignment(access, function)?;
        self.set_text_range(expression, method)?;
        if starts_on_new_line {
            self.start_on_new_line(expression)?;
        }
        Ok(expression)
    }

    /// tsc-port: createMemberAccessForPropertyName @6.0.3
    /// tsc-hash: 88b490bf2cd47503f62314d8fc5fb1c7bca83df86aae8890df643915162ce392
    /// tsc-span: _tsc.js:27206-27217
    ///
    /// Non-computed names are REUSED DIRECTLY (no clone); the resulting
    /// ACCESS takes `setTextRange(…, memberName)` + `NoNestedSourceMaps`;
    /// the computed arm passes `.expression` into an element access ranged
    /// to `location`.
    fn create_member_access_for_property_name(
        &mut self,
        target: TransformNode,
        member_name: TransformNode,
        location: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let kind = self.kind(member_name)?;
        if kind == SyntaxKind::ComputedPropertyName {
            let expression = {
                let NodeData::ComputedPropertyName(data) =
                    &self.context.arena().node(member_name)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::ComputedPropertyName,
                        "computed name",
                    ));
                };
                data.expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::ComputedPropertyName,
                        "expression",
                    ))?
            };
            let access = self.create_element_access(target, expression)?;
            if let Some(location) = location {
                self.set_text_range(access, location)?;
            }
            return Ok(access);
        }
        let access = match &self.context.arena().node(member_name)?.data {
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => {
                self.create_property_access(target, member_name)?
            }
            NodeData::StringLiteral(_) | NodeData::NumericLiteral(_) => {
                self.create_element_access(target, member_name)?
            }
            _ => {
                return Err(assembly_kind_error(
                    kind,
                    "createMemberAccessForPropertyName",
                ));
            }
        };
        self.set_text_range(access, member_name)?;
        self.add_emit_flags(access, EmitFlags::NO_NESTED_SOURCE_MAPS)?;
        Ok(access)
    }

    /// tsc-port: getAllAccessorDeclarations @6.0.3
    /// tsc-hash: 8e23b58d85c286c6344992bac81b90a2c92285508dcf40a9c80d316dca13286a
    /// tsc-span: _tsc.js:16719-16760
    fn get_all_accessor_declarations(
        &self,
        members: &[TransformNode],
        accessor: TransformNode,
    ) -> Result<AllAccessorDeclarations, TransformError> {
        let mut first_accessor: Option<TransformNode> = None;
        let mut second_accessor: Option<TransformNode> = None;
        let mut get_accessor: Option<TransformNode> = None;
        let mut set_accessor: Option<TransformNode> = None;
        if self.has_dynamic_name(accessor)? {
            first_accessor = Some(accessor);
            match self.kind(accessor)? {
                SyntaxKind::GetAccessor => get_accessor = Some(accessor),
                SyntaxKind::SetAccessor => set_accessor = Some(accessor),
                _ => {
                    return Err(assembly_kind_error(
                        self.kind(accessor)?,
                        "getAllAccessorDeclarations",
                    ))
                }
            }
        } else {
            let accessor_name = self.property_name_identity(accessor)?;
            for member in members {
                let member_kind = self.kind(*member)?;
                if !matches!(
                    member_kind,
                    SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
                ) {
                    continue;
                }
                let member_is_static = self.has_static_modifier(*member)?;
                let accessor_is_static = self.has_static_modifier(accessor)?;
                if member_is_static != accessor_is_static {
                    continue;
                }
                let member_name = self.property_name_identity(*member)?;
                if member_name != accessor_name {
                    continue;
                }
                if first_accessor.is_none() {
                    first_accessor = Some(*member);
                } else if second_accessor.is_none() {
                    second_accessor = Some(*member);
                }
                if member_kind == SyntaxKind::GetAccessor && get_accessor.is_none() {
                    get_accessor = Some(*member);
                }
                if member_kind == SyntaxKind::SetAccessor && set_accessor.is_none() {
                    set_accessor = Some(*member);
                }
            }
        }
        let first_accessor = first_accessor.ok_or(assembly_kind_error(
            SyntaxKind::GetAccessor,
            "firstAccessor",
        ))?;
        Ok(AllAccessorDeclarations {
            first_accessor,
            second_accessor,
            get_accessor,
            set_accessor,
        })
    }

    /// tsc-port: hasDynamicName @6.0.3
    /// tsc-hash: d126787bc1b36621098ed5255c26d1e27abe5bf6dbc55570657aa03f95a588bb
    /// tsc-span: _tsc.js:15850-15853
    fn has_dynamic_name(&self, node: TransformNode) -> Result<bool, TransformError> {
        let name = match &self.context.arena().node(node)?.data {
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::PropertyAssignment(data) => data.name,
            _ => None,
        };
        let Some(name) = name else { return Ok(false) };
        let name = self.node(name);
        if self.kind(name)? != SyntaxKind::ComputedPropertyName {
            return Ok(false);
        }
        // isDynamicName: a computed name whose expression is not a
        // string/numeric literal (`getPropertyNameForPropertyNameNode`
        // returns undefined).
        let expression = {
            let NodeData::ComputedPropertyName(data) = &self.context.arena().node(name)?.data
            else {
                return Ok(true);
            };
            data.expression
        };
        match expression {
            Some(expression) => Ok(!matches!(
                self.context.arena().node(self.node(expression))?.data,
                NodeData::StringLiteral(_) | NodeData::NumericLiteral(_)
            )),
            None => Ok(true),
        }
    }

    /// tsc-port: getPropertyNameForPropertyNameNode @6.0.3
    /// tsc-hash: 5770eff9fe2f071f83fce9a7aaff9c54fa6f09141154c33c0f7f3e5dc86ee117
    /// tsc-span: _tsc.js:15861-15887
    ///
    /// Accessor-pair name identity (identifier escaped text; literal text;
    /// literal-computed unwrap; else dynamic → None).
    fn property_name_identity(
        &self,
        member: TransformNode,
    ) -> Result<Option<String>, TransformError> {
        let name = match &self.context.arena().node(member)?.data {
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::PropertyAssignment(data) => data.name,
            _ => None,
        };
        let Some(name) = name else { return Ok(None) };
        let name = self.node(name);
        match &self.context.arena().node(name)?.data {
            NodeData::Identifier(data) => Ok(Some(data.escaped_text.clone())),
            NodeData::StringLiteral(data) => Ok(Some(data.text.clone())),
            NodeData::NumericLiteral(data) => Ok(Some(data.text.clone())),
            NodeData::ComputedPropertyName(data) => match data.expression {
                Some(expression) => match &self.context.arena().node(self.node(expression))?.data {
                    NodeData::StringLiteral(literal) => Ok(Some(literal.text.clone())),
                    NodeData::NumericLiteral(literal) => Ok(Some(literal.text.clone())),
                    _ => Ok(None),
                },
                None => Ok(None),
            },
            _ => Ok(None),
        }
    }

    /// tsc-port: transformAccessorsToExpression @6.0.3
    /// tsc-hash: 8230f8e121121efdb568b49d825d54980c8b0299156d5fead6690f1658cf6cb1
    /// tsc-span: _tsc.js:106085-106150
    fn transform_accessors_to_expression(
        &mut self,
        receiver: TransformNode,
        accessors: &AllAccessorDeclarations,
        container: TransformNode,
        starts_on_new_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let first_accessor = accessors.first_accessor;
        // target: range+parent-threaded clone of the receiver.
        let target = self.clone_node(receiver)?;
        self.set_text_range(target, receiver)?;
        self.add_emit_flags(
            target,
            EmitFlags::NO_COMMENTS | EmitFlags::NO_TRAILING_SOURCE_MAP,
        )?;
        let first_name = self.accessor_name(first_accessor)?;
        self.set_source_map_range_from(target, first_name)?;
        let visited_accessor_name = self.visit_required_expression(first_name)?;
        if matches!(
            self.context.arena().node(visited_accessor_name)?.data,
            NodeData::PrivateIdentifier(_)
        ) {
            return Err(assembly_kind_error(
                SyntaxKind::PrivateIdentifier,
                "Encountered unhandled private identifier while transforming ES2015.",
            ));
        }
        let property_name = self.create_expression_for_property_name(visited_accessor_name)?;
        self.add_emit_flags(
            property_name,
            EmitFlags::NO_COMMENTS | EmitFlags::NO_LEADING_SOURCE_MAP,
        )?;
        self.set_source_map_range_from(property_name, first_name)?;
        let mut properties: Vec<TransformNode> = Vec::new();
        if let Some(get_accessor) = accessors.get_accessor {
            let getter_function = self.transform_function_like_to_expression(
                get_accessor,
                /*location*/ None,
                /*name*/ None,
                Some(container),
            )?;
            self.set_source_map_range_from(getter_function, get_accessor)?;
            self.add_emit_flags(getter_function, EmitFlags::NO_LEADING_COMMENTS)?;
            let getter = self.create_property_assignment_text("get", getter_function)?;
            self.set_comment_range_from(getter, get_accessor)?;
            properties.push(getter);
        }
        if let Some(set_accessor) = accessors.set_accessor {
            let setter_function = self.transform_function_like_to_expression(
                set_accessor,
                /*location*/ None,
                /*name*/ None,
                Some(container),
            )?;
            self.set_source_map_range_from(setter_function, set_accessor)?;
            self.add_emit_flags(setter_function, EmitFlags::NO_LEADING_COMMENTS)?;
            let setter = self.create_property_assignment_text("set", setter_function)?;
            self.set_comment_range_from(setter, set_accessor)?;
            properties.push(setter);
        }
        let enumerable_value =
            if accessors.get_accessor.is_some() || accessors.set_accessor.is_some() {
                self.create_false()?
            } else {
                self.create_true()?
            };
        let enumerable = self.create_property_assignment_text("enumerable", enumerable_value)?;
        properties.push(enumerable);
        let configurable_value = self.create_true()?;
        let configurable =
            self.create_property_assignment_text("configurable", configurable_value)?;
        properties.push(configurable);
        let descriptor = self.create_object_literal(properties, /*multi_line*/ true)?;
        let object_identifier = self.create_identifier("Object")?;
        let define_property =
            self.create_property_access_text(object_identifier, "defineProperty")?;
        let call = self.create_call(define_property, vec![target, property_name, descriptor])?;
        if starts_on_new_line {
            self.start_on_new_line(call)?;
        }
        Ok(call)
    }

    fn accessor_name(&self, accessor: TransformNode) -> Result<TransformNode, TransformError> {
        let name = match &self.context.arena().node(accessor)?.data {
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        name.map(|id| self.node(id)).ok_or(assembly_kind_error(
            SyntaxKind::GetAccessor,
            "accessor name",
        ))
    }

    /// tsc-port: createExpressionForPropertyName @6.0.3
    /// tsc-hash: fc486b593b709b18b266695eed3d95c48147033188cb4fc1c3b0f2a658b8a51d
    /// tsc-span: _tsc.js:27339-27347
    fn create_expression_for_property_name(
        &mut self,
        member_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match &self.context.arena().node(member_name)?.data {
            NodeData::Identifier(data) => {
                let text = data.text.clone();
                self.create_string_literal(&text)
            }
            NodeData::ComputedPropertyName(data) => {
                let expression =
                    data.expression
                        .map(|id| self.node(id))
                        .ok_or(assembly_kind_error(
                            SyntaxKind::ComputedPropertyName,
                            "expression",
                        ))?;
                let clone = self.clone_node(expression)?;
                self.set_text_range(clone, member_name)?;
                Ok(clone)
            }
            NodeData::StringLiteral(_) | NodeData::NumericLiteral(_) => {
                let clone = self.clone_node(member_name)?;
                self.set_text_range(clone, member_name)?;
                Ok(clone)
            }
            _ => Err(assembly_kind_error(
                self.kind(member_name)?,
                "createExpressionForPropertyName",
            )),
        }
    }
}

/// `getAllAccessorDeclarations` result record.
struct AllAccessorDeclarations {
    first_accessor: TransformNode,
    #[allow(dead_code)] // the pinned record shape (getAllAccessorDeclarations)
    second_accessor: Option<TransformNode>,
    get_accessor: Option<TransformNode>,
    set_accessor: Option<TransformNode>,
}

// ---------------------------------------------------------------------------
// Array/call/new spread + call binding + the TS class wrapper
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitArrayLiteralExpression @6.0.3
    /// tsc-hash: 73164d757cad43b2e92f90479df18e4733f74299d6d835b4ea4eff9043c14b32
    /// tsc-span: _tsc.js:107654-107666
    fn visit_array_literal_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (elements, has_trailing_comma) = {
            let NodeData::ArrayLiteralExpression(data) = &self.context.arena().node(node)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::ArrayLiteralExpression,
                    "array literal",
                ));
            };
            let has_trailing_comma = match data.elements {
                Some(elements) => {
                    self.context
                        .arena()
                        .node_array(tsc_syntax_array(self.source, elements))?
                        .has_trailing_comma
                }
                None => false,
            };
            (self.array_nodes(data.elements)?, has_trailing_comma)
        };
        let mut any_spread = false;
        for element in &elements {
            if self.kind(*element)? == SyntaxKind::SpreadElement {
                any_spread = true;
                break;
            }
        }
        if any_spread {
            let multi_line = self.node_is_multi_line(node)?;
            return self.transform_and_spread_elements(
                &elements,
                /*is_argument_list*/ false,
                multi_line,
                has_trailing_comma,
            );
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: visitCallExpression @6.0.3
    /// tsc-hash: 45cabc1dc62c5d5e4f84ecbcdeb1b38b19ede361647ea40e357745f945c7f160
    /// tsc-span: _tsc.js:107667-107686
    fn visit_call_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self
            .internal_emit_flags(node)
            .contains(InternalEmitFlags::TYPE_SCRIPT_CLASS_WRAPPER)
        {
            return self.visit_type_script_class_wrapper(node);
        }
        let (callee, arguments) = {
            let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "call expression",
                ));
            };
            (
                data.expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(SyntaxKind::CallExpression, "callee"))?,
                self.array_nodes(data.arguments)?,
            )
        };
        let skipped = self.skip_outer_expressions(callee)?;
        let mut any_spread = false;
        for argument in &arguments {
            if self.kind(*argument)? == SyntaxKind::SpreadElement {
                any_spread = true;
                break;
            }
        }
        if self.kind(skipped)? == SyntaxKind::SuperKeyword
            || self.is_super_property(skipped)?
            || any_spread
        {
            return self.visit_call_expression_with_potential_captured_this_assignment(
                node, /*assign_to_captured_this*/ true,
            );
        }
        let visited_callee = self.call_expression_visitor(callee)?;
        let mut visited_arguments = Vec::with_capacity(arguments.len());
        for argument in &arguments {
            visited_arguments.push(self.visit_required_expression(*argument)?);
        }
        let arguments_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, visited_arguments)?
        };
        let updated_data = NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
            expression: Some(visited_callee.node()),
            question_dot_token: None,
            type_arguments: None,
            arguments: Some(arguments_array.array()),
        });
        if self.context.arena().node(node)?.data == updated_data {
            return Ok(node);
        }
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: visitCallExpressionWithPotentialCapturedThisAssignment @6.0.3
    /// tsc-hash: 5e6fc555b510aa03d52a0fdffbfd4d9c4b05375f5a5bb5224b5993e84e5db08e
    /// tsc-span: _tsc.js:107784-107828
    fn visit_call_expression_with_potential_captured_this_assignment(
        &mut self,
        node: TransformNode,
        assign_to_captured_this: bool,
    ) -> Result<TransformNode, TransformError> {
        let (callee, arguments) = {
            let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "call expression",
                ));
            };
            (
                data.expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(SyntaxKind::CallExpression, "callee"))?,
                self.array_nodes(data.arguments)?,
            )
        };
        let callee_is_super = self.kind(callee)? == SyntaxKind::SuperKeyword;
        let skipped = self.skip_outer_expressions(callee)?;
        let contains_rest_or_spread = self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_REST_OR_SPREAD);
        if contains_rest_or_spread || callee_is_super || self.is_super_property(skipped)? {
            let (target, this_arg) =
                self.create_call_binding(callee, /*cache_identifiers*/ false)?;
            if callee_is_super {
                self.add_emit_flags(this_arg, EmitFlags::NO_SUBSTITUTION)?;
            }
            let mut resulting_call: TransformNode;
            if contains_rest_or_spread {
                let visited_target = self.call_expression_visitor(target)?;
                let visited_this_arg = if callee_is_super {
                    this_arg
                } else {
                    self.visit_required_expression(this_arg)?
                };
                let spread = self.transform_and_spread_elements(
                    &arguments, /*is_argument_list*/ true, /*multi_line*/ false,
                    /*has_trailing_comma*/ false,
                )?;
                resulting_call =
                    self.create_function_apply_call(visited_target, visited_this_arg, spread)?;
            } else {
                let visited_target = self.call_expression_visitor(target)?;
                let visited_this_arg = if callee_is_super {
                    this_arg
                } else {
                    self.visit_required_expression(this_arg)?
                };
                let mut visited_arguments = Vec::with_capacity(arguments.len());
                for argument in &arguments {
                    visited_arguments.push(self.visit_required_expression(*argument)?);
                }
                resulting_call = self.create_function_call_call(
                    visited_target,
                    visited_this_arg,
                    visited_arguments,
                )?;
                self.set_text_range(resulting_call, node)?;
            }
            if callee_is_super {
                let actual_this = self.create_this_no_substitution()?;
                let initializer = self.create_logical_or(resulting_call, actual_this)?;
                resulting_call = if assign_to_captured_this {
                    let captured = self.create_captured_this()?;
                    self.create_assignment(captured, initializer)?
                } else {
                    initializer
                };
            }
            self.set_original(resulting_call, node)?;
            return Ok(resulting_call);
        }
        if self.is_super_call(node)? {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: visitNewExpression @6.0.3
    /// tsc-hash: f600744e13d11f657ef35a9e79ac71fe932c76773fd6bcf5e4c0299256cdb518
    /// tsc-span: _tsc.js:107829-107852
    fn visit_new_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (callee, arguments) = {
            let NodeData::NewExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::NewExpression,
                    "new expression",
                ));
            };
            (
                data.expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(SyntaxKind::NewExpression, "callee"))?,
                self.array_nodes(data.arguments)?,
            )
        };
        let mut any_spread = false;
        for argument in &arguments {
            if self.kind(*argument)? == SyntaxKind::SpreadElement {
                any_spread = true;
                break;
            }
        }
        if any_spread {
            // const { target, thisArg } = createCallBinding(createPropertyAccessExpression(node.expression, "bind"), hoist)
            let bind_access = self.create_property_access_text(callee, "bind")?;
            let (target, this_arg) =
                self.create_call_binding(bind_access, /*cache_identifiers*/ false)?;
            let visited_target = self.visit_required_expression(target)?;
            let void_zero = self.create_void_zero()?;
            let mut spread_elements = vec![void_zero];
            spread_elements.extend(arguments.iter().copied());
            let spread = self.transform_and_spread_elements(
                &spread_elements,
                /*is_argument_list*/ true,
                /*multi_line*/ false,
                /*has_trailing_comma*/ false,
            )?;
            let apply = self.create_function_apply_call(visited_target, this_arg, spread)?;
            let new_expression = self.create_new_expression(apply, vec![])?;
            return Ok(new_expression);
        }
        self.visit_each_child_required(node)
    }

    /// tsc-port: transformAndSpreadElements @6.0.3
    /// tsc-hash: e8c7f17449f433c1dbed7938b6de257d7a724f594fbddf4c7c9c1284dbf3c994
    /// tsc-span: _tsc.js:107853-107879
    fn transform_and_spread_elements(
        &mut self,
        elements: &[TransformNode],
        is_argument_list: bool,
        multi_line: bool,
        has_trailing_comma: bool,
    ) -> Result<TransformNode, TransformError> {
        let num_elements = elements.len();
        // spanMap over the spread partition (`_tsc.js:324-356`): contiguous
        // runs keyed by spread-ness, visited per run.
        let mut segments: Vec<SpreadSegment> = Vec::new();
        let mut index = 0usize;
        while index < num_elements {
            let start = index;
            let is_spread_run = self.kind(elements[index])? == SyntaxKind::SpreadElement;
            while index < num_elements
                && (self.kind(elements[index])? == SyntaxKind::SpreadElement) == is_spread_run
            {
                index += 1;
            }
            let chunk = &elements[start..index];
            if is_spread_run {
                for element in chunk {
                    segments.push(self.visit_expression_of_spread(*element)?);
                }
            } else {
                segments.push(self.visit_span_of_non_spreads(
                    chunk,
                    multi_line,
                    has_trailing_comma && index == num_elements,
                )?);
            }
        }
        if segments.len() == 1 {
            let first_segment = &segments[0];
            let use_directly = (is_argument_list && !self.downlevel_iteration)
                || self.is_packed_array_literal(first_segment.expression)?
                || self.is_call_to_helper(first_segment.expression, "___spreadArray")?;
            if use_directly {
                return Ok(segments[0].expression);
            }
        }
        let starts_with_spread = segments[0].kind != SpreadSegmentKind::None;
        let mut expression: TransformNode = if starts_with_spread {
            self.create_array_literal(vec![])?
        } else {
            segments[0].expression
        };
        let start_index = if starts_with_spread { 0 } else { 1 };
        for segment in segments.iter().skip(start_index) {
            let helper = self.create_spread_array_helper_call(
                expression,
                segment.expression,
                segment.kind == SpreadSegmentKind::UnpackedSpread && !is_argument_list,
            )?;
            expression = helper;
        }
        Ok(expression)
    }

    /// tsc-port: visitExpressionOfSpread @6.0.3
    /// tsc-hash: 3a184d6548d64722a1e1a352a831c0bfa6d360bb1718f25a82b8d41d4f88d138
    /// tsc-span: _tsc.js:107886-107901
    fn visit_expression_of_spread(
        &mut self,
        node: TransformNode,
    ) -> Result<SpreadSegment, TransformError> {
        let expression = {
            let NodeData::SpreadElement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::SpreadElement, "spread"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::SpreadElement, "expression"))?
        };
        let mut expression = self.visit_required_expression(expression)?;
        let is_call_to_read_helper = self.is_call_to_helper(expression, "___read")?;
        let mut kind = if is_call_to_read_helper || self.is_packed_array_literal(expression)? {
            SpreadSegmentKind::PackedSpread
        } else {
            SpreadSegmentKind::UnpackedSpread
        };
        if self.downlevel_iteration
            && kind == SpreadSegmentKind::UnpackedSpread
            && !matches!(
                self.context.arena().node(expression)?.data,
                NodeData::ArrayLiteralExpression(_)
            )
            && !is_call_to_read_helper
        {
            expression = self.create_read_helper_call(expression)?;
            kind = SpreadSegmentKind::PackedSpread;
        }
        Ok(SpreadSegment { kind, expression })
    }

    /// tsc-port: visitSpanOfNonSpreads @6.0.3
    /// tsc-hash: 36b1da1779ffb146910da0130ca7c5908e731fa807701bf42f83140de1cc7225
    /// tsc-span: _tsc.js:107902-107908
    fn visit_span_of_non_spreads(
        &mut self,
        chunk: &[TransformNode],
        multi_line: bool,
        has_trailing_comma: bool,
    ) -> Result<SpreadSegment, TransformError> {
        let mut visited = Vec::with_capacity(chunk.len());
        for element in chunk {
            visited.push(self.visit_required_expression(*element)?);
        }
        let expression = self.create_array_literal_full(visited, multi_line, has_trailing_comma)?;
        Ok(SpreadSegment {
            kind: SpreadSegmentKind::None,
            expression,
        })
    }

    /// tsc-port: isPackedArrayLiteral @6.0.3 (+ isPackedElement)
    /// tsc-hash: 6728e2c9fc07f76e988b78dfede14091e118d2a7dbffdb76a69ec30452b9211e
    /// tsc-span: _tsc.js:19085-19090
    fn is_packed_array_literal(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ArrayLiteralExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        for element in self.array_nodes(data.elements)? {
            let kind = self.kind(element)?;
            if kind == SyntaxKind::OmittedExpression || kind == SyntaxKind::SpreadElement {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: isCallToHelper @6.0.3
    /// tsc-hash: 65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef
    /// tsc-span: _tsc.js:26566-26568
    fn is_call_to_helper(
        &self,
        node: TransformNode,
        helper_name: &str,
    ) -> Result<bool, TransformError> {
        let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(callee) = data.expression else {
            return Ok(false);
        };
        let callee = self.node(callee);
        if !self.emit_flags(callee).contains(EmitFlags::HELPER_NAME) {
            return Ok(false);
        }
        match &self.context.arena().node(callee)?.data {
            NodeData::Identifier(data) => Ok(data.escaped_text == helper_name),
            _ => Ok(false),
        }
    }

    /// `emitHelpers().createSpreadArrayHelper(to, from, packFrom)`.
    fn create_spread_array_helper_call(
        &mut self,
        to: TransformNode,
        from: TransformNode,
        pack_from: bool,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::spread_array())?;
        let source = self.source;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::SpreadArray)?;
        let pack = if pack_from {
            self.create_true()?
        } else {
            self.create_false()?
        };
        self.create_call(helper, vec![to, from, pack])
    }

    /// `emitHelpers().createReadHelper(iteratorRecord, count?)` — the
    /// spread arm passes no count.
    fn create_read_helper_call(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::read())?;
        let source = self.source;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::Read)?;
        self.create_call(helper, vec![expression])
    }

    /// `emitHelpers().createValuesHelper(expression)`.
    fn create_values_helper_call(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::values())?;
        let source = self.source;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::Values)?;
        self.create_call(helper, vec![expression])
    }

    /// tsc-port: createTemplateObjectHelper @6.0.3
    /// tsc-hash: 270715e6924c8655b32e871c56808bfea6e6230a85a9e66ee9a447828277a5a9
    /// tsc-span: _tsc.js:25861-25869
    pub(super) fn create_template_object_helper_call(
        &mut self,
        cooked: TransformNode,
        raw: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(helpers::make_template_object())?;
        let source = self.source;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::MakeTemplateObject)?;
        self.create_call(helper, vec![cooked, raw])
    }

    /// tsc-port: recordTaggedTemplateString @6.0.3
    /// tsc-hash: e44a8b5f8a01f5174faa2e9ba2d0e629b9175d698d9290e9a78ab4ca9e8126fc
    /// tsc-span: _tsc.js:104759-104764
    pub(super) fn record_tagged_template_string(
        &mut self,
        temp: TransformNode,
    ) -> Result<(), TransformError> {
        let declaration = self.create_variable_declaration_plain(temp, None)?;
        self.tagged_template_string_declarations.push(declaration);
        Ok(())
    }

    /// `isExternalModule(currentSourceFile)` — the parse record's
    /// external-module indicator.
    pub(super) fn is_external_module_source(&self) -> Result<bool, TransformError> {
        Ok(self
            .context
            .arena()
            .source(self.source)?
            .syntax()
            .external_module_indicator
            .is_some())
    }

    /// Shared-module read access to the arena record (the
    /// tagged-template module's data walks).
    pub(super) fn arena_node(
        &self,
        node: TransformNode,
    ) -> Result<&tsc_syntax::Node, TransformError> {
        self.context.arena().node(node)
    }

    /// `emitHelpers().createExtendsHelper(name)` — `__extends(name,
    /// _super)` with the file-level-optimistic `_super` as the second
    /// argument (`_tsc.js:25852-25860`).
    fn create_extends_helper_call(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(helpers::extends())?;
        let source = self.source;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::Extends)?;
        let synthetic_super = self.create_synthetic_super()?;
        self.create_call(helper, vec![name, synthetic_super])
    }

    /// tsc-port: createFunctionApplyCall @6.0.3 (over createMethodCall)
    /// tsc-hash: 923d94cd6667bba3ee96872eee5dcc84aa7dbcf5b2bb71b22f7357af1eb1b21d
    /// tsc-span: _tsc.js:24583-24585
    fn create_function_apply_call(
        &mut self,
        target: TransformNode,
        this_arg: TransformNode,
        arguments_expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_property_access_text(target, "apply")?;
        self.create_call(access, vec![this_arg, arguments_expression])
    }

    /// tsc-port: createFunctionCallCall @6.0.3
    /// tsc-hash: b63730e192802f36eae8706de4ffa67ce7722aadcfd695eeea01f77f1b7a3f00
    /// tsc-span: _tsc.js:24580-24582
    fn create_function_call_call(
        &mut self,
        target: TransformNode,
        this_arg: TransformNode,
        arguments_list: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_property_access_text(target, "call")?;
        let mut arguments = vec![this_arg];
        arguments.extend(arguments_list);
        self.create_call(access, arguments)
    }

    /// tsc-port: shouldBeCapturedInTempVariable @6.0.3
    /// tsc-hash: 930638d4e30da0491d0c7e2612bf2920f6280413e771880f72c3c18f6712baf0
    /// tsc-span: _tsc.js:24669-24690
    fn should_be_captured_in_temp_variable(
        &self,
        node: TransformNode,
        cache_identifiers: bool,
    ) -> Result<bool, TransformError> {
        let mut target = node;
        // skipParentheses
        while let NodeData::ParenthesizedExpression(data) = &self.context.arena().node(target)?.data
        {
            match data.expression {
                Some(expression) => target = self.node(expression),
                None => break,
            }
        }
        Ok(match &self.context.arena().node(target)?.data {
            NodeData::Identifier(_) => cache_identifiers,
            NodeData::Token
                if self.context.arena().node(target)?.kind == SyntaxKind::ThisKeyword =>
            {
                false
            }
            NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::StringLiteral(_) => false,
            NodeData::ArrayLiteralExpression(data) => {
                let elements = self.array_nodes(data.elements)?;
                !elements.is_empty()
            }
            NodeData::ObjectLiteralExpression(data) => {
                let properties = self.array_nodes(data.properties)?;
                !properties.is_empty()
            }
            _ => true,
        })
    }

    /// tsc-port: createCallBinding @6.0.3
    /// tsc-hash: 445f6a3542132e1adf49e01683e039e6fa034bd127cd15ab5447db84951b41bc
    /// tsc-span: _tsc.js:24691-24753
    ///
    /// The ES2015 call sites pass `languageVersion = undefined` and
    /// `cacheIdentifiers = false`; the super arms are LIVE here (§12.9) —
    /// the `languageVersion < ES2015 → "_super"` sub-arm stays dormant
    /// (no caller passes a version) and ports faithfully as unreachable.
    fn create_call_binding(
        &mut self,
        expression: TransformNode,
        cache_identifiers: bool,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        let callee = self.skip_outer_expressions(expression)?;
        let this_arg: TransformNode;
        let target: TransformNode;
        if self.is_super_property(callee)? {
            this_arg = self.create_this_token()?;
            target = callee;
        } else if self.kind(callee)? == SyntaxKind::SuperKeyword {
            this_arg = self.create_this_token()?;
            // languageVersion is never passed at the ES2015 call sites →
            // the `< ES2015` "_super" identifier arm is dormant.
            target = callee;
        } else if self.emit_flags(callee).contains(EmitFlags::HELPER_NAME) {
            this_arg = self.create_void_zero()?;
            target = expression;
        } else {
            match &self.context.arena().node(callee)?.data.clone() {
                NodeData::PropertyAccessExpression(data) => {
                    let receiver =
                        data.expression
                            .map(|id| self.node(id))
                            .ok_or(assembly_kind_error(
                                SyntaxKind::PropertyAccessExpression,
                                "receiver",
                            ))?;
                    let name = data
                        .name
                        .map(|id| self.node(id))
                        .ok_or(assembly_kind_error(
                            SyntaxKind::PropertyAccessExpression,
                            "name",
                        ))?;
                    if self.should_be_captured_in_temp_variable(receiver, cache_identifiers)? {
                        let temp_binding = self.allocate_temp_binding()?;
                        {
                            let hoist_identifier =
                                self.create_generated_identifier(&temp_binding)?;
                            self.context.hoist_variable_declaration(hoist_identifier)?;
                        }
                        let temp = self.create_generated_identifier(&temp_binding)?;
                        let assignment = self.create_assignment(temp, receiver)?;
                        self.set_text_range(assignment, receiver)?;
                        let access = self.create_property_access(assignment, name)?;
                        self.set_text_range(access, callee)?;
                        this_arg = self.create_generated_identifier(&temp_binding)?;
                        target = access;
                    } else {
                        this_arg = receiver;
                        target = callee;
                    }
                }
                NodeData::ElementAccessExpression(data) => {
                    let receiver =
                        data.expression
                            .map(|id| self.node(id))
                            .ok_or(assembly_kind_error(
                                SyntaxKind::ElementAccessExpression,
                                "receiver",
                            ))?;
                    let argument = data.argument_expression.map(|id| self.node(id)).ok_or(
                        assembly_kind_error(SyntaxKind::ElementAccessExpression, "argument"),
                    )?;
                    if self.should_be_captured_in_temp_variable(receiver, cache_identifiers)? {
                        let temp_binding = self.allocate_temp_binding()?;
                        {
                            let hoist_identifier =
                                self.create_generated_identifier(&temp_binding)?;
                            self.context.hoist_variable_declaration(hoist_identifier)?;
                        }
                        let temp = self.create_generated_identifier(&temp_binding)?;
                        let assignment = self.create_assignment(temp, receiver)?;
                        self.set_text_range(assignment, receiver)?;
                        let access = self.create_element_access(assignment, argument)?;
                        self.set_text_range(access, callee)?;
                        this_arg = self.create_generated_identifier(&temp_binding)?;
                        target = access;
                    } else {
                        this_arg = receiver;
                        target = callee;
                    }
                }
                _ => {
                    this_arg = self.create_void_zero()?;
                    target = expression;
                }
            }
        }
        Ok((target, this_arg))
    }

    /// tsc-port: visitTypeScriptClassWrapper @6.0.3
    /// tsc-hash: cd917e405f67b9f0859344acdb38cb908312e37cb2b1bb466bff69ffc3f3c19e
    /// tsc-span: _tsc.js:107687-107783
    fn visit_type_script_class_wrapper(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // const body = cast(cast(skipOuterExpressions(node.expression), isArrowFunction).body, isBlock)
        let call_expression = {
            let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "class wrapper call",
                ));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::CallExpression, "callee"))?
        };
        let arrow = self.skip_outer_expressions(call_expression)?;
        let body = {
            let NodeData::ArrowFunction(data) = &self.context.arena().node(arrow)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::ArrowFunction,
                    "class wrapper arrow",
                ));
            };
            data.body
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::ArrowFunction, "body"))?
        };
        let body_statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "class wrapper body"));
            };
            self.array_nodes(data.statements)?
        };
        // visit with the class-wrapper statement visitor
        let saved_converted_loop_state = self.converted_loop_state.take();
        let mut visited_body_statements: Vec<TransformNode> = Vec::new();
        for statement in &body_statements {
            match self.class_wrapper_statement_visit(*statement)? {
                VisitOutcome::Elided => {}
                VisitOutcome::One(node) => visited_body_statements.push(node),
                VisitOutcome::Many(nodes) => visited_body_statements.extend(nodes),
            }
        }
        self.converted_loop_state = saved_converted_loop_state;
        let is_variable_statement_with_initializer = |visitor: &Self,
                                                      statement: TransformNode|
         -> Result<bool, TransformError> {
            let NodeData::VariableStatement(data) = &visitor.context.arena().node(statement)?.data
            else {
                return Ok(false);
            };
            let Some(list) = data.declaration_list else {
                return Ok(false);
            };
            let NodeData::VariableDeclarationList(list_data) =
                &visitor.context.arena().node(visitor.node(list))?.data
            else {
                return Ok(false);
            };
            let declarations = visitor.array_nodes(list_data.declarations)?;
            match declarations.first() {
                Some(first) => {
                    let NodeData::VariableDeclaration(declaration) =
                        &visitor.context.arena().node(*first)?.data
                    else {
                        return Ok(false);
                    };
                    Ok(declaration.initializer.is_some())
                }
                None => Ok(false),
            }
        };
        let mut class_statements: Vec<TransformNode> = Vec::new();
        let mut remaining_statements: Vec<TransformNode> = Vec::new();
        for statement in &visited_body_statements {
            if is_variable_statement_with_initializer(self, *statement)? {
                class_statements.push(*statement);
            } else {
                remaining_statements.push(*statement);
            }
        }
        let var_statement = *class_statements.first().ok_or(assembly_kind_error(
            SyntaxKind::VariableStatement,
            "class wrapper variable statement",
        ))?;
        let variable = {
            let NodeData::VariableStatement(data) = &self.context.arena().node(var_statement)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableStatement,
                    "class alias statement",
                ));
            };
            let list = data.declaration_list.ok_or(assembly_kind_error(
                SyntaxKind::VariableStatement,
                "declaration list",
            ))?;
            let NodeData::VariableDeclarationList(list_data) =
                &self.context.arena().node(self.node(list))?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclarationList,
                    "declaration list",
                ));
            };
            *self
                .array_nodes(list_data.declarations)?
                .first()
                .ok_or(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "class alias declaration",
                ))?
        };
        let initializer = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(variable)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "class alias",
                ));
            };
            let initializer = data.initializer.ok_or(assembly_kind_error(
                SyntaxKind::VariableDeclaration,
                "initializer",
            ))?;
            self.skip_outer_expressions(self.node(initializer))?
        };
        // aliasAssignment: tryCast(initializer, isAssignmentExpression) or
        // the comma-left arm.
        let mut alias_assignment: Option<TransformNode> = None;
        if self.is_plain_assignment_expression(initializer)? {
            alias_assignment = Some(initializer);
        } else if let NodeData::BinaryExpression(data) =
            &self.context.arena().node(initializer)?.data
        {
            let operator = data.operator_token.map(|id| self.node(id));
            if let Some(operator) = operator {
                if self.kind(operator)? == SyntaxKind::CommaToken {
                    if let Some(left) = data.left {
                        let left = self.node(left);
                        if self.is_plain_assignment_expression(left)? {
                            alias_assignment = Some(left);
                        }
                    }
                }
            }
        }
        let call = {
            let candidate = match alias_assignment {
                Some(assignment) => {
                    let NodeData::BinaryExpression(data) =
                        &self.context.arena().node(assignment)?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::BinaryExpression,
                            "alias assignment",
                        ));
                    };
                    let right = data.right.ok_or(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "alias right",
                    ))?;
                    self.skip_outer_expressions(self.node(right))?
                }
                None => initializer,
            };
            if !matches!(
                self.context.arena().node(candidate)?.data,
                NodeData::CallExpression(_)
            ) {
                return Err(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "class wrapper IIFE call",
                ));
            }
            candidate
        };
        let func = {
            let NodeData::CallExpression(data) = &self.context.arena().node(call)?.data else {
                return Err(assembly_kind_error(SyntaxKind::CallExpression, "IIFE"));
            };
            let callee = data.expression.ok_or(assembly_kind_error(
                SyntaxKind::CallExpression,
                "IIFE callee",
            ))?;
            let skipped = self.skip_outer_expressions(self.node(callee))?;
            if !matches!(
                self.context.arena().node(skipped)?.data,
                NodeData::FunctionExpression(_)
            ) {
                return Err(assembly_kind_error(
                    SyntaxKind::FunctionExpression,
                    "IIFE function",
                ));
            }
            skipped
        };
        let func_statements = {
            let NodeData::FunctionExpression(data) = &self.context.arena().node(func)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::FunctionExpression,
                    "IIFE function",
                ));
            };
            let body = data.body.ok_or(assembly_kind_error(
                SyntaxKind::FunctionExpression,
                "IIFE body",
            ))?;
            let NodeData::Block(block) = &self.context.arena().node(self.node(body))?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "IIFE body"));
            };
            self.array_nodes(block.statements)?
        };
        let mut class_body_start = 0usize;
        let mut statements: Vec<TransformNode> = Vec::new();
        if let Some(assignment) = alias_assignment {
            // extendsCall: tryCast(funcStatements[classBodyStart], isExpressionStatement)
            if let Some(first) = func_statements.first() {
                if matches!(
                    self.context.arena().node(*first)?.data,
                    NodeData::ExpressionStatement(_)
                ) {
                    statements.push(*first);
                    class_body_start += 1;
                }
            }
            let class_statement =
                *func_statements
                    .get(class_body_start)
                    .ok_or(assembly_kind_error(
                        SyntaxKind::FunctionDeclaration,
                        "wrapped class statement",
                    ))?;
            statements.push(class_statement);
            class_body_start += 1;
            // exports.C = C
            let alias_left = {
                let NodeData::BinaryExpression(data) = &self.context.arena().node(assignment)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "alias assignment",
                    ));
                };
                data.left.ok_or(assembly_kind_error(
                    SyntaxKind::BinaryExpression,
                    "alias left",
                ))?
            };
            let variable_name = {
                let NodeData::VariableDeclaration(data) =
                    &self.context.arena().node(variable)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::VariableDeclaration,
                        "alias variable",
                    ));
                };
                data.name.ok_or(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "alias variable name",
                ))?
            };
            let assignment_expression =
                self.create_assignment(self.node(alias_left), self.node(variable_name))?;
            let statement = self.create_expression_statement(assignment_expression)?;
            statements.push(statement);
        }
        // while (!isReturnStatement(elementAt(funcStatements, classBodyEnd))) classBodyEnd--
        let mut class_body_end: isize = -1;
        loop {
            let index = func_statements.len() as isize + class_body_end;
            if index < 0 {
                return Err(assembly_kind_error(
                    SyntaxKind::ReturnStatement,
                    "class wrapper return statement",
                ));
            }
            let statement = func_statements[index as usize];
            if matches!(
                self.context.arena().node(statement)?.data,
                NodeData::ReturnStatement(_)
            ) {
                break;
            }
            class_body_end -= 1;
        }
        let end_index = (func_statements.len() as isize + class_body_end) as usize;
        statements.extend_from_slice(&func_statements[class_body_start..end_index]);
        if class_body_end < -1 {
            statements.extend_from_slice(&func_statements[end_index + 1..]);
        }
        let return_statement = {
            let statement = func_statements[end_index];
            if matches!(
                self.context.arena().node(statement)?.data,
                NodeData::ReturnStatement(_)
            ) {
                Some(statement)
            } else {
                None
            }
        };
        for statement in &remaining_statements {
            let is_return = matches!(
                self.context.arena().node(*statement)?.data,
                NodeData::ReturnStatement(_)
            );
            let replace = if is_return {
                match return_statement {
                    Some(return_statement) => {
                        let NodeData::ReturnStatement(data) =
                            &self.context.arena().node(return_statement)?.data
                        else {
                            return Err(assembly_kind_error(
                                SyntaxKind::ReturnStatement,
                                "wrapper return",
                            ));
                        };
                        match data.expression {
                            Some(expression) => !matches!(
                                self.context.arena().node(self.node(expression))?.data,
                                NodeData::Identifier(_)
                            ),
                            None => false,
                        }
                    }
                    None => false,
                }
            } else {
                false
            };
            if replace {
                statements.push(return_statement.expect("checked above"));
            } else {
                statements.push(*statement);
            }
        }
        // addRange(statements, classStatements, /*start*/ 1)
        statements.extend_from_slice(&class_statements[1..]);
        // rebuild: restoreOuterExpressions(node.expression, restoreOuterExpressions(variable.initializer, restoreOuterExpressions(aliasAssignment?.right, updateCall(...))))
        let func_body = self.function_body(func)?.ok_or(assembly_kind_error(
            SyntaxKind::FunctionExpression,
            "IIFE body",
        ))?;
        let statements_array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, statements)?
        };
        let updated_body_data = NodeData::Block(tsc_syntax::nodes::BlockData {
            statements: Some(statements_array.array()),
        });
        let updated_body = {
            let flags = flags_after_update(self.context.arena(), func_body, &updated_body_data)?;
            self.context
                .factory()?
                .update_node(func_body, updated_body_data, flags)?
        };
        let updated_func = {
            let NodeData::FunctionExpression(data) = &self.context.arena().node(func)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::FunctionExpression,
                    "IIFE function",
                ));
            };
            let mut data = data.clone();
            data.modifiers = None;
            data.asterisk_token = None;
            data.name = None;
            data.type_parameters = None;
            data.r#type = None;
            data.body = Some(updated_body.node());
            let data = NodeData::FunctionExpression(data);
            let flags = flags_after_update(self.context.arena(), func, &data)?;
            self.context.factory()?.update_node(func, data, flags)?
        };
        let iife_callee = {
            let NodeData::CallExpression(data) = &self.context.arena().node(call)?.data else {
                return Err(assembly_kind_error(SyntaxKind::CallExpression, "IIFE"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "IIFE callee",
                ))?
        };
        let restored_callee = self.restore_outer_expressions(Some(iife_callee), updated_func)?;
        let updated_call = {
            let NodeData::CallExpression(data) = &self.context.arena().node(call)?.data else {
                return Err(assembly_kind_error(SyntaxKind::CallExpression, "IIFE"));
            };
            let mut data = data.clone();
            data.expression = Some(restored_callee.node());
            let data = NodeData::CallExpression(data);
            let flags = flags_after_update(self.context.arena(), call, &data)?;
            self.context.factory()?.update_node(call, data, flags)?
        };
        let alias_right = match alias_assignment {
            Some(assignment) => {
                let NodeData::BinaryExpression(data) = &self.context.arena().node(assignment)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "alias assignment",
                    ));
                };
                data.right.map(|id| self.node(id))
            }
            None => None,
        };
        let restored_alias = self.restore_outer_expressions(alias_right, updated_call)?;
        let variable_initializer = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(variable)?.data
            else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "alias variable",
                ));
            };
            data.initializer.map(|id| self.node(id))
        };
        let restored_initializer =
            self.restore_outer_expressions(variable_initializer, restored_alias)?;
        let node_expression = {
            let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::CallExpression,
                    "class wrapper call",
                ));
            };
            data.expression.map(|id| self.node(id))
        };
        self.restore_outer_expressions(node_expression, restored_initializer)
    }

    fn is_plain_assignment_expression(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        match data.operator_token {
            Some(operator) => Ok(self.kind(self.node(operator))? == SyntaxKind::EqualsToken),
            None => Ok(false),
        }
    }

    /// tsc-port: classWrapperStatementVisitor @6.0.3
    /// tsc-hash: a194668e571877263b786ff647eee87cc430cebcd54f37ea5c72eec9c5819572
    /// tsc-span: _tsc.js:104821-104844
    fn class_wrapper_statement_visit(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        if self.should_visit_node(node)? {
            let original = self.context.arena().get_original_node(node);
            let is_static_property = {
                let record = self.context.arena().node(original)?;
                matches!(record.data, NodeData::PropertyDeclaration(_))
                    && self.has_static_modifier(original)?
            };
            if is_static_property {
                let ancestor = enter_subtree(
                    &mut self.print_state.hierarchy_facts,
                    HierarchyFacts::STATIC_INITIALIZER_EXCLUDES,
                    HierarchyFacts::STATIC_INITIALIZER_INCLUDES,
                );
                let result = self.visitor_worker(node, false)?;
                exit_subtree(
                    &mut self.print_state.hierarchy_facts,
                    ancestor,
                    HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
                    HierarchyFacts::NONE,
                );
                return Ok(result);
            }
            return self.visitor_worker(node, false);
        }
        Ok(VisitOutcome::One(node))
    }

    /// tsc-port: restoreOuterExpressions @6.0.3
    /// tsc-hash: 954f25c47999754f47c599c6955c83aa60e378fc74ef0ba8fd54289bbd65abd8
    /// tsc-span: _tsc.js:24646-24654
    fn restore_outer_expressions(
        &mut self,
        outer: Option<TransformNode>,
        inner: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(outer_expression) = outer else {
            return Ok(inner);
        };
        let is_outer = matches!(
            self.context.arena().node(outer_expression)?.data,
            NodeData::ParenthesizedExpression(_)
                | NodeData::TypeAssertionExpression(_)
                | NodeData::AsExpression(_)
                | NodeData::SatisfiesExpression(_)
                | NodeData::NonNullExpression(_)
                | NodeData::ExpressionWithTypeArguments(_)
                | NodeData::PartiallyEmittedExpression(_)
        );
        if !is_outer || self.is_ignorable_paren(outer_expression)? {
            return Ok(inner);
        }
        let inner_expression = {
            let child = match &self.context.arena().node(outer_expression)?.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                _ => None,
            };
            child.map(|id| self.node(id))
        };
        let restored_inner = self.restore_outer_expressions(inner_expression, inner)?;
        // updateOuterExpression (`_tsc.js:24625-24642`)
        let updated_data = match &self.context.arena().node(outer_expression)?.data {
            NodeData::ParenthesizedExpression(_) => {
                NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                    expression: Some(restored_inner.node()),
                })
            }
            NodeData::PartiallyEmittedExpression(_) => NodeData::PartiallyEmittedExpression(
                tsc_syntax::nodes::PartiallyEmittedExpressionData {
                    expression: Some(restored_inner.node()),
                },
            ),
            _ => {
                return Err(assembly_kind_error(
                    self.kind(outer_expression)?,
                    "updateOuterExpression (TS-assertion arms are TS-syntax input)",
                ))
            }
        };
        if self.context.arena().node(outer_expression)?.data == updated_data {
            return Ok(outer_expression);
        }
        let flags = flags_after_update(self.context.arena(), outer_expression, &updated_data)?;
        self.context
            .factory()?
            .update_node(outer_expression, updated_data, flags)
    }

    /// tsc-port: isIgnorableParen @6.0.3
    /// tsc-hash: 8bbd617706416b4ea200ed3b203c13978b3f36b17ca182602f4ac56f0aa68323
    /// tsc-span: _tsc.js:24643-24645
    fn is_ignorable_paren(&self, node: TransformNode) -> Result<bool, TransformError> {
        if !matches!(
            self.context.arena().node(node)?.data,
            NodeData::ParenthesizedExpression(_)
        ) {
            return Ok(false);
        }
        if !self.node_is_synthesized(node)? {
            return Ok(false);
        }
        let metadata = self.context.arena().metadata(node);
        let has_explicit_ranges = metadata.is_some_and(|metadata| {
            metadata.source_map_range().is_some() || metadata.comment_range().is_some()
        });
        let has_synthetic_comments = metadata.is_some_and(|metadata| {
            !metadata.leading_comments().is_empty() || !metadata.trailing_comments().is_empty()
        });
        Ok(!has_explicit_ranges && !has_synthetic_comments)
    }
}

// ---------------------------------------------------------------------------
// Class lowering lanes
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: visitClassDeclaration @6.0.3
    /// tsc-hash: b5eee1e707db4b5a7f0bd97240c5e9614cfd341992a4c5cddf52223940b0626e
    /// tsc-span: _tsc.js:105144-105174
    fn visit_class_declaration(
        &mut self,
        node: TransformNode,
    ) -> Result<VisitOutcome, TransformError> {
        let local_name = self.get_local_name(node, /*allow_comments*/ true)?;
        let class_expression = self.transform_class_like_declaration_to_expression(node)?;
        let variable =
            self.create_variable_declaration_plain(local_name, Some(class_expression))?;
        self.set_original(variable, node)?;
        let mut statements: Vec<TransformNode> = Vec::new();
        let list = self.create_variable_declaration_list(vec![variable])?;
        let statement = self.create_variable_statement_from_list(list)?;
        self.set_original(statement, node)?;
        self.set_text_range(statement, node)?;
        self.start_on_new_line(statement)?;
        statements.push(statement);
        if self.has_syntactic_export_modifier(node)? {
            let export_statement = if self.has_syntactic_default_modifier(node)? {
                let name = self.get_local_name(node, /*allow_comments*/ false)?;
                self.create_export_default(name)?
            } else {
                let name = self.get_local_name(node, /*allow_comments*/ false)?;
                self.create_external_module_export(name)?
            };
            self.set_original(export_statement, statement)?;
            statements.push(export_statement);
        }
        if statements.len() == 1 {
            Ok(VisitOutcome::One(statements.remove(0)))
        } else {
            Ok(VisitOutcome::Many(statements))
        }
    }

    /// tsc-port: createExportDefault @6.0.3 (dormant here — module files
    /// leave the §7 fixture language; unit-driven)
    /// tsc-hash: e531c9901b0419c3d02970c67a28f7e130d9d8701ac5cc6523f5568f8fd6172b
    /// tsc-span: _tsc.js:24522-24530
    fn create_export_default(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let flags = self.child_flags(&[expression])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                expression: Some(expression.node()),
                is_export_equals: Some(false),
                modifiers: None,
            }),
            flags,
        )
    }

    /// tsc-port: createExternalModuleExport @6.0.3 (dormant here)
    /// tsc-hash: 6290f43d065740c822616b58b195cee6622f8f717bdae7037b39e1fc2648495f
    /// tsc-span: _tsc.js:24531-24547
    fn create_external_module_export(
        &mut self,
        export_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source;
        let specifier = {
            let flags = self.child_flags(&[export_name])?;
            self.context.factory()?.create_node(
                source,
                NodeData::ExportSpecifier(tsc_syntax::nodes::ExportSpecifierData {
                    property_name: None,
                    name: Some(export_name.node()),
                    is_type_only: false,
                }),
                flags,
            )?
        };
        let named_exports = {
            let array = self
                .context
                .factory()?
                .create_node_array(source, vec![specifier])?;
            let flags = self.context.arena().array_transform_flags(array);
            self.context.factory()?.create_node(
                source,
                NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                    elements: Some(array.array()),
                }),
                flags,
            )?
        };
        let flags = self.child_flags(&[named_exports])?;
        self.context.factory()?.create_node(
            source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                export_clause: Some(named_exports.node()),
                module_specifier: None,
                is_type_only: false,
                attributes: None,
                modifiers: None,
            }),
            flags,
        )
    }

    /// tsc-port: visitClassExpression @6.0.3
    /// tsc-hash: 290a1ce981403ab42d9557d7235156214cfe66e5c69b388cf2f064038fdfc52d
    /// tsc-span: _tsc.js:105175-105177
    fn visit_class_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.transform_class_like_declaration_to_expression(node)
    }

    /// tsc-port: transformClassLikeDeclarationToExpression @6.0.3
    /// tsc-hash: 73df944ac57dd513d2f9038860b0430ee225c3058023329eb4563f96d4fb7595
    /// tsc-span: _tsc.js:105178-105220
    fn transform_class_like_declaration_to_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let has_name = match &self.context.arena().node(node)?.data {
            NodeData::ClassDeclaration(data) => data.name.is_some(),
            NodeData::ClassExpression(data) => data.name.is_some(),
            _ => false,
        };
        if has_name {
            self.enable_substitutions_for_block_scoped_bindings()?;
        }
        let extends_clause_element = self.get_class_extends_heritage_element(node)?;
        let class_body = self.transform_class_body(node, extends_clause_element)?;
        let parameters = if extends_clause_element.is_some() {
            let synthetic_super = self.create_synthetic_super()?;
            vec![self.create_parameter_declaration(synthetic_super)?]
        } else {
            Vec::new()
        };
        let class_function = self.create_function_expression_full(
            /*asterisk*/ false, /*name*/ None, parameters, class_body,
        )?;
        let node_emit_flags = self.emit_flags(node);
        let indented = EmitFlags::from_bits(node_emit_flags.bits() & EmitFlags::INDENTED.bits());
        self.add_emit_flags(
            class_function,
            indented | EmitFlags::REUSE_TEMP_VARIABLE_SCOPE,
        )?;
        // inner PartiallyEmittedExpression: end = node.end
        let inner = self.create_partially_emitted_expression_positioned(
            class_function,
            node,
            PartiallyEmittedPosition::EndOfNode,
        )?;
        self.add_emit_flags(inner, EmitFlags::NO_COMMENTS)?;
        // outer PartiallyEmittedExpression: end = skipTrivia(currentText, node.pos)
        let outer = self.create_partially_emitted_expression_positioned(
            inner,
            node,
            PartiallyEmittedPosition::SkipTriviaOfPos,
        )?;
        self.add_emit_flags(outer, EmitFlags::NO_COMMENTS)?;
        let arguments = match extends_clause_element {
            Some(element) => {
                let expression = {
                    let NodeData::ExpressionWithTypeArguments(data) =
                        &self.context.arena().node(element)?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::ExpressionWithTypeArguments,
                            "heritage element",
                        ));
                    };
                    data.expression
                        .map(|id| self.node(id))
                        .ok_or(assembly_kind_error(
                            SyntaxKind::ExpressionWithTypeArguments,
                            "heritage expression",
                        ))?
                };
                vec![self.visit_required_expression(expression)?]
            }
            None => Vec::new(),
        };
        let call = self.create_call(outer, arguments)?;
        let result = self.create_paren(call)?;
        // addSyntheticLeadingComment(result, MultiLineCommentTrivia, "* @class ")
        self.context
            .arena_mut()?
            .metadata_mut(result)
            .add_leading_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                "* @class ".to_owned(),
                /*has_leading_new_line*/ false,
                /*has_trailing_new_line*/ false,
            ));
        Ok(result)
    }

    /// tsc-port: getClassExtendsHeritageElement @6.0.3
    /// tsc-hash: 7101b7d0f1e607daa5a4ec5b194f7d3cfe15c24c30ebbdbb41a845dceaae5c7d
    /// tsc-span: _tsc.js:15752-15755
    fn get_class_extends_heritage_element(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let heritage_clauses = match &self.context.arena().node(node)?.data {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => None,
        };
        for clause in self.array_nodes(heritage_clauses)? {
            let NodeData::HeritageClause(data) = &self.context.arena().node(clause)?.data else {
                continue;
            };
            if data.token == SyntaxKind::ExtendsKeyword {
                let types = self.array_nodes(data.types)?;
                return Ok(types.first().copied());
            }
        }
        Ok(None)
    }

    /// tsc-port: transformClassBody @6.0.3
    /// tsc-hash: 9850847f0d39924ad08a7fd966626be67f88fa17b9f4d723c340dace16e67454
    /// tsc-span: _tsc.js:105221-105249
    fn transform_class_body(
        &mut self,
        node: TransformNode,
        extends_clause_element: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut statements: Vec<TransformNode> = Vec::new();
        let name = self.get_internal_name(node)?;
        let constructor_like_name = if self.is_identifier_a_non_contextual_keyword(name)? {
            self.get_generated_name_for_node(name)?
        } else {
            name
        };
        self.context.start_lexical_environment()?;
        self.add_extends_helper_if_needed(&mut statements, node, extends_clause_element)?;
        self.add_constructor(
            &mut statements,
            node,
            constructor_like_name,
            extends_clause_element,
        )?;
        self.add_class_members(&mut statements, node)?;
        // closing-brace return statement, comment/token-map-suppressed and
        // ranged to the close-brace token range.
        let members_end = {
            let members = match &self.context.arena().node(node)?.data {
                NodeData::ClassDeclaration(data) => data.members,
                NodeData::ClassExpression(data) => data.members,
                _ => None,
            };
            match members {
                Some(members) => {
                    let array = self
                        .context
                        .arena()
                        .node_array(tsc_syntax_array(self.source, members))?;
                    array.end
                }
                None => self.context.arena().node(node)?.end,
            }
        };
        let close_brace_start = skip_trivia_bytes(&self.current_text, members_end);
        let outer = {
            let clone = self.clone_node(constructor_like_name)?;
            let created = {
                let source = self.source;
                let flags = self.child_flags(&[clone])?;
                self.context.factory()?.create_node(
                    source,
                    NodeData::PartiallyEmittedExpression(
                        tsc_syntax::nodes::PartiallyEmittedExpressionData {
                            expression: Some(clone.node()),
                        },
                    ),
                    flags,
                )?
            };
            // `createTokenRange(skipTrivia(...), CloseBraceToken)` — for a
            // synthesized members array the position is the -1 sentinel and
            // upstream computes end = -1 + 1 = 0; `wrapping_add` reproduces
            // that two's-complement arithmetic bit-for-bit (set_range_raw
            // drops the unrepresentable range either way).
            self.set_range_raw(
                created,
                close_brace_start,
                close_brace_start.wrapping_add(1),
            )?;
            created
        };
        self.add_emit_flags(outer, EmitFlags::NO_COMMENTS)?;
        let return_statement = self.create_return_statement(Some(outer))?;
        self.set_range_raw(
            return_statement,
            close_brace_start,
            close_brace_start.wrapping_add(1),
        )?;
        self.add_emit_flags(
            return_statement,
            EmitFlags::NO_COMMENTS | EmitFlags::NO_TOKEN_SOURCE_MAPS,
        )?;
        statements.push(return_statement);
        let environment = self.context.end_lexical_environment()?;
        self.insert_statements_after_standard_prologue_materialized(&mut statements, environment)?;
        let block = self.create_block(statements, /*multi_line*/ true)?;
        self.add_emit_flags(block, EmitFlags::NO_COMMENTS)?;
        Ok(block)
    }

    /// `insertStatementsAfterStandardPrologue(statements, endLexicalEnvironment())`
    /// — materialize then splice.
    fn insert_statements_after_standard_prologue_materialized(
        &mut self,
        statements: &mut Vec<TransformNode>,
        environment: LexicalEnvironment,
    ) -> Result<(), TransformError> {
        let declarations = self.materialize_lexical_environment(environment)?;
        self.insert_statements_after_standard_prologue(statements, &declarations)
    }

    /// tsc-port: isIdentifierANonContextualKeyword @6.0.3
    /// tsc-hash: b6d4aae387d7c92d3e2fcd53d07d177cad21e661e6e3e8a9fd838e457b1146cd
    /// tsc-span: _tsc.js:15806-15809
    fn is_identifier_a_non_contextual_keyword(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::Identifier(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        Ok(is_non_contextual_keyword_text(&data.escaped_text))
    }

    /// Raw byte-position range writer (the close-brace/`skipTrivia` ranges
    /// the class body pins; positions are source-byte offsets).
    fn set_range_raw(
        &mut self,
        node: TransformNode,
        pos: u32,
        end: u32,
    ) -> Result<(), TransformError> {
        let range = {
            let source = self.context.arena().source(self.source)?.syntax();
            SourceRange::from_raw(pos, end, source.positions())
        };
        if let Ok(range) = range {
            self.context
                .factory()?
                .set_text_range_from_source_range(node, self.source, range)?;
        }
        Ok(())
    }

    /// tsc-port: addExtendsHelperIfNeeded @6.0.3
    /// tsc-hash: 8fd26c32abea947587ce196fdb828e453bf78c605355140b8f434934bc4add42
    /// tsc-span: _tsc.js:105250-105262
    fn add_extends_helper_if_needed(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
        extends_clause_element: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let Some(element) = extends_clause_element else {
            return Ok(());
        };
        let name = self.get_internal_name(node)?;
        let helper_call = self.create_extends_helper_call(name)?;
        let statement = self.create_expression_statement(helper_call)?;
        self.set_text_range(statement, element)?;
        statements.push(statement);
        Ok(())
    }

    /// tsc-port: addConstructor @6.0.3
    /// tsc-hash: 223c0cfd1cd8d9322b25846e9f09b62c35213006b109ad85ea6c48b9033bfb27
    /// tsc-span: _tsc.js:105263-105289
    fn add_constructor(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
        name: TransformNode,
        extends_clause_element: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let saved_converted_loop_state = self.converted_loop_state.take();
        let ancestor = enter_subtree(
            &mut self.print_state.hierarchy_facts,
            HierarchyFacts::CONSTRUCTOR_EXCLUDES,
            HierarchyFacts::CONSTRUCTOR_INCLUDES,
        );
        let constructor = self.get_first_constructor_with_body(node)?;
        let has_synthesized_super =
            self.has_synthesized_default_super_call(constructor, extends_clause_element.is_some())?;
        let parameters =
            self.transform_constructor_parameters(constructor, has_synthesized_super)?;
        let body = self.transform_constructor_body(
            constructor,
            node,
            extends_clause_element,
            has_synthesized_super,
        )?;
        let constructor_function = {
            let source = self.source;
            let parameters_array = self
                .context
                .factory()?
                .create_node_array(source, parameters)?;
            let flags = self.context.arena().array_transform_flags(parameters_array)
                | self.child_flags(&[name, body])?
                | TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
            self.context.factory()?.create_node(
                source,
                NodeData::FunctionDeclaration(tsc_syntax::nodes::FunctionDeclarationData {
                    name: Some(name.node()),
                    type_parameters: None,
                    parameters: Some(parameters_array.array()),
                    r#type: None,
                    asterisk_token: None,
                    body: Some(body.node()),
                    modifiers: None,
                }),
                flags,
            )?
        };
        let range_source = constructor.unwrap_or(node);
        self.set_text_range(constructor_function, range_source)?;
        if extends_clause_element.is_some() {
            self.add_emit_flags(constructor_function, EmitFlags::CAPTURES_THIS)?;
        }
        statements.push(constructor_function);
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::FUNCTION_SUBTREE_EXCLUDES,
            HierarchyFacts::NONE,
        );
        self.converted_loop_state = saved_converted_loop_state;
        Ok(())
    }

    /// tsc-port: getFirstConstructorWithBody @6.0.3
    /// tsc-hash: 9a7337f235fb939299cfc0513bfd74f5a61039c196919e9dde7af622e2557370
    /// tsc-span: _tsc.js:16674-16676
    fn get_first_constructor_with_body(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let members = match &self.context.arena().node(node)?.data {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for member in self.array_nodes(members)? {
            if let NodeData::Constructor(data) = &self.context.arena().node(member)?.data {
                if data.body.is_some() {
                    return Ok(Some(member));
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: transformConstructorParameters @6.0.3
    /// tsc-hash: 2a339b376b75500624eecbdcadc2462084b3812ff3ddfc1c09625b3734314dca
    /// tsc-span: _tsc.js:105290-105292
    fn transform_constructor_parameters(
        &mut self,
        constructor: Option<TransformNode>,
        has_synthesized_super: bool,
    ) -> Result<Vec<TransformNode>, TransformError> {
        match constructor {
            Some(constructor) if !has_synthesized_super => {
                let parameters = self.function_parameters(constructor)?;
                self.visit_parameter_list(&parameters)
            }
            _ => {
                // `visitParameterList(undefined, ...)`: the environment
                // start/suspend still runs.
                self.context.start_lexical_environment()?;
                self.context.suspend_lexical_environment()?;
                Ok(Vec::new())
            }
        }
    }

    /// tsc-port: createDefaultConstructorBody @6.0.3
    /// tsc-hash: 184565cacd8247fee8f36a43308d1f2b8788a35b84a2b520ca592d0cf3bf8626
    /// tsc-span: _tsc.js:105293-105310
    fn create_default_constructor_body(
        &mut self,
        node: TransformNode,
        is_derived_class: bool,
    ) -> Result<TransformNode, TransformError> {
        let mut statements: Vec<TransformNode> = Vec::new();
        self.context.resume_lexical_environment()?;
        let environment = self.context.end_lexical_environment()?;
        self.merge_lexical_environment(&mut statements, environment)?;
        if is_derived_class {
            let super_call = self.create_default_super_call_or_this()?;
            statements.push(self.create_return_statement(Some(super_call))?);
        }
        let block = self.create_block(statements, /*multi_line*/ true)?;
        self.set_text_range(block, node)?;
        self.add_emit_flags(block, EmitFlags::NO_COMMENTS)?;
        Ok(block)
    }

    /// tsc-port: createDefaultSuperCallOrThis @6.0.3
    /// tsc-hash: 72158ce104853a17c9cbca2ecd902d90ed1bce7609c1d1f028106e494c2dc7d2
    /// tsc-span: _tsc.js:105656-105671
    fn create_default_super_call_or_this(&mut self) -> Result<TransformNode, TransformError> {
        let synthetic_super = self.create_synthetic_super()?;
        let null = self.create_null()?;
        let inequality = self.create_strict_inequality(synthetic_super, null)?;
        let apply_target = self.create_synthetic_super()?;
        let actual_this = self.create_this_no_substitution()?;
        let arguments_identifier = self.create_identifier("arguments")?;
        let apply =
            self.create_function_apply_call(apply_target, actual_this, arguments_identifier)?;
        let and = self.create_logical_and(inequality, apply)?;
        let fallback_this = self.create_this_no_substitution()?;
        self.create_logical_or(and, fallback_this)
    }

    /// tsc-port: isUninitializedVariableStatement @6.0.3
    /// tsc-hash: a6b59363464694f71b6e9d8a151f669545db7305f989027fc998cb662ca77cbb
    /// tsc-span: _tsc.js:105311-105313
    fn is_uninitialized_variable_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::VariableStatement(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(list) = data.declaration_list else {
            return Ok(false);
        };
        let NodeData::VariableDeclarationList(list_data) =
            &self.context.arena().node(self.node(list))?.data
        else {
            return Ok(false);
        };
        for declaration in self.array_nodes(list_data.declarations)? {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(declaration)?.data
            else {
                return Ok(false);
            };
            let name_is_identifier = match data.name {
                Some(name) => matches!(
                    self.context.arena().node(self.node(name))?.data,
                    NodeData::Identifier(_)
                ),
                None => false,
            };
            if !name_is_identifier || data.initializer.is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: containsSuperCall @6.0.3
    /// tsc-hash: b10611b4dfa5fff36d21c92b8a16a226fe46dab99869419e8955c2509a2054bc
    /// tsc-span: _tsc.js:105314-105342
    fn contains_super_call(&self, node: TransformNode) -> Result<bool, TransformError> {
        if self.is_super_call(node)? {
            return Ok(true);
        }
        if !self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_LEXICAL_SUPER)
        {
            return Ok(false);
        }
        let kind = self.kind(node)?;
        match kind {
            // stop at function boundaries
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::ClassStaticBlockDeclaration => return Ok(false),
            // only step into computed property names for class/object elements
            SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::PropertyDeclaration => {
                let name = match &self.context.arena().node(node)?.data {
                    NodeData::GetAccessor(data) => data.name,
                    NodeData::SetAccessor(data) => data.name,
                    NodeData::MethodDeclaration(data) => data.name,
                    NodeData::PropertyDeclaration(data) => data.name,
                    _ => None,
                };
                if let Some(name) = name {
                    let name = self.node(name);
                    if self.kind(name)? == SyntaxKind::ComputedPropertyName {
                        return self.for_each_child_contains_super_call(name);
                    }
                }
                return Ok(false);
            }
            _ => {}
        }
        self.for_each_child_contains_super_call(node)
    }

    fn for_each_child_contains_super_call(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let children = child_nodes_of(self.context.arena(), node)?;
        for child in children {
            if self.contains_super_call(child)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: transformConstructorBody @6.0.3
    /// tsc-hash: a6060ea9732eea48bca4d8e6f6fd30ad01335823010c924c6e4628b96bb40eb4
    /// tsc-span: _tsc.js:105343-105388
    fn transform_constructor_body(
        &mut self,
        constructor: Option<TransformNode>,
        node: TransformNode,
        extends_clause_element: Option<TransformNode>,
        has_synthesized_super: bool,
    ) -> Result<TransformNode, TransformError> {
        let is_derived_class = match extends_clause_element {
            Some(element) => {
                let expression = {
                    let NodeData::ExpressionWithTypeArguments(data) =
                        &self.context.arena().node(element)?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::ExpressionWithTypeArguments,
                            "heritage element",
                        ));
                    };
                    data.expression
                        .map(|id| self.node(id))
                        .ok_or(assembly_kind_error(
                            SyntaxKind::ExpressionWithTypeArguments,
                            "heritage expression",
                        ))?
                };
                self.kind(self.skip_outer_expressions(expression)?)? != SyntaxKind::NullKeyword
            }
            None => false,
        };
        let Some(constructor) = constructor else {
            return self.create_default_constructor_body(node, is_derived_class);
        };
        let mut prologue: Vec<TransformNode> = Vec::new();
        let mut statements: Vec<TransformNode> = Vec::new();
        self.context.resume_lexical_environment()?;
        let constructor_body = self.function_body(constructor)?.ok_or(assembly_kind_error(
            SyntaxKind::Constructor,
            "constructor body",
        ))?;
        let body_statements = {
            let NodeData::Block(data) = &self.context.arena().node(constructor_body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "constructor body"));
            };
            self.array_nodes(data.statements)?
        };
        let standard_prologue_end = self.copy_standard_prologue(
            &body_statements,
            &mut prologue,
            0,
            /*ensure_use_strict*/ false,
        )?;
        if has_synthesized_super || self.contains_super_call(constructor_body)? {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CONSTRUCTOR_WITH_SUPER_CALL);
        }
        self.visit_statements_into(&body_statements, standard_prologue_end, &mut statements)?;
        let may_replace_this = is_derived_class
            || self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::CONSTRUCTOR_WITH_SUPER_CALL);
        self.add_default_value_assignments_if_needed(&mut prologue, constructor)?;
        self.add_rest_parameter_if_needed(&mut prologue, constructor, has_synthesized_super)?;
        self.insert_capture_new_target_if_needed(&mut prologue, constructor)?;
        if may_replace_this {
            let actual_this = self.create_this_no_substitution()?;
            self.insert_capture_this_for_node(&mut prologue, constructor, actual_this)?;
        } else {
            self.insert_capture_this_for_node_if_needed(&mut prologue, constructor)?;
        }
        let environment = self.context.end_lexical_environment()?;
        self.merge_lexical_environment(&mut prologue, environment)?;
        if may_replace_this
            && !self.is_sufficiently_covered_by_return_statements(constructor_body)?
        {
            let captured = self.create_captured_this()?;
            statements.push(self.create_return_statement(Some(captured))?);
        }
        let mut combined = prologue;
        combined.append(&mut statements);
        let body = self.create_block(combined, /*multi_line*/ true)?;
        self.set_text_range(body, constructor_body)?;
        self.simplify_constructor(body, constructor_body, has_synthesized_super)
    }

    /// tsc-port: isSufficientlyCoveredByReturnStatements @6.0.3
    /// tsc-hash: c9017db587b06db7a1da6ff4950bd80c58ab0aacd66accb25115affeffe7c942
    /// tsc-span: _tsc.js:105637-105652
    fn is_sufficiently_covered_by_return_statements(
        &self,
        statement: TransformNode,
    ) -> Result<bool, TransformError> {
        match &self.context.arena().node(statement)?.data {
            NodeData::ReturnStatement(_) => Ok(true),
            NodeData::IfStatement(data) => {
                let Some(else_statement) = data.else_statement else {
                    return Ok(false);
                };
                let Some(then_statement) = data.then_statement else {
                    return Ok(false);
                };
                Ok(
                    self.is_sufficiently_covered_by_return_statements(self.node(then_statement))?
                        && self.is_sufficiently_covered_by_return_statements(
                            self.node(else_statement),
                        )?,
                )
            }
            NodeData::Block(data) => {
                let statements = self.array_nodes(data.statements)?;
                match statements.last() {
                    Some(last) => self.is_sufficiently_covered_by_return_statements(*last),
                    None => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }
}

/// The two synthesized-position flavors of the class-expression
/// `PartiallyEmittedExpression` wrap.
enum PartiallyEmittedPosition {
    EndOfNode,
    SkipTriviaOfPos,
}

impl Es2015Visitor<'_, '_, '_> {
    fn create_partially_emitted_expression_positioned(
        &mut self,
        expression: TransformNode,
        node: TransformNode,
        position: PartiallyEmittedPosition,
    ) -> Result<TransformNode, TransformError> {
        let created = {
            let source = self.source;
            let flags = self.child_flags(&[expression])?;
            self.context.factory()?.create_node(
                source,
                NodeData::PartiallyEmittedExpression(
                    tsc_syntax::nodes::PartiallyEmittedExpressionData {
                        expression: Some(expression.node()),
                    },
                ),
                flags,
            )?
        };
        let record = self.context.arena().node(node)?;
        let (node_pos, node_end) = (record.pos, record.end);
        match position {
            PartiallyEmittedPosition::EndOfNode => {
                // setTextRangeEnd(inner, node.end) — pos stays synthesized;
                // the printer consumes the END boundary.
                self.set_range_raw(created, node_end, node_end)?;
            }
            PartiallyEmittedPosition::SkipTriviaOfPos => {
                let skipped = skip_trivia_bytes(&self.current_text, node_pos);
                self.set_range_raw(created, skipped, skipped)?;
            }
        }
        Ok(created)
    }
}

/// Read-only child collection over a node record (the `forEachChild`
/// analog for the containsSuperCall walk).
fn child_nodes_of(
    arena: &crate::TransformArena,
    node: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let record = arena.node(node)?;
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut arrays: Vec<NodeArrayId> = Vec::new();
    tsc_syntax::observable_fields::for_each_observable_field(record, |_name, field| match field {
        tsc_syntax::observable_fields::ObservableField::Node(id) => nodes.push(id),
        tsc_syntax::observable_fields::ObservableField::NodeArray(id) => arrays.push(id),
        _ => {}
    });
    let mut out: Vec<TransformNode> = nodes
        .into_iter()
        .map(|id| TransformNode::new(node.source(), id))
        .collect();
    for array in arrays {
        let array = TransformNodeArray::new(node.source(), array);
        for id in &arena.node_array(array)?.nodes {
            out.push(TransformNode::new(node.source(), *id));
        }
    }
    Ok(out)
}

/// Non-contextual keyword texts (`isIdentifierANonContextualKeyword` over
/// the token maps: reserved words + strict-mode-reserved + future
/// reserved, EXCLUDING contextual keywords).
fn is_non_contextual_keyword_text(text: &str) -> bool {
    matches!(
        text,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
    )
}

// ---------------------------------------------------------------------------
// Constructor simplification pipeline + class members
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: isCapturedThis @6.0.3
    /// tsc-hash: 446a26fdc1a32915d56e5919798d2bf6cd2fd78b2a5f3a9c217c2f3270678e38
    /// tsc-span: _tsc.js:105389-105391
    fn is_captured_this(&self, node: TransformNode) -> Result<bool, TransformError> {
        let is_generated = self
            .context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_id().is_some());
        if !is_generated {
            return Ok(false);
        }
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Ok(data.text == "_this"),
            _ => Ok(false),
        }
    }

    /// tsc-port: isSyntheticSuper @6.0.3
    /// tsc-hash: 92959d1dd77f16b0e02b4b04f4b2b6790df457198d37b03354ee41f61e59369a
    /// tsc-span: _tsc.js:105392-105394
    fn is_synthetic_super(&self, node: TransformNode) -> Result<bool, TransformError> {
        let is_generated = self
            .context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_id().is_some());
        if !is_generated {
            return Ok(false);
        }
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Ok(data.text == "_super"),
            _ => Ok(false),
        }
    }

    /// tsc-port: isThisCapturingVariableStatement @6.0.3
    /// tsc-hash: 1926c34da63ad10eaf11b3a8b8e2d7285ae469849f1e23cbd916d36e0c85fe59
    /// tsc-span: _tsc.js:105395-105397
    fn is_this_capturing_variable_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::VariableStatement(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(list) = data.declaration_list else {
            return Ok(false);
        };
        let NodeData::VariableDeclarationList(list_data) =
            &self.context.arena().node(self.node(list))?.data
        else {
            return Ok(false);
        };
        let declarations = self.array_nodes(list_data.declarations)?;
        if declarations.len() != 1 {
            return Ok(false);
        }
        self.is_this_capturing_variable_declaration(declarations[0])
    }

    /// tsc-port: isThisCapturingVariableDeclaration @6.0.3
    /// tsc-hash: ecf7d7dd98a74760a84b2490f9e746a1d8b19dc1cb5c205f870f6d8be4a55f67
    /// tsc-span: _tsc.js:105398-105400
    fn is_this_capturing_variable_declaration(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let name_is_captured_this = match data.name {
            Some(name) => self.is_captured_this(self.node(name))?,
            None => false,
        };
        Ok(name_is_captured_this && data.initializer.is_some())
    }

    /// tsc-port: isThisCapturingAssignment @6.0.3
    /// tsc-hash: 8388b581c5b25521a0aae4814000b34640a9585f39a2e35323133c1cba8af4af
    /// tsc-span: _tsc.js:105401-105407
    fn is_this_capturing_assignment(&self, node: TransformNode) -> Result<bool, TransformError> {
        if !self.is_plain_assignment_expression(node)? {
            return Ok(false);
        }
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        match data.left {
            Some(left) => self.is_captured_this(self.node(left)),
            None => Ok(false),
        }
    }

    /// tsc-port: isTransformedSuperCall @6.0.3
    /// tsc-hash: 4adfc61b1ef07a1452ad1b953204442d7ee858deab597ce22d989dbdeb59ad9b
    /// tsc-span: _tsc.js:105408-105410
    fn is_transformed_super_call(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(callee) = data.expression else {
            return Ok(false);
        };
        let callee = self.node(callee);
        let NodeData::PropertyAccessExpression(access) = &self.context.arena().node(callee)?.data
        else {
            return Ok(false);
        };
        let receiver_is_synthetic_super = match access.expression {
            Some(receiver) => self.is_synthetic_super(self.node(receiver))?,
            None => false,
        };
        if !receiver_is_synthetic_super {
            return Ok(false);
        }
        let method_name = match access.name {
            Some(name) => match &self.context.arena().node(self.node(name))?.data {
                NodeData::Identifier(data) => data.text.clone(),
                _ => return Ok(false),
            },
            None => return Ok(false),
        };
        if method_name != "call" && method_name != "apply" {
            return Ok(false);
        }
        let arguments = self.array_nodes(data.arguments)?;
        match arguments.first() {
            Some(first) => Ok(self.kind(*first)? == SyntaxKind::ThisKeyword),
            None => Ok(false),
        }
    }

    /// tsc-port: isTransformedSuperCallWithFallback @6.0.3
    /// tsc-hash: 28e352d09edb6ccc4082e27215912f4301389e2c48b52f63a43e7f91ffef8e30
    /// tsc-span: _tsc.js:105411-105413
    fn is_transformed_super_call_with_fallback(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let operator_is_bar_bar = match data.operator_token {
            Some(operator) => self.kind(self.node(operator))? == SyntaxKind::BarBarToken,
            None => false,
        };
        if !operator_is_bar_bar {
            return Ok(false);
        }
        let right_is_this = match data.right {
            Some(right) => self.kind(self.node(right))? == SyntaxKind::ThisKeyword,
            None => false,
        };
        if !right_is_this {
            return Ok(false);
        }
        match data.left {
            Some(left) => self.is_transformed_super_call(self.node(left)),
            None => Ok(false),
        }
    }

    /// tsc-port: isImplicitSuperCall @6.0.3
    /// tsc-hash: 7165b5d34503c679d3cc1fa518c253aaf72bbbd71e66ca77291f7428f96097bf
    /// tsc-span: _tsc.js:105414-105416
    fn is_implicit_super_call(&self, node: TransformNode) -> Result<bool, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let operator_is_and = match data.operator_token {
            Some(operator) => {
                self.kind(self.node(operator))? == SyntaxKind::AmpersandAmpersandToken
            }
            None => false,
        };
        if !operator_is_and {
            return Ok(false);
        }
        let Some(left) = data.left else {
            return Ok(false);
        };
        let left = self.node(left);
        let NodeData::BinaryExpression(left_data) = &self.context.arena().node(left)?.data else {
            return Ok(false);
        };
        let left_op_is_neq = match left_data.operator_token {
            Some(operator) => {
                self.kind(self.node(operator))? == SyntaxKind::ExclamationEqualsEqualsToken
            }
            None => false,
        };
        if !left_op_is_neq {
            return Ok(false);
        }
        let left_left_is_super = match left_data.left {
            Some(id) => self.is_synthetic_super(self.node(id))?,
            None => false,
        };
        let left_right_is_null = match left_data.right {
            Some(id) => self.kind(self.node(id))? == SyntaxKind::NullKeyword,
            None => false,
        };
        if !left_left_is_super || !left_right_is_null {
            return Ok(false);
        }
        let Some(right) = data.right else {
            return Ok(false);
        };
        let right = self.node(right);
        if !self.is_transformed_super_call(right)? {
            return Ok(false);
        }
        // idText(node.right.expression.name) === "apply"
        let NodeData::CallExpression(call) = &self.context.arena().node(right)?.data else {
            return Ok(false);
        };
        let Some(callee) = call.expression else {
            return Ok(false);
        };
        let NodeData::PropertyAccessExpression(access) =
            &self.context.arena().node(self.node(callee))?.data
        else {
            return Ok(false);
        };
        match access.name {
            Some(name) => match &self.context.arena().node(self.node(name))?.data {
                NodeData::Identifier(data) => Ok(data.text == "apply"),
                _ => Ok(false),
            },
            None => Ok(false),
        }
    }

    /// tsc-port: isImplicitSuperCallWithFallback @6.0.3
    /// tsc-hash: 839350266d2cdb9cae06bbfa934bffcd26c4e23e1dd791e68ea4ab3015a74237
    /// tsc-span: _tsc.js:105417-105419
    fn is_implicit_super_call_with_fallback(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let operator_is_bar_bar = match data.operator_token {
            Some(operator) => self.kind(self.node(operator))? == SyntaxKind::BarBarToken,
            None => false,
        };
        if !operator_is_bar_bar {
            return Ok(false);
        }
        let right_is_this = match data.right {
            Some(right) => self.kind(self.node(right))? == SyntaxKind::ThisKeyword,
            None => false,
        };
        if !right_is_this {
            return Ok(false);
        }
        match data.left {
            Some(left) => self.is_implicit_super_call(self.node(left)),
            None => Ok(false),
        }
    }

    /// tsc-port: isThisCapturingTransformedSuperCallWithFallback @6.0.3
    /// tsc-hash: 30556f05d7ac691c850641ae7478c32c8352b76660804171630bd1c9b882e27b
    /// tsc-span: _tsc.js:105420-105422
    fn is_this_capturing_transformed_super_call_with_fallback(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if !self.is_this_capturing_assignment(node)? {
            return Ok(false);
        }
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        match data.right {
            Some(right) => self.is_transformed_super_call_with_fallback(self.node(right)),
            None => Ok(false),
        }
    }

    /// tsc-port: isThisCapturingImplicitSuperCallWithFallback @6.0.3
    /// tsc-hash: fb16cec1a46b2a3c7b6c5232b6a3c2c6e4f9245fcd824d5f00b6d16574b32f61
    /// tsc-span: _tsc.js:105423-105425
    fn is_this_capturing_implicit_super_call_with_fallback(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if !self.is_this_capturing_assignment(node)? {
            return Ok(false);
        }
        let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        match data.right {
            Some(right) => self.is_implicit_super_call_with_fallback(self.node(right)),
            None => Ok(false),
        }
    }

    /// tsc-port: isTransformedSuperCallLike @6.0.3
    /// tsc-hash: 38ce63e4fe9b06319096dadf49028f44784bf4f86c20be6903b50bb2c10a2743
    /// tsc-span: _tsc.js:105426-105428
    fn is_transformed_super_call_like(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(self.is_transformed_super_call(node)?
            || self.is_transformed_super_call_with_fallback(node)?
            || self.is_this_capturing_transformed_super_call_with_fallback(node)?
            || self.is_implicit_super_call(node)?
            || self.is_implicit_super_call_with_fallback(node)?
            || self.is_this_capturing_implicit_super_call_with_fallback(node)?)
    }

    /// tsc-port: simplifyConstructorInlineSuperInThisCaptureVariable @6.0.3
    /// tsc-hash: a415c9084e8e7e85649b875f311bb8b2c10e6b289d887898905d5d3822fc4cfc
    /// tsc-span: _tsc.js:105429-105485
    fn simplify_constructor_inline_super_in_this_capture_variable(
        &mut self,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "constructor body"));
            };
            self.array_nodes(data.statements)?
        };
        if statements.is_empty() {
            return Ok(body);
        }
        for index in 0..statements.len().saturating_sub(1) {
            let statement = statements[index];
            if !self.is_this_capturing_variable_statement(statement)? {
                continue;
            }
            let var_decl = self.single_declaration_of(statement)?;
            let initializer = {
                let NodeData::VariableDeclaration(data) =
                    &self.context.arena().node(var_decl)?.data
                else {
                    continue;
                };
                data.initializer.map(|id| self.node(id))
            };
            let initializer_is_this = match initializer {
                Some(initializer) => self.kind(initializer)? == SyntaxKind::ThisKeyword,
                None => false,
            };
            if !initializer_is_this {
                continue;
            }
            let this_capture_statement_index = index;
            let mut super_call_index = index + 1;
            loop {
                if super_call_index >= statements.len() {
                    return Ok(body);
                }
                let statement2 = statements[super_call_index];
                if matches!(
                    self.context.arena().node(statement2)?.data,
                    NodeData::ExpressionStatement(_)
                ) {
                    let expression = {
                        let NodeData::ExpressionStatement(data) =
                            &self.context.arena().node(statement2)?.data
                        else {
                            return Ok(body);
                        };
                        data.expression
                            .map(|id| self.node(id))
                            .ok_or(assembly_kind_error(
                                SyntaxKind::ExpressionStatement,
                                "expression",
                            ))?
                    };
                    let skipped = self.skip_outer_expressions(expression)?;
                    if self.is_transformed_super_call_like(skipped)? {
                        break;
                    }
                }
                if self.is_uninitialized_variable_statement(statement2)? {
                    super_call_index += 1;
                    continue;
                }
                return Ok(body);
            }
            let following = statements[super_call_index];
            let mut expression = {
                let NodeData::ExpressionStatement(data) =
                    &self.context.arena().node(following)?.data
                else {
                    return Ok(body);
                };
                data.expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::ExpressionStatement,
                        "expression",
                    ))?
            };
            if self.is_this_capturing_assignment(expression)? {
                let NodeData::BinaryExpression(data) = &self.context.arena().node(expression)?.data
                else {
                    return Ok(body);
                };
                expression = data
                    .right
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "assignment right",
                    ))?;
            }
            let var_name = {
                let NodeData::VariableDeclaration(data) =
                    &self.context.arena().node(var_decl)?.data
                else {
                    return Ok(body);
                };
                data.name
                    .ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?
            };
            let new_var_decl = {
                let data =
                    NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                        name: Some(var_name),
                        exclamation_token: None,
                        r#type: None,
                        initializer: Some(expression.node()),
                    });
                let flags = flags_after_update(self.context.arena(), var_decl, &data)?;
                self.context.factory()?.update_node(var_decl, data, flags)?
            };
            let old_list = self.declaration_list_of(statement)?;
            let new_decl_list = {
                let array = {
                    let source = self.source;
                    self.context
                        .factory()?
                        .create_node_array(source, vec![new_var_decl])?
                };
                let data = NodeData::VariableDeclarationList(
                    tsc_syntax::nodes::VariableDeclarationListData {
                        declarations: Some(array.array()),
                    },
                );
                let flags = flags_after_update(self.context.arena(), old_list, &data)?;
                self.context.factory()?.update_node(old_list, data, flags)?
            };
            let new_var_statement = self.create_variable_statement_from_list(new_decl_list)?;
            self.set_original(new_var_statement, following)?;
            self.set_text_range(new_var_statement, following)?;
            let mut new_statements: Vec<TransformNode> = Vec::new();
            new_statements.extend_from_slice(&statements[..this_capture_statement_index]);
            new_statements
                .extend_from_slice(&statements[this_capture_statement_index + 1..super_call_index]);
            new_statements.push(new_var_statement);
            new_statements.extend_from_slice(&statements[super_call_index + 1..]);
            return self.update_block_statements(body, new_statements);
        }
        Ok(body)
    }

    fn single_declaration_of(
        &self,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let list = self.declaration_list_of(statement)?;
        let NodeData::VariableDeclarationList(data) = &self.context.arena().node(list)?.data else {
            return Err(assembly_kind_error(
                SyntaxKind::VariableDeclarationList,
                "declaration list",
            ));
        };
        self.array_nodes(data.declarations)?
            .first()
            .copied()
            .ok_or(assembly_kind_error(
                SyntaxKind::VariableDeclaration,
                "single declaration",
            ))
    }

    fn declaration_list_of(
        &self,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::VariableStatement(data) = &self.context.arena().node(statement)?.data else {
            return Err(assembly_kind_error(
                SyntaxKind::VariableStatement,
                "variable statement",
            ));
        };
        data.declaration_list
            .map(|id| self.node(id))
            .ok_or(assembly_kind_error(
                SyntaxKind::VariableStatement,
                "declaration list",
            ))
    }

    fn update_block_statements(
        &mut self,
        block: TransformNode,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let array = {
            let source = self.source;
            self.context
                .factory()?
                .create_node_array(source, statements)?
        };
        let data = NodeData::Block(tsc_syntax::nodes::BlockData {
            statements: Some(array.array()),
        });
        if self.context.arena().node(block)?.data == data {
            return Ok(block);
        }
        let flags = flags_after_update(self.context.arena(), block, &data)?;
        self.context.factory()?.update_node(block, data, flags)
    }

    /// tsc-port: simplifyConstructorInlineSuperReturn @6.0.3
    /// tsc-hash: 38195570d984762a2d12b39d3f6f6d6651f2326dbb2c0885af51b93a1d8816aa
    /// tsc-span: _tsc.js:105486-105527
    fn simplify_constructor_inline_super_return(
        &mut self,
        body: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original_statements = {
            let NodeData::Block(data) = &self.context.arena().node(original)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "original body"));
            };
            self.array_nodes(data.statements)?
        };
        for statement in &original_statements {
            if self
                .transform_flags(*statement)
                .contains(TransformFlags::CONTAINS_LEXICAL_SUPER)
                && self.get_super_call_from_statement(*statement)?.is_none()
            {
                return Ok(body);
            }
        }
        let can_elide_this_capturing_variable = !self
            .transform_flags(original)
            .contains(TransformFlags::CONTAINS_LEXICAL_THIS)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::LEXICAL_THIS)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "constructor body"));
            };
            self.array_nodes(data.statements)?
        };
        for index in (1..statements.len()).rev() {
            let statement = statements[index];
            let NodeData::ReturnStatement(data) = &self.context.arena().node(statement)?.data
            else {
                continue;
            };
            let Some(return_expression) = data.expression else {
                continue;
            };
            if !self.is_captured_this(self.node(return_expression))? {
                continue;
            }
            let preceding = statements[index - 1];
            let mut expression: Option<TransformNode> = None;
            if matches!(
                self.context.arena().node(preceding)?.data,
                NodeData::ExpressionStatement(_)
            ) {
                let preceding_expression = {
                    let NodeData::ExpressionStatement(data) =
                        &self.context.arena().node(preceding)?.data
                    else {
                        continue;
                    };
                    data.expression
                        .map(|id| self.node(id))
                        .ok_or(assembly_kind_error(
                            SyntaxKind::ExpressionStatement,
                            "expression",
                        ))?
                };
                let skipped = self.skip_outer_expressions(preceding_expression)?;
                if self.is_this_capturing_transformed_super_call_with_fallback(skipped)? {
                    expression = Some(preceding_expression);
                }
            } else if can_elide_this_capturing_variable
                && self.is_this_capturing_variable_statement(preceding)?
            {
                let var_decl = self.single_declaration_of(preceding)?;
                let initializer = {
                    let NodeData::VariableDeclaration(data) =
                        &self.context.arena().node(var_decl)?.data
                    else {
                        continue;
                    };
                    data.initializer.map(|id| self.node(id))
                };
                if let Some(initializer) = initializer {
                    let skipped = self.skip_outer_expressions(initializer)?;
                    if self.is_transformed_super_call_like(skipped)? {
                        let captured = self.create_captured_this()?;
                        expression = Some(self.create_assignment(captured, initializer)?);
                    }
                }
            }
            let Some(expression) = expression else {
                break;
            };
            let new_return_statement = self.create_return_statement(Some(expression))?;
            self.set_original(new_return_statement, preceding)?;
            self.set_text_range(new_return_statement, preceding)?;
            let mut new_statements: Vec<TransformNode> = Vec::new();
            new_statements.extend_from_slice(&statements[..index - 1]);
            new_statements.push(new_return_statement);
            new_statements.extend_from_slice(&statements[index + 1..]);
            return self.update_block_statements(body, new_statements);
        }
        Ok(body)
    }

    /// tsc-port: elideUnusedThisCaptureWorker @6.0.3
    /// tsc-hash: 5e307dab74d11def5cc21f2825b3ec361acac6fea26c4a78eb9d5ed2a130f985
    /// tsc-span: _tsc.js:105528-105568
    fn elide_unused_this_capture_worker(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if self.is_this_capturing_variable_statement(node)? {
            let var_decl = self.single_declaration_of(node)?;
            let initializer_is_this = {
                let NodeData::VariableDeclaration(data) =
                    &self.context.arena().node(var_decl)?.data
                else {
                    return Ok(Some(node));
                };
                match data.initializer {
                    Some(initializer) => {
                        self.kind(self.node(initializer))? == SyntaxKind::ThisKeyword
                    }
                    None => false,
                }
            };
            if initializer_is_this {
                return Ok(None);
            }
        } else if self.is_this_capturing_assignment(node)? {
            let right = {
                let NodeData::BinaryExpression(data) = &self.context.arena().node(node)?.data
                else {
                    return Ok(Some(node));
                };
                data.right
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::BinaryExpression,
                        "assignment right",
                    ))?
            };
            return Ok(Some(self.create_partially_emitted_expression(right, node)?));
        }
        match self.kind(node)? {
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::ClassStaticBlockDeclaration => return Ok(Some(node)),
            SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::PropertyDeclaration => {
                // computed names step in; other member shapes stop.
                return Ok(Some(node));
            }
            _ => {}
        }
        self.visit_each_child_with(node, &mut |visitor, child| {
            visitor.elide_unused_this_capture_worker(child)
        })
    }

    /// tsc-port: simplifyConstructorElideUnusedThisCapture @6.0.3
    /// tsc-hash: ff8939432c9380ee9e1975ff915e7894e04f4ba09d55ad225ea5f06f08c613a6
    /// tsc-span: _tsc.js:105569-105579
    fn simplify_constructor_elide_unused_this_capture(
        &mut self,
        body: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self
            .transform_flags(original)
            .contains(TransformFlags::CONTAINS_LEXICAL_THIS)
            || self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::LEXICAL_THIS)
            || self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::CAPTURED_LEXICAL_THIS)
        {
            return Ok(body);
        }
        let original_statements = {
            let NodeData::Block(data) = &self.context.arena().node(original)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "original body"));
            };
            self.array_nodes(data.statements)?
        };
        for statement in &original_statements {
            if self
                .transform_flags(*statement)
                .contains(TransformFlags::CONTAINS_LEXICAL_SUPER)
                && self.get_super_call_from_statement(*statement)?.is_none()
            {
                return Ok(body);
            }
        }
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "constructor body"));
            };
            self.array_nodes(data.statements)?
        };
        let mut new_statements: Vec<TransformNode> = Vec::new();
        for statement in &statements {
            if let Some(node) = self.elide_unused_this_capture_worker(*statement)? {
                new_statements.push(node);
            }
        }
        self.update_block_statements(body, new_statements)
    }

    /// tsc-port: injectSuperPresenceCheckWorker @6.0.3
    /// tsc-hash: 1849d41cad4b43c942701794958dd9c116035005c5ca68b1ef6178102e793ab4
    /// tsc-span: _tsc.js:105580-105621
    fn inject_super_presence_check_worker(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if self.is_transformed_super_call(node)? {
            let arguments = {
                let NodeData::CallExpression(data) = &self.context.arena().node(node)?.data else {
                    return Ok(Some(node));
                };
                self.array_nodes(data.arguments)?
            };
            let second_is_arguments = arguments.len() == 2
                && match &self.context.arena().node(arguments[1])?.data {
                    NodeData::Identifier(data) => data.text == "arguments",
                    _ => false,
                };
            if second_is_arguments {
                let synthetic_super = self.create_synthetic_super()?;
                let null = self.create_null()?;
                let inequality = self.create_strict_inequality(synthetic_super, null)?;
                return Ok(Some(self.create_logical_and(inequality, node)?));
            }
        }
        match self.kind(node)? {
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::ClassStaticBlockDeclaration => return Ok(Some(node)),
            SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::PropertyDeclaration => return Ok(Some(node)),
            _ => {}
        }
        self.visit_each_child_with(node, &mut |visitor, child| {
            visitor.inject_super_presence_check_worker(child)
        })
    }

    /// tsc-port: complicateConstructorInjectSuperPresenceCheck @6.0.3
    /// tsc-hash: 82da36a56a8e525f23cbc73c6461c8eed9db102570bdb2b9e02587c66e2e0bee
    /// tsc-span: _tsc.js:105622-105624
    fn complicate_constructor_inject_super_presence_check(
        &mut self,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "constructor body"));
            };
            self.array_nodes(data.statements)?
        };
        let mut new_statements: Vec<TransformNode> = Vec::new();
        for statement in &statements {
            if let Some(node) = self.inject_super_presence_check_worker(*statement)? {
                new_statements.push(node);
            }
        }
        self.update_block_statements(body, new_statements)
    }

    /// tsc-port: simplifyConstructor @6.0.3
    /// tsc-hash: 1d6ecfb11a44aa571b4b00f6fe11fefcbc63a00fa817fa583120ef4405f8083e
    /// tsc-span: _tsc.js:105625-105636
    fn simplify_constructor(
        &mut self,
        body: TransformNode,
        original: TransformNode,
        has_synthesized_super: bool,
    ) -> Result<TransformNode, TransformError> {
        let input_body = body;
        let mut body = self.simplify_constructor_inline_super_in_this_capture_variable(body)?;
        body = self.simplify_constructor_inline_super_return(body, original)?;
        if body != input_body {
            body = self.simplify_constructor_elide_unused_this_capture(body, original)?;
        }
        if has_synthesized_super {
            body = self.complicate_constructor_inject_super_presence_check(body)?;
        }
        Ok(body)
    }

    /// The generic single-child rewriting walk the two simplify workers
    /// use (`visitEachChild(node, worker, /*context*/ undefined)` — a
    /// context-free structural rewrite).
    #[allow(clippy::type_complexity)]
    fn visit_each_child_with(
        &mut self,
        node: TransformNode,
        worker: &mut dyn FnMut(
            &mut Self,
            TransformNode,
        ) -> Result<Option<TransformNode>, TransformError>,
    ) -> Result<Option<TransformNode>, TransformError> {
        let mut data = self.context.arena().node(node)?.data.clone();
        let mut rewriter = SimplifyRewriter {
            visitor: self,
            worker,
            error: None,
        };
        try_visit_each_child(&mut data, &mut rewriter)?;
        if let Some(error) = rewriter.error {
            return Err(error);
        }
        if self.context.arena().node(node)?.data == data {
            return Ok(Some(node));
        }
        let flags = flags_after_update(self.context.arena(), node, &data)?;
        Ok(Some(
            self.context.factory()?.update_node(node, data, flags)?,
        ))
    }

    /// tsc-port: hasSynthesizedDefaultSuperCall @6.0.3
    /// tsc-hash: 3759ff43da6111de19fca9fcccff9beb9b9fd63d75954b1e950cce4e58deaaa7
    /// tsc-span: _tsc.js:108074-108099
    fn has_synthesized_default_super_call(
        &self,
        constructor: Option<TransformNode>,
        has_extends_clause: bool,
    ) -> Result<bool, TransformError> {
        let Some(constructor) = constructor else {
            return Ok(false);
        };
        if !has_extends_clause {
            return Ok(false);
        }
        if !self.function_parameters(constructor)?.is_empty() {
            return Ok(false);
        }
        let body = self.function_body(constructor)?;
        let Some(body) = body else { return Ok(false) };
        let statements = {
            let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                return Ok(false);
            };
            self.array_nodes(data.statements)?
        };
        let Some(statement) = statements.first().copied() else {
            return Ok(false);
        };
        if !self.node_is_synthesized(statement)?
            || !matches!(
                self.context.arena().node(statement)?.data,
                NodeData::ExpressionStatement(_)
            )
        {
            return Ok(false);
        }
        let statement_expression = {
            let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
            else {
                return Ok(false);
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(
                    SyntaxKind::ExpressionStatement,
                    "expression",
                ))?
        };
        if !self.node_is_synthesized(statement_expression)?
            || !matches!(
                self.context.arena().node(statement_expression)?.data,
                NodeData::CallExpression(_)
            )
        {
            return Ok(false);
        }
        let (call_target, arguments) = {
            let NodeData::CallExpression(data) =
                &self.context.arena().node(statement_expression)?.data
            else {
                return Ok(false);
            };
            (
                data.expression.map(|id| self.node(id)),
                self.array_nodes(data.arguments)?,
            )
        };
        let Some(call_target) = call_target else {
            return Ok(false);
        };
        if !self.node_is_synthesized(call_target)?
            || self.kind(call_target)? != SyntaxKind::SuperKeyword
        {
            return Ok(false);
        }
        if arguments.len() != 1 {
            return Ok(false);
        }
        let call_argument = arguments[0];
        if !self.node_is_synthesized(call_argument)?
            || self.kind(call_argument)? != SyntaxKind::SpreadElement
        {
            return Ok(false);
        }
        let spread_expression = {
            let NodeData::SpreadElement(data) = &self.context.arena().node(call_argument)?.data
            else {
                return Ok(false);
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::SpreadElement, "expression"))?
        };
        match &self.context.arena().node(spread_expression)?.data {
            NodeData::Identifier(data) => Ok(data.escaped_text == "arguments"),
            _ => Ok(false),
        }
    }

    /// tsc-port: addClassMembers @6.0.3
    /// tsc-hash: 2c33fd973cf6cbe9089b25235ce4fafb3bf13741dc3a2a97bfd39b7a617b4374
    /// tsc-span: _tsc.js:106006-106030
    fn add_class_members(
        &mut self,
        statements: &mut Vec<TransformNode>,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let members = {
            let members = match &self.context.arena().node(node)?.data {
                NodeData::ClassDeclaration(data) => data.members,
                NodeData::ClassExpression(data) => data.members,
                _ => None,
            };
            self.array_nodes(members)?
        };
        for member in &members {
            match self.kind(*member)? {
                SyntaxKind::SemicolonClassElement => {
                    statements.push(self.transform_semicolon_class_element_to_statement(*member)?);
                }
                SyntaxKind::MethodDeclaration => {
                    let receiver = self.get_class_member_prefix(node, *member)?;
                    statements.push(self.transform_class_method_declaration_to_statement(
                        receiver, *member, node,
                    )?);
                }
                SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                    let accessors = self.get_all_accessor_declarations(&members, *member)?;
                    if *member == accessors.first_accessor {
                        let receiver = self.get_class_member_prefix(node, *member)?;
                        statements.push(
                            self.transform_accessors_to_statement(receiver, &accessors, node)?,
                        );
                    }
                }
                SyntaxKind::Constructor | SyntaxKind::ClassStaticBlockDeclaration => {}
                other => {
                    return Err(assembly_kind_error(
                        other,
                        "addClassMembers (Debug.failBadSyntaxKind)",
                    ));
                }
            }
        }
        Ok(())
    }

    /// tsc-port: transformSemicolonClassElementToStatement @6.0.3
    /// tsc-hash: cda54eaa61e7acee6423576d92b8c77586ce02f2dcf4dcff25a4f8d3338c800e
    /// tsc-span: _tsc.js:106031-106033
    fn transform_semicolon_class_element_to_statement(
        &mut self,
        member: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let statement = self.create_empty_statement()?;
        self.set_text_range(statement, member)?;
        Ok(statement)
    }

    /// tsc-port: transformClassMethodDeclarationToStatement @6.0.3
    /// tsc-hash: 5180ce812b5ee59c2bc0a4366caf06bb31d0a563d5dd6469e6a5bc76ee21e1d7
    /// tsc-span: _tsc.js:106034-106072
    fn transform_class_method_declaration_to_statement(
        &mut self,
        receiver: TransformNode,
        member: TransformNode,
        container: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let member_function = self.transform_function_like_to_expression(
            member,
            /*location*/ Some(member),
            /*name*/ None,
            Some(container),
        )?;
        let member_name = {
            let name = match &self.context.arena().node(member)?.data {
                NodeData::MethodDeclaration(data) => data.name,
                _ => None,
            };
            name.map(|id| self.node(id)).ok_or(assembly_kind_error(
                SyntaxKind::MethodDeclaration,
                "method name",
            ))?
        };
        let property_name = self.visit_required_expression(member_name)?;
        let name_is_private = matches!(
            self.context.arena().node(property_name)?.data,
            NodeData::PrivateIdentifier(_)
        );
        let e: TransformNode = if !name_is_private && self.use_define_for_class_fields {
            let name = match &self.context.arena().node(property_name)?.data {
                NodeData::ComputedPropertyName(data) => data
                    .expression
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::ComputedPropertyName,
                        "expression",
                    ))?,
                NodeData::Identifier(data) => {
                    let text = unescape_leading_underscores(&data.escaped_text).to_owned();
                    self.create_string_literal(&text)?
                }
                _ => property_name,
            };
            let descriptor = self.create_property_descriptor_for_value(member_function)?;
            self.create_object_define_property_call(receiver, name, descriptor)?
        } else {
            let member_access = self.create_member_access_for_property_name(
                receiver,
                property_name,
                Some(member_name),
            )?;
            self.create_assignment(member_access, member_function)?
        };
        self.add_emit_flags(member_function, EmitFlags::NO_COMMENTS)?;
        self.set_source_map_range_from(member_function, member)?;
        let statement = self.create_expression_statement(e)?;
        self.set_text_range(statement, member)?;
        self.set_original(statement, member)?;
        self.set_comment_range_from(statement, member)?;
        self.add_emit_flags(statement, EmitFlags::NO_SOURCE_MAP)?;
        Ok(statement)
    }

    /// `createPropertyDescriptor({ value, enumerable: false, writable: true,
    /// configurable: true })` (`_tsc.js:24614-24624` over
    /// `tryAddPropertyAssignment` :24607-24613; single-line = false).
    fn create_property_descriptor_for_value(
        &mut self,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let enumerable_value = self.create_false()?;
        let enumerable = self.create_property_assignment_text("enumerable", enumerable_value)?;
        let configurable_value = self.create_true()?;
        let configurable =
            self.create_property_assignment_text("configurable", configurable_value)?;
        let writable_value = self.create_true()?;
        let writable = self.create_property_assignment_text("writable", writable_value)?;
        let value_assignment = self.create_property_assignment_text("value", value)?;
        self.create_object_literal(
            vec![enumerable, configurable, writable, value_assignment],
            /*multi_line*/ true,
        )
    }

    /// tsc-port: createObjectDefinePropertyCall @6.0.3
    /// tsc-hash: 82dbc40a8f28d6f589084723a9c4a47b5e6288b2516946eb49b56fbf27d03f38
    /// tsc-span: _tsc.js:24595-24597
    fn create_object_define_property_call(
        &mut self,
        target: TransformNode,
        property_name: TransformNode,
        attributes: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let object_identifier = self.create_identifier("Object")?;
        let define_property =
            self.create_property_access_text(object_identifier, "defineProperty")?;
        self.create_call(define_property, vec![target, property_name, attributes])
    }

    /// tsc-port: transformAccessorsToStatement @6.0.3
    /// tsc-hash: 9fc3590ab0768f12cd93f556a2838fc0a2254e765f2b882d4397cbbd13e536c6
    /// tsc-span: _tsc.js:106073-106084
    fn transform_accessors_to_statement(
        &mut self,
        receiver: TransformNode,
        accessors: &AllAccessorDeclarations,
        container: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let expression = self.transform_accessors_to_expression(
            receiver, accessors, container, /*starts_on_new_line*/ false,
        )?;
        let statement = self.create_expression_statement(expression)?;
        self.add_emit_flags(statement, EmitFlags::NO_COMMENTS)?;
        self.set_source_map_range_from(statement, accessors.first_accessor)?;
        Ok(statement)
    }

    /// tsc-port: getClassMemberPrefix @6.0.3
    /// tsc-hash: cd87f6646a467f778a7b7900a34066d49198e9e5823a370c845f780b71cc689a
    /// tsc-span: _tsc.js:108071-108073
    fn get_class_member_prefix(
        &mut self,
        node: TransformNode,
        member: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let internal_name = self.get_internal_name(node)?;
        if self.has_static_modifier(member)? {
            Ok(internal_name)
        } else {
            self.create_property_access_text(internal_name, "prototype")
        }
    }
}

/// The four for-statement part handles.
type ForStatementParts = (
    Option<TransformNode>,
    Option<TransformNode>,
    Option<TransformNode>,
    Option<TransformNode>,
);

/// Adapter routing `try_visit_each_child` through a simplify worker.
struct SimplifyRewriter<'a, 'b, 'context, 'resolver, 'state> {
    visitor: &'a mut Es2015Visitor<'context, 'resolver, 'state>,
    #[allow(clippy::type_complexity)]
    worker: &'b mut dyn FnMut(
        &mut Es2015Visitor<'context, 'resolver, 'state>,
        TransformNode,
    ) -> Result<Option<TransformNode>, TransformError>,
    error: Option<TransformError>,
}

impl NodeDataChildVisitor for SimplifyRewriter<'_, '_, '_, '_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.visitor
            .kind(self.visitor.node(id))
            .unwrap_or(SyntaxKind::Unknown)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        let node = self.visitor.node(id);
        Ok((self.worker)(self.visitor, node)?.map(|node| node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let original = tsc_syntax_array(self.visitor.source, id);
        let nodes = self
            .visitor
            .context
            .arena()
            .node_array(original)?
            .nodes
            .clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            let node = self.visitor.node(node);
            if let Some(node) = (self.worker)(self.visitor, node)? {
                visited.push(node);
            }
        }
        let updated = self
            .visitor
            .context
            .factory()?
            .update_node_array(original, visited)?;
        Ok(Some(updated.array()))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

// ---------------------------------------------------------------------------
// Loop conversion (the yield-star-synthesis producer)
// ---------------------------------------------------------------------------

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: shouldConvertPartOfIterationStatement @6.0.3
    /// tsc-hash: ae1d4df2b0e2feee943c00bc03955266d0c0c24cb4b6781b4977befa81da8874
    /// tsc-span: _tsc.js:106900-106902
    fn should_convert_part_of_iteration_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(reference) = self.context.arena().parse_tree_resolver_node(node)? else {
            return Ok(false);
        };
        self.resolver
            .has_node_check_flag(
                reference,
                NodeCheckFlags::CONTAINS_CAPTURED_BLOCK_SCOPE_BINDING.bits() as u32,
            )
            .map_err(TransformError::from)
    }

    fn for_statement_parts(
        &self,
        node: TransformNode,
    ) -> Result<ForStatementParts, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::ForStatement(data) => Ok((
                data.initializer.map(|id| self.node(id)),
                data.condition.map(|id| self.node(id)),
                data.incrementor.map(|id| self.node(id)),
                data.statement.map(|id| self.node(id)),
            )),
            _ => Ok((None, None, None, None)),
        }
    }

    /// tsc-port: shouldConvertInitializerOfForStatement @6.0.3
    /// tsc-hash: 34f7fb1bc69073490b78befae72bc14ff5d1d1d87a6794f3322f57e17bd9893e
    /// tsc-span: _tsc.js:106903-106905
    fn should_convert_initializer_of_for_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if self.kind(node)? != SyntaxKind::ForStatement {
            return Ok(false);
        }
        let (initializer, _, _, _) = self.for_statement_parts(node)?;
        match initializer {
            Some(initializer) => self.should_convert_part_of_iteration_statement(initializer),
            None => Ok(false),
        }
    }

    /// tsc-port: shouldConvertConditionOfForStatement @6.0.3
    /// tsc-hash: 1d5d8c978af2409b66085555b18223a0245a30e5141ee284962f61c3afc34aec
    /// tsc-span: _tsc.js:106906-106908
    fn should_convert_condition_of_for_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if self.kind(node)? != SyntaxKind::ForStatement {
            return Ok(false);
        }
        let (_, condition, _, _) = self.for_statement_parts(node)?;
        match condition {
            Some(condition) => self.should_convert_part_of_iteration_statement(condition),
            None => Ok(false),
        }
    }

    /// tsc-port: shouldConvertIncrementorOfForStatement @6.0.3
    /// tsc-hash: 4ac4fd109916bbeb753cd35a801b50a546c37e3b4df1fe97471fb30e375b4bff
    /// tsc-span: _tsc.js:106909-106911
    fn should_convert_incrementor_of_for_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if self.kind(node)? != SyntaxKind::ForStatement {
            return Ok(false);
        }
        let (_, _, incrementor, _) = self.for_statement_parts(node)?;
        match incrementor {
            Some(incrementor) => self.should_convert_part_of_iteration_statement(incrementor),
            None => Ok(false),
        }
    }

    /// tsc-port: shouldConvertIterationStatement @6.0.3
    /// tsc-hash: 09fd782f527c4038cc63193bded9b82ac9ca5c995517728c83c99318d2a6c91a
    /// tsc-span: _tsc.js:106912-106914
    fn should_convert_iteration_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(self.should_convert_body_of_iteration_statement(node)?
            || self.should_convert_initializer_of_for_statement(node)?)
    }

    /// tsc-port: shouldConvertBodyOfIterationStatement @6.0.3
    /// tsc-hash: 6339bb663ae636e10c16f85310a24951d206da87d1bf1506209ee28eb8251073
    /// tsc-span: _tsc.js:106915-106917
    fn should_convert_body_of_iteration_statement(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(reference) = self.context.arena().parse_tree_resolver_node(node)? else {
            return Ok(false);
        };
        self.resolver
            .has_node_check_flag(
                reference,
                NodeCheckFlags::LOOP_WITH_CAPTURED_BLOCK_SCOPED_BINDING.bits() as u32,
            )
            .map_err(TransformError::from)
    }

    /// tsc-port: hoistVariableDeclarationDeclaredInConvertedLoop @6.0.3
    /// tsc-hash: 63349b45803aa21b02603a402e922910139de7bbe9857b9a0ed53f64edd1b506
    /// tsc-span: _tsc.js:106918-106934
    fn hoist_variable_declaration_declared_in_converted_loop(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let name = {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(
                    SyntaxKind::VariableDeclaration,
                    "declaration",
                ));
            };
            data.name
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?
        };
        self.hoist_converted_loop_binding_name(name)
    }

    fn hoist_converted_loop_binding_name(
        &mut self,
        name: TransformNode,
    ) -> Result<(), TransformError> {
        match &self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(_) => {
                self.loop_state_mut().hoisted_local_variables.push(name);
                Ok(())
            }
            NodeData::ObjectBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if self.kind(element)? != SyntaxKind::OmittedExpression {
                        let element_name = {
                            let NodeData::BindingElement(element_data) =
                                &self.context.arena().node(element)?.data
                            else {
                                continue;
                            };
                            element_data.name.map(|id| self.node(id))
                        };
                        if let Some(element_name) = element_name {
                            self.hoist_converted_loop_binding_name(element_name)?;
                        }
                    }
                }
                Ok(())
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if self.kind(element)? != SyntaxKind::OmittedExpression {
                        let element_name = {
                            let NodeData::BindingElement(element_data) =
                                &self.context.arena().node(element)?.data
                            else {
                                continue;
                            };
                            element_data.name.map(|id| self.node(id))
                        };
                        if let Some(element_name) = element_name {
                            self.hoist_converted_loop_binding_name(element_name)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// tsc-port: visitIterationStatement @6.0.3
    /// tsc-hash: 422645a8d8919dd57a850ad214ad5a905a80156245ff7d3663d71e5d9269994c
    /// tsc-span: _tsc.js:106513-106525
    fn visit_iteration_statement(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
    ) -> Result<VisitOutcome, TransformError> {
        match self.kind(node)? {
            SyntaxKind::DoStatement | SyntaxKind::WhileStatement => {
                self.visit_do_or_while_statement(node, outermost_labeled_statement)
            }
            SyntaxKind::ForStatement => self.visit_for_statement(node, outermost_labeled_statement),
            SyntaxKind::ForInStatement => {
                self.visit_for_in_statement(node, outermost_labeled_statement)
            }
            SyntaxKind::ForOfStatement => {
                self.visit_for_of_statement(node, outermost_labeled_statement)
            }
            other => Err(assembly_kind_error(other, "visitIterationStatement")),
        }
    }

    /// tsc-port: visitIterationStatementWithFacts @6.0.3
    /// tsc-hash: 5f0815db9d20f7a9bbdf228511f13008cf6c9026941386bf36fe76bfae8996d1
    /// tsc-span: _tsc.js:106526-106531
    fn visit_iteration_statement_with_facts(
        &mut self,
        exclude: HierarchyFacts,
        include: HierarchyFacts,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        convert: Option<LoopConverter>,
    ) -> Result<VisitOutcome, TransformError> {
        let ancestor = enter_subtree(&mut self.print_state.hierarchy_facts, exclude, include);
        let updated = self.convert_iteration_statement_body_if_necessary(
            node,
            outermost_labeled_statement,
            ancestor,
            convert,
        )?;
        exit_subtree(
            &mut self.print_state.hierarchy_facts,
            ancestor,
            HierarchyFacts::NONE,
            HierarchyFacts::NONE,
        );
        Ok(updated)
    }

    /// tsc-port: visitDoOrWhileStatement @6.0.3
    /// tsc-hash: 736b53bebd94da898f5c5a9412b5b8051155812d1c03af7a9ccfbe371e0981e3
    /// tsc-span: _tsc.js:106532-106539
    fn visit_do_or_while_statement(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
    ) -> Result<VisitOutcome, TransformError> {
        self.visit_iteration_statement_with_facts(
            HierarchyFacts::DO_OR_WHILE_STATEMENT_EXCLUDES,
            HierarchyFacts::DO_OR_WHILE_STATEMENT_INCLUDES,
            node,
            outermost_labeled_statement,
            None,
        )
    }

    /// tsc-port: visitForStatement @6.0.3
    /// tsc-hash: f040e1f84b67a1ed94230ef30d1762f6557bd9a13e7b49ec1d4e3eae85f9b00e
    /// tsc-span: _tsc.js:106540-106547
    fn visit_for_statement(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
    ) -> Result<VisitOutcome, TransformError> {
        self.visit_iteration_statement_with_facts(
            HierarchyFacts::FOR_STATEMENT_EXCLUDES,
            HierarchyFacts::FOR_STATEMENT_INCLUDES,
            node,
            outermost_labeled_statement,
            None,
        )
    }

    /// tsc-port: visitForInStatement @6.0.3
    /// tsc-hash: 0828972826e4ba4aa77bc16c291feed80966d6e38f2b396613400e29a1293861
    /// tsc-span: _tsc.js:106557-106564
    fn visit_for_in_statement(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
    ) -> Result<VisitOutcome, TransformError> {
        self.visit_iteration_statement_with_facts(
            HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT_EXCLUDES,
            HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT_INCLUDES,
            node,
            outermost_labeled_statement,
            None,
        )
    }

    /// tsc-port: visitForOfStatement @6.0.3
    /// tsc-hash: 93f7ac8c9b196b64c10c4f30ec9f10714beb4f5218a7e2d6ce9603dabe482875
    /// tsc-span: _tsc.js:106565-106573
    fn visit_for_of_statement(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
    ) -> Result<VisitOutcome, TransformError> {
        let convert = if self.downlevel_iteration {
            LoopConverter::ForOfIterable
        } else {
            LoopConverter::ForOfArray
        };
        self.visit_iteration_statement_with_facts(
            HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT_EXCLUDES,
            HierarchyFacts::FOR_IN_OR_FOR_OF_STATEMENT_INCLUDES,
            node,
            outermost_labeled_statement,
            Some(convert),
        )
    }

    /// tsc-port: visitEachChildOfForStatement @6.0.3 (bundled as
    /// visitEachChildOfForStatement2)
    /// tsc-hash: d27328859478fc4bf59e3ac4e80c09f3b46719c7b592ba2a64cd561e2fcdd10b
    /// tsc-span: _tsc.js:106548-106556
    fn visit_each_child_of_for_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (initializer, condition, incrementor, statement) = self.for_statement_parts(node)?;
        let visited_initializer = match initializer {
            Some(initializer) => match self.visit_with_unused_expression_result(initializer)? {
                VisitOutcome::One(node) => Some(node),
                VisitOutcome::Elided => None,
                VisitOutcome::Many(_) => {
                    return Err(assembly_kind_error(
                        SyntaxKind::ForStatement,
                        "for initializer received a statement list",
                    ))
                }
            },
            None => None,
        };
        let visited_condition = self.visit_expression_opt(condition)?;
        let visited_incrementor = match incrementor {
            Some(incrementor) => match self.visit_with_unused_expression_result(incrementor)? {
                VisitOutcome::One(node) => Some(node),
                VisitOutcome::Elided => None,
                VisitOutcome::Many(_) => {
                    return Err(assembly_kind_error(
                        SyntaxKind::ForStatement,
                        "for incrementor received a statement list",
                    ))
                }
            },
            None => None,
        };
        let statement =
            statement.ok_or(assembly_kind_error(SyntaxKind::ForStatement, "statement"))?;
        let visited_statement = self
            .visit_statement_lifted(statement)?
            .ok_or(assembly_kind_error(SyntaxKind::ForStatement, "statement"))?;
        let updated_data = NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
            statement: Some(visited_statement.node()),
            initializer: visited_initializer.map(|node| node.node()),
            condition: visited_condition.map(|node| node.node()),
            incrementor: visited_incrementor.map(|node| node.node()),
        });
        if self.context.arena().node(node)?.data == updated_data {
            return Ok(node);
        }
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: convertIterationStatementBodyIfNecessary @6.0.3
    /// tsc-hash: 91611cb45f3877b4283ae47aa6b98a7ddf7044a6053537474e6a9cbb3ec709e3
    /// tsc-span: _tsc.js:106935-106989
    fn convert_iteration_statement_body_if_necessary(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        ancestor_facts: HierarchyFacts,
        convert: Option<LoopConverter>,
    ) -> Result<VisitOutcome, TransformError> {
        if !self.should_convert_iteration_statement(node)? {
            let mut save_allowed_non_labeled_jumps: Option<Jump> = None;
            if let Some(state) = self.converted_loop_state.as_mut() {
                save_allowed_non_labeled_jumps = Some(state.allowed_non_labeled_jumps);
                state.allowed_non_labeled_jumps = Jump::BREAK.union(Jump::CONTINUE);
            }
            let result = match convert {
                Some(converter) => self.run_loop_converter(
                    converter,
                    node,
                    outermost_labeled_statement,
                    None,
                    ancestor_facts,
                )?,
                None => {
                    let inner = if self.kind(node)? == SyntaxKind::ForStatement {
                        self.visit_each_child_of_for_statement(node)?
                    } else {
                        self.visit_each_child_required(node)?
                    };
                    let reset = self.converted_loop_state.is_some();
                    VisitOutcome::One(self.restore_enclosing_label(
                        inner,
                        outermost_labeled_statement,
                        reset,
                    )?)
                }
            };
            if let Some(saved) = save_allowed_non_labeled_jumps {
                if let Some(state) = self.converted_loop_state.as_mut() {
                    state.allowed_non_labeled_jumps = saved;
                }
            }
            return Ok(result);
        }
        let current_state = self.create_converted_loop_state(node)?;
        let mut statements: Vec<TransformNode> = Vec::new();
        let outer_converted_loop_state = self.converted_loop_state.take();
        self.converted_loop_state = Some(Box::new(current_state));
        let initializer_function = if self.should_convert_initializer_of_for_statement(node)? {
            Some(self.create_function_for_initializer_of_for_statement(node)?)
        } else {
            None
        };
        let body_function = if self.should_convert_body_of_iteration_statement(node)? {
            Some(self.create_function_for_body_of_iteration_statement(node)?)
        } else {
            None
        };
        let current_state = *self
            .converted_loop_state
            .take()
            .expect("converted loop state");
        self.converted_loop_state = outer_converted_loop_state;
        // `generateCallToConvertedLoop(functionName, currentState,
        // OUTERState, containsYield)` runs with the outer state as the
        // explicit write target — after the restore above,
        // `self.converted_loop_state` IS that outer state.
        let body_function = match body_function {
            Some(mut body) => {
                body.part = self.generate_call_to_converted_loop_snapshot(
                    &body.function_name,
                    current_state.clone_for_call(),
                    self.converted_loop_state.is_some(),
                    body.contains_yield,
                )?;
                Some(body)
            }
            None => None,
        };
        if let Some(initializer) = &initializer_function {
            statements.push(initializer.function_declaration);
        }
        if let Some(body) = &body_function {
            statements.push(body.function_declaration);
        }
        self.add_extra_declarations_for_converted_loop(&mut statements, &current_state)?;
        if let Some(initializer) = &initializer_function {
            statements.push(self.generate_call_to_converted_loop_initializer(
                &initializer.function_name,
                initializer.contains_yield,
            )?);
        }
        let loop_statement: VisitOutcome;
        if let Some(body) = body_function {
            if let Some(converter) = convert {
                let converted = self.run_loop_converter(
                    converter,
                    node,
                    outermost_labeled_statement,
                    Some(&body.part),
                    ancestor_facts,
                )?;
                loop_statement = converted;
            } else {
                let block = self.create_block(body.part.clone(), /*multi_line*/ true)?;
                let clone = self.convert_iteration_statement_core(
                    node,
                    initializer_function.as_ref(),
                    block,
                )?;
                let reset = self.converted_loop_state.is_some();
                loop_statement = VisitOutcome::One(self.restore_enclosing_label(
                    clone,
                    outermost_labeled_statement,
                    reset,
                )?);
            }
        } else {
            let (_, _, _, statement) = if self.kind(node)? == SyntaxKind::ForStatement {
                self.for_statement_parts(node)?
            } else {
                (None, None, None, self.iteration_statement_body(node)?)
            };
            let statement = statement.ok_or(assembly_kind_error(
                self.kind(node)?,
                "iteration statement body",
            ))?;
            let visited_statement = self
                .visit_statement_lifted(statement)?
                .ok_or(assembly_kind_error(self.kind(node)?, "visited body"))?;
            let clone = self.convert_iteration_statement_core(
                node,
                initializer_function.as_ref(),
                visited_statement,
            )?;
            let reset = self.converted_loop_state.is_some();
            loop_statement = VisitOutcome::One(self.restore_enclosing_label(
                clone,
                outermost_labeled_statement,
                reset,
            )?);
        }
        match loop_statement {
            VisitOutcome::One(loop_node) => {
                statements.push(loop_node);
                Ok(VisitOutcome::Many(statements))
            }
            VisitOutcome::Many(loop_nodes) => {
                statements.extend(loop_nodes);
                Ok(VisitOutcome::Many(statements))
            }
            VisitOutcome::Elided => Ok(VisitOutcome::Many(statements)),
        }
    }

    fn iteration_statement_body(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::DoStatement(data) => data.statement.map(|id| self.node(id)),
            NodeData::WhileStatement(data) => data.statement.map(|id| self.node(id)),
            NodeData::ForStatement(data) => data.statement.map(|id| self.node(id)),
            NodeData::ForInStatement(data) => data.statement.map(|id| self.node(id)),
            NodeData::ForOfStatement(data) => data.statement.map(|id| self.node(id)),
            _ => None,
        })
    }

    /// tsc-port: convertIterationStatementCore @6.0.3
    /// tsc-hash: f90fb8585825172623599e05fc7e4355890ff1a58d8b04bdfef969378aecc8ad
    /// tsc-span: _tsc.js:106990-107005
    fn convert_iteration_statement_core(
        &mut self,
        node: TransformNode,
        initializer_function: Option<&ConvertedLoopFunction>,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.kind(node)? {
            SyntaxKind::ForStatement => {
                self.convert_for_statement(node, initializer_function, converted_loop_body)
            }
            SyntaxKind::ForInStatement => self.convert_for_in_statement(node, converted_loop_body),
            SyntaxKind::ForOfStatement => self.convert_for_of_statement(node, converted_loop_body),
            SyntaxKind::DoStatement => self.convert_do_statement(node, converted_loop_body),
            SyntaxKind::WhileStatement => self.convert_while_statement(node, converted_loop_body),
            other => Err(assembly_kind_error(other, "IterationStatement expected")),
        }
    }

    /// tsc-port: convertForStatement @6.0.3
    /// tsc-hash: d1ec9787f729191e41a6ac2d575f6c2ac009d24b30f42a8ddd71db34dad66105
    /// tsc-span: _tsc.js:107006-107016
    fn convert_for_statement(
        &mut self,
        node: TransformNode,
        initializer_function: Option<&ConvertedLoopFunction>,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (initializer, condition, incrementor, _) = self.for_statement_parts(node)?;
        let should_convert_condition = match condition {
            Some(condition) => self.should_convert_part_of_iteration_statement(condition)?,
            None => false,
        };
        let should_convert_incrementor = should_convert_condition
            || match incrementor {
                Some(incrementor) => {
                    self.should_convert_part_of_iteration_statement(incrementor)?
                }
                None => false,
            };
        let visited_initializer = match initializer_function {
            Some(function) => Some(function.part_declaration_list.ok_or(assembly_kind_error(
                SyntaxKind::ForStatement,
                "initializer out-variable part",
            ))?),
            None => match initializer {
                Some(initializer) => {
                    match self.visit_with_unused_expression_result(initializer)? {
                        VisitOutcome::One(node) => Some(node),
                        VisitOutcome::Elided => None,
                        VisitOutcome::Many(_) => {
                            return Err(assembly_kind_error(
                                SyntaxKind::ForStatement,
                                "for initializer received a statement list",
                            ))
                        }
                    }
                }
                None => None,
            },
        };
        let visited_condition = if should_convert_condition {
            None
        } else {
            self.visit_expression_opt(condition)?
        };
        let visited_incrementor = if should_convert_incrementor {
            None
        } else {
            match incrementor {
                Some(incrementor) => match self.visit_with_unused_expression_result(incrementor)? {
                    VisitOutcome::One(node) => Some(node),
                    VisitOutcome::Elided => None,
                    VisitOutcome::Many(_) => {
                        return Err(assembly_kind_error(
                            SyntaxKind::ForStatement,
                            "for incrementor received a statement list",
                        ))
                    }
                },
                None => None,
            }
        };
        let updated_data = NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
            statement: Some(converted_loop_body.node()),
            initializer: visited_initializer.map(|node| node.node()),
            condition: visited_condition.map(|node| node.node()),
            incrementor: visited_incrementor.map(|node| node.node()),
        });
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: convertForOfStatement @6.0.3
    /// tsc-hash: 274f8709c95acd37440d505d323b00cc2387f3288cd7eb16ee17bbeddfa32922
    /// tsc-span: _tsc.js:107017-107026
    fn convert_for_of_statement(
        &mut self,
        node: TransformNode,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (initializer, expression) = {
            let NodeData::ForOfStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::ForOfStatement, "for-of"));
            };
            (
                data.initializer.map(|id| self.node(id)),
                data.expression.map(|id| self.node(id)),
            )
        };
        let initializer = initializer.ok_or(assembly_kind_error(
            SyntaxKind::ForOfStatement,
            "initializer",
        ))?;
        let expression = expression.ok_or(assembly_kind_error(
            SyntaxKind::ForOfStatement,
            "expression",
        ))?;
        let visited_initializer = self.visit_required_expression_or_list(initializer)?;
        let visited_expression = self.visit_required_expression(expression)?;
        let updated_data = NodeData::ForOfStatement(tsc_syntax::nodes::ForOfStatementData {
            await_modifier: None,
            initializer: Some(visited_initializer.node()),
            expression: Some(visited_expression.node()),
            statement: Some(converted_loop_body.node()),
        });
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// `visitNode(node.initializer, visitor, isForInitializer)` — the head
    /// may be a declaration list or an expression.
    fn visit_required_expression_or_list(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.visit(node)? {
            VisitOutcome::One(node) => Ok(node),
            _ => Err(assembly_kind_error(
                SyntaxKind::ForOfStatement,
                "for initializer",
            )),
        }
    }

    /// tsc-port: convertForInStatement @6.0.3
    /// tsc-hash: 47dc93245170b45b5460e57e46947715743257cd428221d1bcbd832f81c77e0d
    /// tsc-span: _tsc.js:107027-107034
    fn convert_for_in_statement(
        &mut self,
        node: TransformNode,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (initializer, expression) = {
            let NodeData::ForInStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::ForInStatement, "for-in"));
            };
            (
                data.initializer.map(|id| self.node(id)),
                data.expression.map(|id| self.node(id)),
            )
        };
        let initializer = initializer.ok_or(assembly_kind_error(
            SyntaxKind::ForInStatement,
            "initializer",
        ))?;
        let expression = expression.ok_or(assembly_kind_error(
            SyntaxKind::ForInStatement,
            "expression",
        ))?;
        let visited_initializer = self.visit_required_expression_or_list(initializer)?;
        let visited_expression = self.visit_required_expression(expression)?;
        let updated_data = NodeData::ForInStatement(tsc_syntax::nodes::ForInStatementData {
            initializer: Some(visited_initializer.node()),
            expression: Some(visited_expression.node()),
            statement: Some(converted_loop_body.node()),
        });
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: convertDoStatement @6.0.3
    /// tsc-hash: 26d0afc9d37d0038fce2be9eef460ed0b1b86c542e6dde422eaf3369960720a2
    /// tsc-span: _tsc.js:107035-107041
    fn convert_do_statement(
        &mut self,
        node: TransformNode,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let expression = {
            let NodeData::DoStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::DoStatement, "do"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(SyntaxKind::DoStatement, "expression"))?
        };
        let visited_expression = self.visit_required_expression(expression)?;
        let updated_data = NodeData::DoStatement(tsc_syntax::nodes::DoStatementData {
            statement: Some(converted_loop_body.node()),
            expression: Some(visited_expression.node()),
        });
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }

    /// tsc-port: convertWhileStatement @6.0.3
    /// tsc-hash: 92d42279b892c99ee5908d5053cb72c4f6a36b764cd59a77635124872d2bd7e0
    /// tsc-span: _tsc.js:107042-107048
    fn convert_while_statement(
        &mut self,
        node: TransformNode,
        converted_loop_body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let expression = {
            let NodeData::WhileStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::WhileStatement, "while"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(
                    SyntaxKind::WhileStatement,
                    "expression",
                ))?
        };
        let visited_expression = self.visit_required_expression(expression)?;
        let updated_data = NodeData::WhileStatement(tsc_syntax::nodes::WhileStatementData {
            expression: Some(visited_expression.node()),
            statement: Some(converted_loop_body.node()),
        });
        let flags = flags_after_update(self.context.arena(), node, &updated_data)?;
        self.context
            .factory()?
            .update_node(node, updated_data, flags)
    }
}

/// The `convert` callback selector (`convertForOfStatementForArray` /
/// `convertForOfStatementForIterable`).
#[derive(Clone, Copy)]
enum LoopConverter {
    ForOfArray,
    ForOfIterable,
}

/// `createFunctionForInitializerOfForStatement` /
/// `createFunctionForBodyOfIterationStatement` result record.
struct ConvertedLoopFunction {
    function_name: TargetBinding,
    contains_yield: bool,
    function_declaration: TransformNode,
    /// body flavor: the loop-body statements; initializer flavor: None.
    part: Vec<TransformNode>,
    /// initializer flavor: the out-variable declaration list for the FOR
    /// head; body flavor: None.
    part_declaration_list: Option<TransformNode>,
}

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: createConvertedLoopState @6.0.3
    /// tsc-hash: 669273fa3a9362caa338addc7f71db0cc202d64692e08e9e3797c22994b73f13
    /// tsc-span: _tsc.js:107049-107082
    fn create_converted_loop_state(
        &mut self,
        node: TransformNode,
    ) -> Result<ConvertedLoopState, TransformError> {
        let mut loop_initializer: Option<TransformNode> = None;
        match self.kind(node)? {
            SyntaxKind::ForStatement | SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement => {
                let initializer = match &self.context.arena().node(node)?.data {
                    NodeData::ForStatement(data) => data.initializer,
                    NodeData::ForInStatement(data) => data.initializer,
                    NodeData::ForOfStatement(data) => data.initializer,
                    _ => None,
                };
                if let Some(initializer) = initializer {
                    let initializer = self.node(initializer);
                    if self.kind(initializer)? == SyntaxKind::VariableDeclarationList {
                        loop_initializer = Some(initializer);
                    }
                }
            }
            _ => {}
        }
        let mut loop_parameters: Vec<TransformNode> = Vec::new();
        let mut loop_out_parameters: Vec<LoopOutParameter> = Vec::new();
        if let Some(initializer) = loop_initializer {
            // getCombinedNodeFlags(loopInitializer) & NodeFlags.BlockScoped
            if self.get_combined_node_flags(initializer)? & 7 != 0 {
                let has_captured_bindings_in_for_head = self
                    .should_convert_initializer_of_for_statement(node)?
                    || self.should_convert_condition_of_for_statement(node)?
                    || self.should_convert_incrementor_of_for_statement(node)?;
                let declarations = {
                    let NodeData::VariableDeclarationList(data) =
                        &self.context.arena().node(initializer)?.data
                    else {
                        return Err(assembly_kind_error(
                            SyntaxKind::VariableDeclarationList,
                            "loop initializer",
                        ));
                    };
                    self.array_nodes(data.declarations)?
                };
                for declaration in declarations {
                    self.process_loop_variable_declaration(
                        node,
                        declaration,
                        &mut loop_parameters,
                        &mut loop_out_parameters,
                        has_captured_bindings_in_for_head,
                    )?;
                }
            }
        }
        let mut state = ConvertedLoopState {
            loop_parameters,
            loop_out_parameters,
            ..ConvertedLoopState::default()
        };
        if let Some(outer) = self.converted_loop_state.as_ref() {
            if let Some(binding) = &outer.arguments_name {
                state.arguments_name = Some(binding.clone());
            }
            if let Some(binding) = &outer.this_name {
                state.this_name = Some(binding.clone());
            }
            if !outer.hoisted_local_variables.is_empty() {
                state.hoisted_local_variables = outer.hoisted_local_variables.clone();
            }
        }
        Ok(state)
    }

    /// tsc-port: getCombinedNodeFlags @6.0.3
    /// tsc-hash: 5d698610c67a2e73fafa1a1472315acb0ae677c2b67f4bf46e0d5d970f792061
    /// tsc-span: _tsc.js:11342-11344
    fn get_combined_node_flags(&self, node: TransformNode) -> Result<i32, TransformError> {
        let arena = self.context.arena();
        let mut flags = arena.node(node)?.flags;
        let record = arena.node(node)?;
        // walk VariableDeclaration -> list -> statement per getCombinedFlags
        let mut current = record.parent;
        let mut kind = record.kind;
        while let Some(parent_id) = current {
            let parent = TransformNode::new(node.source(), parent_id);
            let parent_record = arena.node(parent)?;
            match (kind, parent_record.kind) {
                (SyntaxKind::VariableDeclaration, SyntaxKind::VariableDeclarationList)
                | (SyntaxKind::VariableDeclarationList, SyntaxKind::VariableStatement) => {
                    flags |= parent_record.flags;
                    kind = parent_record.kind;
                    current = parent_record.parent;
                }
                _ => break,
            }
        }
        Ok(flags)
    }

    /// tsc-port: processLoopVariableDeclaration @6.0.3
    /// tsc-hash: 36518bd98c58010b529ec039cd838439bdb3e5fb8dcb475498547dbd2492abe0
    /// tsc-span: _tsc.js:107449-107483
    fn process_loop_variable_declaration(
        &mut self,
        container: TransformNode,
        declaration: TransformNode,
        loop_parameters: &mut Vec<TransformNode>,
        loop_out_parameters: &mut Vec<LoopOutParameter>,
        has_captured_bindings_in_for_head: bool,
    ) -> Result<(), TransformError> {
        let name = match &self.context.arena().node(declaration)?.data {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::BindingElement(data) => data.name,
            _ => None,
        };
        let name = name.map(|id| self.node(id)).ok_or(assembly_kind_error(
            SyntaxKind::VariableDeclaration,
            "loop variable name",
        ))?;
        if self.is_binding_pattern(name)? {
            let elements = match &self.context.arena().node(name)?.data {
                NodeData::ObjectBindingPattern(data) => data.elements,
                NodeData::ArrayBindingPattern(data) => data.elements,
                _ => None,
            };
            for element in self.array_nodes(elements)? {
                if self.kind(element)? != SyntaxKind::OmittedExpression {
                    self.process_loop_variable_declaration(
                        container,
                        element,
                        loop_parameters,
                        loop_out_parameters,
                        has_captured_bindings_in_for_head,
                    )?;
                }
            }
            return Ok(());
        }
        let parameter = self.create_parameter_declaration(name)?;
        loop_parameters.push(parameter);
        let needs_out_param = {
            match self.context.arena().parse_tree_resolver_node(declaration)? {
                Some(reference) => self.resolver.has_node_check_flag(
                    reference,
                    NodeCheckFlags::NEEDS_LOOP_OUT_PARAMETER.bits() as u32,
                )?,
                None => false,
            }
        };
        if needs_out_param || has_captured_bindings_in_for_head {
            let name_text = self.identifier_text(name)?;
            let out_param_name = self.allocate_numbered_binding(&format!("out_{name_text}"))?;
            let mut flags = LoopOutParameterFlags::default();
            if needs_out_param {
                flags = flags.union(LoopOutParameterFlags::BODY);
            }
            if self.kind(container)? == SyntaxKind::ForStatement {
                let (initializer, condition, incrementor, _) =
                    self.for_statement_parts(container)?;
                let declaration_reference =
                    self.context.arena().parse_tree_resolver_node(declaration)?;
                if let (Some(initializer), Some(declaration_reference)) =
                    (initializer, declaration_reference)
                {
                    if let Some(initializer_reference) =
                        self.context.arena().parse_tree_resolver_node(initializer)?
                    {
                        if self.resolver.is_binding_captured_by_node(
                            initializer_reference,
                            declaration_reference,
                        )? {
                            flags = flags.union(LoopOutParameterFlags::INITIALIZER);
                        }
                    }
                }
                let mut body_flagged = false;
                if let (Some(condition), Some(declaration_reference)) = (
                    condition,
                    self.context.arena().parse_tree_resolver_node(declaration)?,
                ) {
                    if let Some(condition_reference) =
                        self.context.arena().parse_tree_resolver_node(condition)?
                    {
                        if self.resolver.is_binding_captured_by_node(
                            condition_reference,
                            declaration_reference,
                        )? {
                            body_flagged = true;
                        }
                    }
                }
                if !body_flagged {
                    if let (Some(incrementor), Some(declaration_reference)) = (
                        incrementor,
                        self.context.arena().parse_tree_resolver_node(declaration)?,
                    ) {
                        if let Some(incrementor_reference) =
                            self.context.arena().parse_tree_resolver_node(incrementor)?
                        {
                            if self.resolver.is_binding_captured_by_node(
                                incrementor_reference,
                                declaration_reference,
                            )? {
                                body_flagged = true;
                            }
                        }
                    }
                }
                if body_flagged {
                    flags = flags.union(LoopOutParameterFlags::BODY);
                }
            }
            loop_out_parameters.push(LoopOutParameter {
                flags,
                original_name: name,
                out_param_name,
            });
        }
        Ok(())
    }

    /// tsc-port: addExtraDeclarationsForConvertedLoop @6.0.3
    /// tsc-hash: 5cf33cd652664d05c078f8516837e513444eabcdb3734e585a99b4dd87553a34
    /// tsc-span: _tsc.js:107083-107157
    fn add_extra_declarations_for_converted_loop(
        &mut self,
        statements: &mut Vec<TransformNode>,
        state: &ConvertedLoopState,
    ) -> Result<(), TransformError> {
        let mut extra_variable_declarations: Vec<TransformNode> = Vec::new();
        if let Some(arguments_name) = &state.arguments_name {
            if let Some(outer) = self.converted_loop_state.as_mut() {
                outer.arguments_name = Some(arguments_name.clone());
            } else {
                let name = self.create_generated_identifier(&arguments_name.clone())?;
                let arguments_identifier = self.create_identifier("arguments")?;
                extra_variable_declarations.push(
                    self.create_variable_declaration_plain(name, Some(arguments_identifier))?,
                );
            }
        }
        if let Some(this_name) = &state.this_name {
            if let Some(outer) = self.converted_loop_state.as_mut() {
                outer.this_name = Some(this_name.clone());
            } else {
                let name = self.create_generated_identifier(&this_name.clone())?;
                let this_identifier = self.create_identifier("this")?;
                extra_variable_declarations
                    .push(self.create_variable_declaration_plain(name, Some(this_identifier))?);
            }
        }
        if !state.hoisted_local_variables.is_empty() {
            if let Some(outer) = self.converted_loop_state.as_mut() {
                outer.hoisted_local_variables = state.hoisted_local_variables.clone();
            } else {
                for identifier in &state.hoisted_local_variables {
                    extra_variable_declarations
                        .push(self.create_variable_declaration_plain(*identifier, None)?);
                }
            }
        }
        if !state.loop_out_parameters.is_empty() {
            for out_param in &state.loop_out_parameters {
                let name = self.create_generated_identifier(&out_param.out_param_name)?;
                extra_variable_declarations
                    .push(self.create_variable_declaration_plain(name, None)?);
            }
        }
        if let Some(condition_variable) = &state.condition_variable {
            let name = self.create_generated_identifier(&condition_variable.clone())?;
            let false_literal = self.create_false()?;
            extra_variable_declarations
                .push(self.create_variable_declaration_plain(name, Some(false_literal))?);
        }
        if !extra_variable_declarations.is_empty() {
            let statement =
                self.create_variable_statement_from_declarations(extra_variable_declarations)?;
            statements.push(statement);
        }
        Ok(())
    }

    /// tsc-port: createOutVariable @6.0.3
    /// tsc-hash: 509144252ca51bbed068e9bb8f00d0fc059cd07da3329dd47fc9d1a5353d0836
    /// tsc-span: _tsc.js:107158-107167
    fn create_out_variable(
        &mut self,
        out_param: &LoopOutParameter,
    ) -> Result<TransformNode, TransformError> {
        let out_param_identifier = self.create_generated_identifier(&out_param.out_param_name)?;
        self.create_variable_declaration_plain(out_param.original_name, Some(out_param_identifier))
    }

    /// tsc-port: createFunctionForInitializerOfForStatement @6.0.3
    /// tsc-hash: bc8e778206246a551c531ac4e89eb4b4ebf6b64cf23182053a8497394babe1bb
    /// tsc-span: _tsc.js:107168-107224
    fn create_function_for_initializer_of_for_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<ConvertedLoopFunction, TransformError> {
        let function_name = self.allocate_numbered_binding("_loop_init")?;
        let (initializer, _, _, _) = self.for_statement_parts(node)?;
        let initializer =
            initializer.ok_or(assembly_kind_error(SyntaxKind::ForStatement, "initializer"))?;
        let contains_yield = self
            .transform_flags(initializer)
            .contains(TransformFlags::CONTAINS_YIELD);
        let mut emit_flags = EmitFlags::NONE;
        let contains_lexical_this = self.loop_state().contains_lexical_this;
        if contains_lexical_this {
            emit_flags |= EmitFlags::CAPTURES_THIS;
        }
        if contains_yield
            && self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::ASYNC_FUNCTION_BODY)
        {
            emit_flags |= EmitFlags::ASYNC_FUNCTION_BODY;
        }
        self.enter_function_scope_path();
        let mut statements: Vec<TransformNode> = Vec::new();
        let initializer_statement = self.create_variable_statement_from_list(initializer)?;
        statements.push(initializer_statement);
        let out_params: Vec<(TransformNode, TargetBinding, LoopOutParameterFlags)> = self
            .loop_state()
            .loop_out_parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.original_name,
                    parameter.out_param_name.clone(),
                    parameter.flags,
                )
            })
            .collect();
        for (original_name, out_param, flags) in &out_params {
            if flags.intersects(LoopOutParameterFlags::INITIALIZER) {
                let copy = self.copy_out_parameter_pair(
                    *original_name,
                    out_param,
                    CopyDirection::ToOutParameter,
                )?;
                statements.push(self.create_expression_statement(copy)?);
            }
        }
        // visitNode(createBlock(statements, true), visitor, isBlock)
        let block = self.create_block(statements, /*multi_line*/ true)?;
        let visited_block = match self.visit(block)? {
            VisitOutcome::One(node) => node,
            _ => {
                return Err(assembly_kind_error(
                    SyntaxKind::Block,
                    "initializer function body",
                ))
            }
        };
        self.exit_function_scope_path();
        let function = self.create_function_expression_full(
            /*asterisk*/ contains_yield,
            /*name*/ None,
            /*parameters*/ Vec::new(),
            visited_block,
        )?;
        if emit_flags != EmitFlags::NONE {
            self.add_emit_flags(function, emit_flags)?;
        }
        let function_name_identifier = self.create_generated_identifier(&function_name)?;
        let declaration =
            self.create_variable_declaration_plain(function_name_identifier, Some(function))?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        self.add_emit_flags(list, EmitFlags::NO_HOISTING)?;
        let function_declaration = self.create_variable_statement_from_list(list)?;
        // part: the out-variable declaration list for the loop head.
        let out_variables = {
            let mut declarations = Vec::new();
            for (original_name, out_param, _) in &out_params {
                let out_param = LoopOutParameter {
                    flags: LoopOutParameterFlags::default(),
                    original_name: *original_name,
                    out_param_name: out_param.clone(),
                };
                declarations.push(self.create_out_variable(&out_param)?);
            }
            declarations
        };
        let part_declaration_list = self.create_variable_declaration_list(out_variables)?;
        Ok(ConvertedLoopFunction {
            function_name,
            contains_yield,
            function_declaration,
            part: Vec::new(),
            part_declaration_list: Some(part_declaration_list),
        })
    }

    /// tsc-port: createFunctionForBodyOfIterationStatement @6.0.3
    /// tsc-hash: 16d0864da1c018f8ee227fe9e941165f10e8aab9804485bb4ae3deb610d1db9c
    /// tsc-span: _tsc.js:107225-107306
    fn create_function_for_body_of_iteration_statement(
        &mut self,
        node: TransformNode,
    ) -> Result<ConvertedLoopFunction, TransformError> {
        let function_name = self.allocate_numbered_binding("_loop")?;
        self.enter_function_scope_path();
        self.context.start_lexical_environment()?;
        let body = self
            .iteration_statement_body(node)?
            .ok_or(assembly_kind_error(self.kind(node)?, "iteration body"))?;
        let visited_statement = self
            .visit_statement_lifted(body)?
            .ok_or(assembly_kind_error(self.kind(node)?, "visited body"))?;
        let lexical_environment = self.context.end_lexical_environment()?;
        let mut statements: Vec<TransformNode> = Vec::new();
        if self.should_convert_condition_of_for_statement(node)?
            || self.should_convert_incrementor_of_for_statement(node)?
        {
            let condition_variable = self.allocate_numbered_binding("inc")?;
            self.loop_state_mut().condition_variable = Some(condition_variable.clone());
            let (_, condition, incrementor, _) = self.for_statement_parts(node)?;
            if let Some(incrementor) = incrementor {
                let condition_reference = self.create_generated_identifier(&condition_variable)?;
                let visited_incrementor = self.visit_required_expression(incrementor)?;
                let then_statement = self.create_expression_statement(visited_incrementor)?;
                let condition_write = self.create_generated_identifier(&condition_variable)?;
                let true_literal = self.create_true()?;
                let assignment = self.create_assignment(condition_write, true_literal)?;
                let else_statement = self.create_expression_statement(assignment)?;
                statements.push(self.create_if_statement(
                    condition_reference,
                    then_statement,
                    Some(else_statement),
                )?);
            } else {
                let condition_reference = self.create_generated_identifier(&condition_variable)?;
                let not = self.create_logical_not(condition_reference)?;
                let condition_write = self.create_generated_identifier(&condition_variable)?;
                let true_literal = self.create_true()?;
                let assignment = self.create_assignment(condition_write, true_literal)?;
                let then_statement = self.create_expression_statement(assignment)?;
                statements.push(self.create_if_statement(not, then_statement, None)?);
            }
            if self.should_convert_condition_of_for_statement(node)? {
                let condition =
                    condition.ok_or(assembly_kind_error(SyntaxKind::ForStatement, "condition"))?;
                let visited_condition = self.visit_required_expression(condition)?;
                let source = self.source;
                let flags = self.child_flags(&[visited_condition])?;
                let negated = self.context.factory()?.create_node(
                    source,
                    NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                        operator: SyntaxKind::ExclamationToken,
                        operand: Some(visited_condition.node()),
                    }),
                    flags,
                )?;
                let break_statement = {
                    let created = self.context.factory()?.create_node(
                        self.source,
                        NodeData::BreakStatement(tsc_syntax::nodes::BreakStatementData {
                            label: None,
                        }),
                        TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
                    )?;
                    created
                };
                let visited_break =
                    self.visit_statement_lifted(break_statement)?
                        .ok_or(assembly_kind_error(
                            SyntaxKind::BreakStatement,
                            "converted break",
                        ))?;
                statements.push(self.create_if_statement(negated, visited_break, None)?);
            }
        }
        if self.kind(visited_statement)? == SyntaxKind::Block {
            let NodeData::Block(data) = &self.context.arena().node(visited_statement)?.data else {
                return Err(assembly_kind_error(SyntaxKind::Block, "visited body"));
            };
            let inner = self.array_nodes(data.statements)?;
            statements.extend(inner);
        } else {
            statements.push(visited_statement);
        }
        let out_params: Vec<(TransformNode, TargetBinding, LoopOutParameterFlags)> = self
            .loop_state()
            .loop_out_parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.original_name,
                    parameter.out_param_name.clone(),
                    parameter.flags,
                )
            })
            .collect();
        for (original_name, out_param, flags) in &out_params {
            if flags.intersects(LoopOutParameterFlags::BODY) {
                let copy = self.copy_out_parameter_pair(
                    *original_name,
                    out_param,
                    CopyDirection::ToOutParameter,
                )?;
                statements.push(self.create_expression_statement(copy)?);
            }
        }
        self.insert_statements_after_standard_prologue_materialized(
            &mut statements,
            lexical_environment,
        )?;
        let loop_body = self.create_block(statements, /*multi_line*/ true)?;
        if self.kind(visited_statement)? == SyntaxKind::Block {
            self.set_original(loop_body, visited_statement)?;
        }
        let contains_yield = {
            let body = self.iteration_statement_body(node)?.expect("body present");
            self.transform_flags(body)
                .contains(TransformFlags::CONTAINS_YIELD)
        };
        let mut emit_flags = EmitFlags::REUSE_TEMP_VARIABLE_SCOPE;
        let contains_lexical_this = self.loop_state().contains_lexical_this;
        if contains_lexical_this {
            emit_flags |= EmitFlags::CAPTURES_THIS;
        }
        if contains_yield
            && self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::ASYNC_FUNCTION_BODY)
        {
            emit_flags |= EmitFlags::ASYNC_FUNCTION_BODY;
        }
        self.exit_function_scope_path();
        let loop_parameters = self.loop_state().loop_parameters.clone();
        let function =
            self.create_function_expression_full(contains_yield, None, loop_parameters, loop_body)?;
        self.add_emit_flags(function, emit_flags)?;
        let function_name_identifier = self.create_generated_identifier(&function_name)?;
        let declaration =
            self.create_variable_declaration_plain(function_name_identifier, Some(function))?;
        let list = self.create_variable_declaration_list(vec![declaration])?;
        self.add_emit_flags(list, EmitFlags::NO_HOISTING)?;
        let function_declaration = self.create_variable_statement_from_list(list)?;
        Ok(ConvertedLoopFunction {
            function_name,
            contains_yield,
            function_declaration,
            part: Vec::new(),
            part_declaration_list: None,
        })
    }

    /// tsc-port: generateCallToConvertedLoopInitializer @6.0.3
    /// tsc-hash: 4a0b400a40b21ed872ee8dd8ca175d345558c50113df092cebd1849bdcc7cfcb
    /// tsc-span: _tsc.js:107319-107331
    ///
    /// SITE A of the pinned `yield-star-synthesis` edge: the synthesized
    /// `yield* call` takes `EmitFlags::ITERATOR` on the CALL, which B-3's
    /// `visitYieldExpression` consumer-skips (no `__values` wrap).
    fn generate_call_to_converted_loop_initializer(
        &mut self,
        function_name: &TargetBinding,
        contains_yield: bool,
    ) -> Result<TransformNode, TransformError> {
        let function_reference = self.create_generated_identifier(function_name)?;
        let call = self.create_call(function_reference, vec![])?;
        let call_result = if contains_yield {
            self.add_emit_flags(call, EmitFlags::ITERATOR)?;
            self.create_yield_star(call)?
        } else {
            call
        };
        self.create_expression_statement(call_result)
    }

    /// tsc-port: generateCallToConvertedLoop @6.0.3
    /// tsc-hash: c5ca870e65bed1b57dabee006e28542b2ce95047c0bde442fddc525cd16a57e2
    /// tsc-span: _tsc.js:107332-107419
    ///
    /// SITE B of the pinned `yield-star-synthesis` edge (the loop-body
    /// delegation call).
    fn generate_call_to_converted_loop_snapshot(
        &mut self,
        loop_function_name: &TargetBinding,
        state: ConvertedLoopCallSnapshot,
        has_outer_state: bool,
        contains_yield: bool,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements: Vec<TransformNode> = Vec::new();
        let is_simple_loop = !state
            .non_local_jumps
            .without(Jump::CONTINUE)
            .intersects(Jump::BREAK.union(Jump::RETURN))
            && state.labeled_non_local_breaks.is_empty()
            && state.labeled_non_local_continues.is_empty();
        let function_reference = self.create_generated_identifier(loop_function_name)?;
        let mut call_arguments: Vec<TransformNode> = Vec::new();
        for parameter in &state.loop_parameters {
            let name = {
                let NodeData::Parameter(data) = &self.context.arena().node(*parameter)?.data else {
                    return Err(assembly_kind_error(SyntaxKind::Parameter, "loop parameter"));
                };
                data.name
                    .map(|id| self.node(id))
                    .ok_or(assembly_kind_error(
                        SyntaxKind::Parameter,
                        "loop parameter name",
                    ))?
            };
            call_arguments.push(self.clone_node(name)?);
        }
        let call = self.create_call(function_reference, call_arguments)?;
        let call_result = if contains_yield {
            self.add_emit_flags(call, EmitFlags::ITERATOR)?;
            self.create_yield_star(call)?
        } else {
            call
        };
        if is_simple_loop {
            statements.push(self.create_expression_statement(call_result)?);
            for (original_name, out_param, flags) in &state.loop_out_parameters {
                if flags.intersects(LoopOutParameterFlags::BODY) {
                    let copy = self.copy_out_parameter_pair(
                        *original_name,
                        out_param,
                        CopyDirection::ToOriginal,
                    )?;
                    statements.push(self.create_expression_statement(copy)?);
                }
            }
        } else {
            let loop_result_binding = self.allocate_numbered_binding("state")?;
            let record_index = self.state_binding_records.len();
            self.state_binding_records.push(StateBindingRecord {
                scope_path: self.function_scope_path.clone(),
                sequence: record_index as u32,
                binding: loop_result_binding.clone(),
                identifiers: Vec::new(),
            });
            let loop_result_name = self.create_generated_identifier(&loop_result_binding)?;
            self.state_binding_records[record_index]
                .identifiers
                .push(loop_result_name);
            let state_variable =
                self.create_variable_statement_single(loop_result_name, Some(call_result))?;
            statements.push(state_variable);
            for (original_name, out_param, flags) in &state.loop_out_parameters {
                if flags.intersects(LoopOutParameterFlags::BODY) {
                    let copy = self.copy_out_parameter_pair(
                        *original_name,
                        out_param,
                        CopyDirection::ToOriginal,
                    )?;
                    statements.push(self.create_expression_statement(copy)?);
                }
            }
            if state.non_local_jumps.intersects(Jump::RETURN) {
                let return_statement: TransformNode;
                if has_outer_state {
                    if let Some(outer) = self.converted_loop_state.as_mut() {
                        outer.non_local_jumps = outer.non_local_jumps.union(Jump::RETURN);
                    }
                    let result_reference =
                        self.create_state_identifier(record_index, &loop_result_binding)?;
                    return_statement = self.create_return_statement(Some(result_reference))?;
                } else {
                    let result_reference =
                        self.create_state_identifier(record_index, &loop_result_binding)?;
                    let value_access =
                        self.create_property_access_text(result_reference, "value")?;
                    return_statement = self.create_return_statement(Some(value_access))?;
                }
                let result_reference =
                    self.create_state_identifier(record_index, &loop_result_binding)?;
                let type_check = self.create_type_check(result_reference, "object")?;
                statements.push(self.create_if_statement(type_check, return_statement, None)?);
            }
            if state.non_local_jumps.intersects(Jump::BREAK) {
                let result_reference =
                    self.create_state_identifier(record_index, &loop_result_binding)?;
                let break_literal = self.create_string_literal("break")?;
                let equality = self.create_strict_equality(result_reference, break_literal)?;
                let break_statement = self.context.factory()?.create_node(
                    self.source,
                    NodeData::BreakStatement(tsc_syntax::nodes::BreakStatementData { label: None }),
                    TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
                )?;
                statements.push(self.create_if_statement(equality, break_statement, None)?);
            }
            if !state.labeled_non_local_breaks.is_empty()
                || !state.labeled_non_local_continues.is_empty()
            {
                let mut case_clauses: Vec<TransformNode> = Vec::new();
                let breaks = state.labeled_non_local_breaks.clone();
                let continues = state.labeled_non_local_continues.clone();
                self.process_labeled_jumps(
                    &breaks,
                    /*is_break*/ true,
                    record_index,
                    &loop_result_binding,
                    has_outer_state,
                    &mut case_clauses,
                )?;
                self.process_labeled_jumps(
                    &continues,
                    /*is_break*/ false,
                    record_index,
                    &loop_result_binding,
                    has_outer_state,
                    &mut case_clauses,
                )?;
                let result_reference =
                    self.create_state_identifier(record_index, &loop_result_binding)?;
                let case_block = {
                    let source = self.source;
                    let array = self
                        .context
                        .factory()?
                        .create_node_array(source, case_clauses)?;
                    let flags = self.context.arena().array_transform_flags(array);
                    self.context.factory()?.create_node(
                        source,
                        NodeData::CaseBlock(tsc_syntax::nodes::CaseBlockData {
                            clauses: Some(array.array()),
                        }),
                        flags,
                    )?
                };
                let switch_statement = {
                    let source = self.source;
                    let flags = self.child_flags(&[result_reference, case_block])?;
                    self.context.factory()?.create_node(
                        source,
                        NodeData::SwitchStatement(tsc_syntax::nodes::SwitchStatementData {
                            expression: Some(result_reference.node()),
                            case_block: Some(case_block.node()),
                        }),
                        flags,
                    )?
                };
                statements.push(switch_statement);
            }
        }
        Ok(statements)
    }

    fn create_state_identifier(
        &mut self,
        record_index: usize,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_generated_identifier(binding)?;
        self.state_binding_records[record_index]
            .identifiers
            .push(identifier);
        Ok(identifier)
    }

    /// tsc-port: processLabeledJumps @6.0.3
    /// tsc-hash: e00b53461f6a95a971d911b5595e0d7e04a1ef3918d73bbeee9d8d65b5a24339
    /// tsc-span: _tsc.js:107433-107448
    #[allow(clippy::too_many_arguments)]
    fn process_labeled_jumps(
        &mut self,
        table: &[(String, String)],
        is_break: bool,
        record_index: usize,
        loop_result_binding: &TargetBinding,
        has_outer_state: bool,
        case_clauses: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        for (label_text, label_marker) in table {
            let mut statements: Vec<TransformNode> = Vec::new();
            let outer_has_label = match self.converted_loop_state.as_ref() {
                Some(outer) => outer.labels.get(label_text).copied().unwrap_or(false),
                None => false,
            };
            if !has_outer_state || outer_has_label {
                let label = self.create_identifier(label_text)?;
                let statement = self.context.factory()?.create_node(
                    self.source,
                    if is_break {
                        NodeData::BreakStatement(tsc_syntax::nodes::BreakStatementData {
                            label: Some(label.node()),
                        })
                    } else {
                        NodeData::ContinueStatement(tsc_syntax::nodes::ContinueStatementData {
                            label: Some(label.node()),
                        })
                    },
                    TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
                )?;
                statements.push(statement);
            } else {
                if let Some(outer) = self.converted_loop_state.as_mut() {
                    set_labeled_jump(outer, is_break, label_text.clone(), label_marker.clone());
                }
                let result_reference =
                    self.create_state_identifier(record_index, loop_result_binding)?;
                statements.push(self.create_return_statement(Some(result_reference))?);
            }
            let marker_literal = self.create_string_literal(label_marker)?;
            let clause = {
                let source = self.source;
                let array = self
                    .context
                    .factory()?
                    .create_node_array(source, statements)?;
                let flags = self.context.arena().array_transform_flags(array)
                    | self.child_flags(&[marker_literal])?;
                self.context.factory()?.create_node(
                    source,
                    NodeData::CaseClause(tsc_syntax::nodes::CaseClauseData {
                        expression: Some(marker_literal.node()),
                        statements: Some(array.array()),
                    }),
                    flags,
                )?
            };
            case_clauses.push(clause);
        }
        Ok(())
    }

    /// Dispatch to the for-of converters (`convert` parameter).
    fn run_loop_converter(
        &mut self,
        converter: LoopConverter,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        converted_loop_body_statements: Option<&[TransformNode]>,
        ancestor_facts: HierarchyFacts,
    ) -> Result<VisitOutcome, TransformError> {
        match converter {
            LoopConverter::ForOfArray => {
                Ok(VisitOutcome::One(self.convert_for_of_statement_for_array(
                    node,
                    outermost_labeled_statement,
                    converted_loop_body_statements,
                )?))
            }
            LoopConverter::ForOfIterable => Ok(VisitOutcome::One(
                self.convert_for_of_statement_for_iterable(
                    node,
                    outermost_labeled_statement,
                    converted_loop_body_statements,
                    ancestor_facts,
                )?,
            )),
        }
    }
}

/// The `generateCallToConvertedLoop` state snapshot (the call generator
/// runs while `convertedLoopState` still points at the CURRENT loop, but
/// reads its recorded fields; the outer-state writes go through
/// `self.converted_loop_state` which by then is the OUTER state — the
/// upstream call happens after `convertedLoopState = outerConvertedLoopState`).
#[derive(Clone)]
struct ConvertedLoopCallSnapshot {
    non_local_jumps: Jump,
    labeled_non_local_breaks: Vec<(String, String)>,
    labeled_non_local_continues: Vec<(String, String)>,
    loop_parameters: Vec<TransformNode>,
    loop_out_parameters: Vec<(TransformNode, TargetBinding, LoopOutParameterFlags)>,
}

impl ConvertedLoopState {
    fn clone_for_call(&self) -> ConvertedLoopCallSnapshot {
        ConvertedLoopCallSnapshot {
            non_local_jumps: self.non_local_jumps,
            labeled_non_local_breaks: self.labeled_non_local_breaks.clone(),
            labeled_non_local_continues: self.labeled_non_local_continues.clone(),
            loop_parameters: self.loop_parameters.clone(),
            loop_out_parameters: self
                .loop_out_parameters
                .iter()
                .map(|parameter| {
                    (
                        parameter.original_name,
                        parameter.out_param_name.clone(),
                        parameter.flags,
                    )
                })
                .collect(),
        }
    }
}

impl Es2015Visitor<'_, '_, '_> {
    /// tsc-port: convertForOfStatementHead @6.0.3
    /// tsc-hash: b36ba10e482e3c2d129b9ddb1b17835aae94fb81371dc6c16850f0205a51eee6
    /// tsc-span: _tsc.js:106574-106655
    fn convert_for_of_statement_head(
        &mut self,
        node: TransformNode,
        bound_value: TransformNode,
        converted_loop_body_statements: Option<&[TransformNode]>,
    ) -> Result<TransformNode, TransformError> {
        let mut statements: Vec<TransformNode> = Vec::new();
        let (initializer, statement) = {
            let NodeData::ForOfStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::ForOfStatement, "for-of"));
            };
            (
                data.initializer.map(|id| self.node(id)),
                data.statement.map(|id| self.node(id)),
            )
        };
        let initializer = initializer.ok_or(assembly_kind_error(
            SyntaxKind::ForOfStatement,
            "initializer",
        ))?;
        if self.kind(initializer)? == SyntaxKind::VariableDeclarationList {
            if self.context.arena().node(initializer)?.flags & 7 != 0 {
                self.enable_substitutions_for_block_scoped_bindings()?;
            }
            let declarations = {
                let NodeData::VariableDeclarationList(data) =
                    &self.context.arena().node(initializer)?.data
                else {
                    return Err(assembly_kind_error(
                        SyntaxKind::VariableDeclarationList,
                        "for-of head",
                    ));
                };
                self.array_nodes(data.declarations)?
            };
            let first_original_declaration = declarations.first().copied();
            let first_is_pattern = match first_original_declaration {
                Some(declaration) => {
                    let name = {
                        let NodeData::VariableDeclaration(data) =
                            &self.context.arena().node(declaration)?.data
                        else {
                            return Err(assembly_kind_error(
                                SyntaxKind::VariableDeclaration,
                                "for-of declaration",
                            ));
                        };
                        data.name.map(|id| self.node(id))
                    };
                    match name {
                        Some(name) => self.is_binding_pattern(name)?,
                        None => false,
                    }
                }
                None => false,
            };
            if first_is_pattern {
                let declaration = first_original_declaration.expect("pattern declaration");
                let flattened = flatten_destructuring_binding(
                    self,
                    declaration,
                    FlattenLevel::All,
                    Some(bound_value),
                    /*hoist_temp_variables*/ false,
                    /*skip_initializer*/ false,
                )?;
                let range = self.range_union(&flattened)?;
                let list = self.create_variable_declaration_list(flattened)?;
                self.set_text_range(list, initializer)?;
                self.set_original(list, initializer)?;
                if let Some(range) = range {
                    self.context
                        .arena_mut()?
                        .metadata_mut(list)
                        .set_source_map_range(SourceMapRange::new(self.source, range));
                }
                statements.push(self.create_variable_statement_from_list(list)?);
            } else {
                let declaration_name = match first_original_declaration {
                    Some(declaration) => {
                        let NodeData::VariableDeclaration(data) =
                            &self.context.arena().node(declaration)?.data
                        else {
                            return Err(assembly_kind_error(
                                SyntaxKind::VariableDeclaration,
                                "for-of declaration",
                            ));
                        };
                        data.name
                            .map(|id| self.node(id))
                            .ok_or(assembly_kind_error(SyntaxKind::VariableDeclaration, "name"))?
                    }
                    None => {
                        let binding = self.allocate_temp_binding()?;
                        self.create_generated_identifier(&binding)?
                    }
                };
                let declaration =
                    self.create_variable_declaration_plain(declaration_name, Some(bound_value))?;
                let list = self.create_variable_declaration_list(vec![declaration])?;
                self.set_original(list, initializer)?;
                // ranged moveRangePos(initializer, -1) / moveRangeEnd(initializer, -1)
                // — synthesized-position threading is byte-inert here; the
                // statement takes the initializer range.
                let statement = self.create_variable_statement_from_list(list)?;
                self.set_text_range(statement, initializer)?;
                statements.push(statement);
            }
        } else {
            let assignment = self.create_assignment(initializer, bound_value)?;
            if self.is_destructuring_assignment(assignment)? {
                let flattened = flatten_destructuring_assignment(
                    self,
                    assignment,
                    FlattenLevel::All,
                    /*needs_value*/ false,
                    /*use_assignment_completion*/ false,
                )?;
                statements.push(self.create_expression_statement(flattened)?);
            } else {
                // setTextRangeEnd(assignment, initializer.end)
                self.set_text_range(assignment, initializer)?;
                let visited = self.visit_required_expression(assignment)?;
                let statement = self.create_expression_statement(visited)?;
                self.set_text_range(statement, initializer)?;
                statements.push(statement);
            }
        }
        if let Some(converted) = converted_loop_body_statements {
            statements.extend_from_slice(converted);
            return self.create_synthetic_block_for_converted_statements(statements);
        }
        let statement =
            statement.ok_or(assembly_kind_error(SyntaxKind::ForOfStatement, "statement"))?;
        let visited_statement = self
            .visit_statement_lifted(statement)?
            .ok_or(assembly_kind_error(SyntaxKind::ForOfStatement, "body"))?;
        if self.kind(visited_statement)? == SyntaxKind::Block {
            let inner = {
                let NodeData::Block(data) = &self.context.arena().node(visited_statement)?.data
                else {
                    return Err(assembly_kind_error(SyntaxKind::Block, "for-of body"));
                };
                self.array_nodes(data.statements)?
            };
            let mut combined = statements;
            combined.extend(inner);
            let array = {
                let source = self.source;
                self.context
                    .factory()?
                    .create_node_array(source, combined)?
            };
            let updated_data = NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(array.array()),
            });
            let flags = flags_after_update(self.context.arena(), visited_statement, &updated_data)?;
            self.context
                .factory()?
                .update_node(visited_statement, updated_data, flags)
        } else {
            statements.push(visited_statement);
            self.create_synthetic_block_for_converted_statements(statements)
        }
    }

    /// tsc-port: createSyntheticBlockForConvertedStatements @6.0.3
    /// tsc-hash: 1bb56192da2351cc83b39761bedbe1437ec3cb21317e0986bac41d5c21f8b942
    /// tsc-span: _tsc.js:106656-106665
    fn create_synthetic_block_for_converted_statements(
        &mut self,
        statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let block = self.create_block(statements, /*multi_line*/ true)?;
        self.add_emit_flags(
            block,
            EmitFlags::NO_SOURCE_MAP | EmitFlags::NO_TOKEN_SOURCE_MAPS,
        )?;
        Ok(block)
    }

    /// tsc-port: convertForOfStatementForArray @6.0.3
    /// tsc-hash: a31a359ace6132e5eb162c28a9a6899fdd8e750503e48c1a6ee4369762136a81
    /// tsc-span: _tsc.js:106666-106725
    fn convert_for_of_statement_for_array(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        converted_loop_body_statements: Option<&[TransformNode]>,
    ) -> Result<TransformNode, TransformError> {
        let expression = {
            let NodeData::ForOfStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::ForOfStatement, "for-of"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(
                    SyntaxKind::ForOfStatement,
                    "expression",
                ))?
        };
        let visited_expression = self.visit_required_expression(expression)?;
        let counter_binding = self.allocate_loop_variable_binding()?;
        let rhs_reference: TransformNode;
        let rhs_binding: Option<TargetBinding>;
        if matches!(
            self.context.arena().node(visited_expression)?.data,
            NodeData::Identifier(_)
        ) {
            rhs_reference = self.get_generated_name_for_node(visited_expression)?;
            rhs_binding = None;
        } else {
            let binding = self.allocate_temp_binding()?;
            rhs_reference = self.create_generated_identifier(&binding)?;
            rhs_binding = Some(binding);
        }
        let existing_flags = self.emit_flags(visited_expression);
        self.add_emit_flags(
            visited_expression,
            EmitFlags::NO_SOURCE_MAP | existing_flags,
        )?;
        // initializer list: var _i = 0, _a = expr;
        let counter_init = self.create_generated_identifier(&counter_binding)?;
        let zero = self.create_numeric_literal("0")?;
        let counter_declaration =
            self.create_variable_declaration_plain(counter_init, Some(zero))?;
        let rhs_declaration =
            self.create_variable_declaration_plain(rhs_reference, Some(visited_expression))?;
        self.set_text_range(rhs_declaration, expression)?;
        let init_list =
            self.create_variable_declaration_list(vec![counter_declaration, rhs_declaration])?;
        self.set_text_range(init_list, expression)?;
        self.add_emit_flags(init_list, EmitFlags::NO_HOISTING)?;
        // condition: _i < _a.length
        let counter_cond = self.create_generated_identifier(&counter_binding)?;
        let rhs_cond = match &rhs_binding {
            Some(binding) => self.create_generated_identifier(binding)?,
            None => self.get_generated_name_for_node(visited_expression)?,
        };
        let length_access = self.create_property_access_text(rhs_cond, "length")?;
        let condition = self.create_less_than(counter_cond, length_access)?;
        self.set_text_range(condition, expression)?;
        // incrementor: _i++
        let counter_incr = self.create_generated_identifier(&counter_binding)?;
        let incrementor = self.create_postfix_increment(counter_incr)?;
        self.set_text_range(incrementor, expression)?;
        // bound value: _a[_i]
        let rhs_body = match &rhs_binding {
            Some(binding) => self.create_generated_identifier(binding)?,
            None => self.get_generated_name_for_node(visited_expression)?,
        };
        let counter_body = self.create_generated_identifier(&counter_binding)?;
        let bound_value = self.create_element_access(rhs_body, counter_body)?;
        let body =
            self.convert_for_of_statement_head(node, bound_value, converted_loop_body_statements)?;
        let for_statement = {
            let source = self.source;
            let flags = self.child_flags(&[init_list, condition, incrementor, body])?;
            self.context.factory()?.create_node(
                source,
                NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                    statement: Some(body.node()),
                    initializer: Some(init_list.node()),
                    condition: Some(condition.node()),
                    incrementor: Some(incrementor.node()),
                }),
                flags,
            )?
        };
        self.set_text_range(for_statement, node)?;
        self.add_emit_flags(for_statement, EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS)?;
        let reset = self.converted_loop_state.is_some();
        self.restore_enclosing_label(for_statement, outermost_labeled_statement, reset)
    }

    /// tsc-port: convertForOfStatementForIterable @6.0.3
    /// tsc-hash: f82ef9afe08d89403b8995a26cb5ebeda48ce9fcca7236238fd35c689265f0c2
    /// tsc-span: _tsc.js:106726-106866
    fn convert_for_of_statement_for_iterable(
        &mut self,
        node: TransformNode,
        outermost_labeled_statement: Option<TransformNode>,
        converted_loop_body_statements: Option<&[TransformNode]>,
        ancestor_facts: HierarchyFacts,
    ) -> Result<TransformNode, TransformError> {
        let expression = {
            let NodeData::ForOfStatement(data) = &self.context.arena().node(node)?.data else {
                return Err(assembly_kind_error(SyntaxKind::ForOfStatement, "for-of"));
            };
            data.expression
                .map(|id| self.node(id))
                .ok_or(assembly_kind_error(
                    SyntaxKind::ForOfStatement,
                    "expression",
                ))?
        };
        let visited_expression = self.visit_required_expression(expression)?;
        let expression_is_identifier = matches!(
            self.context.arena().node(visited_expression)?.data,
            NodeData::Identifier(_)
        );
        // iterator: getGeneratedNameForNode(expr-ident) or temp; result:
        // getGeneratedNameForNode(iterator) or temp.
        let (iterator_reference, iterator_key): (TransformNode, TransformNode) =
            if expression_is_identifier {
                let reference = self.get_generated_name_for_node(visited_expression)?;
                (reference, reference)
            } else {
                let binding = self.allocate_temp_binding()?;
                let reference = self.create_generated_identifier(&binding)?;
                (reference, reference)
            };
        let result_reference: TransformNode = if expression_is_identifier {
            self.get_generated_name_for_node(iterator_key)?
        } else {
            let binding = self.allocate_temp_binding()?;
            self.create_generated_identifier(&binding)?
        };
        let error_binding = self.allocate_numbered_binding("e")?;
        let error_record = self.create_generated_identifier(&error_binding)?;
        let catch_variable = self.get_generated_name_for_node(error_record)?;
        let return_method_binding = self.allocate_temp_binding()?;
        let values_call = self.create_values_helper_call(visited_expression)?;
        self.set_text_range(values_call, expression)?;
        let next_call = {
            let next_access = self.create_property_access_text(iterator_reference, "next")?;
            self.create_call(next_access, vec![])?
        };
        // hoistVariableDeclaration(errorRecord); hoistVariableDeclaration(returnMethod);
        {
            let hoist_error = self.create_generated_identifier(&error_binding)?;
            self.context.hoist_variable_declaration(hoist_error)?;
            let hoist_return = self.create_generated_identifier(&return_method_binding)?;
            self.context.hoist_variable_declaration(hoist_return)?;
        }
        let initializer = if ancestor_facts.intersects(HierarchyFacts::ITERATION_CONTAINER) {
            let reset_error = self.create_generated_identifier(&error_binding)?;
            let void_zero = self.create_void_zero()?;
            let reset = self.create_assignment(reset_error, void_zero)?;
            self.inline_expressions(vec![reset, values_call])?
        } else {
            values_call
        };
        // for (var x_1 = __values(expr), x_1_1 = x_1.next(); !x_1_1.done; x_1_1 = x_1.next())
        let iterator_declaration = {
            let declaration =
                self.create_variable_declaration_plain(iterator_reference, Some(initializer))?;
            self.set_text_range(declaration, expression)?;
            declaration
        };
        let result_declaration =
            self.create_variable_declaration_plain(result_reference, Some(next_call))?;
        let init_list =
            self.create_variable_declaration_list(vec![iterator_declaration, result_declaration])?;
        self.set_text_range(init_list, expression)?;
        self.add_emit_flags(init_list, EmitFlags::NO_HOISTING)?;
        let condition = {
            let result_cond = self.clone_node(result_reference)?;
            let done_access = self.create_property_access_text(result_cond, "done")?;
            self.create_logical_not(done_access)?
        };
        let incrementor = {
            let result_incr = self.clone_node(result_reference)?;
            let iterator_incr = self.clone_node(iterator_reference)?;
            let next_access = self.create_property_access_text(iterator_incr, "next")?;
            let next_call = self.create_call(next_access, vec![])?;
            self.create_assignment(result_incr, next_call)?
        };
        let bound_value = {
            let result_body = self.clone_node(result_reference)?;
            self.create_property_access_text(result_body, "value")?
        };
        let body =
            self.convert_for_of_statement_head(node, bound_value, converted_loop_body_statements)?;
        let for_statement = {
            let source = self.source;
            let flags = self.child_flags(&[init_list, condition, incrementor, body])?;
            self.context.factory()?.create_node(
                source,
                NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                    statement: Some(body.node()),
                    initializer: Some(init_list.node()),
                    condition: Some(condition.node()),
                    incrementor: Some(incrementor.node()),
                }),
                flags,
            )?
        };
        self.set_text_range(for_statement, node)?;
        self.add_emit_flags(for_statement, EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS)?;
        let reset = self.converted_loop_state.is_some();
        let labeled_for =
            self.restore_enclosing_label(for_statement, outermost_labeled_statement, reset)?;
        let try_block = self.create_block(vec![labeled_for], /*multi_line*/ false)?;
        // catch (e_1_1) { e_1 = { error: e_1_1 }; }
        let catch_clause = {
            let catch_declaration = self.create_variable_declaration_plain(catch_variable, None)?;
            let error_write = self.create_generated_identifier(&error_binding)?;
            let error_property = {
                let catch_reference = self.clone_node(catch_variable)?;
                self.create_property_assignment_text("error", catch_reference)?
            };
            let error_object = self.create_object_literal(vec![error_property], false)?;
            let assignment = self.create_assignment(error_write, error_object)?;
            let statement = self.create_expression_statement(assignment)?;
            let block = self.create_block(vec![statement], /*multi_line*/ false)?;
            self.add_emit_flags(block, EmitFlags::SINGLE_LINE)?;
            let source = self.source;
            let flags = self.child_flags(&[catch_declaration, block])?;
            self.context.factory()?.create_node(
                source,
                NodeData::CatchClause(tsc_syntax::nodes::CatchClauseData {
                    variable_declaration: Some(catch_declaration.node()),
                    block: Some(block.node()),
                }),
                flags,
            )?
        };
        // finally { try { if (x_1_1 && !x_1_1.done && (_a = x_1.return)) _a.call(x_1); } finally { if (e_1) throw e_1.error; } }
        let finally_block = {
            let inner_try_block = {
                let result_check = self.clone_node(result_reference)?;
                let result_done = {
                    let result_clone = self.clone_node(result_reference)?;
                    let done = self.create_property_access_text(result_clone, "done")?;
                    self.create_logical_not(done)?
                };
                let and_left = self.create_logical_and(result_check, result_done)?;
                let return_write = self.create_generated_identifier(&return_method_binding)?;
                let return_access = {
                    let iterator_clone = self.clone_node(iterator_reference)?;
                    self.create_property_access_text(iterator_clone, "return")?
                };
                let return_assignment = self.create_assignment(return_write, return_access)?;
                let condition = self.create_logical_and(and_left, return_assignment)?;
                let call = {
                    let return_reference =
                        self.create_generated_identifier(&return_method_binding)?;
                    let iterator_clone = self.clone_node(iterator_reference)?;
                    self.create_function_call_call(return_reference, iterator_clone, vec![])?
                };
                let call_statement = self.create_expression_statement(call)?;
                let if_statement = self.create_if_statement(condition, call_statement, None)?;
                self.add_emit_flags(if_statement, EmitFlags::SINGLE_LINE)?;
                self.create_block(vec![if_statement], /*multi_line*/ false)?
            };
            let inner_finally_block = {
                let error_check = self.create_generated_identifier(&error_binding)?;
                let throw_statement = {
                    let error_reference = self.create_generated_identifier(&error_binding)?;
                    let error_access =
                        self.create_property_access_text(error_reference, "error")?;
                    let source = self.source;
                    let flags = self.child_flags(&[error_access])?;
                    self.context.factory()?.create_node(
                        source,
                        NodeData::ThrowStatement(tsc_syntax::nodes::ThrowStatementData {
                            expression: Some(error_access.node()),
                        }),
                        flags,
                    )?
                };
                let if_statement = self.create_if_statement(error_check, throw_statement, None)?;
                self.add_emit_flags(if_statement, EmitFlags::SINGLE_LINE)?;
                let block = self.create_block(vec![if_statement], /*multi_line*/ false)?;
                self.add_emit_flags(block, EmitFlags::SINGLE_LINE)?;
                block
            };
            let inner_try = {
                let source = self.source;
                let flags = self.child_flags(&[inner_try_block, inner_finally_block])?;
                self.context.factory()?.create_node(
                    source,
                    NodeData::TryStatement(tsc_syntax::nodes::TryStatementData {
                        try_block: Some(inner_try_block.node()),
                        catch_clause: None,
                        finally_block: Some(inner_finally_block.node()),
                    }),
                    flags,
                )?
            };
            self.create_block(vec![inner_try], /*multi_line*/ false)?
        };
        let source = self.source;
        let flags = self.child_flags(&[try_block, catch_clause, finally_block])?;
        self.context.factory()?.create_node(
            source,
            NodeData::TryStatement(tsc_syntax::nodes::TryStatementData {
                try_block: Some(try_block.node()),
                catch_clause: Some(catch_clause.node()),
                finally_block: Some(finally_block.node()),
            }),
            flags,
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/es2015/tests.rs"]
mod tests;
