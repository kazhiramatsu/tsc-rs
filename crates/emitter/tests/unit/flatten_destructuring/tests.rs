//! H2.5h-b B-2 focused projections (packet §7.3): drive the shared
//! FlattenLevel-All destructuring family directly and byte-compare against
//! fresh-process oracle emits.
//!
//! Every `expected` string below is the COMPLETE oracle output minted by
//! the frozen probe (packet §7.3): vendored `typescript.js` 6.0.3,
//! `ts.createProgram` over one virtual `/project/input.ts`, options
//! `{ target: ES5 (ObjectRest fixtures: ES2017), alwaysStrict: false,
//! newLine: LineFeed, downlevelIteration: per fixture }` — no prologue, LF.
//! Reproduction: `node b2-probe.mjs` (script body reproduced in the PR).

use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};
use tsc_types::NodeFlags;

use super::super::{
    generated_bindings::{AncestorBindingPolicy, GeneratedBindingScopes},
    initialize_transform_flags,
    target_bindings::{collect_untagged_identifier_texts, finalize_generated_binding_names},
};
use super::{
    flatten_destructuring_assignment, flatten_destructuring_binding, FlattenHost, FlattenLevel,
};
use crate::{
    create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest, PrinterOptions,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};

struct FlattenProjectionTransformer {
    level: FlattenLevel,
    downlevel_iteration: bool,
}

impl Transformer for FlattenProjectionTransformer {
    fn name(&self) -> &'static str {
        "flattenDestructuringProjection"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            unreachable!("projection fixtures are single source files");
        };
        initialize_transform_flags(context.arena_mut()?, source)?;
        context.start_lexical_environment()?;
        let current_root = context.arena().root(source)?;
        let mut visitor = ProjectionVisitor {
            generated_bindings: GeneratedBindingScopes::new(
                collect_untagged_identifier_texts(context.arena(), source, current_root)?,
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            level: self.level,
            downlevel_iteration: self.downlevel_iteration,
        };
        let transformed = visitor.visit_source_file(current_root)?;
        let lexical_environment = visitor.context.end_lexical_environment()?;
        let transformed = visitor.merge_hoisted(transformed, lexical_environment)?;
        finalize_generated_binding_names(visitor.context, source, transformed)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

struct ProjectionVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    level: FlattenLevel,
    downlevel_iteration: bool,
    generated_bindings: GeneratedBindingScopes,
}

impl FlattenHost for ProjectionVisitor<'_> {
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
        self.visit_expression_with_use(node, true)
    }

    fn visit_binding_or_assignment_element(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }
}

