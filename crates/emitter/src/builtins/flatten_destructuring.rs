//! H2.5h-b B-2: the shared destructuring flattener.
//!
//! Function-per-function port of tsc's `destructuring.ts` family as bundled
//! at `_tsc.js:93251-93697` (the 18-function `destructuring-flattener`
//! shared module frozen in `ratchets/h2-5h-a-owner-graph.v1.json`), with
//! both `FlattenLevel` arms. The module is reached from `transformES2015`
//! only (owner-graph edge `destructuring-shared-module`); until the B-4/B-5
//! owners land, the only callers are the focused projection suite below.
//! The active ObjectRestSpread production path stays the independent
//! plan-based lowering in `es2018.rs` (packet
//! `docs/design/greenfield/slices/h2-5h-b-b-2.md` §12.3).

use tsc_syntax::{NodeData, SyntaxKind};

use crate::{
    factory::EmitHelperName, SourceRange, TransformError, TransformFlags, TransformNode,
    TransformSourceId, TransformationContext,
};

use super::{generated_bindings::GeneratedBindingScopes, helpers, target_bindings::TargetBinding};

/// tsc's `FlattenLevel` (bundler-inlined at every use site: `0 /* All */`,
/// `1 /* ObjectRest */`; the family compares `level >= 1` at
/// `_tsc.js:93499/:93556` and `level < 1` at `:93534/:93548`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // constructed by the B-4/B-5 owners and the focused suite
pub(super) enum FlattenLevel {
    All,
    ObjectRest,
}

impl FlattenLevel {
    const fn is_object_rest(self) -> bool {
        matches!(self, Self::ObjectRest)
    }
}

/// Which closure set drives pattern reconstruction: tsc installs the
/// `makeArray/ObjectBindingPattern` + `makeBindingElement` constructors
/// in `flattenDestructuringBinding`, and the assignment constructors
/// (`makeArray/ObjectAssignmentPattern` + `makeAssignmentElement`) in
/// `flattenDestructuringAssignment`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FlattenPatternKind {
    Binding,
    Assignment,
}

/// The consumer seam. tsc passes `(visitor, context)` into the family and
/// reads `context.getCompilerOptions().downlevelIteration` once per flatten;
/// the Rust port folds both into one host: the B-4 `Es2015Visitor` and the
/// focused-suite driver each own a `TransformationContext`, a
/// `GeneratedBindingScopes`, the option snapshot, and the visitation
/// policy.
pub(super) trait FlattenHost {
    fn context(&mut self) -> &mut TransformationContext;
    fn context_ref(&self) -> &TransformationContext;
    fn flatten_source(&self) -> TransformSourceId;
    fn downlevel_iteration(&self) -> bool;
    fn generated_bindings(&mut self) -> &mut GeneratedBindingScopes;
    /// `Debug.checkDefined(visitNode(node, visitor, isExpression))`.
    fn visit_expression(&mut self, node: TransformNode) -> Result<TransformNode, TransformError>;
    /// `visitNode(node, visitor, isBindingOrAssignmentElement)` — the
    /// ObjectRest retained-chunk arm only.
    fn visit_binding_or_assignment_element(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError>;
    /// tsc's `createAssignmentCallback` parameter. The default is the
    /// standard-assignment arm; module-transform owners
    /// (`createAllExportExpressions`) override at their own slices.
    fn create_assignment_completion(
        &mut self,
        _target: TransformNode,
        _value: TransformNode,
        _location: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(None)
    }
}

/// One pending variable declaration of the binding flatten
/// (`pendingDeclarations` records: `{pendingExpressions, name, value,
/// location, original}`). `pending_expressions` is always empty at push
/// time (the sink folds them into `value` first); only the trailing-temp
/// arm appends to the LAST record afterwards (`_tsc.js:93412-93421`).
#[derive(Debug)]
struct PendingFlattenDeclaration {
    pending_expressions: Vec<TransformNode>,
    name: TransformNode,
    value: TransformNode,
    location: Option<TransformNode>,
    original: Option<TransformNode>,
}

/// The flatten-context closure record (`_tsc.js:93266-93277` /
/// `:93362-93373`): level, `downlevelIteration`, `hoistTempVariables`,
/// `hasTransformedPriorElement`, the constructor set, and the emit sinks.
#[derive(Debug)]
struct FlattenContext {
    level: FlattenLevel,
    downlevel_iteration: bool,
    hoist_temp_variables: bool,
    has_transformed_prior_element: bool,
    kind: FlattenPatternKind,
    /// `createAssignmentCallback` present (assignment flatten only): the
    /// emit sink asserts an identifier target and delegates to the host.
    use_assignment_completion: bool,
    pending_expressions: Vec<TransformNode>,
    pending_declarations: Vec<PendingFlattenDeclaration>,
    expressions: Vec<TransformNode>,
}

impl FlattenContext {
    fn new(
        kind: FlattenPatternKind,
        level: FlattenLevel,
        downlevel_iteration: bool,
        hoist_temp_variables: bool,
        use_assignment_completion: bool,
    ) -> Self {
        Self {
            level,
            downlevel_iteration,
            hoist_temp_variables,
            has_transformed_prior_element: false,
            kind,
            use_assignment_completion,
            pending_expressions: Vec::new(),
            pending_declarations: Vec::new(),
            expressions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// The two entries
// ---------------------------------------------------------------------------

/// tsc-port: flattenDestructuringAssignment @6.0.3
/// tsc-hash: 8303d862131f74b895085ac8968b52d5d0267330e000e0e91546757aaf278ee0
/// tsc-span: _tsc.js:93251-93328
#[allow(dead_code)] // production consumers arrive with the B-4/B-5 owners
pub(super) fn flatten_destructuring_assignment<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
    level: FlattenLevel,
    needs_value: bool,
    use_assignment_completion: bool,
) -> Result<TransformNode, TransformError> {
    let mut location = node;
    let mut node = node;
    let mut value = None;
    if is_destructuring_assignment(host, node)? {
        value = Some(binary_right(host, node)?);
        loop {
            let left = binary_left(host, node)?;
            if !(is_empty_array_literal(host, left)? || is_empty_object_literal(host, left)?) {
                break;
            }
            let unwrapped = value.expect("destructuring assignment carries a right side");
            if is_destructuring_assignment(host, unwrapped)? {
                location = unwrapped;
                node = unwrapped;
                value = Some(binary_right(host, node)?);
            } else {
                return host.visit_expression(unwrapped);
            }
        }
    }
    let mut fx = FlattenContext::new(
        FlattenPatternKind::Assignment,
        level,
        host.downlevel_iteration(),
        /*hoist_temp_variables*/ true,
        use_assignment_completion,
    );
    if let Some(raw) = value {
        let mut visited = host.visit_expression(raw)?;
        let collides = match &host.context_ref().arena().node(visited)?.data {
            NodeData::Identifier(data) => {
                let text = data.text.clone();
                binding_or_assignment_element_assigns_to_name(host, node, &text)?
            }
            _ => false,
        };
        if collides || binding_or_assignment_element_contains_non_literal_computed_name(host, node)?
        {
            visited = ensure_identifier(host, &mut fx, visited, false, Some(location))?;
        } else if needs_value {
            visited = ensure_identifier(host, &mut fx, visited, true, Some(location))?;
        } else if node_is_synthesized(host, node)? {
            location = visited;
        }
        value = Some(visited);
    }
    flatten_binding_or_assignment_element(
        host,
        &mut fx,
        node,
        value,
        Some(location),
        /*skip_initializer*/ is_destructuring_assignment(host, node)?,
    )?;
    if let Some(value) = value {
        if needs_value {
            if fx.expressions.is_empty() {
                return Ok(value);
            }
            fx.expressions.push(value);
        }
    }
    if fx.expressions.is_empty() {
        // Upstream reaches `inlineExpressions(undefined)` here and crashes
        // (`_tsc.js:93326`, reproduced on 6.0.3 with `[,,] = x;`); the
        // `|| createOmittedExpression()` fallback is dead. Typed fail-closed
        // arm per the packet's §11 no-fallback rule.
        return Err(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "flattened assignment expressions",
        });
    }
    inline_expressions(host, fx.expressions)
}

/// tsc-port: flattenDestructuringBinding @6.0.3
/// tsc-hash: ab53debce805e94e3a0f018f2bdf6d7724e4439fdd1ee40898b06059b8d6681e
/// tsc-span: _tsc.js:93358-93448
#[allow(dead_code)] // production consumers arrive with the B-4/B-5 owners
pub(super) fn flatten_destructuring_binding<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
    level: FlattenLevel,
    rval: Option<TransformNode>,
    hoist_temp_variables: bool,
    skip_initializer: bool,
) -> Result<Vec<TransformNode>, TransformError> {
    let mut fx = FlattenContext::new(
        FlattenPatternKind::Binding,
        level,
        host.downlevel_iteration(),
        hoist_temp_variables,
        /*use_assignment_completion*/ false,
    );
    let mut node = node;
    if matches!(
        host.context_ref().arena().node(node)?.kind,
        SyntaxKind::VariableDeclaration
    ) {
        if let Some(initializer) = get_initializer_of_binding_or_assignment_element(host, node)? {
            let collides = match &host.context_ref().arena().node(initializer)?.data {
                NodeData::Identifier(data) => {
                    let text = data.text.clone();
                    binding_or_assignment_element_assigns_to_name(host, node, &text)?
                }
                _ => false,
            };
            if collides
                || binding_or_assignment_element_contains_non_literal_computed_name(host, node)?
            {
                let visited = host.visit_expression(initializer)?;
                let ensured = ensure_identifier(host, &mut fx, visited, false, Some(initializer))?;
                let NodeData::VariableDeclaration(data) =
                    host.context_ref().arena().node(node)?.data.clone()
                else {
                    unreachable!("kind checked above");
                };
                let updated = tsc_syntax::nodes::VariableDeclarationData {
                    name: data.name,
                    exclamation_token: None,
                    r#type: None,
                    initializer: Some(ensured.node()),
                };
                let flags = super::flags_after_update(
                    host.context_ref().arena(),
                    node,
                    &NodeData::VariableDeclaration(updated.clone()),
                )?;
                node = host.context().factory()?.update_node(
                    node,
                    NodeData::VariableDeclaration(updated),
                    flags,
                )?;
            }
        }
    }
    flatten_binding_or_assignment_element(host, &mut fx, node, rval, Some(node), skip_initializer)?;
    if !fx.pending_expressions.is_empty() {
        if fx.hoist_temp_variables {
            let pending = std::mem::take(&mut fx.pending_expressions);
            let value = inline_expressions(host, pending)?;
            let temp = allocate_flatten_temp(host, /*hoist*/ false)?;
            let target = create_generated_identifier(host, &temp)?;
            emit_binding_or_assignment(host, &mut fx, target, value, None, None)?;
        } else {
            let temp = allocate_flatten_temp(host, /*hoist*/ true)?;
            let last =
                fx.pending_declarations
                    .last_mut()
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::VariableDeclarationList,
                        field: "pending declaration for trailing temp",
                    })?;
            let last_value = last.value;
            let read = create_generated_identifier(host, &temp)?;
            let assignment = create_assignment(host, read, last_value)?;
            let last = fx.pending_declarations.last_mut().expect("checked above");
            last.pending_expressions.push(assignment);
            let pending = std::mem::take(&mut fx.pending_expressions);
            let last = fx.pending_declarations.last_mut().expect("checked above");
            last.pending_expressions.extend(pending);
            last.value = create_generated_identifier(host, &temp)?;
        }
    }
    let pending = std::mem::take(&mut fx.pending_declarations);
    let mut declarations = Vec::with_capacity(pending.len());
    for record in pending {
        let initializer = if record.pending_expressions.is_empty() {
            record.value
        } else {
            let mut expressions = record.pending_expressions;
            expressions.push(record.value);
            inline_expressions(host, expressions)?
        };
        let declaration = create_variable_declaration(host, record.name, Some(initializer))?;
        if let Some(original) = record.original {
            host.context()
                .arena_mut()?
                .set_original_node(declaration, Some(original))?;
        }
        if let Some(location) = record.location {
            host.context()
                .factory()?
                .set_text_range(declaration, location)?;
        }
        declarations.push(declaration);
    }
    Ok(declarations)
}

