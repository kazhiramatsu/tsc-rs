use super::*;
use crate::containers::{get_module_instance_state, ModuleInstanceState};
use tsc_syntax::{parse_source_file, ParseOptions, SourceFile};
use tsc_types::CompilerOptions;

fn parse(text: &str) -> SourceFile {
    parse_source_file("main.ts", text, ParseOptions::default(), None)
}

fn default_options() -> CompilerOptions {
    CompilerOptions::default()
}

fn bind(source: &SourceFile) -> Binder<'_> {
    // Leak the options for test lifetimes only.
    let options: &'static CompilerOptions = Box::leak(Box::new(default_options()));
    let mut binder = Binder::new(source, options);
    binder.bind_source_file();
    binder
}

#[test]
fn jsdoc_function_type_binds_call_and_construct_members() {
    for (text, member) in [
        (
            "let ctor: function(new: number, string);\n",
            InternalSymbolName::NEW,
        ),
        (
            "let call: function(this: number, string): string;\n",
            InternalSymbolName::CALL,
        ),
    ] {
        let source = parse(text);
        let function_type = find_nodes(&source, SyntaxKind::JSDocFunctionType)[0];
        assert_eq!(
            crate::node_util::is_jsdoc_construct_signature(&source, function_type),
            member == InternalSymbolName::NEW
        );
        let binder = bind(&source);
        let symbol = binder.node_symbol[&function_type];
        assert!(binder.symbols.symbol(symbol).members.contains_key(member));
    }
}

#[test]
fn jsdoc_reference_contexts_match_expression_classification() {
    let source = parse(
        "/**\n\
             * See {@link ns.Member} and {@link ns#member}.\n\
             * @see Other#field\n\
             * @extends {Base<string>}\n\
             */\n\
             class Derived {}\n",
    );

    let links = find_nodes(&source, SyntaxKind::JSDocLink);
    assert_eq!(links.len(), 2);
    for link in links {
        let NodeData::JSDocLink(data) = &source.arena.node(link).data else {
            unreachable!("kind/data agree");
        };
        let name = data.name.expect("parsed link name");
        assert!(crate::node_util::is_expression_node(&source, name));
    }

    let name_reference = find_nodes(&source, SyntaxKind::JSDocNameReference)
        .into_iter()
        .next()
        .expect("parsed @see name reference");
    let NodeData::JSDocNameReference(data) = &source.arena.node(name_reference).data else {
        unreachable!("kind/data agree");
    };
    assert!(crate::node_util::is_expression_node(
        &source,
        data.name.expect("@see entity name"),
    ));
    assert!(find_nodes(&source, SyntaxKind::JSDocMemberName)
        .into_iter()
        .all(|node| crate::node_util::is_expression_node(&source, node)));

    let augments_type = find_nodes(&source, SyntaxKind::ExpressionWithTypeArguments)
        .into_iter()
        .find(|&node| {
            source.arena.node(node).parent.is_some_and(|parent| {
                source.arena.node(parent).kind == SyntaxKind::JSDocAugmentsTag
            })
        })
        .expect("parsed @extends type");
    assert!(!crate::node_util::is_expression_node(
        &source,
        augments_type,
    ));
}

fn find_nodes(source: &SourceFile, kind: SyntaxKind) -> Vec<NodeId> {
    (0..source.arena.len() as u32)
        .map(NodeId)
        .filter(|&id| source.arena.node(id).kind == kind)
        .collect()
}

fn diag_pins(binder: &Binder<'_>) -> Vec<(u32, u32, u32)> {
    binder
        .bind_diagnostics
        .iter()
        .map(|diag| (diag.code(), diag.start.unwrap(), diag.length.unwrap()))
        .collect()
}

#[test]
fn container_flags_table_pins() {
    let source = parse(
        "function f() { { let a; } }\n\
             const g = function() {};\n\
             const h = () => 1;\n\
             const o = { m() {} };\n\
             class C { m() {} constructor() {} }\n\
             interface I { m(): void }\n\
             namespace N { }\n\
             type T = { a: string };\n",
    );
    let flags = |id: NodeId| get_container_flags(&source, id);
    assert_eq!(flags(source.root).0, 1 | 4 | 32);
    let function = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    assert_eq!(flags(function).0, 1 | 4 | 32 | 8);
    let function_expression = find_nodes(&source, SyntaxKind::FunctionExpression)[0];
    assert_eq!(flags(function_expression).0, 1 | 4 | 32 | 8 | 16);
    let arrow = find_nodes(&source, SyntaxKind::ArrowFunction)[0];
    assert_eq!(flags(arrow).0, 1 | 4 | 32 | 8 | 16 | 256);
    let methods = find_nodes(&source, SyntaxKind::MethodDeclaration);
    assert_eq!(flags(methods[0]).0, 1 | 4 | 32 | 8 | 128);
    assert_eq!(flags(methods[1]).0, 1 | 4 | 32 | 8);
    let constructor = find_nodes(&source, SyntaxKind::Constructor)[0];
    assert_eq!(flags(constructor).0, 1 | 4 | 32 | 8);
    let interface = find_nodes(&source, SyntaxKind::InterfaceDeclaration)[0];
    assert_eq!(flags(interface).0, 1 | 64);
    let module = find_nodes(&source, SyntaxKind::ModuleDeclaration)[0];
    assert_eq!(flags(module).0, 1 | 32);
    let method_signature = find_nodes(&source, SyntaxKind::MethodSignature)[0];
    assert_eq!(flags(method_signature).0, 1 | 4 | 32 | 8 | 256);
    let blocks = find_nodes(&source, SyntaxKind::Block);
    assert_eq!(flags(blocks[0]).0, 2 | 32);
    assert_eq!(flags(blocks[1]).0, 0);
}

