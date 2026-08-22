//! H2.5h-b B-4 focused projections (packet §7 step 5): drive the REAL
//! `[transform_es2015, transform_generators]` chain (the upstream
//! registration order) on parsed fixtures and byte-compare against
//! fresh-process oracle emits.
//!
//! Every `expected` string below is the COMPLETE oracle output minted by
//! the frozen probe (packet §7): vendored `typescript.js` 6.0.3,
//! `ts.createProgram` over one virtual `/project/input.ts`, options
//! `{ target: ES5, alwaysStrict: false, newLine: LineFeed }` plus the
//! per-case `downlevelIteration`/`useDefineForClassFields` variants —
//! no prologue, LF. Reproduction: `node b4-probe.mjs` (script body
//! reproduced in the PR; the 123 exact sources are §7 step 5 of the
//! packet).

use std::collections::{BTreeMap, BTreeSet};

use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, NodeData, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use super::super::generators::transform_generators;
use super::transform_es2015;
use crate::{
    create_printer, transform_nodes, DisabledSourceMapRecorder, EmitResolver, EmitResolverError,
    EmitResolverNode, NewLineKind, PrintRequest, PrinterOptions, TransformArena, TransformError,
    TransformRoot, TransformSourceId,
};

fn fixture_options(
    downlevel_iteration: bool,
    use_define_for_class_fields: bool,
) -> CompilerOptions {
    CompilerOptions {
        target: Some(1), // ScriptTarget.ES5
        always_strict: Some(false),
        downlevel_iteration: downlevel_iteration.then_some(true),
        use_define_for_class_fields: use_define_for_class_fields.then_some(true),
        ..CompilerOptions::default()
    }
}

fn project_with(
    source_text: &str,
    downlevel_iteration: bool,
    use_define_for_class_fields: bool,
) -> String {
    let parsed = parse_source_file("input.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = build_fixture_resolver(&arena, source);
    let options = fixture_options(downlevel_iteration, use_define_for_class_fields);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_es2015(&options, &resolver),
            transform_generators(tsc_types::ScriptTarget::ES5, &resolver),
        ],
        false,
    )
    .expect("es2015+generators transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(tsc_types::ScriptTarget::ES5),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print es2015 projection")
    .text()
    .to_owned()
}

#[track_caller]
fn assert_projection(
    source: &str,
    expected: &str,
    downlevel_iteration: bool,
    use_define_for_class_fields: bool,
) {
    let projected = project_with(source, downlevel_iteration, use_define_for_class_fields);
    assert_eq!(projected, expected);
}

fn project_error(source_text: &str) -> TransformError {
    let parsed = parse_source_file("input.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = build_fixture_resolver(&arena, source);
    let options = fixture_options(false, false);
    let outcome = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            transform_es2015(&options, &resolver),
            transform_generators(tsc_types::ScriptTarget::ES5, &resolver),
        ],
        false,
    );
    match outcome {
        Ok(_) => panic!("the projection must fail closed"),
        Err(error) => error,
    }
}
/// The §12.2 fixture resolver: a mini lexical binder over the single
/// parse tree. It precomputes the five ES2015 resolver answers by running
/// the PINNED checker rules verbatim (packet §4.3):
/// `checkNestedBlockScopedBinding` (_tsc.js:72250-72290),
/// `isBindingCapturedByNode` (:72291-72294),
/// `isAssignedInBodyOfForStatement` (:72295-72311),
/// `isSymbolOfDeclarationWithCollidingName` (:87921-87958) with
/// `getReferencedDeclarationWithCollidingName` (:87959-87970) /
/// `isDeclarationWithCollidingName` (:87971-87979), and
/// `isArgumentsLocalBinding` (:87858-87866). Exact for the §7 fixture
/// language (lexical resolution IS name resolution: no eval/with/TS value
/// namespaces), and SELF-CHECKING: every answer shapes emitted names, so
/// the oracle byte projections verify the binder. The B-3 lexical
/// catch-clause arm backs `get_referenced_value_declaration` for the
/// joint chain.
struct FixtureResolver {
    node_check_flags: BTreeMap<NodeId, u32>,
    captured_bindings_by_part: BTreeMap<NodeId, BTreeSet<NodeId>>,
    colliding_declarations: BTreeSet<NodeId>,
    reference_to_colliding: BTreeMap<NodeId, NodeId>,
    arguments_references: BTreeSet<NodeId>,
    /// B-3 arm: (name, catch-block pos, catch-block end, declaration).
    catches: Vec<(String, u32, u32, NodeId)>,
    identifiers: BTreeMap<NodeId, (String, u32)>,
}

impl EmitResolver for FixtureResolver {
    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(self
            .node_check_flags
            .get(&node.node())
            .copied()
            .unwrap_or(0)
            & flag
            != 0)
    }

    fn is_binding_captured_by_node(
        &self,
        node: EmitResolverNode,
        declaration: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(self
            .captured_bindings_by_part
            .get(&node.node())
            .is_some_and(|set| set.contains(&declaration.node())))
    }

    fn is_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(self.colliding_declarations.contains(&node.node()))
    }

    fn get_referenced_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .reference_to_colliding
            .get(&node.node())
            .map(|declaration| EmitResolverNode::new(node.source(), *declaration)))
    }

    fn is_arguments_local_binding(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(self.arguments_references.contains(&node.node()))
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        let Some((text, position)) = self.identifiers.get(&node.node()) else {
            return Ok(None);
        };
        let mut best: Option<(u32, NodeId)> = None;
        for (name, start, end, declaration) in &self.catches {
            if name == text
                && position >= start
                && position < end
                && best.map(|(s, _)| *start > s).unwrap_or(true)
            {
                best = Some((*start, *declaration));
            }
        }
        Ok(best.map(|(_, declaration)| EmitResolverNode::new(node.source(), declaration)))
    }
}

// --- binder -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum DeclKind {
    BlockScoped,     // let/const names incl. pattern elements; class decl names
    ClassExpression, // class expression name (own scope)
    Value,           // var/function/parameter names (function scope)
    CatchVariable,   // plain catch variable
    CatchElement,    // destructured catch binding element
}

struct FixtureDecl {
    name: String,
    /// the symbol's valueDeclaration analog (VariableDeclaration,
    /// BindingElement, ClassDeclaration/Expression, Parameter, Function).
    declaration: NodeId,
    /// the scope container the name is bound in.
    scope: NodeId,
    kind: DeclKind,
}

struct FixtureBinder<'a> {
    nodes: &'a [tsc_syntax::Node],
    node_base: u32,
}

const NODE_CHECK_LOOP_WITH_CAPTURED: u32 = 4096;
const NODE_CHECK_CONTAINS_CAPTURED: u32 = 8192;
const NODE_CHECK_CAPTURED: u32 = 16384;
const NODE_CHECK_IN_LOOP: u32 = 32768;
const NODE_CHECK_NEEDS_OUT_PARAM: u32 = 65536;
const NODE_FLAGS_BLOCK_SCOPED: i32 = 7;