// ---------------------------------------------------------------------------
// The emit sinks and `ensureIdentifier`
// ---------------------------------------------------------------------------

fn emit_expression(fx: &mut FlattenContext, expression: TransformNode) {
    match fx.kind {
        FlattenPatternKind::Assignment => fx.expressions.push(expression),
        FlattenPatternKind::Binding => fx.pending_expressions.push(expression),
    }
}

fn emit_binding_or_assignment<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    target: TransformNode,
    value: TransformNode,
    location: Option<TransformNode>,
    original: Option<TransformNode>,
) -> Result<(), TransformError> {
    match fx.kind {
        FlattenPatternKind::Binding => {
            let kind = host.context_ref().arena().node(target)?.kind;
            if !matches!(
                kind,
                SyntaxKind::Identifier
                    | SyntaxKind::ObjectBindingPattern
                    | SyntaxKind::ArrayBindingPattern
            ) {
                return Err(TransformError::RequiredChildRemoved {
                    parent: kind,
                    field: "binding name target",
                });
            }
            let value = if fx.pending_expressions.is_empty() {
                value
            } else {
                let mut expressions = std::mem::take(&mut fx.pending_expressions);
                expressions.push(value);
                inline_expressions(host, expressions)?
            };
            fx.pending_declarations.push(PendingFlattenDeclaration {
                pending_expressions: Vec::new(),
                name: target,
                value,
                location,
                original,
            });
        }
        FlattenPatternKind::Assignment => {
            let expression = if fx.use_assignment_completion {
                let kind = host.context_ref().arena().node(target)?.kind;
                if kind != SyntaxKind::Identifier {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: kind,
                        field: "identifier assignment-callback target",
                    });
                }
                host.create_assignment_completion(target, value, location)?
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "assignment-callback completion",
                    })?
            } else {
                let visited = host.visit_expression(target)?;
                let assignment = create_assignment(host, visited, value)?;
                if let Some(location) = location {
                    host.context()
                        .factory()?
                        .set_text_range(assignment, location)?;
                }
                assignment
            };
            if let Some(original) = original {
                host.context()
                    .arena_mut()?
                    .set_original_node(expression, Some(original))?;
            }
            emit_expression(fx, expression);
        }
    }
    Ok(())
}

/// tsc-port: ensureIdentifier @6.0.3
/// tsc-hash: 0930118fbbb030e6f038a1c1ad35661bb7f4656a7801d78014b5f480702c14bb
/// tsc-span: _tsc.js:93650-93672
fn ensure_identifier<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    value: TransformNode,
    reuse_identifier_expressions: bool,
    location: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    if reuse_identifier_expressions
        && host.context_ref().arena().node(value)?.kind == SyntaxKind::Identifier
    {
        return Ok(value);
    }
    let temp = allocate_flatten_temp(host, fx.hoist_temp_variables)?;
    if fx.hoist_temp_variables {
        let target = create_generated_identifier(host, &temp)?;
        let assignment = create_assignment(host, target, value)?;
        if let Some(location) = location {
            host.context()
                .factory()?
                .set_text_range(assignment, location)?;
        }
        emit_expression(fx, assignment);
    } else {
        let target = create_generated_identifier(host, &temp)?;
        emit_binding_or_assignment(host, fx, target, value, location, None)?;
    }
    create_generated_identifier(host, &temp)
}

/// The `createTempVariable(/*recordTempVariable*/ void 0)` +
/// `hoistVariableDeclaration` mapping under the E-NAMES-H eager model
/// (the reviewed `es2018.rs:3531-3548` precedent): a hoisted temp reserves
/// through ancestor scopes and registers a `var` hoist; a pending-declared
/// temp allocates locally and is declared by its own emitted binding.
fn allocate_flatten_temp<H: FlattenHost>(
    host: &mut H,
    hoist: bool,
) -> Result<TargetBinding, TransformError> {
    if hoist {
        let name = host.generated_bindings().allocate_temp();
        let binding = TargetBinding::allocate(host.context(), name)?;
        let declaration = create_generated_identifier(host, &binding)?;
        host.context().hoist_variable_declaration(declaration)?;
        Ok(binding)
    } else {
        let name = host.generated_bindings().allocate_local_temp();
        TargetBinding::allocate(host.context(), name)
    }
}

// ---------------------------------------------------------------------------
// The element walkers
// ---------------------------------------------------------------------------

/// tsc-port: flattenBindingOrAssignmentElement @6.0.3
/// tsc-hash: 63040df542279c36cc444040317fca2d769b2789c020fca8369bd3935334c11f
/// tsc-span: _tsc.js:93449-93485
fn flatten_binding_or_assignment_element<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    element: TransformNode,
    value: Option<TransformNode>,
    location: Option<TransformNode>,
    skip_initializer: bool,
) -> Result<(), TransformError> {
    let binding_target = get_target_of_binding_or_assignment_element(host, element)?.ok_or(
        TransformError::RequiredChildRemoved {
            parent: host.context_ref().arena().node(element)?.kind,
            field: "binding or assignment target",
        },
    )?;
    let mut value = value;
    if !skip_initializer {
        let initializer = get_initializer_of_binding_or_assignment_element(host, element)?
            .map(|initializer| host.visit_expression(initializer))
            .transpose()?;
        if let Some(initializer) = initializer {
            if let Some(current) = value {
                let mut checked =
                    create_default_value_check(host, fx, current, initializer, location)?;
                if !is_simple_inlineable_expression(host, initializer)?
                    && is_binding_or_assignment_pattern(host, binding_target)?
                {
                    checked = ensure_identifier(host, fx, checked, true, location)?;
                }
                value = Some(checked);
            } else {
                value = Some(initializer);
            }
        } else if value.is_none() {
            value = Some(create_void_zero(host)?);
        }
    }
    if is_object_binding_or_assignment_pattern(host, binding_target)? {
        flatten_object_binding_or_assignment_pattern(
            host,
            fx,
            element,
            binding_target,
            value.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ObjectBindingPattern,
                field: "flatten value",
            })?,
            location,
        )
    } else if is_array_binding_or_assignment_pattern(host, binding_target)? {
        flatten_array_binding_or_assignment_pattern(
            host,
            fx,
            element,
            binding_target,
            value.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ArrayBindingPattern,
                field: "flatten value",
            })?,
            location,
        )
    } else {
        emit_binding_or_assignment(
            host,
            fx,
            binding_target,
            value.ok_or(TransformError::RequiredChildRemoved {
                parent: host.context_ref().arena().node(element)?.kind,
                field: "flatten value",
            })?,
            location,
            Some(element),
        )
    }
}