#[test]
fn property_declaration_is_flow_container_only_with_initializer() {
    let source = parse("class C { a = 1; b: string; }\n");
    let properties = find_nodes(&source, SyntaxKind::PropertyDeclaration);
    assert_eq!(get_container_flags(&source, properties[0]).0, 4);
    assert_eq!(get_container_flags(&source, properties[1]).0, 0);
}

#[test]
fn module_instance_state_pins() {
    let source = parse(
        "namespace A { interface I {} type T = I; }\n\
             namespace B { const enum E { X } }\n\
             namespace C { var v: number; }\n\
             namespace D { export { I2 }; interface I2 {} }\n\
             interface I2 {}\n",
    );
    let modules = find_nodes(&source, SyntaxKind::ModuleDeclaration);
    let state =
        |id: NodeId| get_module_instance_state(&source, id, &mut std::collections::HashMap::new());
    assert_eq!(state(modules[0]), ModuleInstanceState::NonInstantiated);
    assert_eq!(state(modules[1]), ModuleInstanceState::ConstEnumOnly);
    assert_eq!(state(modules[2]), ModuleInstanceState::Instantiated);
    assert_eq!(state(modules[3]), ModuleInstanceState::NonInstantiated);
}

#[test]
fn end_to_end_bind_declares_top_level_symbols() {
    let source = parse(
        "function f(x: number) {}\nfunction f(x: string) {}\n\
             class C { m() {} }\ninterface I { a: string }\n\
             enum E { A, B }\nnamespace N { export const v = 1; }\n",
    );
    let binder = bind(&source);
    let locals = binder.locals.get(&source.root).expect("file locals");
    // Overloads merged into one symbol.
    let f = locals["f"];
    assert_eq!(binder.symbols.symbol(f).declarations.len(), 2);
    // Class exports carry the synthetic prototype symbol.
    let class_symbol = locals["C"];
    assert!(binder
        .symbols
        .symbol(class_symbol)
        .exports
        .contains_key("prototype"));
    assert!(binder
        .symbols
        .symbol(class_symbol)
        .members
        .contains_key("m"));
    // Interface members, enum members (exports), namespace exports.
    assert!(binder.symbols.symbol(locals["I"]).members.contains_key("a"));
    let enum_symbol = locals["E"];
    assert!(binder.symbols.symbol(enum_symbol).exports.contains_key("A"));
    let namespace_symbol = locals["N"];
    assert!(binder
        .symbols
        .symbol(namespace_symbol)
        .exports
        .contains_key("v"));
    assert!(binder.bind_diagnostics.is_empty());
}

#[test]
fn duplicate_diagnostic_order_matches_functions_first_binding() {
    // Oracle pins: "var f: any;\nfunction f() {}" reports 2300 at
    // (21,1) then (4,1) because the FUNCTION binds first.
    let source = parse("var f: any;\nfunction f() {}");
    let binder = bind(&source);
    assert_eq!(diag_pins(&binder), [(2300, 21, 1), (2300, 4, 1)]);
}

#[test]
fn module_always_strict_reserves_future_reserved_words() {
    // Oracle-pinned: `var private = 1; export {};` in a module file
    // reports 1214 (the Modules-are-automatically-strict variant)
    // at (4,7).
    let source = parse("var private = 1;\nexport {};\n");
    assert!(source.external_module_indicator.is_some());
    let binder = bind(&source);
    assert_eq!(diag_pins(&binder), [(1214, 4, 7)]);
}

#[test]
fn use_strict_prologue_flips_strict_mode() {
    let source = parse("\"use strict\";\nvar eval = 1;\n");
    let binder = bind(&source);
    // Oracle-pinned: 1100 Invalid use of 'eval' in strict mode @(18,4).
    assert_eq!(diag_pins(&binder), [(1100, 18, 4)]);
}

#[test]
fn export_assignment_conflict_reports_2528_end_to_end() {
    // Oracle pins from stage 3.2: "export default 1;\nexport default 2;"
    let source = parse("export default 1;\nexport default 2;");
    let binder = bind(&source);
    assert_eq!(diag_pins(&binder), [(2528, 0, 17), (2528, 18, 17)]);
}

#[test]
fn export_default_identifier_keeps_current_flow() {
    let source = parse("export default x;\nconst x = 'x';\n");
    let export = find_nodes(&source, SyntaxKind::ExportAssignment)[0];
    let expression = match &source.arena.node(export).data {
        NodeData::ExportAssignment(data) => data.expression.expect("export expression"),
        _ => unreachable!(),
    };
    let binder = bind(&source);
    assert!(binder.node_flow.contains_key(&expression));
}

#[test]
fn external_module_file_symbol_and_export_links() {
    let source = parse("export function f() {}\nconst local = 1;\n");
    let binder = bind(&source);
    let file_symbol = binder.node_symbol[&source.root];
    assert_eq!(binder.symbols.symbol(file_symbol).escaped_name, "\"main\"");
    assert!(binder.symbols.symbol(file_symbol).exports.contains_key("f"));
    // Local side of the exported function is linked.
    let locals = binder.locals.get(&source.root).expect("locals");
    let local_f = locals["f"];
    assert!(binder.symbols.symbol(local_f).export_symbol.is_some());
    assert!(locals.contains_key("local"));
}