impl<'a> FixtureBinder<'a> {
    fn record(&self, id: NodeId) -> &'a tsc_syntax::Node {
        &self.nodes[(id.0 - self.node_base) as usize]
    }

    fn kind(&self, id: NodeId) -> SyntaxKind {
        self.record(id).kind
    }

    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.record(id).parent
    }

    fn is_function_like(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::Constructor
        )
    }

    fn is_iteration_statement(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::DoStatement
        )
    }

    /// `isBlockScope(node, parent)`.
    fn is_block_scope(&self, id: NodeId) -> bool {
        match self.kind(id) {
            SyntaxKind::SourceFile
            | SyntaxKind::CatchClause
            | SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::CaseBlock => true,
            SyntaxKind::Block => {
                // a Block is a scope unless it IS a function body
                !self.parent(id).is_some_and(|p| self.is_function_like(p))
            }
            // NOTE: ClassExpression is deliberately NOT a block scope here
            // (upstream isBlockScope has no arm) — the class-expression
            // NAME's resolution visibility is modeled on the FixtureDecl
            // record (scope = the ClassExpression subtree), while the
            // checker-rule container of that symbol is
            // getEnclosingBlockScopeContainer(classExpression) as upstream.
            _ => self.is_function_like(id),
        }
    }

    /// `getEnclosingBlockScopeContainer(node)` — findAncestor(node.parent, isBlockScope).
    fn enclosing_block_scope_container(&self, id: NodeId) -> Option<NodeId> {
        let mut current = self.parent(id);
        while let Some(node) = current {
            if self.is_block_scope(node) {
                return Some(node);
            }
            current = self.parent(node);
        }
        None
    }

    /// `isInsideFunctionOrInstancePropertyInitializer(node, threshold)` —
    /// a function-like strictly below `threshold` encloses `node` (the
    /// instance-property arm is outside the fixture language).
    fn is_inside_function_below(&self, node: NodeId, threshold: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(id) = current {
            if id == threshold {
                return false;
            }
            if self.is_function_like(id) {
                return true;
            }
            current = self.parent(id);
        }
        false
    }

    /// `getEnclosingIterationStatement(node)` — findAncestor quitting at a
    /// new lexical environment (function-likes / source file).
    fn enclosing_iteration_statement(&self, node: NodeId) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(id) = current {
            if self.is_function_like(id) || self.kind(id) == SyntaxKind::SourceFile {
                return None;
            }
            if self.is_iteration_statement(id) {
                return Some(id);
            }
            current = self.parent(id);
        }
        None
    }

    /// `getPartOfForStatementContainingNode(node, container)`.
    fn part_of_for_statement_containing(
        &self,
        node: NodeId,
        container: NodeId,
        parts: &ForParts,
    ) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(id) = current {
            if id == container {
                return None;
            }
            if Some(id) == parts.initializer
                || Some(id) == parts.condition
                || Some(id) == parts.incrementor
                || Some(id) == parts.statement
            {
                return Some(id);
            }
            current = self.parent(id);
        }
        None
    }

    /// `isAssignedInBodyOfForStatement(node, container)`.
    fn is_assigned_in_body_of_for_statement(
        &self,
        reference: NodeId,
        container: NodeId,
        statement: Option<NodeId>,
    ) -> bool {
        let mut current = reference;
        while self
            .parent(current)
            .is_some_and(|p| self.kind(p) == SyntaxKind::ParenthesizedExpression)
        {
            current = self.parent(current).unwrap();
        }
        let assigned = match self.parent(current).map(|p| (p, self.record(p))) {
            Some((_, record)) => match &record.data {
                NodeData::BinaryExpression(data) => {
                    data.left == Some(current)
                        && data
                            .operator_token
                            .is_some_and(|token| is_assignment_operator_kind(self.kind(token)))
                }
                NodeData::PrefixUnaryExpression(data) => {
                    matches!(
                        data.operator,
                        SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                    )
                }
                NodeData::PostfixUnaryExpression(data) => {
                    matches!(
                        data.operator,
                        SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                    )
                }
                _ => false,
            },
            None => false,
        };
        if !assigned {
            return false;
        }
        // findAncestor(current, n => n === container ? "quit" : n === container.statement)
        let mut walk = Some(current);
        while let Some(id) = walk {
            if id == container {
                return false;
            }
            if Some(id) == statement {
                return true;
            }
            walk = self.parent(id);
        }
        false
    }

    /// `isStatementWithLocals`.
    fn is_statement_with_locals(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            SyntaxKind::Block
                | SyntaxKind::CaseBlock
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
        )
    }

    /// `isBlockScopedContainerTopLevel`.
    fn is_block_scoped_container_top_level(&self, id: NodeId) -> bool {
        self.kind(id) == SyntaxKind::SourceFile || self.is_function_like(id)
    }
}

struct ForParts {
    initializer: Option<NodeId>,
    condition: Option<NodeId>,
    incrementor: Option<NodeId>,
    statement: Option<NodeId>,
}