/// tsc-port: flattenObjectBindingOrAssignmentPattern @6.0.3
/// tsc-hash: 8fe5ff016903ce2b5f80496f1016b132a6724907bb2850c53e6bba9b677001c6
/// tsc-span: _tsc.js:93486-93530
fn flatten_object_binding_or_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    parent: TransformNode,
    pattern: TransformNode,
    value: TransformNode,
    location: Option<TransformNode>,
) -> Result<(), TransformError> {
    let elements = get_elements_of_binding_or_assignment_pattern(host, pattern)?;
    let num_elements = elements.len();
    let mut value = value;
    if num_elements != 1 {
        let reuse_identifier_expressions =
            !is_declaration_binding_element(host, parent)? || num_elements != 0;
        value = ensure_identifier(host, fx, value, reuse_identifier_expressions, location)?;
    }
    let mut binding_elements: Vec<TransformNode> = Vec::new();
    let mut computed_temp_variables: Vec<TransformNode> = Vec::new();
    for (i, element) in elements.iter().copied().enumerate() {
        if get_rest_indicator_of_binding_or_assignment_element(host, element)?.is_none() {
            let property_name = get_property_name_of_binding_or_assignment_element(host, element)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: host.context_ref().arena().node(element)?.kind,
                    field: "property name",
                })?;
            let target_flags = get_target_of_binding_or_assignment_element(host, element)?
                .map(|target| host.context_ref().arena().transform_flags(target))
                .unwrap_or(TransformFlags::NONE);
            let element_flags = host.context_ref().arena().transform_flags(element);
            let rest_mask = TransformFlags::CONTAINS_REST_OR_SPREAD
                | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
            if fx.level.is_object_rest()
                && !flags_intersect(element_flags, rest_mask)
                && !flags_intersect(target_flags, rest_mask)
                && !is_computed_property_name(host, property_name)?
            {
                binding_elements.push(host.visit_binding_or_assignment_element(element)?);
            } else {
                if !binding_elements.is_empty() {
                    let chunk = std::mem::take(&mut binding_elements);
                    let chunk_pattern = create_flatten_object_pattern(host, fx.kind, chunk)?;
                    emit_binding_or_assignment(
                        host,
                        fx,
                        chunk_pattern,
                        value,
                        location,
                        Some(pattern),
                    )?;
                }
                let (rhs_value, computed_argument) =
                    create_destructuring_property_access(host, fx, value, property_name)?;
                if let Some(argument) = computed_argument {
                    computed_temp_variables.push(argument);
                }
                flatten_binding_or_assignment_element(
                    host,
                    fx,
                    element,
                    Some(rhs_value),
                    Some(element),
                    false,
                )?;
            }
        } else if i == num_elements - 1 {
            if !binding_elements.is_empty() {
                let chunk = std::mem::take(&mut binding_elements);
                let chunk_pattern = create_flatten_object_pattern(host, fx.kind, chunk)?;
                emit_binding_or_assignment(
                    host,
                    fx,
                    chunk_pattern,
                    value,
                    location,
                    Some(pattern),
                )?;
            }
            let rhs_value =
                create_rest_helper_call(host, value, &elements, &computed_temp_variables, pattern)?;
            flatten_binding_or_assignment_element(
                host,
                fx,
                element,
                Some(rhs_value),
                Some(element),
                false,
            )?;
        }
    }
    if !binding_elements.is_empty() {
        let chunk = std::mem::take(&mut binding_elements);
        let chunk_pattern = create_flatten_object_pattern(host, fx.kind, chunk)?;
        emit_binding_or_assignment(host, fx, chunk_pattern, value, location, Some(pattern))?;
    }
    Ok(())
}