#[test]
fn infer_type_parameter_binds_into_conditional_type_locals() {
    let source = parse("type T<X> = X extends Array<infer U> ? U : never;\n");
    let binder = bind(&source);
    let conditional = find_nodes(&source, SyntaxKind::ConditionalType)[0];
    let locals = binder.locals.get(&conditional).expect("conditional locals");
    assert!(locals.contains_key("U"));
}

#[test]
fn ambient_module_pattern_and_export_modifier_diagnostics() {
    // Oracle pins: 'declare module "a*b*c" {}' -> 5061@(15,7);
    // 'export declare module "m" {}' -> 2668@(0,6).
    for (text, code, start, length) in [
        ("declare module \"a*b*c\" {}\n", 5061u32, 15u32, 7u32),
        ("export declare module \"m\" {}\n", 2668, 0, 6),
    ] {
        let source = parse(text);
        let binder = bind(&source);
        assert_eq!(diag_pins(&binder), [(code, start, length)], "case: {text}");
    }
    // A single star is a valid pattern; it lands in
    // patternAmbientModules.
    let source = parse("declare module \"good*\" {}\n");
    let binder = bind(&source);
    assert!(binder.bind_diagnostics.is_empty());
    assert_eq!(binder.pattern_ambient_modules.len(), 1);
    assert_eq!(binder.pattern_ambient_modules[0].0, "good");
}

// ---- stage 3.5 flow-shape pins (each names its tsc anchor) ----

fn flow_flags(binder: &Binder<'_>, id: crate::flow::FlowId) -> tsc_types::FlowFlags {
    binder.flow.flow(id).flags
}

#[test]
fn if_statement_join_has_two_antecedents_and_condition_nodes() {
    // bindIfStatement (43277) + createFlowCondition (43107): a
    // narrowable condition creates True/FalseCondition nodes; the
    // post-if label joins both branches.
    let source = parse("function f(x: any) { if (x) { x; } else { x; } x; }\n");
    let binder = bind(&source);
    let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let end = binder.node_end_flow[&f];
    let end_flags = flow_flags(&binder, end);
    assert!(end_flags.intersects(tsc_types::FlowFlags::BRANCH_LABEL));
    assert_eq!(binder.flow.flow(end).antecedent.len(), 2);
    for &antecedent in &binder.flow.flow(end).antecedent {
        assert!(flow_flags(&binder, antecedent).intersects(
            tsc_types::FlowFlags::TRUE_CONDITION | tsc_types::FlowFlags::FALSE_CONDITION
        ));
    }
}

#[test]
fn non_narrowing_condition_creates_no_flow_nodes() {
    // createFlowCondition returns its antecedent for non-narrowing
    // expressions: both branches join the SAME node and the label
    // collapses back to Start.
    let source = parse("function f() { if (1) { } else { } }\n");
    let binder = bind(&source);
    let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let end = binder.node_end_flow[&f];
    assert!(flow_flags(&binder, end).intersects(tsc_types::FlowFlags::START));
}

#[test]
fn while_loop_label_gets_entry_and_back_edge() {
    // bindWhileStatement (43218): preWhileLabel is a LoopLabel with
    // the entry edge and the loop-body back edge.
    let source = parse("function f(x: any) { while (x) { x; } }\n");
    let binder = bind(&source);
    let loop_labels: Vec<_> = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| flow_flags(&binder, id).intersects(tsc_types::FlowFlags::LOOP_LABEL))
        .collect();
    assert_eq!(loop_labels.len(), 1);
    assert_eq!(binder.flow.flow(loop_labels[0]).antecedent.len(), 2);
}

#[test]
fn code_after_return_is_unreachable() {
    // bindReturnOrThrow (43290) sets currentFlow = unreachableFlow;
    // bindChildren stamps the Unreachable node flag.
    let source = parse("function f() { return; f(); }\n");
    let binder = bind(&source);
    let statements = find_nodes(&source, SyntaxKind::ExpressionStatement);
    assert!(binder
        .flags_of(statements[0])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
    assert!(!binder.node_flow.contains_key(&statements[0]));
    // The function has an explicit return and NO implicit return.
    let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    assert!(!binder
        .flags_of(f)
        .intersects(tsc_types::NodeFlags::HAS_IMPLICIT_RETURN));
}

#[test]
fn try_finally_produces_reduce_labels() {
    // bindTryStatement (43332): finally wiring reduces through
    // ReduceLabel nodes.
    let source = parse("function f(x: any) { try { x(); } catch (e) { x; } finally { x; } x; }\n");
    let binder = bind(&source);
    let reduce_count = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| flow_flags(&binder, id).intersects(tsc_types::FlowFlags::REDUCE_LABEL))
        .count();
    assert!(reduce_count >= 1, "expected ReduceLabel nodes, got none");
}

#[test]
fn narrowing_switch_creates_switch_clause_nodes() {
    // bindCaseBlock (43393) + createFlowSwitchClause (43123): a
    // narrowing switch expression yields per-clause SwitchClause
    // nodes plus the implicit-default clause (bindSwitchStatement).
    let source = parse(
            "function f(x: string | number) { switch (typeof x) { case \"string\": x; break; case \"number\": x; break; } x; }\n",
        );
    let binder = bind(&source);
    let switch_statement = find_nodes(&source, SyntaxKind::SwitchStatement)[0];
    let clauses: Vec<_> = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| flow_flags(&binder, id).intersects(tsc_types::FlowFlags::SWITCH_CLAUSE))
        .collect();
    // 2 case clauses + the implicit default (clauseStart==clauseEnd==0).
    assert_eq!(clauses.len(), 3);
    let implicit_default = clauses.iter().any(|&id| {
        matches!(
            binder.flow.flow(id).payload,
            crate::flow::FlowPayload::SwitchClause {
                switch_statement: s,
                clause_start: 0,
                clause_end: 0,
            } if s == switch_statement
        )
    });
    assert!(implicit_default);
    assert_eq!(
        binder.possibly_exhaustive.get(&switch_statement),
        Some(&false)
    );
}