impl ProjectionVisitor<'_> {
    fn node(&self, id: tsc_syntax::NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    fn visit_source_file(&mut self, root: TransformNode) -> Result<TransformNode, TransformError> {
        let NodeData::SourceFile(data) = self.context.arena().node(root)?.data.clone() else {
            unreachable!("projection root is a source file");
        };
        let statements = self.array_nodes_local(data.statements)?;
        let mut visited = Vec::with_capacity(statements.len());
        for statement in statements {
            visited.push(self.visit_statement(statement)?);
        }
        let updated = data
            .statements
            .map(|array| {
                let array = tsc_syntax_array(self.source, array);
                self.context
                    .factory()?
                    .update_node_array(array, visited.clone())
            })
            .transpose()?;
        let mut data = data;
        data.statements = updated.map(|array| array.array());
        let flags = super::super::flags_after_update(
            self.context.arena(),
            root,
            &NodeData::SourceFile(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn visit_statement(
        &mut self,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(statement)?.data.clone() {
            NodeData::VariableStatement(data) => {
                let Some(list) = data.declaration_list.map(|list| self.node(list)) else {
                    return Ok(statement);
                };
                let NodeData::VariableDeclarationList(list_data) =
                    self.context.arena().node(list)?.data.clone()
                else {
                    return Ok(statement);
                };
                let declarations = self.array_nodes_local(list_data.declarations)?;
                let mut lowered = Vec::with_capacity(declarations.len());
                let mut changed = false;
                for declaration in declarations {
                    let is_pattern = match &self.context.arena().node(declaration)?.data {
                        NodeData::VariableDeclaration(decl) => decl
                            .name
                            .map(|name| self.node(name))
                            .map(|name| {
                                Ok::<_, TransformError>(matches!(
                                    self.context.arena().node(name)?.kind,
                                    SyntaxKind::ObjectBindingPattern
                                        | SyntaxKind::ArrayBindingPattern
                                ))
                            })
                            .transpose()?
                            .unwrap_or(false),
                        _ => false,
                    };
                    if is_pattern {
                        changed = true;
                        lowered.extend(flatten_destructuring_binding(
                            self,
                            declaration,
                            self.level,
                            None,
                            false,
                            false,
                        )?);
                    } else {
                        lowered.push(declaration);
                    }
                }
                if !changed {
                    return Ok(statement);
                }
                let list_flags = NodeFlags::from_bits(self.context.arena().node(list)?.flags);
                let new_array = self
                    .context
                    .factory()?
                    .create_node_array(self.source, lowered)?;
                let transform_flags = self.context.arena().array_transform_flags(new_array)
                    | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
                let new_list = self.context.factory()?.create_node(
                    self.source,
                    NodeData::VariableDeclarationList(
                        tsc_syntax::nodes::VariableDeclarationListData {
                            declarations: Some(new_array.array()),
                        },
                    ),
                    transform_flags,
                )?;
                self.context
                    .factory()?
                    .set_node_flags(new_list, list_flags)?;
                self.context.factory()?.set_text_range(new_list, list)?;
                let mut new_statement_data = data;
                new_statement_data.declaration_list = Some(new_list.node());
                let flags = super::super::flags_after_update(
                    self.context.arena(),
                    statement,
                    &NodeData::VariableStatement(new_statement_data.clone()),
                )?;
                self.context.factory()?.update_node(
                    statement,
                    NodeData::VariableStatement(new_statement_data),
                    flags,
                )
            }
            NodeData::ExpressionStatement(data) => {
                let Some(expression) = data.expression.map(|expression| self.node(expression))
                else {
                    return Ok(statement);
                };
                let visited = self.visit_expression_with_use(expression, false)?;
                if visited == expression {
                    return Ok(statement);
                }
                let mut new_data = data;
                new_data.expression = Some(visited.node());
                let flags = super::super::flags_after_update(
                    self.context.arena(),
                    statement,
                    &NodeData::ExpressionStatement(new_data.clone()),
                )?;
                self.context.factory()?.update_node(
                    statement,
                    NodeData::ExpressionStatement(new_data),
                    flags,
                )
            }
            _ => Ok(statement),
        }
    }

    fn visit_expression_with_use(
        &mut self,
        expression: TransformNode,
        needs_value: bool,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(expression)?.data.clone() {
            NodeData::ParenthesizedExpression(data) => {
                let Some(inner) = data.expression.map(|inner| self.node(inner)) else {
                    return Ok(expression);
                };
                let visited = self.visit_expression_with_use(inner, needs_value)?;
                if visited == inner {
                    return Ok(expression);
                }
                let mut new_data = data;
                new_data.expression = Some(visited.node());
                let flags = super::super::flags_after_update(
                    self.context.arena(),
                    expression,
                    &NodeData::ParenthesizedExpression(new_data.clone()),
                )?;
                self.context.factory()?.update_node(
                    expression,
                    NodeData::ParenthesizedExpression(new_data),
                    flags,
                )
            }
            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|operator| self.node(operator))
                    .map(|operator| {
                        Ok::<_, TransformError>(self.context.arena().node(operator)?.kind)
                    })
                    .transpose()?;
                let left_is_pattern = data
                    .left
                    .map(|left| self.node(left))
                    .map(|left| {
                        Ok::<_, TransformError>(matches!(
                            self.context.arena().node(left)?.kind,
                            SyntaxKind::ObjectLiteralExpression
                                | SyntaxKind::ArrayLiteralExpression
                        ))
                    })
                    .transpose()?
                    .unwrap_or(false);
                if operator == Some(SyntaxKind::EqualsToken) && left_is_pattern {
                    return flatten_destructuring_assignment(
                        self,
                        expression,
                        self.level,
                        needs_value,
                        false,
                    );
                }
                if operator == Some(SyntaxKind::EqualsToken) {
                    // plain assignment: the value position requires a value
                    let Some(right) = data.right.map(|right| self.node(right)) else {
                        return Ok(expression);
                    };
                    let visited = self.visit_expression_with_use(right, true)?;
                    if visited == right {
                        return Ok(expression);
                    }
                    let mut new_data = data;
                    new_data.right = Some(visited.node());
                    let flags = super::super::flags_after_update(
                        self.context.arena(),
                        expression,
                        &NodeData::BinaryExpression(new_data.clone()),
                    )?;
                    return self.context.factory()?.update_node(
                        expression,
                        NodeData::BinaryExpression(new_data),
                        flags,
                    );
                }
                Ok(expression)
            }
            _ => Ok(expression),
        }
    }

    fn merge_hoisted(
        &mut self,
        root: TransformNode,
        lexical_environment: crate::LexicalEnvironment,
    ) -> Result<TransformNode, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(root);
        }
        let NodeData::SourceFile(mut data) = self.context.arena().node(root)?.data.clone() else {
            unreachable!("projection root is a source file");
        };
        let mut statements = self.array_nodes_local(data.statements)?;
        let declarations = lexical_environment
            .variable_declarations()
            .iter()
            .copied()
            .map(|name| self.create_hoisted_declaration(name))
            .collect::<Result<Vec<_>, _>>()?;
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list_flags = self.context.arena().array_transform_flags(declarations)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            list_flags,
        )?;
        self.context
            .factory()?
            .set_node_flags(list, NodeFlags::NONE)?;
        let statement_flags = self.context.arena().propagate_child_flags(list)?;
        let statement = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            statement_flags,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        statements.insert(0, statement);
        let updated = data
            .statements
            .map(|array| {
                let array = tsc_syntax_array(self.source, array);
                self.context
                    .factory()?
                    .update_node_array(array, statements.clone())
            })
            .transpose()?;
        data.statements = updated.map(|array| array.array());
        let flags = super::super::flags_after_update(
            self.context.arena(),
            root,
            &NodeData::SourceFile(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn create_hoisted_declaration(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            flags,
        )
    }

    fn array_nodes_local(
        &self,
        array: Option<tsc_syntax::NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) =
            array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        else {
            return Ok(Vec::new());
        };
        let nodes = self.context.arena().node_array(array)?.nodes.clone();
        Ok(nodes.iter().map(|node| self.node(*node)).collect())
    }
}