/// tsc-port: flattenArrayBindingOrAssignmentPattern @6.0.3
/// tsc-hash: 7c9aa1d2d4dcc64cfcc543b819b65310d59487c805946c9e90ffba19ed9fc4b8
/// tsc-span: _tsc.js:93531-93601
fn flatten_array_binding_or_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    parent: TransformNode,
    pattern: TransformNode,
    value: TransformNode,
    location: Option<TransformNode>,
) -> Result<(), TransformError> {
    let elements = get_elements_of_binding_or_assignment_pattern(host, pattern)?;
    let num_elements = elements.len();
    let mut value = value;
    if !fx.level.is_object_rest() && fx.downlevel_iteration {
        let count = if num_elements > 0
            && get_rest_indicator_of_binding_or_assignment_element(
                host,
                elements[num_elements - 1],
            )?
            .is_some()
        {
            None
        } else {
            Some(num_elements)
        };
        let read = create_read_helper_call(host, value, count)?;
        if let Some(location) = location {
            host.context().factory()?.set_text_range(read, location)?;
        }
        value = ensure_identifier(host, fx, read, false, location)?;
    } else {
        let mut all_omitted = true;
        for element in &elements {
            if host.context_ref().arena().node(*element)?.kind != SyntaxKind::OmittedExpression {
                all_omitted = false;
                break;
            }
        }
        if (num_elements != 1 && (!fx.level.is_object_rest() || num_elements == 0)) || all_omitted {
            let reuse_identifier_expressions =
                !is_declaration_binding_element(host, parent)? || num_elements != 0;
            value = ensure_identifier(host, fx, value, reuse_identifier_expressions, location)?;
        }
    }
    let mut binding_elements: Vec<TransformNode> = Vec::new();
    let mut rest_containing_elements: Vec<(TransformNode, TransformNode)> = Vec::new();
    for (i, element) in elements.iter().copied().enumerate() {
        if fx.level.is_object_rest() {
            let element_flags = host.context_ref().arena().transform_flags(element);
            if element_flags.contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
                || (fx.has_transformed_prior_element
                    && !is_simple_binding_or_assignment_element(host, element)?)
            {
                fx.has_transformed_prior_element = true;
                let temp = allocate_flatten_temp(host, fx.hoist_temp_variables)?;
                let reference = create_generated_identifier(host, &temp)?;
                rest_containing_elements.push((reference, element));
                let placeholder = create_generated_identifier(host, &temp)?;
                binding_elements.push(create_flatten_array_element(host, fx.kind, placeholder)?);
            } else {
                binding_elements.push(element);
            }
        } else if host.context_ref().arena().node(element)?.kind == SyntaxKind::OmittedExpression {
            continue;
        } else if get_rest_indicator_of_binding_or_assignment_element(host, element)?.is_none() {
            let index = create_numeric_literal(host, &i.to_string())?;
            let rhs_value = create_element_access(host, value, index)?;
            flatten_binding_or_assignment_element(
                host,
                fx,
                element,
                Some(rhs_value),
                Some(element),
                false,
            )?;
        } else if i == num_elements - 1 {
            let rhs_value = create_array_slice_call(host, value, i)?;
            flatten_binding_or_assignment_element(
                host,
                fx,
                element,
                Some(rhs_value),
                Some(element),
                false,
            )?;
        }
    }
    if !binding_elements.is_empty() {
        let chunk = std::mem::take(&mut binding_elements);
        let chunk_pattern = create_flatten_array_pattern(host, fx.kind, chunk)?;
        emit_binding_or_assignment(host, fx, chunk_pattern, value, location, Some(pattern))?;
    }
    for (reference, element) in rest_containing_elements {
        flatten_binding_or_assignment_element(
            host,
            fx,
            element,
            Some(reference),
            Some(element),
            false,
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// In-family predicates
// ---------------------------------------------------------------------------

/// tsc-port: bindingOrAssignmentElementAssignsToName @6.0.3
/// tsc-hash: 4a6499b3dfa091b10245389ca3dc6d5a09d821a95fa0d49d5832eb942fededaf
/// tsc-span: _tsc.js:93329-93337
fn binding_or_assignment_element_assigns_to_name<H: FlattenHost>(
    host: &H,
    element: TransformNode,
    name: &str,
) -> Result<bool, TransformError> {
    let Some(target) = get_target_of_binding_or_assignment_element(host, element)? else {
        return Ok(false);
    };
    if is_binding_or_assignment_pattern(host, target)? {
        return binding_or_assignment_pattern_assigns_to_name(host, target, name);
    }
    match &host.context_ref().arena().node(target)?.data {
        NodeData::Identifier(data) => Ok(data.text == name),
        _ => Ok(false),
    }
}

/// tsc-port: bindingOrAssignmentPatternAssignsToName @6.0.3
/// tsc-hash: f44ebdc8793a4c61c9949833b6369a47c3aba4fc2cf40aa6a03f459a51dc0bd8
/// tsc-span: _tsc.js:93338-93346
fn binding_or_assignment_pattern_assigns_to_name<H: FlattenHost>(
    host: &H,
    pattern: TransformNode,
    name: &str,
) -> Result<bool, TransformError> {
    for element in get_elements_of_binding_or_assignment_pattern(host, pattern)? {
        if binding_or_assignment_element_assigns_to_name(host, element, name)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// tsc-port: bindingOrAssignmentElementContainsNonLiteralComputedName @6.0.3
/// tsc-hash: 908c2e74f1688ce04c71afa624f20261f36251a8ed7b2c40ae8d1cb096ef3ef8
/// tsc-span: _tsc.js:93347-93354
fn binding_or_assignment_element_contains_non_literal_computed_name<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<bool, TransformError> {
    if let Some(property_name) =
        try_get_property_name_of_binding_or_assignment_element(host, element)?
    {
        if is_computed_property_name(host, property_name)? {
            let NodeData::ComputedPropertyName(data) =
                host.context_ref().arena().node(property_name)?.data.clone()
            else {
                unreachable!("kind checked above");
            };
            let expression = data
                .expression
                .and_then(|expression| {
                    host.context_ref()
                        .arena()
                        .node_ref(host.flatten_source(), expression)
                })
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let kind = host.context_ref().arena().node(expression)?.kind;
            let literal = kind.value() >= SyntaxKind::FirstLiteralToken.value()
                && kind.value() <= SyntaxKind::LastLiteralToken.value();
            if !literal {
                return Ok(true);
            }
        }
    }
    let Some(target) = get_target_of_binding_or_assignment_element(host, element)? else {
        return Ok(false);
    };
    if is_binding_or_assignment_pattern(host, target)? {
        return binding_or_assignment_pattern_contains_non_literal_computed_name(host, target);
    }
    Ok(false)
}

/// tsc-port: bindingOrAssignmentPatternContainsNonLiteralComputedName @6.0.3
/// tsc-hash: 06f126b6b428017c5b579a14f4c44f61e560ee7bf325e987c928732973f8b6fd
/// tsc-span: _tsc.js:93355-93357
fn binding_or_assignment_pattern_contains_non_literal_computed_name<H: FlattenHost>(
    host: &H,
    pattern: TransformNode,
) -> Result<bool, TransformError> {
    for element in get_elements_of_binding_or_assignment_pattern(host, pattern)? {
        if binding_or_assignment_element_contains_non_literal_computed_name(host, element)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// tsc-port: isSimpleBindingOrAssignmentElement @6.0.3
/// tsc-hash: c32008e16aff4c697941b5aca185f394e97533ad3be405710d2638ae4233b495
/// tsc-span: _tsc.js:93602-93611
fn is_simple_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<bool, TransformError> {
    let Some(target) = get_target_of_binding_or_assignment_element(host, element)? else {
        return Ok(true);
    };
    if host.context_ref().arena().node(target)?.kind == SyntaxKind::OmittedExpression {
        return Ok(true);
    }
    if let Some(property_name) =
        try_get_property_name_of_binding_or_assignment_element(host, element)?
    {
        if !is_property_name_literal(host, property_name)? {
            return Ok(false);
        }
    }
    if let Some(initializer) = get_initializer_of_binding_or_assignment_element(host, element)? {
        if !is_simple_inlineable_expression(host, initializer)? {
            return Ok(false);
        }
    }
    if is_binding_or_assignment_pattern(host, target)? {
        for element in get_elements_of_binding_or_assignment_pattern(host, target)? {
            if !is_simple_binding_or_assignment_element(host, element)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(host.context_ref().arena().node(target)?.kind == SyntaxKind::Identifier)
}

// ---------------------------------------------------------------------------
// Value construction
// ---------------------------------------------------------------------------

/// tsc-port: createDefaultValueCheck @6.0.3
/// tsc-hash: c3af049d742326be9c4a9847bb0cc2cb5945ed96f166bd20432eed847dc07b18
/// tsc-span: _tsc.js:93612-93629
fn create_default_value_check<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    value: TransformNode,
    default_value: TransformNode,
    location: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let value = ensure_identifier(host, fx, value, true, location)?;
    let undefined = create_void_zero(host)?;
    let condition = create_binary(host, value, SyntaxKind::EqualsEqualsEqualsToken, undefined)?;
    create_conditional(host, condition, default_value, value)
}

/// tsc-port: createDestructuringPropertyAccess @6.0.3
/// tsc-hash: d9a831e921e64f0142bcf4826daf4d38068a264ebfba7b35be510fedfa1ed7eb
/// tsc-span: _tsc.js:93630-93649
///
/// Returns the access plus the computed-key temp (the upstream caller reads
/// `rhsValue.argumentExpression` back; the Rust port returns it directly).
fn create_destructuring_property_access<H: FlattenHost>(
    host: &mut H,
    fx: &mut FlattenContext,
    value: TransformNode,
    property_name: TransformNode,
) -> Result<(TransformNode, Option<TransformNode>), TransformError> {
    if is_computed_property_name(host, property_name)? {
        let NodeData::ComputedPropertyName(data) =
            host.context_ref().arena().node(property_name)?.data.clone()
        else {
            unreachable!("kind checked above");
        };
        let expression = data
            .expression
            .and_then(|expression| {
                host.context_ref()
                    .arena()
                    .node_ref(host.flatten_source(), expression)
            })
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        let visited = host.visit_expression(expression)?;
        let argument = ensure_identifier(host, fx, visited, false, Some(property_name))?;
        let access = create_element_access(host, value, argument)?;
        return Ok((access, Some(argument)));
    }
    let kind = host.context_ref().arena().node(property_name)?.kind;
    if matches!(
        kind,
        SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
    ) {
        let argument = clone_property_name_literal(host, property_name)?;
        return Ok((create_element_access(host, value, argument)?, None));
    }
    let text = identifier_text(host, property_name)?;
    let name = create_identifier(host, &text)?;
    Ok((create_property_access(host, value, name)?, None))
}

/// tsc-port: createRestHelper @6.0.3
/// tsc-hash: 9f8d5a4c75d0f742d506008b65adc3929c78b7f8e683adeec95c82a6c3d44106
/// tsc-span: _tsc.js:25784-25823
fn create_rest_helper_call<H: FlattenHost>(
    host: &mut H,
    value: TransformNode,
    elements: &[TransformNode],
    computed_temp_variables: &[TransformNode],
    location: TransformNode,
) -> Result<TransformNode, TransformError> {
    host.context().request_emit_helper(helpers::object_rest())?;
    let mut property_names = Vec::new();
    let mut computed_offset = 0usize;
    for element in &elements[..elements.len().saturating_sub(1)] {
        if let Some(property_name) =
            get_property_name_of_binding_or_assignment_element(host, *element)?
        {
            if is_computed_property_name(host, property_name)? {
                let temp = *computed_temp_variables.get(computed_offset).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "computed temp variable",
                    },
                )?;
                computed_offset += 1;
                let symbol_literal = create_string_literal(host, "symbol")?;
                let type_of = create_typeof(host, temp)?;
                let condition = create_binary(
                    host,
                    type_of,
                    SyntaxKind::EqualsEqualsEqualsToken,
                    symbol_literal,
                )?;
                let empty = create_string_literal(host, "")?;
                let coerced = create_binary(host, temp, SyntaxKind::PlusToken, empty)?;
                property_names.push(create_conditional(host, condition, temp, coerced)?);
            } else {
                property_names.push(create_string_literal_from_property_name(
                    host,
                    property_name,
                )?);
            }
        }
    }
    let names = create_array_literal(host, property_names)?;
    host.context().factory()?.set_text_range(names, location)?;
    let source = host.flatten_source();
    let helper = host
        .context()
        .factory()?
        .create_unscoped_helper_identifier(source, EmitHelperName::Rest)?;
    create_call(host, helper, vec![value, names])
}

/// tsc-port: createReadHelper @6.0.3
/// tsc-hash: f0baad214d517818636a8a2f1391a0f4521fc9b216a40bec37fb32ac179f82f3
/// tsc-span: _tsc.js:25906-25914
fn create_read_helper_call<H: FlattenHost>(
    host: &mut H,
    iterator_record: TransformNode,
    count: Option<usize>,
) -> Result<TransformNode, TransformError> {
    host.context().request_emit_helper(helpers::read())?;
    let source = host.flatten_source();
    let helper = host
        .context()
        .factory()?
        .create_unscoped_helper_identifier(source, EmitHelperName::Read)?;
    let arguments = match count {
        Some(count) => {
            let count = create_numeric_literal(host, &count.to_string())?;
            vec![iterator_record, count]
        }
        None => vec![iterator_record],
    };
    create_call(host, helper, arguments)
}

// ---------------------------------------------------------------------------
// The pattern constructors (the two closure sets)
// ---------------------------------------------------------------------------

fn create_flatten_object_pattern<H: FlattenHost>(
    host: &mut H,
    kind: FlattenPatternKind,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    match kind {
        FlattenPatternKind::Binding => make_object_binding_pattern(host, elements),
        FlattenPatternKind::Assignment => make_object_assignment_pattern(host, elements),
    }
}

fn create_flatten_array_pattern<H: FlattenHost>(
    host: &mut H,
    kind: FlattenPatternKind,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    match kind {
        FlattenPatternKind::Binding => make_array_binding_pattern(host, elements),
        FlattenPatternKind::Assignment => make_array_assignment_pattern(host, elements),
    }
}

fn create_flatten_array_element<H: FlattenHost>(
    host: &mut H,
    kind: FlattenPatternKind,
    name: TransformNode,
) -> Result<TransformNode, TransformError> {
    match kind {
        FlattenPatternKind::Binding => make_binding_element(host, name),
        FlattenPatternKind::Assignment => Ok(make_assignment_element(name)),
    }
}

/// tsc-port: makeArrayBindingPattern @6.0.3
/// tsc-hash: 0d388aba458e4ce89f66e014d433801c2f9b336ca5745c2a439e04a498973fef
/// tsc-span: _tsc.js:93673-93676
fn make_array_binding_pattern<H: FlattenHost>(
    host: &mut H,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    for element in &elements {
        let kind = host.context_ref().arena().node(*element)?.kind;
        if !matches!(
            kind,
            SyntaxKind::BindingElement | SyntaxKind::OmittedExpression
        ) {
            return Err(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "array binding element",
            });
        }
    }
    let source = host.flatten_source();
    let elements = host
        .context()
        .factory()?
        .create_node_array(source, elements)?;
    let flags = host.context_ref().arena().array_transform_flags(elements)
        | TransformFlags::CONTAINS_ES_2015
        | TransformFlags::CONTAINS_BINDING_PATTERN;
    host.context().factory()?.create_node(
        source,
        NodeData::ArrayBindingPattern(tsc_syntax::nodes::ArrayBindingPatternData {
            elements: Some(elements.array()),
        }),
        flags,
    )
}

/// tsc-port: makeArrayAssignmentPattern @6.0.3
/// tsc-hash: f1bd32dfcb0316067e434c2091cb8d51614bf8f81baa20e7e753a53bb91a297d
/// tsc-span: _tsc.js:93677-93680
fn make_array_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let mut converted = Vec::with_capacity(elements.len());
    for element in elements {
        converted.push(convert_to_array_assignment_element(host, element)?);
    }
    let source = host.flatten_source();
    let elements = host
        .context()
        .factory()?
        .create_node_array(source, converted)?;
    let flags = host.context_ref().arena().array_transform_flags(elements);
    host.context().factory()?.create_node(
        source,
        NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
            elements: Some(elements.array()),
        }),
        flags,
    )
}

/// tsc-port: makeObjectBindingPattern @6.0.3
/// tsc-hash: 37da6610c94c9910dc0ac79b03c2dc47bedc7385da2575e7b71061d18e669ad0
/// tsc-span: _tsc.js:93681-93684
fn make_object_binding_pattern<H: FlattenHost>(
    host: &mut H,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    for element in &elements {
        let kind = host.context_ref().arena().node(*element)?.kind;
        if kind != SyntaxKind::BindingElement {
            return Err(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "object binding element",
            });
        }
    }
    let source = host.flatten_source();
    let elements = host
        .context()
        .factory()?
        .create_node_array(source, elements)?;
    let mut flags = host.context_ref().arena().array_transform_flags(elements)
        | TransformFlags::CONTAINS_ES_2015
        | TransformFlags::CONTAINS_BINDING_PATTERN;
    if flags.contains(TransformFlags::CONTAINS_REST_OR_SPREAD) {
        flags |= TransformFlags::CONTAINS_ES_2018 | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
    }
    host.context().factory()?.create_node(
        source,
        NodeData::ObjectBindingPattern(tsc_syntax::nodes::ObjectBindingPatternData {
            elements: Some(elements.array()),
        }),
        flags,
    )
}

/// tsc-port: makeObjectAssignmentPattern @6.0.3
/// tsc-hash: b4b3a87526d8fe5aa891460fdb6d316503a622c7ddbab26cff50af715c91fe87
/// tsc-span: _tsc.js:93685-93688
fn make_object_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let mut converted = Vec::with_capacity(elements.len());
    for element in elements {
        converted.push(convert_to_object_assignment_element(host, element)?);
    }
    let source = host.flatten_source();
    let elements = host
        .context()
        .factory()?
        .create_node_array(source, converted)?;
    let flags = host.context_ref().arena().array_transform_flags(elements);
    host.context().factory()?.create_node(
        source,
        NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
            properties: Some(elements.array()),
        }),
        flags,
    )
}