#[test]
fn assignment_creates_flow_mutation_and_stamps_references() {
    // bindAssignmentTargetFlow (43462) + the Identifier flowNode
    // stamp in bindWorker.
    let source = parse("function f(x: any) { x = 1; x; }\n");
    let binder = bind(&source);
    let assignments = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| flow_flags(&binder, id).intersects(tsc_types::FlowFlags::ASSIGNMENT))
        .count();
    assert_eq!(assignments, 1);
    // The trailing reference's flowNode is the Assignment node.
    let identifiers = find_nodes(&source, SyntaxKind::Identifier);
    let last_x = *identifiers.last().unwrap();
    let flow = binder.node_flow[&last_x];
    assert!(flow_flags(&binder, flow).intersects(tsc_types::FlowFlags::ASSIGNMENT));
}

#[test]
fn logical_expression_in_condition_adds_no_top_level_condition_nodes() {
    // bindCondition (43193): logical operators create their edges
    // during sub-expression binding — the a && b condition itself
    // adds no extra nodes on top.
    let source = parse("function f(a: any, b: any) { if (a && b) { a; } }\n");
    let binder = bind(&source);
    // Conditions come from `a` and from `b`, joined by the then/
    // else labels: the then-branch flow has 2 antecedents (a-true
    // via preRight collapse and b-true).
    let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let end = binder.node_end_flow[&f];
    // post-if joins then-branch and else-label.
    assert!(flow_flags(&binder, end).intersects(tsc_types::FlowFlags::BRANCH_LABEL));
}

#[test]
fn optional_chain_creates_outermost_conditions() {
    // bindOptionalChain (43768): the outermost chain contributes
    // True/FalseCondition nodes.
    let source = parse("function f(a: any) { if (a?.b) { a; } }\n");
    let binder = bind(&source);
    let conditions = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| {
            flow_flags(&binder, id).intersects(
                tsc_types::FlowFlags::TRUE_CONDITION | tsc_types::FlowFlags::FALSE_CONDITION,
            )
        })
        .count();
    assert!(
        conditions >= 2,
        "expected chain conditions, got {conditions}"
    );
}

#[test]
fn deep_binary_chain_binds_without_overflow() {
    // createBindBinaryExpressionFlow (43540) is a non-recursive
    // work-stack machine; a deep chain must not overflow.
    let mut text = String::from("const x = 1");
    for _ in 0..50_000 {
        text.push_str(" + 1");
    }
    text.push_str(";\n");
    let source = parse(&text);
    let binder = bind(&source);
    assert!(!binder.symbols.is_empty());
}