fn tsc_syntax_array(
    source: TransformSourceId,
    array: tsc_syntax::NodeArrayId,
) -> crate::TransformNodeArray {
    crate::TransformNodeArray::new(source, array)
}

fn project(source_text: &str, level: FlattenLevel, downlevel_iteration: bool) -> String {
    let parsed = parse_source_file("input.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(FlattenProjectionTransformer {
            level,
            downlevel_iteration,
        })],
        false,
    )
    .expect("flatten projection transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(tsc_types::ScriptTarget::ES5),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print flatten projection")
    .text()
    .to_owned()
}

fn project_all(source_text: &str) -> String {
    project(source_text, FlattenLevel::All, false)
}

#[test]
fn projects_obj_single() {
    assert_eq!(project_all("var { a } = obj;\n"), "var a = obj.a;\n");
}

#[test]
fn projects_obj_multi() {
    assert_eq!(
        project_all("var { a, b } = obj;\n"),
        "var a = obj.a, b = obj.b;\n"
    );
}

#[test]
fn projects_obj_default() {
    assert_eq!(
        project_all("var { b = 1 } = obj;\n"),
        "var _a = obj.b, b = _a === void 0 ? 1 : _a;\n"
    );
}

#[test]
fn projects_obj_computed() {
    assert_eq!(
        project_all("var { [k]: c } = obj;\n"),
        "var _a = obj, _b = k, c = _a[_b];\n"
    );
}

#[test]
fn projects_obj_computed_literal() {
    assert_eq!(
        project_all("var { [\"s\"]: c } = obj;\n"),
        "var c = obj[\"s\"];\n"
    );
}

#[test]
fn projects_obj_string_prop() {
    assert_eq!(
        project_all("var { \"s p\": c } = obj;\n"),
        "var c = obj[\"s p\"];\n"
    );
}