fn build_fixture_resolver(arena: &TransformArena, source: TransformSourceId) -> FixtureResolver {
    let syntax = arena
        .source(source)
        .expect("fixture source registered")
        .syntax();
    let node_base = syntax.arena.node_base();
    let binder = FixtureBinder {
        nodes: syntax.arena.nodes(),
        node_base,
    };
    let id_at = |offset: usize| NodeId(node_base + offset as u32);

    // -- collect identifiers + catch clauses (B-3 arm) + declarations -----
    let mut identifiers: BTreeMap<NodeId, (String, u32)> = BTreeMap::new();
    let mut catches: Vec<(String, u32, u32, NodeId)> = Vec::new();
    let mut declarations: Vec<FixtureDecl> = Vec::new();

    // helper: collect the identifier names of a binding name (identifier or
    // pattern), yielding (nameNode, valueDeclaration) pairs. For a plain
    // identifier name the valueDeclaration is `declaration`; for pattern
    // elements it is the BindingElement (getSymbolOfDeclaration analog).
    let collect_binding_names =
        |start_name: NodeId, declaration: NodeId, out: &mut Vec<(String, NodeId)>| {
            let mut stack = vec![(start_name, declaration)];
            while let Some((name, value_declaration)) = stack.pop() {
                let record = binder.record(name);
                match &record.data {
                    NodeData::Identifier(data) => out.push((data.text.clone(), value_declaration)),
                    NodeData::ObjectBindingPattern(pattern) => {
                        if let Some(elements) = pattern.elements {
                            for element in &syntax.arena.node_array(elements).nodes {
                                stack.push((*element, *element));
                            }
                        }
                    }
                    NodeData::ArrayBindingPattern(pattern) => {
                        if let Some(elements) = pattern.elements {
                            for element in &syntax.arena.node_array(elements).nodes {
                                if binder.kind(*element) != SyntaxKind::OmittedExpression {
                                    stack.push((*element, *element));
                                }
                            }
                        }
                    }
                    NodeData::BindingElement(element) => {
                        if let Some(inner) = element.name {
                            stack.push((inner, name));
                        }
                    }
                    _ => {}
                }
            }
        };

    for (offset, record) in syntax.arena.nodes().iter().enumerate() {
        let id = id_at(offset);
        match &record.data {
            NodeData::Identifier(data) => {
                identifiers.insert(id, (data.text.clone(), record.pos));
            }
            NodeData::CatchClause(data) => {
                let (Some(declaration), Some(block)) = (data.variable_declaration, data.block)
                else {
                    continue;
                };
                let declaration_record = binder.record(declaration);
                let NodeData::VariableDeclaration(variable) = &declaration_record.data else {
                    continue;
                };
                let Some(name) = variable.name else { continue };
                match &binder.record(name).data {
                    NodeData::Identifier(name_data) => {
                        let block_record = binder.record(block);
                        catches.push((
                            name_data.text.clone(),
                            block_record.pos,
                            block_record.end,
                            declaration,
                        ));
                        declarations.push(FixtureDecl {
                            name: name_data.text.clone(),
                            declaration,
                            scope: id,
                            kind: DeclKind::CatchVariable,
                        });
                    }
                    _ => {
                        // destructured catch binding: elements are
                        // catch-scoped block-scoped bindings.
                        let mut names = Vec::new();
                        collect_binding_names(name, declaration, &mut names);
                        for (text, value_declaration) in names {
                            declarations.push(FixtureDecl {
                                name: text,
                                declaration: value_declaration,
                                scope: id,
                                kind: DeclKind::CatchElement,
                            });
                        }
                    }
                }
            }
            NodeData::VariableDeclaration(variable) => {
                // skip catch-clause variables (handled above)
                if record
                    .parent
                    .is_some_and(|p| binder.kind(p) == SyntaxKind::CatchClause)
                {
                    continue;
                }
                let Some(list) = record.parent else { continue };
                let Some(name) = variable.name else { continue };
                let block_scoped = binder.record(list).flags & NODE_FLAGS_BLOCK_SCOPED != 0;
                let mut names = Vec::new();
                collect_binding_names(name, id, &mut names);
                for (text, value_declaration) in names {
                    if block_scoped {
                        let scope = binder
                            .enclosing_block_scope_container(value_declaration)
                            .expect("block-scoped declaration has a container");
                        declarations.push(FixtureDecl {
                            name: text,
                            declaration: value_declaration,
                            scope,
                            kind: DeclKind::BlockScoped,
                        });
                    } else {
                        let scope = nearest_function_scope(&binder, value_declaration);
                        declarations.push(FixtureDecl {
                            name: text,
                            declaration: value_declaration,
                            scope,
                            kind: DeclKind::Value,
                        });
                    }
                }
            }
            NodeData::Parameter(parameter) => {
                let Some(name) = parameter.name else { continue };
                let mut names = Vec::new();
                collect_binding_names(name, id, &mut names);
                let scope = nearest_function_scope(&binder, id);
                for (text, value_declaration) in names {
                    declarations.push(FixtureDecl {
                        name: text,
                        declaration: value_declaration,
                        scope,
                        kind: DeclKind::Value,
                    });
                }
            }
            NodeData::FunctionDeclaration(function) => {
                if let Some(name) = function.name {
                    if let NodeData::Identifier(data) = &binder.record(name).data {
                        let scope = nearest_function_scope(&binder, id);
                        declarations.push(FixtureDecl {
                            name: data.text.clone(),
                            declaration: id,
                            scope,
                            kind: DeclKind::Value,
                        });
                    }
                }
            }
            NodeData::ClassDeclaration(class) => {
                if let Some(name) = class.name {
                    if let NodeData::Identifier(data) = &binder.record(name).data {
                        let scope = binder
                            .enclosing_block_scope_container(id)
                            .expect("class declaration has a container");
                        declarations.push(FixtureDecl {
                            name: data.text.clone(),
                            declaration: id,
                            scope,
                            kind: DeclKind::BlockScoped,
                        });
                    }
                }
            }
            NodeData::ClassExpression(class) => {
                if let Some(name) = class.name {
                    if let NodeData::Identifier(data) = &binder.record(name).data {
                        declarations.push(FixtureDecl {
                            name: data.text.clone(),
                            declaration: id,
                            scope: id, // visible within the class expression only
                            kind: DeclKind::ClassExpression,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // -- reference resolution --------------------------------------------
    // A reference resolves to the declaration with the DEEPEST scope that
    // (a) declares its text and (b) whose scope node is an ancestor-or-self
    // of the reference. Depth by ancestor containment.
    let is_ancestor_or_self = |ancestor: NodeId, mut node: NodeId| loop {
        if node == ancestor {
            return true;
        }
        match binder.parent(node) {
            Some(parent) => node = parent,
            None => return false,
        }
    };
    let depth_of = |mut node: NodeId| {
        let mut depth = 0u32;
        while let Some(parent) = binder.parent(node) {
            depth += 1;
            node = parent;
        }
        depth
    };

    let mut references: Vec<(NodeId, usize)> = Vec::new(); // (ref ident, decl index)
    let mut arguments_references: BTreeSet<NodeId> = BTreeSet::new();
    for (&id, (text, _)) in &identifiers {
        if !is_reference_position(&binder, id) {
            continue;
        }
        let mut best: Option<(u32, usize)> = None;
        for (index, decl) in declarations.iter().enumerate() {
            if &decl.name != text {
                continue;
            }
            if !is_ancestor_or_self(decl.scope, id) {
                continue;
            }
            let depth = depth_of(decl.scope);
            if best.map(|(d, _)| depth > d).unwrap_or(true) {
                best = Some((depth, index));
            }
        }
        if let Some((_, index)) = best {
            references.push((id, index));
        } else if text == "arguments" {
            // the implicit arguments object (no local binding in the
            // fixture language): true when inside a function.
            let mut current = binder.parent(id);
            while let Some(node) = current {
                if binder.is_function_like(node) {
                    arguments_references.insert(id);
                    break;
                }
                current = binder.parent(node);
            }
        }
    }

    // -- checkNestedBlockScopedBinding per reference ---------------------
    let mut node_check_flags: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut captured_bindings_by_part: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for &(reference, decl_index) in &references {
        let decl = &declarations[decl_index];
        if !matches!(decl.kind, DeclKind::BlockScoped | DeclKind::ClassExpression) {
            continue; // catch-parented and function-scoped symbols are excluded
        }
        let container = if decl.kind == DeclKind::ClassExpression {
            binder.enclosing_block_scope_container(decl.declaration)
        } else {
            Some(decl.scope)
        };
        let Some(container) = container else { continue };
        let captured = binder.is_inside_function_below(reference, container);
        let enclosing_loop = binder.enclosing_iteration_statement(container);
        if let Some(loop_node) = enclosing_loop {
            // is the declaration part of a for-statement HEAD list?
            let for_parts = for_parts_of(&binder, container);
            let in_for_head = binder.kind(container) == SyntaxKind::ForStatement
                && var_decl_list_of(&binder, decl.declaration)
                    .is_some_and(|list| binder.parent(list) == Some(container));
            if captured {
                let mut captures_in_loop_body = true;
                if in_for_head {
                    if let (Some(parts), Some(parent)) =
                        (for_parts.as_ref(), binder.parent(reference))
                    {
                        if let Some(part) =
                            binder.part_of_for_statement_containing(parent, container, parts)
                        {
                            *node_check_flags.entry(part).or_default() |=
                                NODE_CHECK_CONTAINS_CAPTURED;
                            captured_bindings_by_part
                                .entry(part)
                                .or_default()
                                .insert(decl.declaration);
                            if Some(part) == parts.initializer {
                                captures_in_loop_body = false;
                            }
                        }
                    }
                }
                if captures_in_loop_body {
                    *node_check_flags.entry(loop_node).or_default() |=
                        NODE_CHECK_LOOP_WITH_CAPTURED;
                }
            }
            if in_for_head
                && binder.is_assigned_in_body_of_for_statement(
                    reference,
                    container,
                    for_parts.as_ref().and_then(|parts| parts.statement),
                )
            {
                *node_check_flags.entry(decl.declaration).or_default() |=
                    NODE_CHECK_NEEDS_OUT_PARAM;
            }
            *node_check_flags.entry(decl.declaration).or_default() |= NODE_CHECK_IN_LOOP;
        }
        if captured {
            *node_check_flags.entry(decl.declaration).or_default() |= NODE_CHECK_CAPTURED;
        }
    }

    // -- collisions (isSymbolOfDeclarationWithCollidingName) -------------
    let mut colliding_declarations: BTreeSet<NodeId> = BTreeSet::new();
    for decl in &declarations {
        let eligible_kind = matches!(decl.kind, DeclKind::BlockScoped | DeclKind::CatchElement);
        if !eligible_kind {
            continue;
        }
        // (catch elements' container IS the CatchClause == decl.scope)
        let container = decl.scope;
        let gate =
            binder.is_statement_with_locals(container) || decl.kind == DeclKind::CatchElement;
        if !gate {
            continue;
        }
        let Some(lookup_from) = binder.parent(container) else {
            continue;
        };
        // resolveName(container.parent, name, Value): any value binding of
        // that text visible from container.parent.
        let outer_hit = declarations.iter().any(|other| {
            other.name == decl.name
                && !std::ptr::eq(other, decl)
                && other.kind != DeclKind::ClassExpression
                && is_ancestor_or_self(other.scope, lookup_from)
        });
        let flags = node_check_flags
            .get(&decl.declaration)
            .copied()
            .unwrap_or(0);
        let colliding = if outer_hit {
            true
        } else if flags & NODE_CHECK_CAPTURED != 0 {
            let declared_in_loop = flags & NODE_CHECK_IN_LOOP != 0;
            let in_loop_initializer = binder.is_iteration_statement(container);
            let in_loop_body_block = binder.kind(container) == SyntaxKind::Block
                && binder
                    .parent(container)
                    .is_some_and(|p| binder.is_iteration_statement(p));
            !binder.is_block_scoped_container_top_level(container)
                && (!declared_in_loop || (!in_loop_initializer && !in_loop_body_block))
        } else {
            false
        };
        if colliding {
            colliding_declarations.insert(decl.declaration);
        }
    }

    let mut reference_to_colliding: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for &(reference, decl_index) in &references {
        let decl = &declarations[decl_index];
        if colliding_declarations.contains(&decl.declaration) {
            reference_to_colliding.insert(reference, decl.declaration);
        }
    }

    FixtureResolver {
        node_check_flags,
        captured_bindings_by_part,
        colliding_declarations,
        reference_to_colliding,
        arguments_references,
        catches,
        identifiers,
    }
}

fn nearest_function_scope(binder: &FixtureBinder<'_>, node: NodeId) -> NodeId {
    let mut current = binder.parent(node);
    while let Some(id) = current {
        if binder.is_function_like(id) || binder.kind(id) == SyntaxKind::SourceFile {
            return id;
        }
        current = binder.parent(id);
    }
    node
}

fn var_decl_list_of(binder: &FixtureBinder<'_>, declaration: NodeId) -> Option<NodeId> {
    // getAncestor(valueDeclaration, VariableDeclarationList)
    let mut current = Some(declaration);
    while let Some(id) = current {
        if binder.kind(id) == SyntaxKind::VariableDeclarationList {
            return Some(id);
        }
        current = binder.parent(id);
    }
    None
}

fn for_parts_of(binder: &FixtureBinder<'_>, container: NodeId) -> Option<ForParts> {
    match &binder.record(container).data {
        NodeData::ForStatement(data) => Some(ForParts {
            initializer: data.initializer,
            condition: data.condition,
            incrementor: data.incrementor,
            statement: data.statement,
        }),
        _ => None,
    }
}

/// Reference-position test (parent-directed; packet §12.2).
fn is_reference_position(binder: &FixtureBinder<'_>, id: NodeId) -> bool {
    let Some(parent) = binder.parent(id) else {
        return false;
    };
    match &binder.record(parent).data {
        NodeData::PropertyAccessExpression(data) => data.name != Some(id),
        NodeData::PropertyAssignment(data) => data.name != Some(id),
        NodeData::MethodDeclaration(data) => data.name != Some(id),
        NodeData::GetAccessor(data) => data.name != Some(id),
        NodeData::SetAccessor(data) => data.name != Some(id),
        NodeData::VariableDeclaration(data) => data.name != Some(id),
        NodeData::BindingElement(data) => data.name != Some(id) && data.property_name != Some(id),
        NodeData::Parameter(data) => data.name != Some(id),
        NodeData::FunctionDeclaration(data) => data.name != Some(id),
        NodeData::FunctionExpression(data) => data.name != Some(id),
        NodeData::ClassDeclaration(data) => data.name != Some(id),
        NodeData::ClassExpression(data) => data.name != Some(id),
        NodeData::LabeledStatement(data) => data.label != Some(id),
        NodeData::BreakStatement(data) => data.label != Some(id),
        NodeData::ContinueStatement(data) => data.label != Some(id),
        NodeData::MetaProperty(_) => false,
        _ => true,
    }
}

fn is_assignment_operator_kind(kind: SyntaxKind) -> bool {
    kind >= SyntaxKind::FirstAssignment && kind <= SyntaxKind::LastAssignment
}
// --- §7 step 5: the 116 oracle byte projections (generated from the
// frozen probe output b4-expectations.json; do not hand-edit bodies). ---

#[test]
fn projects_let_basic() {
    assert_projection(
        r#"let a = 1;
use(a);
"#,
        r#"var a = 1;
use(a);
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_no_init_toplevel() {
    assert_projection(
        r#"let a;
use(a);
"#,
        r#"var a;
use(a);
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_explicit_init_block() {
    assert_projection(
        r#"function f() { { let a; use(a); } }
"#,
        r#"function f() { {
    var a = void 0;
    use(a);
} }
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_block_collision() {
    assert_projection(
        r#"{ let x = 1; use(x); }
var x = 2;
"#,
        r#"{
    var x_1 = 1;
    use(x_1);
}
var x = 2;
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_two_blocks() {
    assert_projection(
        r#"{ let x = 1; a(x); }
{ let x = 2; b(x); }
"#,
        r#"{
    var x = 1;
    a(x);
}
{
    var x = 2;
    b(x);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_shadow_fn() {
    assert_projection(
        r#"let x = 1;
function f() { let x = 2; use(x); }
use(x, f);
"#,
        r#"var x = 1;
function f() { var x = 2; use(x); }
use(x, f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_const_basic() {
    assert_projection(
        r#"const c = compute();
use(c);
"#,
        r#"var c = compute();
use(c);
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_in_switch_case() {
    assert_projection(
        r#"switch (t) { case 1: let v = one(); use(v); break; default: other(); }
"#,
        r#"switch (t) {
    case 1:
        var v = one();
        use(v);
        break;
    default: other();
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_for_in() {
    assert_projection(
        r#"for (let k in o) { use(k); }
"#,
        r#"for (var k in o) {
    use(k);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_loop_no_capture() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { use(i); }
"#,
        r#"for (var i = 0; i < n; i++) {
    use(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_basic() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
};
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_out_param() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); i = step(i); }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
    i = step(i);
    out_i_1 = i;
};
var out_i_1;
for (var i = 0; i < n; i++) {
    _loop_1(i);
    i = out_i_1;
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_break() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) break; }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
    if (c)
        return "break";
};
for (var i = 0; i < n; i++) {
    var state_1 = _loop_1(i);
    if (state_1 === "break")
        break;
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_continue() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) continue; tail(i); }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
    if (c)
        return "continue";
    tail(i);
};
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_return() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) return i; } return fallback(); }
"#,
        r#"function f() { var _loop_1 = function (i) {
    sink(function () { return i; });
    if (c)
        return { value: i };
}; for (var i = 0; i < n; i++) {
    var state_1 = _loop_1(i);
    if (typeof state_1 === "object")
        return state_1.value;
} return fallback(); }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_return_void() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) return; } tail(); }
"#,
        r#"function f() { var _loop_1 = function (i) {
    sink(function () { return i; });
    if (c)
        return { value: void 0 };
}; for (var i = 0; i < n; i++) {
    var state_1 = _loop_1(i);
    if (typeof state_1 === "object")
        return state_1.value;
} tail(); }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_labeled_break() {
    assert_projection(
        r#"outer: for (let i = 0; i < n; i++) { for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) break outer; } }
"#,
        r#"var _loop_1 = function (i) {
    var _loop_2 = function (j) {
        sink(function () { return i + j; });
        if (c)
            return "break-outer";
    };
    for (var j = 0; j < m; j++) {
        var state_2 = _loop_2(j);
        switch (state_2) {
            case "break-outer": return state_2;
        }
    }
};
outer: for (var i = 0; i < n; i++) {
    var state_1 = _loop_1(i);
    switch (state_1) {
        case "break-outer": break outer;
    }
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_labeled_continue() {
    assert_projection(
        r#"outer: for (let i = 0; i < n; i++) { inner: for (let j = 0; j < m; j++) { sink(function () { return j; }); if (c) continue outer; } }
"#,
        r#"outer: for (var i = 0; i < n; i++) {
    var _loop_1 = function (j) {
        sink(function () { return j; });
        if (c)
            return "continue-outer";
    };
    inner: for (var j = 0; j < m; j++) {
        var state_1 = _loop_1(j);
        switch (state_1) {
            case "continue-outer": continue outer;
        }
    }
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_this() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); use(this); } }
"#,
        r#"function f() { var _loop_1 = function (i) {
    sink(function () { return i; });
    use(this_1);
}; var this_1 = this; for (var i = 0; i < n; i++) {
    _loop_1(i);
} }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_this_arrow() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { sink(() => i + this.x); } }
"#,
        r#"function f() {
    var _this = this;
    var _loop_1 = function (i) {
        sink(function () { return i + _this.x; });
    };
    for (var i = 0; i < n; i++) {
        _loop_1(i);
    }
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_arguments() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); use(arguments[0]); } }
"#,
        r#"function f() { var _loop_1 = function (i) {
    sink(function () { return i; });
    use(arguments_1[0]);
}; var arguments_1 = arguments; for (var i = 0; i < n; i++) {
    _loop_1(i);
} }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_while() {
    assert_projection(
        r#"while (c()) { let v = p(); sink(function () { return v; }); }
"#,
        r#"var _loop_1 = function () {
    var v = p();
    sink(function () { return v; });
};
while (c()) {
    _loop_1();
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_do() {
    assert_projection(
        r#"do { let v = p(); sink(function () { return v; }); } while (c());
"#,
        r#"var _loop_1 = function () {
    var v = p();
    sink(function () { return v; });
};
do {
    _loop_1();
} while (c());
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_for_in() {
    assert_projection(
        r#"for (var k in o) { let v = o[k]; sink(function () { return v; }); }
"#,
        r#"var _loop_1 = function () {
    var v = o[k];
    sink(function () { return v; });
};
for (var k in o) {
    _loop_1();
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_for_of() {
    assert_projection(
        r#"for (var e of xs) { let v = e; sink(function () { return v; }); }
"#,
        r#"var _loop_1 = function () {
    var v = e;
    sink(function () { return v; });
};
for (var _i = 0, xs_1 = xs; _i < xs_1.length; _i++) {
    var e = xs_1[_i];
    _loop_1();
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_var_hoist() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { var t = q(i); sink(function () { return i; }); } use(t);
"#,
        r#"var _loop_1 = function (i) {
    t = q(i);
    sink(function () { return i; });
};
var t;
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
use(t);
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_var_destr() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { var [p, q2] = pair(i); sink(function () { return i; }); } use(p, q2);
"#,
        r#"var _loop_1 = function (i) {
    var _a;
    _a = pair(i), p = _a[0], q2 = _a[1];
    sink(function () { return i; });
};
var p, q2;
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
use(p, q2);
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_nested_return() {
    assert_projection(
        r#"function f() { for (let i = 0; i < n; i++) { for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) return j; } } }
"#,
        r#"function f() { var _loop_1 = function (i) {
    var _loop_2 = function (j) {
        sink(function () { return i + j; });
        if (c)
            return { value: j };
    };
    for (var j = 0; j < m; j++) {
        var state_2 = _loop_2(j);
        if (typeof state_2 === "object")
            return state_2;
    }
}; for (var i = 0; i < n; i++) {
    var state_1 = _loop_1(i);
    if (typeof state_1 === "object")
        return state_1.value;
} }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_init_conversion() {
    assert_projection(
        r#"for (let seed = (sink(function () { return seed; }), 0), index = 0; index < n; index++) { body(seed, index); }
"#,
        r#"var _loop_init_1 = function () {
    var seed = (sink(function () { return seed; }), 0), index = 0;
    out_seed_1 = seed;
};
var out_seed_1, out_index_1;
_loop_init_1();
for (var seed = out_seed_1, index = out_index_1; index < n; index++) {
    body(seed, index);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_cond_conversion() {
    assert_projection(
        r#"for (let i = 0; check(function () { return i; }); i++) { body(i); }
"#,
        r#"var _loop_1 = function (i) {
    if (inc_1)
        i++;
    else
        inc_1 = true;
    if (!check(function () { return i; }))
        return out_i_1 = i, "break";
    body(i);
    out_i_1 = i;
};
var out_i_1, inc_1 = false;
for (var i = 0;;) {
    var state_1 = _loop_1(i);
    i = out_i_1;
    if (state_1 === "break")
        break;
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_incr_conversion() {
    assert_projection(
        r#"for (let i = 0; i < n; i = bump(function () { return i; })) { body(i); }
"#,
        r#"var _loop_1 = function (i) {
    if (inc_1)
        i = bump(function () { return i; });
    else
        inc_1 = true;
    body(i);
    out_i_1 = i;
};
var out_i_1, inc_1 = false;
for (var i = 0; i < n;) {
    _loop_1(i);
    i = out_i_1;
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_yield_star_body_site() {
    assert_projection(
        r#"function* g() { for (let i = 0; i < n; i++) { sink(function () { return i; }); yield i; } }
"#,
        r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
function g() { var _loop_1, i; return __generator(this, function (_a) {
    switch (_a.label) {
        case 0:
            _loop_1 = function (i) {
                return __generator(this, function (_b) {
                    switch (_b.label) {
                        case 0:
                            sink(function () { return i; });
                            return [4 /*yield*/, i];
                        case 1:
                            _b.sent();
                            return [2 /*return*/];
                    }
                });
            };
            i = 0;
            _a.label = 1;
        case 1:
            if (!(i < n)) return [3 /*break*/, 4];
            return [5 /*yield**/, _loop_1(i)];
        case 2:
            _a.sent();
            _a.label = 3;
        case 3:
            i++;
            return [3 /*break*/, 1];
        case 4: return [2 /*return*/];
    }
}); }
"#,
        false,
        false,
    );
}

#[test]
fn projects_yield_star_init_site() {
    assert_projection(
        r#"function* g() { for (let seed = (sink(function () { return seed; }), yield 1), index = 0; index < n; index++) { body(seed, index); } }
"#,
        r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
function g() { var _loop_init_1, out_seed_1, out_index_1, seed, index; return __generator(this, function (_a) {
    switch (_a.label) {
        case 0:
            _loop_init_1 = function () {
                var seed, index;
                return __generator(this, function (_a) {
                    switch (_a.label) {
                        case 0:
                            sink(function () { return seed; });
                            return [4 /*yield*/, 1];
                        case 1:
                            seed = (_a.sent()), index = 0;
                            out_seed_1 = seed;
                            return [2 /*return*/];
                    }
                });
            };
            return [5 /*yield**/, _loop_init_1()];
        case 1:
            _a.sent();
            for (seed = out_seed_1, index = out_index_1; index < n; index++) {
                body(seed, index);
            }
            return [2 /*return*/];
    }
}); }
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_switch_break() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); switch (t(i)) { case 1: break; default: other(); } }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
    switch (t(i)) {
        case 1: break;
        default: other();
    }
};
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_basic() {
    assert_projection(
        r#"class C {
  m() { return 1; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    C.prototype.m = function () { return 1; };
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_ctor() {
    assert_projection(
        r#"class C {
  constructor(a, b) { this.a = a; this.b = b; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C(a, b) {
        this.a = a;
        this.b = b;
    }
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_extends() {
    assert_projection(
        r#"class D extends B {
  constructor(x) { super(x); this.x = x; }
  m() { return this.x; }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D(x) {
        var _this = _super.call(this, x) || this;
        _this.x = x;
        return _this;
    }
    D.prototype.m = function () { return this.x; };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_extends_default_ctor() {
    assert_projection(
        r#"class D extends B {
  m() { return 2; }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        return _super !== null && _super.apply(this, arguments) || this;
    }
    D.prototype.m = function () { return 2; };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_extends_null() {
    assert_projection(
        r#"class D extends null {
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
    }
    return D;
}(null));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_methods_static() {
    assert_projection(
        r#"class C {
  m() { return 1; }
  static s() { return 2; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    C.prototype.m = function () { return 1; };
    C.s = function () { return 2; };
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_accessors() {
    assert_projection(
        r#"class C {
  get p() { return this._p; }
  set p(v) { this._p = v; }
  static get sp() { return 1; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    Object.defineProperty(C.prototype, "p", {
        get: function () { return this._p; },
        set: function (v) { this._p = v; },
        enumerable: false,
        configurable: true
    });
    Object.defineProperty(C, "sp", {
        get: function () { return 1; },
        enumerable: false,
        configurable: true
    });
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_expr_named() {
    assert_projection(
        r#"var E = class Name {
  m() { return Name; }
};
use(E);
"#,
        r#"var E = /** @class */ (function () {
    function Name() {
    }
    Name.prototype.m = function () { return Name; };
    return Name;
}());
use(E);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_expr_anon() {
    assert_projection(
        r#"use(class {
  m() { return 3; }
});
"#,
        r#"use(/** @class */ (function () {
    function class_1() {
    }
    class_1.prototype.m = function () { return 3; };
    return class_1;
}()));
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_semicolon_element() {
    assert_projection(
        r#"class C {
  ;
  m() { return 1; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    ;
    C.prototype.m = function () { return 1; };
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_super_property() {
    assert_projection(
        r#"class D extends B {
  m() { return super.m() + super.p; }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        return _super !== null && _super.apply(this, arguments) || this;
    }
    D.prototype.m = function () { return _super.prototype.m.call(this) + _super.prototype.p; };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_static_super_property() {
    assert_projection(
        r#"class D extends B {
  static s() { return super.s(); }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        return _super !== null && _super.apply(this, arguments) || this;
    }
    D.s = function () { return _super.s.call(this); };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_new_target_ctor() {
    assert_projection(
        r#"class C {
  constructor() { use(new.target); }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
        var _newTarget = this.constructor;
        use(_newTarget);
    }
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_derived_return_this() {
    assert_projection(
        r#"class D extends B {
  constructor() { super(); if (c) { return; } tail(); }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        var _this = _super.call(this) || this;
        if (c) {
            return _this;
        }
        tail();
        return _this;
    }
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_derived_super_tail() {
    assert_projection(
        r#"class D extends B {
  constructor() { effect(); super(a1()); }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        effect();
        return _super.call(this, a1()) || this;
    }
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_generator_method() {
    assert_projection(
        r#"class C {
  *m() { yield 1; }
}
use(C);
"#,
        r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
var C = /** @class */ (function () {
    function C() {
    }
    C.prototype.m = function () { return __generator(this, function (_a) {
        switch (_a.label) {
            case 0: return [4 /*yield*/, 1];
            case 1:
                _a.sent();
                return [2 /*return*/];
        }
    }); };
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_method_default_param() {
    assert_projection(
        r#"class C {
  m(a = 1) { return a; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    C.prototype.m = function (a) {
        if (a === void 0) { a = 1; }
        return a;
    };
    return C;
}());
use(C);
"#,
        false,
        false,
    );
}

#[test]
fn projects_class_udcf_method() {
    assert_projection(
        r#"class C {
  m() { return 1; }
}
use(C);
"#,
        r#"var C = /** @class */ (function () {
    function C() {
    }
    Object.defineProperty(C.prototype, "m", {
        enumerable: false,
        configurable: true,
        writable: true,
        value: function () { return 1; }
    });
    return C;
}());
use(C);
"#,
        false,
        true,
    );
}

#[test]
fn projects_fn_new_target() {
    assert_projection(
        r#"function f() { return new.target; }
use(f);
"#,
        r#"function f() {
    var _newTarget = this && this instanceof f ? this.constructor : void 0;
    return _newTarget;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_fn_expr_named_new_target() {
    assert_projection(
        r#"var f = function named() { return new.target; };
use(f);
"#,
        r#"var f = function named() {
    var _newTarget = this && this instanceof named ? this.constructor : void 0;
    return _newTarget;
};
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_basic() {
    assert_projection(
        r#"var f = (a) => a + 1;
use(f);
"#,
        r#"var f = function (a) { return a + 1; };
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_multiline() {
    assert_projection(
        r#"var f = (a) =>
  a + 1;
use(f);
"#,
        r#"var f = function (a) {
    return a + 1;
};
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_block_body() {
    assert_projection(
        r#"var f = (a) => { const b = a + 1; return b; };
use(f);
"#,
        r#"var f = function (a) { var b = a + 1; return b; };
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_this_fn() {
    assert_projection(
        r#"function f() { var g = () => this.x; return g; }
use(f);
"#,
        r#"function f() {
    var _this = this;
    var g = function () { return _this.x; };
    return g;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_this_toplevel() {
    assert_projection(
        r#"var g = () => this;
use(g);
"#,
        r#"var _this = this;
var g = function () { return _this; };
use(g);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_nested_this() {
    assert_projection(
        r#"function f() { var g = () => () => this.x; return g; }
use(f);
"#,
        r#"function f() {
    var _this = this;
    var g = function () { return function () { return _this.x; }; };
    return g;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_default_param() {
    assert_projection(
        r#"var f = (a = seed()) => a;
use(f);
"#,
        r#"var f = function (a) {
    if (a === void 0) { a = seed(); }
    return a;
};
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_arrow_in_method_super() {
    assert_projection(
        r#"class D extends B {
  m() { return () => super.m(); }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        return _super !== null && _super.apply(this, arguments) || this;
    }
    D.prototype.m = function () {
        var _this = this;
        return function () { return _super.prototype.m.call(_this); };
    };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_default() {
    assert_projection(
        r#"function f(a, b = a + 1) { return b; }
use(f);
"#,
        r#"function f(a, b) {
    if (b === void 0) { b = a + 1; }
    return b;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_default_two() {
    assert_projection(
        r#"function f(a = one(), b = two(a)) { return a + b; }
use(f);
"#,
        r#"function f(a, b) {
    if (a === void 0) { a = one(); }
    if (b === void 0) { b = two(a); }
    return a + b;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_rest() {
    assert_projection(
        r#"function f(a, ...rest) { return rest.length + a; }
use(f);
"#,
        r#"function f(a) {
    var rest = [];
    for (var _i = 1; _i < arguments.length; _i++) {
        rest[_i - 1] = arguments[_i];
    }
    return rest.length + a;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_rest_only() {
    assert_projection(
        r#"function f(...xs) { return xs; }
use(f);
"#,
        r#"function f() {
    var xs = [];
    for (var _i = 0; _i < arguments.length; _i++) {
        xs[_i] = arguments[_i];
    }
    return xs;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_pattern_array() {
    assert_projection(
        r#"function f([a, b]) { return a + b; }
use(f);
"#,
        r#"function f(_a) {
    var a = _a[0], b = _a[1];
    return a + b;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_pattern_object_default() {
    assert_projection(
        r#"function f({ x = 1 } = {}) { return x; }
use(f);
"#,
        r#"function f(_a) {
    var _b = _a === void 0 ? {} : _a, _c = _b.x, x = _c === void 0 ? 1 : _c;
    return x;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_param_pattern_rest_mix() {
    assert_projection(
        r#"function f(a = 0, [b] = [1], ...rest) { return a + b + rest.length; }
use(f);
"#,
        r#"function f(a, _a) {
    if (a === void 0) { a = 0; }
    var _b = _a === void 0 ? [1] : _a, b = _b[0];
    var rest = [];
    for (var _i = 2; _i < arguments.length; _i++) {
        rest[_i - 2] = arguments[_i];
    }
    return a + b + rest.length;
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_var_array() {
    assert_projection(
        r#"var [a, b] = pair();
use(a, b);
"#,
        r#"var _a = pair(), a = _a[0], b = _a[1];
use(a, b);
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_var_object() {
    assert_projection(
        r#"var { x, y: z } = o;
use(x, z);
"#,
        r#"var x = o.x, z = o.y;
use(x, z);
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_assignment() {
    assert_projection(
        r#"var a, b;
[a, b] = pair();
use(a, b);
"#,
        r#"var _a;
var a, b;
_a = pair(), a = _a[0], b = _a[1];
use(a, b);
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_assignment_expr_value() {
    assert_projection(
        r#"var a, b;
use([a, b] = pair());
"#,
        r#"var _a;
var a, b;
use((_a = pair(), a = _a[0], b = _a[1], _a));
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_let_block() {
    assert_projection(
        r#"{ let [a, b] = pair(); use(a, b); }
var a = 1;
use(a);
"#,
        r#"{
    var _a = pair(), a_1 = _a[0], b = _a[1];
    use(a_1, b);
}
var a = 1;
use(a);
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_for_of_pattern() {
    assert_projection(
        r#"for (var [k, v] of pairs) { use(k, v); }
"#,
        r#"for (var _i = 0, pairs_1 = pairs; _i < pairs_1.length; _i++) {
    var _a = pairs_1[_i], k = _a[0], v = _a[1];
    use(k, v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_catch() {
    assert_projection(
        r#"try { t(); } catch ({ message }) { use(message); }
"#,
        r#"try {
    t();
}
catch (_a) {
    var message = _a.message;
    use(message);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_destr_nested_defaults() {
    assert_projection(
        r#"var { a: { b = one() } = {} } = o;
use(b);
"#,
        r#"var _a = o.a, _b = _a === void 0 ? {} : _a, _c = _b.b, b = _c === void 0 ? one() : _c;
use(b);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_call() {
    assert_projection(
        r#"f(a, ...xs, b);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
f.apply(void 0, __spreadArray(__spreadArray([a], xs, false), [b], false));
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_call_only() {
    assert_projection(
        r#"f(...xs);
"#,
        r#"f.apply(void 0, xs);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_new() {
    assert_projection(
        r#"var r = new C(...xs);
use(r);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var r = new (C.bind.apply(C, __spreadArray([void 0], xs, false)))();
use(r);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_array() {
    assert_projection(
        r#"var r = [1, ...xs, 2];
use(r);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var r = __spreadArray(__spreadArray([1], xs, true), [2], false);
use(r);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_array_literal_seg() {
    assert_projection(
        r#"var r = [...[1, 2], ...xs];
use(r);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var r = __spreadArray(__spreadArray([], [1, 2], false), xs, true);
use(r);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_array_downlevel() {
    assert_projection(
        r#"var r = [1, ...xs, 2];
use(r);
"#,
        r#"var __read = (this && this.__read) || function (o, n) {
    var m = typeof Symbol === "function" && o[Symbol.iterator];
    if (!m) return o;
    var i = m.call(o), r, ar = [], e;
    try {
        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);
    }
    catch (error) { e = { error: error }; }
    finally {
        try {
            if (r && !r.done && (m = i["return"])) m.call(i);
        }
        finally { if (e) throw e.error; }
    }
    return ar;
};
var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var r = __spreadArray(__spreadArray([1], __read(xs), false), [2], false);
use(r);
"#,
        true,
        false,
    );
}

#[test]
fn projects_spread_call_downlevel() {
    assert_projection(
        r#"f(a, ...xs);
"#,
        r#"var __read = (this && this.__read) || function (o, n) {
    var m = typeof Symbol === "function" && o[Symbol.iterator];
    if (!m) return o;
    var i = m.call(o), r, ar = [], e;
    try {
        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);
    }
    catch (error) { e = { error: error }; }
    finally {
        try {
            if (r && !r.done && (m = i["return"])) m.call(i);
        }
        finally { if (e) throw e.error; }
    }
    return ar;
};
var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
f.apply(void 0, __spreadArray([a], __read(xs), false));
"#,
        true,
        false,
    );
}

#[test]
fn projects_spread_method_call() {
    assert_projection(
        r#"o.m(a, ...xs);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
o.m.apply(o, __spreadArray([a], xs, false));
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_elem_call() {
    assert_projection(
        r#"o[k()](...xs);
"#,
        r#"o[k()].apply(o, xs);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_super_call() {
    assert_projection(
        r#"class D extends B {
  constructor() { super(...args()); }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var D = /** @class */ (function (_super) {
    __extends(D, _super);
    function D() {
        return _super.apply(this, args()) || this;
    }
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_template_basic() {
    assert_projection(
        r#"var s = `a${x}b`;
use(s);
"#,
        r#"var s = "a".concat(x, "b");
use(s);
"#,
        false,
        false,
    );
}

#[test]
fn projects_template_expr_only() {
    assert_projection(
        r#"var s = `${x}`;
use(s);
"#,
        r#"var s = "".concat(x);
use(s);
"#,
        false,
        false,
    );
}

#[test]
fn projects_template_no_subst() {
    assert_projection(
        r#"var s = `plain`;
use(s);
"#,
        r#"var s = "plain";
use(s);
"#,
        false,
        false,
    );
}

#[test]
fn projects_template_multi_span() {
    assert_projection(
        r#"var s = `a${x}${y}b${z}`;
use(s);
"#,
        r#"var s = "a".concat(x).concat(y, "b").concat(z);
use(s);
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_array_basic() {
    assert_projection(
        r#"for (var v of xs) { use(v); }
"#,
        r#"for (var _i = 0, xs_1 = xs; _i < xs_1.length; _i++) {
    var v = xs_1[_i];
    use(v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_array_expr() {
    assert_projection(
        r#"for (var v of make()) { use(v); }
"#,
        r#"for (var _i = 0, _a = make(); _i < _a.length; _i++) {
    var v = _a[_i];
    use(v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_let_head() {
    assert_projection(
        r#"for (let v of xs) { use(v); }
"#,
        r#"for (var _i = 0, xs_1 = xs; _i < xs_1.length; _i++) {
    var v = xs_1[_i];
    use(v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_assign_head() {
    assert_projection(
        r#"var v;
for (v of xs) { use(v); }
"#,
        r#"var v;
for (var _i = 0, xs_1 = xs; _i < xs_1.length; _i++) {
    v = xs_1[_i];
    use(v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_iterable() {
    assert_projection(
        r#"for (var v of xs) { use(v); }
"#,
        r#"var __values = (this && this.__values) || function(o) {
    var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
    if (m) return m.call(o);
    if (o && typeof o.length === "number") return {
        next: function () {
            if (o && i >= o.length) o = void 0;
            return { value: o && o[i++], done: !o };
        }
    };
    throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
};
var e_1, _a;
try {
    for (var xs_1 = __values(xs), xs_1_1 = xs_1.next(); !xs_1_1.done; xs_1_1 = xs_1.next()) {
        var v = xs_1_1.value;
        use(v);
    }
}
catch (e_1_1) { e_1 = { error: e_1_1 }; }
finally {
    try {
        if (xs_1_1 && !xs_1_1.done && (_a = xs_1.return)) _a.call(xs_1);
    }
    finally { if (e_1) throw e_1.error; }
}
"#,
        true,
        false,
    );
}

#[test]
fn projects_forof_iterable_nested() {
    assert_projection(
        r#"for (var a of xs) { for (var b of ys) { use(a, b); } }
"#,
        r#"var __values = (this && this.__values) || function(o) {
    var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
    if (m) return m.call(o);
    if (o && typeof o.length === "number") return {
        next: function () {
            if (o && i >= o.length) o = void 0;
            return { value: o && o[i++], done: !o };
        }
    };
    throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
};
var e_1, _a, e_2, _b;
try {
    for (var xs_1 = __values(xs), xs_1_1 = xs_1.next(); !xs_1_1.done; xs_1_1 = xs_1.next()) {
        var a = xs_1_1.value;
        try {
            for (var ys_1 = (e_2 = void 0, __values(ys)), ys_1_1 = ys_1.next(); !ys_1_1.done; ys_1_1 = ys_1.next()) {
                var b = ys_1_1.value;
                use(a, b);
            }
        }
        catch (e_2_1) { e_2 = { error: e_2_1 }; }
        finally {
            try {
                if (ys_1_1 && !ys_1_1.done && (_b = ys_1.return)) _b.call(ys_1);
            }
            finally { if (e_2) throw e_2.error; }
        }
    }
}
catch (e_1_1) { e_1 = { error: e_1_1 }; }
finally {
    try {
        if (xs_1_1 && !xs_1_1.done && (_a = xs_1.return)) _a.call(xs_1);
    }
    finally { if (e_1) throw e_1.error; }
}
"#,
        true,
        false,
    );
}

#[test]
fn projects_forof_labeled_break() {
    assert_projection(
        r#"outer: for (var v of xs) { if (c(v)) break outer; use(v); }
"#,
        r#"outer: for (var _i = 0, xs_1 = xs; _i < xs_1.length; _i++) {
    var v = xs_1[_i];
    if (c(v))
        break outer;
    use(v);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_capture_body() {
    assert_projection(
        r#"for (var e of xs) { let v = e; sink(function () { return v; }); }
"#,
        r#"var __values = (this && this.__values) || function(o) {
    var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
    if (m) return m.call(o);
    if (o && typeof o.length === "number") return {
        next: function () {
            if (o && i >= o.length) o = void 0;
            return { value: o && o[i++], done: !o };
        }
    };
    throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
};
var e_1, _a;
var _loop_1 = function () {
    var v = e;
    sink(function () { return v; });
};
try {
    for (var xs_1 = __values(xs), xs_1_1 = xs_1.next(); !xs_1_1.done; xs_1_1 = xs_1.next()) {
        var e = xs_1_1.value;
        _loop_1();
    }
}
catch (e_1_1) { e_1 = { error: e_1_1 }; }
finally {
    try {
        if (xs_1_1 && !xs_1_1.done && (_a = xs_1.return)) _a.call(xs_1);
    }
    finally { if (e_1) throw e_1.error; }
}
"#,
        true,
        false,
    );
}

#[test]
fn projects_obj_computed_first() {
    assert_projection(
        r#"var o = { [k()]: 1, b: 2 };
use(o);
"#,
        r#"var _a;
var o = (_a = {}, _a[k()] = 1, _a.b = 2, _a);
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_computed_middle() {
    assert_projection(
        r#"var o = { a: 1, [k()]: 2, c: 3 };
use(o);
"#,
        r#"var _a;
var o = (_a = { a: 1 }, _a[k()] = 2, _a.c = 3, _a);
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_computed_multiline() {
    assert_projection(
        r#"var o = {
  a: 1,
  [k()]: 2,
  c: 3
};
use(o);
"#,
        r#"var _a;
var o = (_a = {
        a: 1
    },
    _a[k()] = 2,
    _a.c = 3,
    _a);
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_shorthand() {
    assert_projection(
        r#"var o = { a, b };
use(o);
"#,
        r#"var o = { a: a, b: b };
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_method() {
    assert_projection(
        r#"var o = { m() { return 1; } };
use(o);
"#,
        r#"var o = { m: function () { return 1; } };
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_generator_method() {
    assert_projection(
        r#"var o = { *m() { yield 1; } };
use(o);
"#,
        r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
var o = { m: function () { return __generator(this, function (_a) {
        switch (_a.label) {
            case 0: return [4 /*yield*/, 1];
            case 1:
                _a.sent();
                return [2 /*return*/];
        }
    }); } };
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_accessor_computed_mix() {
    assert_projection(
        r#"var o = { get p() { return 1; }, [k()]: 2, set p(v) { s(v); } };
use(o);
"#,
        r#"var _a;
var o = (_a = { get p() { return 1; } }, _a[k()] = 2, _a);
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_obj_computed_shorthand_method_mix() {
    assert_projection(
        r#"var o = { [k()]: 1, s: a, m() { return 2; } };
use(o);
"#,
        r#"var _a;
var o = (_a = {}, _a[k()] = 1, _a.s = a, _a.m = function () { return 2; }, _a);
use(o);
"#,
        false,
        false,
    );
}

#[test]
fn projects_string_extended_unicode() {
    assert_projection(
        r#"var s = "\u{1F600}";
use(s);
"#,
        r#"var s = "\uD83D\uDE00";
use(s);
"#,
        false,
        false,
    );
}

#[test]
fn projects_numeric_binary_octal() {
    assert_projection(
        r#"var n = 0b101 + 0o17;
use(n);
"#,
        r#"var n = 5 + 15;
use(n);
"#,
        false,
        false,
    );
}

#[test]
fn projects_ident_unicode_escape() {
    assert_projection(
        r#"var \u{61}b = 1;
use(ab);
"#,
        r#"var ab = 1;
use(ab);
"#,
        false,
        false,
    );
}

#[test]
fn projects_switch_in_converted_loop() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { sink(function () { return i; }); switch (i) { case 1: break; } }
"#,
        r#"var _loop_1 = function (i) {
    sink(function () { return i; });
    switch (i) {
        case 1: break;
    }
};
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_labeled_nonloop() {
    assert_projection(
        r#"lbl: { work(); break lbl; }
tail();
"#,
        r#"lbl: {
    work();
    break lbl;
}
tail();
"#,
        false,
        false,
    );
}

#[test]
fn projects_comma_unused_result() {
    assert_projection(
        r#"a(), b();
"#,
        r#"a(), b();
"#,
        false,
        false,
    );
}

#[test]
fn projects_captured_this_collision() {
    assert_projection(
        r#"var _this = 1;
function f() { var g = () => this.x; return [g, _this]; }
use(f);
"#,
        r#"var _this = 1;
function f() {
    var _this_1 = this;
    var g = function () { return _this_1.x; };
    return [g, _this];
}
use(f);
"#,
        false,
        false,
    );
}

#[test]
fn projects_captured_this_two_fns() {
    assert_projection(
        r#"function f() { var g = () => this.x; return g; }
function h() { var g = () => this.y; return g; }
use(f, h);
"#,
        r#"function f() {
    var _this = this;
    var g = function () { return _this.x; };
    return g;
}
function h() {
    var _this = this;
    var g = function () { return _this.y; };
    return g;
}
use(f, h);
"#,
        false,
        false,
    );
}

#[test]
fn projects_synthetic_super_collision() {
    assert_projection(
        r#"var _super = 1;
class D extends B {
  m() { return super.m() + _super; }
}
use(D);
"#,
        r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };
    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();
var _super = 1;
var D = /** @class */ (function (_super_1) {
    __extends(D, _super_1);
    function D() {
        return _super_1 !== null && _super_1.apply(this, arguments) || this;
    }
    D.prototype.m = function () { return _super_1.prototype.m.call(this) + _super; };
    return D;
}(B));
use(D);
"#,
        false,
        false,
    );
}

#[test]
fn projects_loop_capture_labeled_break_mid() {
    assert_projection(
        r#"outer: for (let i = 0; i < n; i++) { mid: for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) break mid; } }
"#,
        r#"var _loop_1 = function (i) {
    var _loop_2 = function (j) {
        sink(function () { return i + j; });
        if (c)
            return "break-mid";
    };
    mid: for (var j = 0; j < m; j++) {
        var state_1 = _loop_2(j);
        switch (state_1) {
            case "break-mid": break mid;
        }
    }
};
outer: for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_uninit_captured_in_loop() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { let v; sink(function () { return v; }); v = q(i); }
"#,
        r#"var _loop_1 = function (i) {
    var v;
    sink(function () { return v; });
    v = q(i);
};
for (var i = 0; i < n; i++) {
    _loop_1(i);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_uninit_colliding_block() {
    assert_projection(
        r#"{ let x; use(x); }
var x = 1;
use(x);
"#,
        r#"{
    var x_1;
    use(x_1);
}
var x = 1;
use(x);
"#,
        false,
        false,
    );
}

#[test]
fn projects_let_uninit_colliding_in_loop() {
    assert_projection(
        r#"for (let i = 0; i < n; i++) { let x; use(x); x = i; }
var x = 9;
use(x);
"#,
        r#"for (var i = 0; i < n; i++) {
    var x_1 = void 0;
    use(x_1);
    x_1 = i;
}
var x = 9;
use(x);
"#,
        false,
        false,
    );
}

#[test]
fn projects_forof_assign_destr_head() {
    assert_projection(
        r#"var a, b;
for ([a, b] of pairs) { use(a, b); }
"#,
        r#"var _a;
var a, b;
for (var _i = 0, pairs_1 = pairs; _i < pairs_1.length; _i++) {
    _a = pairs_1[_i], a = _a[0], b = _a[1];
    use(a, b);
}
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_array_trailing_comma() {
    assert_projection(
        r#"var r = [1, ...xs, 2, ];
use(r);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var r = __spreadArray(__spreadArray([1], xs, true), [2,], false);
use(r);
"#,
        false,
        false,
    );
}

#[test]
fn projects_spread_new_prop_callee() {
    assert_projection(
        r#"var r = new o.C(...xs);
use(r);
"#,
        r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
var _a;
var r = new ((_a = o.C).bind.apply(_a, __spreadArray([void 0], xs, false)))();
use(r);
"#,
        false,
        false,
    );
}

// --- Fault-shaped typed-error contracts (§7): not oracle-mintable — the
// upstream pipeline shields these arms. ---

/// The tagged-template seam: `processTaggedTemplateExpression` is the B-5
/// shared module (gap row 12); until it lands the arm is a TYPED error.
#[test]
fn tagged_template_is_a_typed_error_until_b5() {
    let error = project_error("var r = tag`x${y}`;\nuse(r);\n");
    assert!(
        matches!(error, TransformError::RequiredChildRemoved { .. }),
        "unexpected error shape: {error:?}"
    );
}

/// `addClassMembers`'s `Debug.failBadSyntaxKind` arm: a class
/// PropertyDeclaration (class fields are lowered by earlier passes in the
/// real pipeline).
#[test]
fn class_property_declaration_is_a_typed_error() {
    let error = project_error("class C {\n  x = 1;\n}\nuse(C);\n");
    assert!(
        matches!(error, TransformError::RequiredChildRemoved { .. }),
        "unexpected error shape: {error:?}"
    );
}

/// `transformAccessorsToExpression`'s private-identifier guard
/// (`Debug.failBadSyntaxKind`): private members are class-fields input.
#[test]
fn private_accessor_name_is_a_typed_error() {
    let error = project_error(
        "var o = { get p() { return 1; } };\nclass C {\n  get #p() { return 1; }\n}\nuse(o, C);\n",
    );
    assert!(
        matches!(error, TransformError::RequiredChildRemoved { .. }),
        "unexpected error shape: {error:?}"
    );
}