/// tsc-port: makeBindingElement @6.0.3
/// tsc-hash: 98a1fdd8c589b9944fdae0d7d8a520d5d7105197de1b23f9983e92bbdae3e651
/// tsc-span: _tsc.js:93689-93697
fn make_binding_element<H: FlattenHost>(
    host: &mut H,
    name: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags =
        host.context_ref().arena().propagate_child_flags(name)? | TransformFlags::CONTAINS_ES_2015;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::BindingElement(tsc_syntax::nodes::BindingElementData {
            name: Some(name.node()),
            property_name: None,
            dot_dot_dot_token: None,
            initializer: None,
        }),
        flags,
    )
}

/// tsc-port: makeAssignmentElement @6.0.3
/// tsc-hash: 7851d63da1a407a05b43e09ea5678957490184527f12e4e1e4ed33c5c847ac99
/// tsc-span: _tsc.js:93698-93700
const fn make_assignment_element(name: TransformNode) -> TransformNode {
    name
}

// ---------------------------------------------------------------------------
// The node converters (factory.converters, reached from the two
// make*AssignmentPattern constructors)
// ---------------------------------------------------------------------------

/// tsc-port: convertToArrayAssignmentElement @6.0.3
/// tsc-hash: a53d9e38c4ae559cf9c2cd29ef7b8bb1f189b05f53f47ca88e0ca5228c1c23bb
/// tsc-span: _tsc.js:20716-20732
fn convert_to_array_assignment_element<H: FlattenHost>(
    host: &mut H,
    element: TransformNode,
) -> Result<TransformNode, TransformError> {
    if let NodeData::BindingElement(data) = host.context_ref().arena().node(element)?.data.clone() {
        let name = data
            .name
            .and_then(|name| {
                host.context_ref()
                    .arena()
                    .node_ref(host.flatten_source(), name)
            })
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        if data.dot_dot_dot_token.is_some() {
            require_identifier(host, name)?;
            let flags = host.context_ref().arena().propagate_child_flags(name)?
                | TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_REST_OR_SPREAD;
            let source = host.flatten_source();
            let spread = host.context().factory()?.create_node(
                source,
                NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                    expression: Some(name.node()),
                }),
                flags,
            )?;
            return with_original_and_range(host, spread, element);
        }
        let expression = convert_to_assignment_element_target(host, name)?;
        return match data.initializer {
            Some(initializer) => {
                let initializer = host
                    .context_ref()
                    .arena()
                    .node_ref(host.flatten_source(), initializer)
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BindingElement,
                        field: "initializer",
                    })?;
                let assignment = create_assignment(host, expression, initializer)?;
                with_original_and_range(host, assignment, element)
            }
            None => Ok(expression),
        };
    }
    require_not_binding_shape(host, element, "array assignment element")?;
    Ok(element)
}

/// tsc-port: convertToObjectAssignmentElement @6.0.3
/// tsc-hash: ceda6ad456fc04784db6d09c68f51944a485356b200901d4289907922e469b09
/// tsc-span: _tsc.js:20733-20747
fn convert_to_object_assignment_element<H: FlattenHost>(
    host: &mut H,
    element: TransformNode,
) -> Result<TransformNode, TransformError> {
    if let NodeData::BindingElement(data) = host.context_ref().arena().node(element)?.data.clone() {
        let name = data
            .name
            .and_then(|name| {
                host.context_ref()
                    .arena()
                    .node_ref(host.flatten_source(), name)
            })
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        if data.dot_dot_dot_token.is_some() {
            require_identifier(host, name)?;
            let flags = host.context_ref().arena().propagate_child_flags(name)?
                | TransformFlags::CONTAINS_ES_2018
                | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
            let source = host.flatten_source();
            let spread = host.context().factory()?.create_node(
                source,
                NodeData::SpreadAssignment(tsc_syntax::nodes::SpreadAssignmentData {
                    expression: Some(name.node()),
                }),
                flags,
            )?;
            return with_original_and_range(host, spread, element);
        }
        if let Some(property_name) = data.property_name {
            let property_name = host
                .context_ref()
                .arena()
                .node_ref(host.flatten_source(), property_name)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BindingElement,
                    field: "property name",
                })?;
            let expression = convert_to_assignment_element_target(host, name)?;
            let initializer = match data.initializer {
                Some(initializer) => {
                    let initializer = host
                        .context_ref()
                        .arena()
                        .node_ref(host.flatten_source(), initializer)
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BindingElement,
                            field: "initializer",
                        })?;
                    create_assignment(host, expression, initializer)?
                }
                None => expression,
            };
            let flags = {
                let arena = host.context_ref().arena();
                arena.propagate_child_flags(property_name)?
                    | arena.propagate_child_flags(initializer)?
            };
            let source = host.flatten_source();
            let assignment = host.context().factory()?.create_node(
                source,
                NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                    name: Some(property_name.node()),
                    initializer: Some(initializer.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                }),
                flags,
            )?;
            return with_original_and_range(host, assignment, element);
        }
        require_identifier(host, name)?;
        let initializer = data.initializer.and_then(|initializer| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), initializer)
        });
        let flags = {
            let arena = host.context_ref().arena();
            let mut flags = arena.propagate_child_flags(name)? | TransformFlags::CONTAINS_ES_2015;
            if let Some(initializer) = initializer {
                flags |= arena.propagate_child_flags(initializer)?;
            }
            flags
        };
        let source = host.flatten_source();
        let shorthand = host.context().factory()?.create_node(
            source,
            NodeData::ShorthandPropertyAssignment(
                tsc_syntax::nodes::ShorthandPropertyAssignmentData {
                    name: Some(name.node()),
                    equals_token: None,
                    object_assignment_initializer: initializer.map(TransformNode::node),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                },
            ),
            flags,
        )?;
        return with_original_and_range(host, shorthand, element);
    }
    require_not_binding_shape(host, element, "object assignment element")?;
    Ok(element)
}

/// tsc-port: convertToAssignmentPattern @6.0.3
/// tsc-hash: 63b77454be5b3cb32c2723a1990da4cb9ac84663c7182dcb8c955302aba5b6a9
/// tsc-span: _tsc.js:20748-20757
fn convert_to_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    match host.context_ref().arena().node(node)?.kind {
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ArrayLiteralExpression => {
            convert_to_array_assignment_pattern(host, node)
        }
        SyntaxKind::ObjectBindingPattern | SyntaxKind::ObjectLiteralExpression => {
            convert_to_object_assignment_pattern(host, node)
        }
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "assignment pattern",
        }),
    }
}

/// tsc-port: convertToObjectAssignmentPattern @6.0.3
/// tsc-hash: e6f6e3aa9e2039d2937eea5b5032475643399a980d61a32b504b0a33fd4b9f0d
/// tsc-span: _tsc.js:20758-20769
fn convert_to_object_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    match host.context_ref().arena().node(node)?.kind {
        SyntaxKind::ObjectBindingPattern => {
            let NodeData::ObjectBindingPattern(data) =
                host.context_ref().arena().node(node)?.data.clone()
            else {
                unreachable!("kind checked above");
            };
            let elements = array_nodes(host, data.elements)?;
            let literal = make_object_assignment_pattern(host, elements)?;
            with_original_and_range(host, literal, node)
        }
        SyntaxKind::ObjectLiteralExpression => Ok(node),
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "object assignment pattern",
        }),
    }
}