#[test]
fn projects_obj_numeric_prop() {
    assert_eq!(project_all("var { 1: c } = obj;\n"), "var c = obj[1];\n");
}

#[test]
fn projects_obj_nested() {
    assert_eq!(
        project_all("var { a: { b } } = obj;\n"),
        "var b = obj.a.b;\n"
    );
}

#[test]
fn projects_obj_nested_default() {
    assert_eq!(
        project_all("var { a: { b } = { b: 1 } } = obj;\n"),
        "var _a = obj.a, _b = _a === void 0 ? { b: 1 } : _a, b = _b.b;\n"
    );
}

#[test]
fn projects_obj_empty() {
    assert_eq!(project_all("var {} = init();\n"), "var _a = init();\n");
}

#[test]
fn projects_arr_single() {
    assert_eq!(project_all("var [x] = arr;\n"), "var x = arr[0];\n");
}

#[test]
fn projects_arr_holes_default_rest() {
    assert_eq!(
        project_all("var [x, , y = 2, ...zs] = arr;\n"),
        "var x = arr[0], _a = arr[2], y = _a === void 0 ? 2 : _a, zs = arr.slice(3);\n"
    );
}

#[test]
fn projects_arr_empty() {
    assert_eq!(project_all("var [] = init();\n"), "var _a = init();\n");
}

#[test]
fn projects_arr_nested_deep() {
    assert_eq!(
        project_all("var { a: [b, { c = 3 }] } = obj;\n"),
        "var _a = obj.a, b = _a[0], _b = _a[1].c, c = _b === void 0 ? 3 : _b;\n"
    );
}

#[test]
fn projects_arr_downlevel_pair() {
    assert_eq!(project("var [x, y] = pair;\n", FlattenLevel::All, true), "var __read = (this && this.__read) || function (o, n) {\n    var m = typeof Symbol === \"function\" && o[Symbol.iterator];\n    if (!m) return o;\n    var i = m.call(o), r, ar = [], e;\n    try {\n        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);\n    }\n    catch (error) { e = { error: error }; }\n    finally {\n        try {\n            if (r && !r.done && (m = i[\"return\"])) m.call(i);\n        }\n        finally { if (e) throw e.error; }\n    }\n    return ar;\n};\nvar _a = __read(pair, 2), x = _a[0], y = _a[1];\n");
}

#[test]
fn projects_arr_downlevel_rest() {
    assert_eq!(project("var [x, ...r] = arr;\n", FlattenLevel::All, true), "var __read = (this && this.__read) || function (o, n) {\n    var m = typeof Symbol === \"function\" && o[Symbol.iterator];\n    if (!m) return o;\n    var i = m.call(o), r, ar = [], e;\n    try {\n        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);\n    }\n    catch (error) { e = { error: error }; }\n    finally {\n        try {\n            if (r && !r.done && (m = i[\"return\"])) m.call(i);\n        }\n        finally { if (e) throw e.error; }\n    }\n    return ar;\n};\nvar _a = __read(arr), x = _a[0], r = _a.slice(1);\n");
}

#[test]
fn projects_arr_downlevel_empty() {
    assert_eq!(project("var [] = init();\n", FlattenLevel::All, true), "var __read = (this && this.__read) || function (o, n) {\n    var m = typeof Symbol === \"function\" && o[Symbol.iterator];\n    if (!m) return o;\n    var i = m.call(o), r, ar = [], e;\n    try {\n        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);\n    }\n    catch (error) { e = { error: error }; }\n    finally {\n        try {\n            if (r && !r.done && (m = i[\"return\"])) m.call(i);\n        }\n        finally { if (e) throw e.error; }\n    }\n    return ar;\n};\nvar _a = __read(init(), 0);\n");
}

#[test]
fn projects_assign_unused() {
    assert_eq!(
        project_all("({ a, b } = obj);\n"),
        "(a = obj.a, b = obj.b);\n"
    );
}

#[test]
fn projects_assign_used() {
    assert_eq!(
        project_all("r = ({ a } = obj);\n"),
        "r = (a = obj.a, obj);\n"
    );
}

#[test]
fn projects_assign_collision() {
    assert_eq!(
        project_all("({ x } = x);\n"),
        "var _a;\n(_a = x, x = _a.x);\n"
    );
}