#[test]
fn labeled_statement_break_references_label() {
    // bindLabeledStatement (43437) + bindBreakOrContinueStatement
    // (43320): a referenced label keeps its flag clear; an
    // unreferenced label is stamped Unreachable.
    let source = parse("function f(x: any) { a: { if (x) break a; x; } b: { x; } }\n");
    let binder = bind(&source);
    let labels: Vec<_> = find_nodes(&source, SyntaxKind::LabeledStatement)
        .into_iter()
        .filter_map(|statement| match &source.arena.node(statement).data {
            NodeData::LabeledStatement(data) => data.label,
            _ => None,
        })
        .collect();
    assert!(!binder
        .flags_of(labels[0])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
    assert!(binder
        .flags_of(labels[1])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
}

fn statement_labels(source: &SourceFile) -> Vec<NodeId> {
    find_nodes(source, SyntaxKind::LabeledStatement)
        .into_iter()
        .filter_map(|statement| match &source.arena.node(statement).data {
            NodeData::LabeledStatement(data) => data.label,
            _ => None,
        })
        .collect()
}

#[test]
fn nested_function_break_cannot_see_outer_label() {
    // bindContainer (42734): a control-flow container saves and
    // CLEARS activeLabelList (42761/42781/42812), so `break outer`
    // inside a nested function finds no active label: no flow edge
    // crosses the function boundary and the outer label stays
    // unreferenced (stamped Unreachable).
    let source = parse("outer: { function f() { break outer; } }\n");
    let binder = bind(&source);
    let labels = statement_labels(&source);
    assert!(binder
        .flags_of(labels[0])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
    // The only BranchLabel is outer's post-statement label; the
    // nested break must not have added a second antecedent.
    let branch_labels: Vec<_> = (0..binder.flow.len() as u32)
        .map(crate::flow::FlowId)
        .filter(|&id| flow_flags(&binder, id).intersects(tsc_types::FlowFlags::BRANCH_LABEL))
        .collect();
    assert_eq!(branch_labels.len(), 1);
    assert_eq!(binder.flow.flow(branch_labels[0]).antecedent.len(), 1);
}

#[test]
fn class_static_block_break_cannot_see_outer_label() {
    // ClassStaticBlockDeclaration is a control-flow container
    // (isImmediatelyInvoked keeps currentFlow flowing through, but
    // the label list still clears): its `break outer` finds nothing.
    let source = parse("outer: { class C { static { break outer; } } }\n");
    let binder = bind(&source);
    let labels = statement_labels(&source);
    assert!(binder
        .flags_of(labels[0])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
}

#[test]
fn label_list_restores_after_nested_container() {
    // The outer label is visible again once the nested function
    // ends: the trailing break references it (flag stays clear).
    let source = parse("outer: { function f() {} break outer; }\n");
    let binder = bind(&source);
    let labels = statement_labels(&source);
    assert!(!binder
        .flags_of(labels[0])
        .intersects(tsc_types::NodeFlags::UNREACHABLE));
}

fn parse_named(name: &str, text: &str, javascript_file: bool) -> SourceFile {
    parse_source_file(
        name,
        text,
        ParseOptions {
            javascript_file,
            ..ParseOptions::default()
        },
        None,
    )
}

#[test]
fn declaration_file_root_gets_export_context_and_implicit_exports() {
    // setExportContextFlag (43902) reads the root's Ambient flag
    // (tsc sourceFlags): a .d.ts external module with no export
    // declarations is an implicit export context, so `declare
    // const Named` merges into exports.Named beside the exported
    // type alias (exportAsNamespace5's three.d.ts shape:
    // TypeAlias|BlockScopedVariable, 2 declarations).
    let source = parse_named(
        "three.d.ts",
        "export type Named = 0;\ndeclare const Named: 0;\n",
        false,
    );
    let binder = bind(&source);
    assert!(binder
        .flags_of(source.root)
        .intersects(tsc_types::NodeFlags::EXPORT_CONTEXT));
    let file_symbol = binder.node_symbol[&source.root];
    let named = binder.symbols.symbol(file_symbol).exports["Named"];
    let named = binder.symbols.symbol(named);
    assert_eq!(named.declarations.len(), 2);
    assert!(named.flags.intersects(tsc_types::SymbolFlags::TYPE_ALIAS));
    assert!(named
        .flags
        .intersects(tsc_types::SymbolFlags::BLOCK_SCOPED_VARIABLE));
}

#[test]
fn javascript_file_flag_drives_js_return_targets() {
    // isInJSFile is flag-driven (parse option -> node flags), not
    // extension-sniffed: a JS function declaration gets a return
    // target and returnFlowNode (bindContainer 42777/42801); a TS
    // one does not.
    let js = parse_named("a.js", "function f() { return 1; }\n", true);
    let binder = bind(&js);
    let f = find_nodes(&js, SyntaxKind::FunctionDeclaration)[0];
    assert!(binder.node_return_flow.contains_key(&f));

    let ts = parse("function f() { return 1; }\n");
    let binder = bind(&ts);
    let f = find_nodes(&ts, SyntaxKind::FunctionDeclaration)[0];
    assert!(!binder.node_return_flow.contains_key(&f));
}

/// setValueDeclaration (15190): an ambient non-assignment
/// declaration displaces an assignment-declaration
/// valueDeclaration only in a JS file, and isInJSFile is
/// flag-driven (root JavaScriptFile from ParseOptions), not
/// extension-sniffed — the file NAME says .ts in both directions
/// here. The expando symbol-producing bodies that feed assignment
/// declarations through addDeclarationToSymbol are live.
#[test]
fn set_value_declaration_ambient_displacement_reads_root_js_flag() {
    let text = "f.x = 1;\ndeclare var d: number;\n";
    for (javascript_file, ambient_wins) in [(true, true), (false, false)] {
        let source = parse_named("a.ts", text, javascript_file);
        let options: &'static CompilerOptions = Box::leak(Box::new(default_options()));
        let mut binder = Binder::new(&source, options);
        let assignment = find_nodes(&source, SyntaxKind::BinaryExpression)[0];
        let ambient = find_nodes(&source, SyntaxKind::VariableDeclaration)[0];
        assert!(crate::node_util::node_flags(&source, ambient).intersects(NodeFlags::AMBIENT));
        assert!(!crate::node_util::node_flags(&source, assignment).intersects(NodeFlags::AMBIENT));
        let symbol = binder.symbols.alloc(SymbolFlags::NONE, "x".to_owned());
        binder.set_value_declaration(symbol, assignment);
        binder.set_value_declaration(symbol, ambient);
        let expected = if ambient_wins { ambient } else { assignment };
        assert_eq!(
            binder.symbols.symbol(symbol).value_declaration,
            Some(expected)
        );
    }
}

#[test]
fn assignment_declarations_bind_oracle_pinned_symbol_faces() {
    // Oracle: TypeScript 6.0.3, allowJs/checkJs/noLib. This single
    // canary covers function expandos, instance/prototype members,
    // descriptor members, CommonJS named exports and export=.
    let source = parse_named(
        "a.js",
        "\
function F() { this.i = 1; }
F.s = function() {};
F[\"b\"] = 1;
F.prototype.p = 1;
F.prototype = { q: 1 };
Object.defineProperty(F, \"d\", { value: function() {} });
Object.defineProperty(F.prototype, \"g\", { get() { return 1; } });
exports.x = 1;
module.exports.y = 2;
module.exports = F;
",
        true,
    );
    let binder = bind(&source);
    let root_locals = &binder.locals[&source.root];
    let f = root_locals["F"];
    let f_symbol = binder.symbols.symbol(f);
    assert_eq!(f_symbol.flags.bits(), 67_110_448);
    assert_eq!(
        f_symbol
            .members
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["i", "p", "g"]
    );
    assert_eq!(
        f_symbol
            .exports
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["s", "b", "prototype", "d"]
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.members["i"]).flags.bits(),
        67_108_868
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.members["p"]).flags.bits(),
        67_108_868
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.members["g"]).flags.bits(),
        67_141_636
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.exports["s"]).flags.bits(),
        67_117_056
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.exports["b"]).flags.bits(),
        67_108_868
    );
    assert_eq!(
        binder
            .symbols
            .symbol(f_symbol.exports["prototype"])
            .flags
            .bits(),
        67_108_868
    );
    assert_eq!(
        binder.symbols.symbol(f_symbol.exports["d"]).flags.bits(),
        67_117_056
    );

    let file = binder.node_symbol[&source.root];
    let file_symbol = binder.symbols.symbol(file);
    assert_eq!(file_symbol.flags, SymbolFlags::VALUE_MODULE);
    assert_eq!(
        file_symbol
            .exports
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["x", "y", InternalSymbolName::EXPORT_EQUALS]
    );
    assert_eq!(
        binder.symbols.symbol(file_symbol.exports["x"]).flags.bits(),
        1_048_580
    );
    assert_eq!(
        binder.symbols.symbol(file_symbol.exports["y"]).flags.bits(),
        1_048_580
    );
    assert_eq!(
        binder
            .symbols
            .symbol(file_symbol.exports[InternalSymbolName::EXPORT_EQUALS])
            .flags
            .bits(),
        69_206_016
    );
    assert!(binder.common_js_module_indicator.is_some());
    assert!(binder
        .symbols
        .symbol(root_locals["module"])
        .flags
        .intersects(SymbolFlags::MODULE_EXPORTS));
}

#[test]
fn aliased_this_and_dynamic_assignments_bind_the_constructor_owner() {
    // bindWorker 44358-44365 routes `var self = this; self.x = ...`
    // through bindThisPropertyAssignment. Its dynamic-name twin and
    // bindSpecialPropertyAssignment 44840-44850 both record the
    // declaration in the owning symbol's late-bound assignment map.
    let source = parse_named(
        "a.js",
        "\
const key = Symbol();
function F() {
    var self = this;
    self.x = 1;
    self[key] = 2;
}
function G() {
    var self = {};
    self.y = 1;
}
F[key] = 3;
",
        true,
    );
    let binder = bind(&source);
    let root_locals = &binder.locals[&source.root];
    let f = root_locals["F"];
    let g = root_locals["G"];
    let f_symbol = binder.symbols.symbol(f);
    assert!(f_symbol.flags.intersects(SymbolFlags::CLASS));
    assert!(f_symbol.members.contains_key("x"));
    assert!(f_symbol.members.contains_key(InternalSymbolName::COMPUTED));
    assert_eq!(f_symbol.assignment_declaration_members.len(), 2);
    assert!(
        !binder.symbols.symbol(g).members.contains_key("y"),
        "an object-valued local with the same spelling is not a this alias"
    );
}

#[test]
fn destructured_require_elements_bind_as_aliases_only_at_the_require_boundary() {
    // bindVariableDeclarationOrBindingElement 45004-45020 uses the
    // root VariableDeclaration for a BindingElement's require test.
    let source = parse_named(
        "a.js",
        "\
const { K: Renamed, V } = require('./mod');
const ordinary = { V: 1 };
const { V: local } = ordinary;
",
        true,
    );
    let binder = bind(&source);
    let elements = find_nodes(&source, SyntaxKind::BindingElement);
    assert_eq!(elements.len(), 3);
    for &element in &elements[..2] {
        let symbol = binder.node_symbol[&element];
        assert!(binder
            .symbols
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS));
    }
    let ordinary = binder.node_symbol[&elements[2]];
    assert!(!binder
        .symbols
        .symbol(ordinary)
        .flags
        .intersects(SymbolFlags::ALIAS));
}

#[test]
fn common_js_aliases_follow_initializers_without_treating_every_alias_as_exports() {
    // Oracle: TypeScript 6.0.3. Aliases initialized from exports or
    // module.exports feed named exports, while an unrelated entity
    // name on the right of module.exports remains an export= alias.
    let source = parse_named(
        "a.js",
        "\
const e = exports;
e.x = 1;
const m = module.exports;
m.y = 2;
function F() {}
const imported = F;
module.exports = imported;
",
        true,
    );
    let binder = bind(&source);
    let file = binder.node_symbol[&source.root];
    let file_symbol = binder.symbols.symbol(file);
    assert_eq!(
        file_symbol
            .exports
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["x", "y", InternalSymbolName::EXPORT_EQUALS]
    );
    assert_eq!(
        binder
            .symbols
            .symbol(file_symbol.exports[InternalSymbolName::EXPORT_EQUALS])
            .flags
            .bits(),
        69_206_016
    );
}

#[test]
fn common_js_require_indicator_requires_exactly_one_argument() {
    for (text, expected) in [
        ("require();\n", false),
        ("require('a', 'b');\n", false),
        ("require('a');\n", true),
    ] {
        let source = parse_named("a.js", text, true);
        let binder = bind(&source);
        assert_eq!(binder.common_js_module_indicator.is_some(), expected);
    }

    // bindCallExpression is not JS-gated in tsc: a require call
    // also makes an otherwise-script TypeScript source external.
    let source = parse_named("a.ts", "require('a');\n", false);
    let binder = bind(&source);
    assert!(binder.common_js_module_indicator.is_some());
    assert!(binder.node_symbol.contains_key(&source.root));
}

#[test]
fn checked_js_require_variables_bind_as_aliases_except_jsdoc_type_tags() {
    let source = parse_named(
        "a.js",
        "const bare = require('./m');\n\
             const accessed = require('./m').value;\n\
             /** @type {number} */ const typed = require('./m');\n",
        true,
    );
    let binder = bind(&source);
    let declarations = find_nodes(&source, SyntaxKind::VariableDeclaration);
    assert_eq!(declarations.len(), 3);
    let flags: Vec<_> = declarations
        .iter()
        .map(|declaration| binder.symbols.symbol(binder.node_symbol[declaration]).flags)
        .collect();
    assert_eq!(flags[0], SymbolFlags::ALIAS);
    assert_eq!(flags[1], SymbolFlags::ALIAS);
    assert!(flags[2].intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE));
    assert!(!flags[2].intersects(SymbolFlags::ALIAS));
}