/// tsc-port: convertToArrayAssignmentPattern @6.0.3
/// tsc-hash: 395755484303c41fbecc2ff8af6fc8509f4b4480fe1bb957772acec107f002ed
/// tsc-span: _tsc.js:20770-20781
fn convert_to_array_assignment_pattern<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    match host.context_ref().arena().node(node)?.kind {
        SyntaxKind::ArrayBindingPattern => {
            let NodeData::ArrayBindingPattern(data) =
                host.context_ref().arena().node(node)?.data.clone()
            else {
                unreachable!("kind checked above");
            };
            let elements = array_nodes(host, data.elements)?;
            let literal = make_array_assignment_pattern(host, elements)?;
            with_original_and_range(host, literal, node)
        }
        SyntaxKind::ArrayLiteralExpression => Ok(node),
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "array assignment pattern",
        }),
    }
}

/// tsc-port: convertToAssignmentElementTarget @6.0.3
/// tsc-hash: d2b8e01bb66232d9ecebbf2b7b8874357bfaf509cc0423b7ed4655c545ab2e0c
/// tsc-span: _tsc.js:20782-20787
fn convert_to_assignment_element_target<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    if matches!(
        host.context_ref().arena().node(node)?.kind,
        SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
    ) {
        return convert_to_assignment_pattern(host, node);
    }
    require_not_binding_shape(host, node, "assignment element target")?;
    Ok(node)
}

// ---------------------------------------------------------------------------
// Ported element accessors
// ---------------------------------------------------------------------------

/// tsc-port: getInitializerOfBindingOrAssignmentElement @6.0.3
/// tsc-hash: aa9009ea650c27b73622759579cf755ef09b53bf651ceb7242d9e7ad2c4b8f86
/// tsc-span: _tsc.js:27739-27764
fn get_initializer_of_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let lift = |id: Option<tsc_syntax::NodeId>| {
        id.and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })
    };
    match &host.context_ref().arena().node(element)?.data {
        NodeData::VariableDeclaration(data) => Ok(lift(data.initializer)),
        NodeData::Parameter(data) => Ok(lift(data.initializer)),
        NodeData::BindingElement(data) => Ok(lift(data.initializer)),
        NodeData::PropertyAssignment(data) => {
            let Some(initializer) = lift(data.initializer) else {
                return Ok(None);
            };
            if is_simple_assignment_expression(host, initializer)? {
                let NodeData::BinaryExpression(binary) =
                    host.context_ref().arena().node(initializer)?.data.clone()
                else {
                    unreachable!("assignment shape checked above");
                };
                Ok(lift(binary.right))
            } else {
                Ok(None)
            }
        }
        NodeData::ShorthandPropertyAssignment(data) => Ok(lift(data.object_assignment_initializer)),
        NodeData::BinaryExpression(data) => {
            if is_simple_assignment_expression(host, element)? {
                Ok(lift(data.right))
            } else {
                Ok(None)
            }
        }
        NodeData::SpreadElement(data) => {
            let Some(expression) = lift(data.expression) else {
                return Ok(None);
            };
            get_initializer_of_binding_or_assignment_element(host, expression)
        }
        _ => Ok(None),
    }
}

/// tsc-port: getTargetOfBindingOrAssignmentElement @6.0.3
/// tsc-hash: 7852fe130a4fdc160138ce8deb2b6fb1fe8ef0c352df36406b65ad6370db4445
/// tsc-span: _tsc.js:27765-27791
fn get_target_of_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let lift = |id: Option<tsc_syntax::NodeId>| {
        id.and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })
    };
    match &host.context_ref().arena().node(element)?.data {
        NodeData::VariableDeclaration(data) => Ok(lift(data.name)),
        NodeData::Parameter(data) => Ok(lift(data.name)),
        NodeData::BindingElement(data) => Ok(lift(data.name)),
        NodeData::PropertyAssignment(data) => {
            let Some(initializer) = lift(data.initializer) else {
                return Ok(None);
            };
            get_target_of_binding_or_assignment_element(host, initializer)
        }
        NodeData::ShorthandPropertyAssignment(data) => Ok(lift(data.name)),
        NodeData::SpreadAssignment(data) => {
            let Some(expression) = lift(data.expression) else {
                return Ok(None);
            };
            get_target_of_binding_or_assignment_element(host, expression)
        }
        NodeData::BinaryExpression(data) => {
            if is_simple_assignment_expression(host, element)? {
                let Some(left) = lift(data.left) else {
                    return Ok(None);
                };
                get_target_of_binding_or_assignment_element(host, left)
            } else {
                Ok(Some(element))
            }
        }
        NodeData::SpreadElement(data) => {
            let Some(expression) = lift(data.expression) else {
                return Ok(None);
            };
            get_target_of_binding_or_assignment_element(host, expression)
        }
        _ => Ok(Some(element)),
    }
}

/// tsc-port: getRestIndicatorOfBindingOrAssignmentElement @6.0.3
/// tsc-hash: b66844eeeecde2db166db5291416739d7de0975983bd9caacca6e6eb484d52d1
/// tsc-span: _tsc.js:27792-27802
fn get_rest_indicator_of_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    match &host.context_ref().arena().node(element)?.data {
        NodeData::Parameter(data) => Ok(data.dot_dot_dot_token.and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })),
        NodeData::BindingElement(data) => Ok(data.dot_dot_dot_token.and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })),
        NodeData::SpreadElement(_) | NodeData::SpreadAssignment(_) => Ok(Some(element)),
        _ => Ok(None),
    }
}

/// tsc-port: getPropertyNameOfBindingOrAssignmentElement @6.0.3
/// tsc-hash: b90bf9d010fe009f8cd6dda3d152946fd09b012e16a586234212eb4d16aa917e
/// tsc-span: _tsc.js:27803-27807
///
/// `None` is legal only for spread assignments (`Debug.assert(!!propertyName
/// || isSpreadAssignment(...))`); every other shape without a name is a
/// typed failure at the caller.
fn get_property_name_of_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let property_name = try_get_property_name_of_binding_or_assignment_element(host, element)?;
    if property_name.is_none()
        && !matches!(
            host.context_ref().arena().node(element)?.data,
            NodeData::SpreadAssignment(_)
        )
    {
        return Err(TransformError::RequiredChildRemoved {
            parent: host.context_ref().arena().node(element)?.kind,
            field: "property name",
        });
    }
    Ok(property_name)
}

/// tsc-port: tryGetPropertyNameOfBindingOrAssignmentElement @6.0.3
/// tsc-hash: 5d9d3b218899be017e5efa4340acc66602553cb3211f87d7983d953426ff42fb
/// tsc-span: _tsc.js:27808-27838
///
/// A computed property name whose expression is a string or numeric literal
/// (`isStringOrNumericLiteral`, exactly kinds 11/9) UNWRAPS to the literal —
/// the reason `{ ["s"]: c }` reads as direct element access with no temp.
fn try_get_property_name_of_binding_or_assignment_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let lift = |id: Option<tsc_syntax::NodeId>| {
        id.and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })
    };
    let unwrap_literal_computed =
        |property_name: TransformNode| -> Result<Option<TransformNode>, TransformError> {
            let node = host.context_ref().arena().node(property_name)?;
            if node.kind == SyntaxKind::PrivateIdentifier {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PrivateIdentifier,
                    field: "binding property name",
                });
            }
            if let NodeData::ComputedPropertyName(data) = &node.data {
                if let Some(expression) = data.expression.and_then(|id| {
                    host.context_ref()
                        .arena()
                        .node_ref(host.flatten_source(), id)
                }) {
                    if matches!(
                        host.context_ref().arena().node(expression)?.kind,
                        SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral
                    ) {
                        return Ok(Some(expression));
                    }
                }
            }
            Ok(Some(property_name))
        };
    match &host.context_ref().arena().node(element)?.data {
        NodeData::BindingElement(data) => {
            if let Some(property_name) = lift(data.property_name) {
                return unwrap_literal_computed(property_name);
            }
        }
        NodeData::PropertyAssignment(data) => {
            if let Some(property_name) = lift(data.name) {
                return unwrap_literal_computed(property_name);
            }
        }
        NodeData::SpreadAssignment(_) => {
            return Ok(None);
        }
        _ => {}
    }
    let target = get_target_of_binding_or_assignment_element(host, element)?;
    if let Some(target) = target {
        // `isPropertyName` (_tsc.js:11984-11987): Identifier |
        // PrivateIdentifier | StringLiteral | NumericLiteral |
        // ComputedPropertyName.
        if matches!(
            host.context_ref().arena().node(target)?.kind,
            SyntaxKind::Identifier
                | SyntaxKind::PrivateIdentifier
                | SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::ComputedPropertyName
        ) {
            return Ok(Some(target));
        }
    }
    Ok(None)
}

/// tsc-port: getElementsOfBindingOrAssignmentPattern @6.0.3
/// tsc-hash: 38a18465bc091e8a8d849a5bed5263a39e737a8908329c4a805c00dee7fc1d62
/// tsc-span: _tsc.js:27843-27852
fn get_elements_of_binding_or_assignment_pattern<H: FlattenHost>(
    host: &H,
    pattern: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let arrays = match &host.context_ref().arena().node(pattern)?.data {
        NodeData::ObjectBindingPattern(data) => data.elements,
        NodeData::ArrayBindingPattern(data) => data.elements,
        NodeData::ArrayLiteralExpression(data) => data.elements,
        NodeData::ObjectLiteralExpression(data) => data.properties,
        _ => None,
    };
    array_nodes(host, arrays)
}