#[test]
fn projects_assign_empty_unwrap() {
    assert_eq!(project_all("({} = {} = obj);\n"), "(obj);\n");
}

#[test]
fn projects_assign_array() {
    assert_eq!(
        project_all("[x, y = 1] = arr;\n"),
        "var _a;\nx = arr[0], _a = arr[1], y = _a === void 0 ? 1 : _a;\n"
    );
}

#[test]
fn projects_objectrest_binding() {
    assert_eq!(project("var { a, ...rest } = obj;\n", FlattenLevel::ObjectRest, false), "var __rest = (this && this.__rest) || function (s, e) {\n    var t = {};\n    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)\n        t[p] = s[p];\n    if (s != null && typeof Object.getOwnPropertySymbols === \"function\")\n        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {\n            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))\n                t[p[i]] = s[p[i]];\n        }\n    return t;\n};\nvar { a } = obj, rest = __rest(obj, [\"a\"]);\n");
}

#[test]
fn projects_objectrest_computed() {
    assert_eq!(project("var { [k]: c, ...rest } = obj;\n", FlattenLevel::ObjectRest, false), "var __rest = (this && this.__rest) || function (s, e) {\n    var t = {};\n    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)\n        t[p] = s[p];\n    if (s != null && typeof Object.getOwnPropertySymbols === \"function\")\n        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {\n            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))\n                t[p[i]] = s[p[i]];\n        }\n    return t;\n};\nvar _a = obj, _b = k, c = _a[_b], rest = __rest(_a, [typeof _b === \"symbol\" ? _b : _b + \"\"]);\n");
}

#[test]
fn projects_objectrest_assign() {
    assert_eq!(project("({ a, ...r } = o);\n", FlattenLevel::ObjectRest, false), "var __rest = (this && this.__rest) || function (s, e) {\n    var t = {};\n    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)\n        t[p] = s[p];\n    if (s != null && typeof Object.getOwnPropertySymbols === \"function\")\n        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {\n            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))\n                t[p[i]] = s[p[i]];\n        }\n    return t;\n};\n({ a } = o, r = __rest(o, [\"a\"]));\n");
}

#[test]
fn projects_effectful_computed_rebind() {
    assert_eq!(
        project_all("var { [k()]: c } = o;\n"),
        "var _a = o, _b = k(), c = _a[_b];\n"
    );
}

fn project_error(source_text: &str, level: FlattenLevel) -> TransformError {
    let parsed = parse_source_file("input.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    match transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(FlattenProjectionTransformer {
            level,
            downlevel_iteration: false,
        })],
        false,
    ) {
        Ok(_) => panic!("the projection must fail closed"),
        Err(error) => error,
    }
}

/// The §4.3 frozen crash edge: an all-omitted assignment pattern with an
/// unused result reaches `inlineExpressions(undefined)` upstream and
/// crashes (vendored 6.0.3, reproduced at review); the port is a typed
/// fail-closed arm.
#[test]
fn all_omitted_unused_assignment_is_a_typed_error() {
    let error = project_error("[,,] = x;\n", FlattenLevel::All);
    assert!(
        matches!(
            error,
            TransformError::RequiredChildRemoved {
                field: "flattened assignment expressions",
                ..
            }
        ),
        "unexpected error: {error:?}"
    );
}

/// The needsValue sibling of the crash edge returns the bare value
/// (oracle: `y = ([,,] = x);` emits `y = (x);`).
#[test]
fn all_omitted_used_assignment_returns_the_bare_value() {
    assert_eq!(project_all("y = ([,,] = x);\n"), "y = (x);\n");
}

/// An object-literal method element has no binding-or-assignment target
/// (`getTargetOfBindingOrAssignmentElement` returns undefined for the
/// remaining `isObjectLiteralElementLike` kinds), so
/// `getPropertyNameOfBindingOrAssignmentElement`'s must-exist assert is
/// the upstream failure point; the port fails closed at the same spot.
#[test]
fn method_element_in_assignment_pattern_is_a_typed_error() {
    let error = project_error("({ m() { } } = o);\n", FlattenLevel::All);
    assert!(
        matches!(
            error,
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::MethodDeclaration,
                field: "property name",
            }
        ),
        "unexpected error: {error:?}"
    );
}