#[test]
fn forced_external_module_still_accepts_common_js_assignments() {
    // tsc's boolean `true` externalModuleIndicator is represented
    // by the SourceFile root. Unlike a syntax indicator, it does
    // not block the CommonJS indicator or its exports.
    let source = parse_source_file(
        "a.js",
        "module.exports.x = 1;\n",
        ParseOptions {
            javascript_file: true,
            force_external_module: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(source.external_module_indicator, Some(source.root));
    let binder = bind(&source);
    assert!(binder.common_js_module_indicator.is_some());
    let file = binder.node_symbol[&source.root];
    assert!(binder.symbols.symbol(file).exports.contains_key("x"));
}

#[test]
fn nested_expando_initializer_shapes_match_tsc() {
    let source = parse_named(
        "a.js",
        "\
function outer() {
    const empty = {};
    empty.x = 1;
    const rich = { a: 1 };
    rich.y = 1;
    const defaulted = defaulted || {};
    defaulted.z = 1;
    const called = (function () {})();
    called.w = 1;
}
",
        true,
    );
    let binder = bind(&source);
    let outer = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let locals = &binder.locals[&outer];
    assert!(binder
        .symbols
        .symbol(locals["empty"])
        .exports
        .contains_key("x"));
    assert!(!binder
        .symbols
        .symbol(locals["rich"])
        .exports
        .contains_key("y"));
    assert!(binder
        .symbols
        .symbol(locals["defaulted"])
        .exports
        .contains_key("z"));
    assert!(binder
        .symbols
        .symbol(locals["called"])
        .exports
        .contains_key("w"));
}

#[test]
fn jsdoc_typedef_and_properties_bind_real_declaration_nodes() {
    let source = parse_named(
        "a.js",
        "/**\n\
             * @typedef {Object} Required\n\
             * @property {number} required\n\
             */\n\
             const value = {};\n",
        true,
    );
    let binder = bind(&source);
    let alias = binder.locals[&source.root]["Required"];
    let alias_symbol = binder.symbols.symbol(alias);
    assert!(alias_symbol.flags.intersects(SymbolFlags::TYPE_ALIAS));
    assert_eq!(alias_symbol.declarations.len(), 1);
    let typedef = alias_symbol.declarations[0];
    assert_eq!(kind_of(&source, typedef), SyntaxKind::JSDocTypedefTag);
    let type_expression = match &source.arena.node(typedef).data {
        NodeData::JSDocTypedefTag(data) => data.type_expression.expect("type literal"),
        _ => unreachable!(),
    };
    let type_literal = match &source.arena.node(type_expression).data {
        NodeData::JSDocTypeExpression(data) => data.r#type.expect("wrapped type literal"),
        _ => type_expression,
    };
    assert_eq!(kind_of(&source, type_literal), SyntaxKind::JSDocTypeLiteral);
    let type_symbol = binder.node_symbol[&type_literal];
    let property = binder.symbols.symbol(type_symbol).members["required"];
    let property_symbol = binder.symbols.symbol(property);
    assert!(property_symbol.flags.intersects(SymbolFlags::PROPERTY));
    assert_eq!(property_symbol.declarations.len(), 1);
    assert_eq!(
        kind_of(&source, property_symbol.declarations[0]),
        SyntaxKind::JSDocPropertyTag
    );
}

#[test]
fn host_parameter_jsdoc_tag_does_not_redeclare_the_parameter() {
    let source = parse_named(
        "a.js",
        "/** @param {number} value */\nfunction f(value) {}\n",
        true,
    );
    let binder = bind(&source);
    let function = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let parameter = find_nodes(&source, SyntaxKind::Parameter)[0];
    let tag = find_nodes(&source, SyntaxKind::JSDocParameterTag)[0];
    let symbol = binder.locals[&function]["value"];

    assert_eq!(binder.symbols.symbol(symbol).declarations, [parameter]);
    assert!(!binder.node_symbol.contains_key(&tag));
}

#[test]
fn dotted_jsdoc_typedef_binds_namespace_then_leaf_alias() {
    let source = parse_named(
        "a.js",
        "/** @typedef {number} Types.Count */\nfunction host() {}\n",
        true,
    );
    let binder = bind(&source);
    let host = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    assert!(!binder.locals[&host].contains_key("Types"));
    let types = binder.locals[&source.root]["Types"];
    let count = binder.symbols.symbol(types).exports["Count"];
    let count = binder.symbols.symbol(count);

    assert!(count.flags.intersects(SymbolFlags::TYPE_ALIAS));
    assert_eq!(count.declarations.len(), 1);
    assert_eq!(
        kind_of(&source, count.declarations[0]),
        SyntaxKind::JSDocTypedefTag
    );
}

#[test]
fn dotted_jsdoc_typedef_merges_namespace_face_into_explicit_export_alias() {
    let source = parse_named(
        "a.js",
        "/**\n\
             * @namespace myTypes\n\
             * @global\n\
             * @type {Object<string, *>}\n\
             */\n\
             const myTypes = {};\n\
             /** @typedef {string} myTypes.typeA */\n\
             export { myTypes };\n",
        true,
    );
    let binder = bind(&source);
    let file = binder.node_symbol[&source.root];
    let exported = binder.symbols.symbol(file).exports["myTypes"];
    let exported = binder.symbols.symbol(exported);

    assert!(exported.flags.intersects(SymbolFlags::ALIAS));
    assert!(exported.flags.intersects(SymbolFlags::NAMESPACE_MODULE));
    assert!(exported
        .declarations
        .iter()
        .any(|&declaration| kind_of(&source, declaration) == SyntaxKind::ExportSpecifier));
    assert!(exported
        .declarations
        .iter()
        .any(|&declaration| kind_of(&source, declaration) == SyntaxKind::ModuleDeclaration));
}

#[test]
fn nameless_jsdoc_typedef_on_property_routes_to_namespace_export() {
    let source = parse_named("a.js", "/** @typedef {number} */\nTypes.Count;\n", true);
    let binder = bind(&source);
    let types = binder.js_global_augmentations["Types"];
    let count = binder.symbols.symbol(types).exports["Count"];
    let count = binder.symbols.symbol(count);
    assert!(count.flags.intersects(SymbolFlags::TYPE_ALIAS));
    assert!(count
        .declarations
        .iter()
        .any(|&declaration| kind_of(&source, declaration) == SyntaxKind::JSDocTypedefTag));
}

#[test]
fn jsdoc_import_clause_binds_in_host_enclosing_scope() {
    let source = parse_named(
        "a.js",
        "function f() {\n\
             /** @import { RootOnly } from \"./root\" */\n\
             /** @import { Foo } from \"./foo\" */\n\
             const value = 0;\n\
             }\n",
        true,
    );
    let import_tag = find_nodes(&source, SyntaxKind::JSDocImportTag)[0];
    assert_eq!(
        get_container_flags(&source, import_tag).0,
        ContainerFlags::IS_CONTAINER.0
            | ContainerFlags::IS_CONTROL_FLOW_CONTAINER.0
            | ContainerFlags::HAS_LOCALS.0
            | ContainerFlags::PROPAGATES_THIS_KEYWORD.0
    );

    let binder = bind(&source);
    let function = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
    let locals = &binder.locals[&function];
    assert!(locals.contains_key("Foo"));
    assert!(binder
        .symbols
        .symbol(locals["Foo"])
        .flags
        .intersects(SymbolFlags::ALIAS));
    assert!(!binder.locals[&source.root].contains_key("Foo"));
    assert!(binder.locals[&source.root].contains_key("RootOnly"));
    assert!(!locals.contains_key("RootOnly"));
}

#[test]
fn jsdoc_template_callback_and_class_tags_use_effective_hosts() {
    let source = parse_named(
        "a.js",
        "/**\n\
             * @template T\n\
             * @callback Mapper\n\
             * @param {T} value\n\
             * @returns {T}\n\
             */\n\
             const mapper = null;\n\
             /** @class */\n\
             function C() {}\n",
        true,
    );
    let binder = bind(&source);
    let callback = find_nodes(&source, SyntaxKind::JSDocCallbackTag)[0];
    assert!(binder.locals[&callback].contains_key("T"));
    let signature = find_nodes(&source, SyntaxKind::JSDocSignature)[0];
    let signature_type = binder.node_symbol[&signature];
    assert!(binder
        .symbols
        .symbol(signature_type)
        .members
        .contains_key(InternalSymbolName::CALL));

    let class_function = binder.locals[&source.root]["C"];
    let flags = binder.symbols.symbol(class_function).flags;
    assert!(flags.intersects(SymbolFlags::FUNCTION));
    assert!(flags.intersects(SymbolFlags::CLASS));
}

#[test]
fn jsdoc_type_special_property_uses_materialized_ast_tag() {
    let source = parse_named(
        "a.js",
        "function F() {}\n\
             /** @type {number} */\n\
             F.count;\n",
        true,
    );
    let binder = bind(&source);
    let function = binder.locals[&source.root]["F"];
    let count = binder.symbols.symbol(function).exports["count"];
    assert!(binder
        .symbols
        .symbol(count)
        .flags
        .intersects(SymbolFlags::PROPERTY | SymbolFlags::ASSIGNMENT));
}

#[test]
fn jsdoc_visibility_tags_contribute_effective_modifier_flags() {
    let source = parse_named(
        "a.js",
        "class C {\n\
             /**\n\
             * @private\n\
             * @readonly\n\
             * @deprecated\n\
             */\n\
             value;\n\
             }\n",
        true,
    );
    let property = find_nodes(&source, SyntaxKind::PropertyDeclaration)[0];
    let flags = crate::node_util::get_combined_modifier_flags(&source, property);
    assert!(flags.intersects(ModifierFlags::PRIVATE));
    assert!(flags.intersects(ModifierFlags::READONLY));
    assert!(flags.intersects(ModifierFlags::DEPRECATED));
}

#[test]
fn function_in_block_es5_strict_reports_1250_family() {
    let options = CompilerOptions {
        target: Some(1),
        always_strict: Some(true),
        ..CompilerOptions::default()
    };
    let source = parse("{ function g() {} }\n");
    let options_ref: &'static CompilerOptions = Box::leak(Box::new(options));
    let mut binder = Binder::new(&source, options_ref);
    binder.bind_source_file();
    // Oracle-pinned: 1250 @ (11,1) (the function name g).
    assert_eq!(diag_pins(&binder), [(1250, 11, 1)]);
}