/// tsc-port: isDeclarationBindingElement @6.0.3
/// tsc-hash: 9b9e7fd00908376088df1064569a8fe280be9cc0a55052e0b898c7b0fcf2a105
/// tsc-span: _tsc.js:12106-12114
fn is_declaration_binding_element<H: FlattenHost>(
    host: &H,
    element: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        host.context_ref().arena().node(element)?.kind,
        SyntaxKind::VariableDeclaration | SyntaxKind::Parameter | SyntaxKind::BindingElement
    ))
}

/// tsc-port: isBindingOrAssignmentPattern @6.0.3
/// tsc-hash: fb8c2a2f89f30c8744af2ecb3a77c9ba5e179666f8060c662b579f607bde5713
/// tsc-span: _tsc.js:12118-12120
fn is_binding_or_assignment_pattern<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(is_object_binding_or_assignment_pattern(host, node)?
        || is_array_binding_or_assignment_pattern(host, node)?)
}

/// tsc-port: isObjectBindingOrAssignmentPattern @6.0.3
/// tsc-hash: 4f8f3dfc4d16547ea88fd11c1e14389ff91a382338093400e9bba25fb1d12763
/// tsc-span: _tsc.js:12121-12128
fn is_object_binding_or_assignment_pattern<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        host.context_ref().arena().node(node)?.kind,
        SyntaxKind::ObjectBindingPattern | SyntaxKind::ObjectLiteralExpression
    ))
}

/// tsc-port: isArrayBindingOrAssignmentPattern @6.0.3
/// tsc-hash: f14f2b1eef9741c8ac3dd3d3fc063c4567b278b8de75a6801a97f00835afb6db
/// tsc-span: _tsc.js:12141-12148
fn is_array_binding_or_assignment_pattern<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        host.context_ref().arena().node(node)?.kind,
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ArrayLiteralExpression
    ))
}

/// tsc-port: isSimpleCopiableExpression @6.0.3
/// tsc-hash: 388e8823ae5507fbcabb38b5bbd06c28c38b7381ce8b0eb987dbad1150ed52f1
/// tsc-span: _tsc.js:93027-93029
/// tsc-port: isSimpleInlineableExpression @6.0.3
/// tsc-hash: 75411b5859a6888595a6e090ab2d42fe4f904d7becfe81a855f1b6111fa27cee
/// tsc-span: _tsc.js:93030-93032
///
/// `!isIdentifier(e) && isSimpleCopiableExpression(e)` — string-literal-like,
/// numeric, or keyword expressions (the reviewed `es2018.rs:4574-4590`
/// keyword subset: the corpus-exercised keyword expressions).
fn is_simple_inlineable_expression<H: FlattenHost>(
    host: &H,
    expression: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        host.context_ref().arena().node(expression)?.kind,
        SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::ThisKeyword
    ))
}

/// tsc-port: isPropertyNameLiteral @6.0.3
/// tsc-hash: daae3011f849f003859aaa1373c2cc6c65b1fd0ea3fe264741b6eb3318030f6d
/// tsc-span: _tsc.js:15888-15898
///
/// The `isSimpleBindingOrAssignmentElement` guard: identifier or
/// string/numeric-literal-like property names are "literal".
fn is_property_name_literal<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        host.context_ref().arena().node(node)?.kind,
        SyntaxKind::Identifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
    ))
}

/// tsc-port: isAssignmentExpression @6.0.3
/// tsc-hash: cf13ad3dbebb98bf2a29a53d70992d7a24259be2616df4e4298e9f7ea8bf76fd
/// tsc-span: _tsc.js:17111-17113
///
/// The `excludeCompoundAssignment: true` arm only (every family use).
fn is_simple_assignment_expression<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::BinaryExpression(data) = &host.context_ref().arena().node(node)?.data else {
        return Ok(false);
    };
    let Some(operator) = data.operator_token.and_then(|id| {
        host.context_ref()
            .arena()
            .node_ref(host.flatten_source(), id)
    }) else {
        return Ok(false);
    };
    Ok(host.context_ref().arena().node(operator)?.kind == SyntaxKind::EqualsToken)
}

/// tsc-port: isDestructuringAssignment @6.0.3
/// tsc-hash: 57f11978bed7f73705f836f943b584fbe39823ae01178fff5a5b6b046b44268b
/// tsc-span: _tsc.js:17114-17124
fn is_destructuring_assignment<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    if !is_simple_assignment_expression(host, node)? {
        return Ok(false);
    }
    let NodeData::BinaryExpression(data) = &host.context_ref().arena().node(node)?.data else {
        return Ok(false);
    };
    let Some(left) = data.left.and_then(|id| {
        host.context_ref()
            .arena()
            .node_ref(host.flatten_source(), id)
    }) else {
        return Ok(false);
    };
    Ok(matches!(
        host.context_ref().arena().node(left)?.kind,
        SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
    ))
}

/// tsc-port: isEmptyObjectLiteral @6.0.3
/// tsc-hash: 330be2296e95c47508474b91c1c9678fff171d14291250f0519d81602b234c51
/// tsc-span: _tsc.js:17189-17191
fn is_empty_object_literal<H: FlattenHost>(
    host: &H,
    expression: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::ObjectLiteralExpression(data) =
        &host.context_ref().arena().node(expression)?.data
    else {
        return Ok(false);
    };
    Ok(array_nodes(host, data.properties)?.is_empty())
}

/// tsc-port: isEmptyArrayLiteral @6.0.3
/// tsc-hash: 6cd2789388794ede983ceb3f9dcb4b362bfce3d582b9a63b71c65e7c181a7dcd
/// tsc-span: _tsc.js:17192-17194
fn is_empty_array_literal<H: FlattenHost>(
    host: &H,
    expression: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::ArrayLiteralExpression(data) = &host.context_ref().arena().node(expression)?.data
    else {
        return Ok(false);
    };
    Ok(array_nodes(host, data.elements)?.is_empty())
}

fn is_computed_property_name<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(host.context_ref().arena().node(node)?.kind == SyntaxKind::ComputedPropertyName)
}

/// tsc `nodeIsSynthesized` (position-based: `pos < 0 || end < 0`). The
/// arena's synthetic sentinel is `u32::MAX` on both bounds
/// (`SourceRange::Synthesized`); a mixed raw range is an upstream-invalid
/// state and stays a typed error.
fn node_is_synthesized<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let arena = host.context_ref().arena();
    let source = arena.source(node.source())?;
    let record = arena.node(node)?;
    let range = SourceRange::from_raw(record.pos, record.end, source.syntax().positions())
        .map_err(|error| TransformError::InvalidSourceRange { node, error })?;
    Ok(matches!(range, SourceRange::Synthesized))
}

// ---------------------------------------------------------------------------
// Construction plumbing (the es2018 idioms over the shared factory)
// ---------------------------------------------------------------------------

fn require_identifier<H: FlattenHost>(host: &H, node: TransformNode) -> Result<(), TransformError> {
    let kind = host.context_ref().arena().node(node)?.kind;
    if kind != SyntaxKind::Identifier {
        return Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "identifier",
        });
    }
    Ok(())
}

fn require_not_binding_shape<H: FlattenHost>(
    host: &H,
    node: TransformNode,
    field: &'static str,
) -> Result<(), TransformError> {
    let kind = host.context_ref().arena().node(node)?.kind;
    if matches!(
        kind,
        SyntaxKind::BindingElement
            | SyntaxKind::ObjectBindingPattern
            | SyntaxKind::ArrayBindingPattern
    ) {
        return Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field,
        });
    }
    Ok(())
}

fn with_original_and_range<H: FlattenHost>(
    host: &mut H,
    node: TransformNode,
    original: TransformNode,
) -> Result<TransformNode, TransformError> {
    host.context()
        .arena_mut()?
        .set_original_node(node, Some(original))?;
    host.context().factory()?.set_text_range(node, original)?;
    Ok(node)
}

fn array_nodes<H: FlattenHost>(
    host: &H,
    array: Option<tsc_syntax::NodeArrayId>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(array) = array.and_then(|array| {
        host.context_ref()
            .arena()
            .node_array_ref(host.flatten_source(), array)
    }) else {
        return Ok(Vec::new());
    };
    let nodes = host.context_ref().arena().node_array(array)?.nodes.clone();
    nodes
        .iter()
        .map(|node| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), *node)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SyntaxList,
                    field: "array element",
                })
        })
        .collect()
}

fn create_generated_identifier<H: FlattenHost>(
    host: &mut H,
    binding: &TargetBinding,
) -> Result<TransformNode, TransformError> {
    let identifier = create_identifier(host, binding.provisional_name())?;
    binding.write_generated_metadata(host.context().arena_mut()?, identifier);
    Ok(identifier)
}

fn create_identifier<H: FlattenHost>(
    host: &mut H,
    text: &str,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
            escaped_text: tsc_syntax::escape_leading_underscores(text),
            text: text.to_owned(),
        }),
        TransformFlags::NONE,
    )
}

fn identifier_text<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<String, TransformError> {
    match &host.context_ref().arena().node(node)?.data {
        NodeData::Identifier(data) => Ok(data.text.clone()),
        _ => Err(TransformError::RequiredChildRemoved {
            parent: host.context_ref().arena().node(node)?.kind,
            field: "identifier property name",
        }),
    }
}

fn clone_property_name_literal<H: FlattenHost>(
    host: &mut H,
    property_name: TransformNode,
) -> Result<TransformNode, TransformError> {
    let clone = host.context().factory()?.clone_node(property_name)?;
    if host.context_ref().arena().node(property_name)?.kind == SyntaxKind::StringLiteral {
        host.context()
            .arena_mut()?
            .metadata_mut(clone)
            .set_string_literal_text_source(property_name);
    }
    Ok(clone)
}

fn create_string_literal_from_property_name<H: FlattenHost>(
    host: &mut H,
    property_name: TransformNode,
) -> Result<TransformNode, TransformError> {
    let text = match &host.context_ref().arena().node(property_name)?.data {
        NodeData::Identifier(data) => data.text.clone(),
        NodeData::StringLiteral(data) => data.text.clone(),
        NodeData::NumericLiteral(data) => data.text.clone(),
        NodeData::BigIntLiteral(data) => data.text.clone(),
        _ => {
            return Err(TransformError::RequiredChildRemoved {
                parent: host.context_ref().arena().node(property_name)?.kind,
                field: "literal property name",
            });
        }
    };
    let literal = create_string_literal(host, &text)?;
    if host.context_ref().arena().node(property_name)?.kind == SyntaxKind::StringLiteral {
        host.context()
            .arena_mut()?
            .metadata_mut(literal)
            .set_string_literal_text_source(property_name);
    }
    Ok(literal)
}

fn create_string_literal<H: FlattenHost>(
    host: &mut H,
    text: &str,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
            text: text.to_owned(),
            has_extended_unicode_escape: None,
        }),
        TransformFlags::NONE,
    )
}

fn create_numeric_literal<H: FlattenHost>(
    host: &mut H,
    text: &str,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
            text: text.to_owned(),
        }),
        TransformFlags::NONE,
    )
}

fn create_array_literal<H: FlattenHost>(
    host: &mut H,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    let elements = host
        .context()
        .factory()?
        .create_node_array(source, elements)?;
    let flags = host.context_ref().arena().array_transform_flags(elements);
    host.context().factory()?.create_node(
        source,
        NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
            elements: Some(elements.array()),
        }),
        flags,
    )
}

fn create_element_access<H: FlattenHost>(
    host: &mut H,
    expression: TransformNode,
    argument: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags = child_flags(host, &[expression, argument])?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
            expression: Some(expression.node()),
            question_dot_token: None,
            argument_expression: Some(argument.node()),
        }),
        flags,
    )
}

fn create_property_access<H: FlattenHost>(
    host: &mut H,
    expression: TransformNode,
    name: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags = child_flags(host, &[expression, name])?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
            expression: Some(expression.node()),
            question_dot_token: None,
            name: Some(name.node()),
        }),
        flags,
    )
}

/// `createArraySliceCall(value, i)` — `value.slice(i)`.
fn create_array_slice_call<H: FlattenHost>(
    host: &mut H,
    value: TransformNode,
    start: usize,
) -> Result<TransformNode, TransformError> {
    let slice = create_identifier(host, "slice")?;
    let access = create_property_access(host, value, slice)?;
    let start = create_numeric_literal(host, &start.to_string())?;
    create_call(host, access, vec![start])
}

fn create_call<H: FlattenHost>(
    host: &mut H,
    callee: TransformNode,
    arguments: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    let arguments = host
        .context()
        .factory()?
        .create_node_array(source, arguments)?;
    let flags = host.context_ref().arena().propagate_child_flags(callee)?
        | host.context_ref().arena().array_transform_flags(arguments);
    host.context().factory()?.create_node(
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

fn create_assignment<H: FlattenHost>(
    host: &mut H,
    left: TransformNode,
    right: TransformNode,
) -> Result<TransformNode, TransformError> {
    create_binary(host, left, SyntaxKind::EqualsToken, right)
}

fn create_binary<H: FlattenHost>(
    host: &mut H,
    left: TransformNode,
    operator: SyntaxKind,
    right: TransformNode,
) -> Result<TransformNode, TransformError> {
    let source = host.flatten_source();
    let operator_kind = operator;
    let operator =
        host.context()
            .factory()?
            .create_token(source, operator, TransformFlags::NONE)?;
    let mut flags = child_flags(host, &[left, operator, right])?;
    // `createBinaryExpression`'s EqualsToken facet arms
    // (_tsc.js:22794-22801): a literal-pattern left marks the
    // destructuring assignment for the downstream owners.
    if operator_kind == SyntaxKind::EqualsToken {
        let left_kind = host.context_ref().arena().node(left)?.kind;
        let pattern_flags = if host
            .context_ref()
            .arena()
            .transform_flags(left)
            .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
        {
            TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD
        } else {
            TransformFlags::NONE
        };
        if left_kind == SyntaxKind::ObjectLiteralExpression {
            flags |= TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_ES_2018
                | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                | pattern_flags;
        } else if left_kind == SyntaxKind::ArrayLiteralExpression {
            flags |= TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                | pattern_flags;
        }
    }
    host.context().factory()?.create_node(
        source,
        NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
            left: Some(left.node()),
            operator_token: Some(operator.node()),
            right: Some(right.node()),
        }),
        flags,
    )
}

fn create_conditional<H: FlattenHost>(
    host: &mut H,
    condition: TransformNode,
    when_true: TransformNode,
    when_false: TransformNode,
) -> Result<TransformNode, TransformError> {
    let when_true = parenthesize_comma_operand(host, when_true)?;
    let when_false = parenthesize_comma_operand(host, when_false)?;
    let source = host.flatten_source();
    let question = host.context().factory()?.create_token(
        source,
        SyntaxKind::QuestionToken,
        TransformFlags::NONE,
    )?;
    let colon = host.context().factory()?.create_token(
        source,
        SyntaxKind::ColonToken,
        TransformFlags::NONE,
    )?;
    let flags = child_flags(host, &[condition, when_true, when_false])?;
    host.context().factory()?.create_node(
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

fn parenthesize_comma_operand<H: FlattenHost>(
    host: &mut H,
    expression: TransformNode,
) -> Result<TransformNode, TransformError> {
    let is_comma = match &host.context_ref().arena().node(expression)?.data {
        NodeData::BinaryExpression(data) => data
            .operator_token
            .and_then(|id| {
                host.context_ref()
                    .arena()
                    .node_ref(host.flatten_source(), id)
            })
            .map(|operator| {
                host.context_ref()
                    .arena()
                    .node(operator)
                    .map(|node| node.kind == SyntaxKind::CommaToken)
            })
            .transpose()?
            .unwrap_or(false),
        NodeData::CommaListExpression(_) => true,
        _ => false,
    };
    if !is_comma {
        return Ok(expression);
    }
    let flags = host
        .context_ref()
        .arena()
        .propagate_child_flags(expression)?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
            expression: Some(expression.node()),
        }),
        flags,
    )
}

fn create_typeof<H: FlattenHost>(
    host: &mut H,
    expression: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags = host
        .context_ref()
        .arena()
        .propagate_child_flags(expression)?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
            expression: Some(expression.node()),
        }),
        flags,
    )
}

fn create_void_zero<H: FlattenHost>(host: &mut H) -> Result<TransformNode, TransformError> {
    let zero = create_numeric_literal(host, "0")?;
    let flags = host.context_ref().arena().propagate_child_flags(zero)?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
            expression: Some(zero.node()),
        }),
        flags,
    )
}

fn create_variable_declaration<H: FlattenHost>(
    host: &mut H,
    name: TransformNode,
    initializer: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let mut children = vec![name];
    children.extend(initializer);
    let flags = child_flags(host, &children)?;
    let source = host.flatten_source();
    host.context().factory()?.create_node(
        source,
        NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
            name: Some(name.node()),
            exclamation_token: None,
            r#type: None,
            initializer: initializer.map(TransformNode::node),
        }),
        flags,
    )
}

fn inline_expressions<H: FlattenHost>(
    host: &mut H,
    mut expressions: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let first = expressions
        .first()
        .copied()
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::CommaListExpression,
            field: "expression",
        })?;
    expressions.remove(0);
    expressions.into_iter().try_fold(first, |left, right| {
        create_binary(host, left, SyntaxKind::CommaToken, right)
    })
}

fn binary_left<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    let NodeData::BinaryExpression(data) = &host.context_ref().arena().node(node)?.data else {
        return Err(TransformError::RequiredChildRemoved {
            parent: host.context_ref().arena().node(node)?.kind,
            field: "left",
        });
    };
    data.left
        .and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "left",
        })
}

fn binary_right<H: FlattenHost>(
    host: &H,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    let NodeData::BinaryExpression(data) = &host.context_ref().arena().node(node)?.data else {
        return Err(TransformError::RequiredChildRemoved {
            parent: host.context_ref().arena().node(node)?.kind,
            field: "right",
        });
    };
    data.right
        .and_then(|id| {
            host.context_ref()
                .arena()
                .node_ref(host.flatten_source(), id)
        })
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "right",
        })
}

fn flags_intersect(flags: TransformFlags, mask: TransformFlags) -> bool {
    flags & mask != TransformFlags::NONE
}

fn child_flags<H: FlattenHost>(
    host: &H,
    children: &[TransformNode],
) -> Result<TransformFlags, TransformError> {
    children
        .iter()
        .try_fold(TransformFlags::NONE, |flags, child| {
            host.context_ref()
                .arena()
                .propagate_child_flags(*child)
                .map(|child_flags| flags | child_flags)
        })
}

#[cfg(test)]
#[path = "../../tests/unit/flatten_destructuring/tests.rs"]
mod tests;
