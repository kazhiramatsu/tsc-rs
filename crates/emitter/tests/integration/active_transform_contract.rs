use std::cell::Cell;

use serde_json::Value;
use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, DisabledSourceMapRecorder,
    EmitConstantValue, EmitResolver, EmitResolverError, EmitResolverNode, NewLineKind,
    PrintRequest, PrinterOptions, SourceFileTextMode, TransformArena, TransformRoot,
    UnavailableEmitResolver,
};
use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_child, parse_source_file, JSDocParsingMode, LanguageVariant, NodeData, ParseOptions,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, ScriptTarget};

const EMIT_ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-emit-oracle.v1.json"
));
const ACTIVE_TRANSFORM_ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-active-transform.v1.json"
));

fn active_transform_probe_number(key: &str) -> i32 {
    let oracle: Value = serde_json::from_slice(ACTIVE_TRANSFORM_ORACLE)
        .expect("H1.3 active-transform oracle is valid JSON");
    i32::try_from(
        oracle["structural_probe"][key]
            .as_u64()
            .unwrap_or_else(|| panic!("missing H1.3 structural probe number {key}")),
    )
    .unwrap_or_else(|_| panic!("H1.3 structural probe number {key} exceeds i32"))
}

fn emit_oracle_callback_text(case_id: &str, path: &str) -> String {
    let oracle: Value = serde_json::from_slice(EMIT_ORACLE).expect("H1 emit oracle is valid JSON");
    oracle["cases"]
        .as_array()
        .expect("oracle cases")
        .iter()
        .find(|case| case["input"]["id"] == case_id)
        .and_then(|case| case["observation"]["writes"].as_array())
        .and_then(|writes| writes.iter().find(|write| write["path"] == path))
        .and_then(|write| write["callback_text"].as_str())
        .unwrap_or_else(|| panic!("missing H1 emit oracle write {case_id} {path}"))
        .to_owned()
}

const ERASABLE_TYPESCRIPT: &str = concat!(
    "export interface Shape { value: number }\n",
    "export type Boxed<T> = { value: T };\n",
    "export const answer: number = 41 as number;\n",
    "export function inc(value: number): number { return value + 1; }\n",
    "export class Box<T> {\n",
    "    readonly value: T;\n",
    "    constructor(value: T) { this.value = value; }\n",
    "    get(): T { return this.value; }\n",
    "}\n",
    "export const boxed = new Box(answer satisfies number);\n",
);

fn bootstrap_options() -> CompilerOptions {
    CompilerOptions {
        target: Some(ScriptTarget::ES_NEXT.bits()),
        module: Some(200),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    }
}

fn transform_and_print_at_target(source_text: &str, target: ScriptTarget) -> String {
    transform_and_print_at_target_with_resolver(source_text, target, &NoConstantValueResolver)
}

fn transform_and_print_at_target_with_resolver(
    source_text: &str,
    target: ScriptTarget,
    resolver: &dyn EmitResolver,
) -> String {
    let parsed = parse_source_file("target.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(target.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, resolver).unwrap(),
        false,
    )
    .expect("target transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(target))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print target transform")
        .text()
        .to_owned()
}

#[test]
fn numeric_literal_source_text_is_reused_only_when_the_target_allows_it() {
    let source = "const timestamp = 1_553_993_100_000; const fraction = 1_000.0;\n";
    assert_eq!(
        transform_and_print_at_target(source, ScriptTarget::ES2020),
        "const timestamp = 1553993100000;\nconst fraction = 1000;\n",
    );
    assert_eq!(
        transform_and_print_at_target(source, ScriptTarget::ES2021),
        source,
    );
}

struct NoConstantValueResolver;

impl EmitResolver for NoConstantValueResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

struct UnresolvedDecoratorResolver;

impl EmitResolver for UnresolvedDecoratorResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

struct InstantiatedModuleResolver;

impl EmitResolver for InstantiatedModuleResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn exact_bootstrap_transformer_order_erases_the_frozen_typescript_tree() {
    let parsed = parse_source_file("main.ts", ERASABLE_TYPESCRIPT, Default::default(), None);
    let original_statement_count = match &parsed.arena.node(parsed.root).data {
        NodeData::SourceFile(data) => parsed
            .arena
            .node_array(data.statements.unwrap())
            .nodes
            .len(),
        _ => unreachable!(),
    };
    assert_eq!(original_statement_count, 6);

    let resolver = NoConstantValueResolver;
    let transformers = get_script_transformers(&bootstrap_options(), &resolver).unwrap();
    assert_eq!(
        transformers
            .iter()
            .map(|transformer| transformer.name())
            .collect::<Vec<_>>(),
        [
            "transformTypeScript",
            "transformClassFields",
            "transformECMAScriptModule"
        ]
    );

    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("frozen erasable TypeScript transform");
    let root = result.arena().root(source).unwrap();
    assert_ne!(root.node(), parsed.root);
    assert_eq!(result.arena().node(root).unwrap().pos, 0);
    assert_eq!(
        result.arena().node(root).unwrap().end,
        ERASABLE_TYPESCRIPT.len() as u32
    );
    assert_eq!(
        result.arena().transform_flags(root).bits(),
        active_transform_probe_number("transformed_root_transform_flags")
    );

    let statement_count = match &result.arena().node(root).unwrap().data {
        NodeData::SourceFile(data) => result
            .arena()
            .node_array_ref(source, data.statements.unwrap())
            .map(|array| result.arena().node_array(array).unwrap().nodes.len())
            .unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(
        statement_count,
        active_transform_probe_number("emitted_statement_count") as usize
    );

    let syntax = result.arena().source(source).unwrap().syntax();
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = syntax.arena.node(id);
        assert!(
            !(node.kind >= SyntaxKind::FirstTypeNode && node.kind <= SyntaxKind::LastTypeNode),
            "type node survived: {:?}",
            node.kind
        );
        assert!(!matches!(
            node.kind,
            SyntaxKind::AsExpression
                | SyntaxKind::SatisfiesExpression
                | SyntaxKind::TypeAssertionExpression
                | SyntaxKind::NonNullExpression
                | SyntaxKind::ReadonlyKeyword
        ));
        for_each_child(&syntax.arena, node, |child| {
            stack.push(child);
            false
        });
    }

    assert_eq!(
        match &parsed.arena.node(parsed.root).data {
            NodeData::SourceFile(data) => parsed
                .arena
                .node_array(data.statements.unwrap())
                .nodes
                .len(),
            _ => unreachable!(),
        },
        original_statement_count,
        "the parsed tree must remain immutable"
    );

    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print transformed source");
    assert_eq!(
        printed.text(),
        emit_oracle_callback_text("erasable-typescript", "/project/src/main.js")
    );
}

#[test]
fn es2020_logical_assignments_stabilize_each_access_operand_once() {
    assert_eq!(
        transform_and_print_at_target(
            "let a, b; a ||= b; a &&= b; a ??= b;\n",
            ScriptTarget::ES2020,
        ),
        concat!(
            "let a, b;\n",
            "a || (a = b);\n",
            "a && (a = b);\n",
            "a ?? (a = b);\n",
        ),
    );
    assert_eq!(
        transform_and_print_at_target(
            "getObj().prop ||= rhs();\ngetObj()[getKey()] ??= rhs();\n",
            ScriptTarget::ES2020,
        ),
        concat!(
            "var _a, _b, _c;\n",
            "(_a = getObj()).prop || (_a.prop = rhs());\n",
            "(_b = getObj())[_c = getKey()] ?? (_b[_c] = rhs());\n",
        ),
    );
    assert_eq!(
        transform_and_print_at_target(
            "let _a, _b; getObj()[getKey()] ||= rhs();\n",
            ScriptTarget::ES2020,
        ),
        concat!(
            "var _c, _d;\n",
            "let _a, _b;\n",
            "(_c = getObj())[_d = getKey()] || (_c[_d] = rhs());\n",
        ),
    );
}

#[test]
fn es2020_logical_assignment_hoists_are_owned_by_each_function_scope() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "function f() { getObj()[getKey()] &&= rhs(); }\n",
                "const arrow = () => getObj()[getKey()] ||= rhs();\n",
                "class C extends B { m() { super.x ||= rhs(); super[getKey()] ??= rhs(); } }\n",
            ),
            ScriptTarget::ES2020,
        ),
        concat!(
            "function f() { var _a, _b; (_a = getObj())[_b = getKey()] && (_a[_b] = rhs()); }\n",
            "const arrow = () => { var _a, _b; return (_a = getObj())[_b = getKey()] || (_a[_b] = rhs()); };\n",
            "class C extends B {\n",
            "    m() { var _a; super.x || (super.x = rhs()); super[_a = getKey()] ?? (super[_a] = rhs()); }\n",
            "}\n",
        ),
    );
}

#[test]
fn es2020_parameter_hoists_move_defaults_into_typed_function_prologues() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "function f(x = getObj()[getKey()] ||= rhs()) { return x; }\n",
                "const g = ({value} = (getObj()[getKey()] ??= rhs())) => value;\n",
            ),
            ScriptTarget::ES2020,
        ),
        concat!(
            "function f(x) { var _a, _b; if (x === void 0) { x = (_a = getObj())[_b = getKey()] || (_a[_b] = rhs()); } return x; }\n",
            "const g = (_a) => { var _b, _c; var { value } = _a === void 0 ? ((_b = getObj())[_c = getKey()] ?? (_b[_c] = rhs())) : _a; return value; };\n",
        ),
    );
}

#[test]
fn es2018_optional_catch_bindings_use_scoped_generated_names() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "let _a;\n",
                "try { get()?.x; } catch { let _b; get2()?.y; }\n",
                "function f(){ try{}catch{} }\n",
                "function g(){ try{}catch(e){} try{}catch{} }\n",
            ),
            ScriptTarget::ES2018,
        ),
        concat!(
            "var _c, _d;\n",
            "let _a;\n",
            "try {\n",
            "    (_c = get()) === null || _c === void 0 ? void 0 : _c.x;\n",
            "}\n",
            "catch (_e) {\n",
            "    let _b;\n",
            "    (_d = get2()) === null || _d === void 0 ? void 0 : _d.y;\n",
            "}\n",
            "function f() { try { }\n",
            "catch (_c) { } }\n",
            "function g() { try { }\n",
            "catch (e) { } try { }\n",
            "catch (_c) { } }\n",
        ),
    );
}

#[test]
fn es2018_optional_catch_bindings_preserve_original_token_comments() {
    assert_eq!(
        transform_and_print_at_target(
            "try { work(); } /* before catch */ catch /* before body */ { /* in body */ recover(); }\n",
            ScriptTarget::ES2018,
        ),
        concat!(
            "try {\n",
            "    work();\n",
            "} /* before catch */\n",
            "catch /* before body */ ( /* in body */_a) { /* in body */\n",
            "    recover();\n",
            "}\n",
        ),
    );
}

#[test]
fn es2018_using_named_evaluation_restores_erased_outer_expressions() {
    let output = transform_and_print_at_target_with_resolver(
        "declare const dec: any; using Value = (@dec class {}) as any;\n",
        ScriptTarget::ES2018,
        &UnresolvedDecoratorResolver,
    );
    let set_function_name_helper = output
        .find("var __setFunctionName")
        .expect("named evaluation requests the setFunctionName helper");
    let disposable_helper = output
        .find("var __addDisposableResource")
        .expect("using requests the disposable helper");
    assert!(set_function_name_helper < disposable_helper);
    assert!(output.contains("__setFunctionName(_classThis, \"Value\");"));
}

#[test]
fn es2019_optional_chains_and_nullish_coalescing_preserve_evaluation_order() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "let x = a?.b;\n",
                "let y = f()?.[key()];\n",
                "let z = value ?? fallback();\n",
                "let w = compute() ?? fallback();\n",
            ),
            ScriptTarget::ES2019,
        ),
        concat!(
            "var _a, _b;\n",
            "let x = a === null || a === void 0 ? void 0 : a.b;\n",
            "let y = (_a = f()) === null || _a === void 0 ? void 0 : _a[key()];\n",
            "let z = value !== null && value !== void 0 ? value : fallback();\n",
            "let w = (_b = compute()) !== null && _b !== void 0 ? _b : fallback();\n",
        ),
    );
}

#[test]
fn es2019_optional_calls_keep_the_exact_javascript_receiver() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "let a = obj.method?.(arg());\n",
                "let b = getObj().method?.();\n",
                "let c = obj?.method();\n",
                "let d = (obj?.method)();\n",
                "let e = delete obj?.[key()];\n",
            ),
            ScriptTarget::ES2019,
        ),
        concat!(
            "var _a, _b, _c;\n",
            "let a = (_a = obj.method) === null || _a === void 0 ? void 0 : _a.call(obj, arg());\n",
            "let b = (_c = (_b = getObj()).method) === null || _c === void 0 ? void 0 : _c.call(_b);\n",
            "let c = obj === null || obj === void 0 ? void 0 : obj.method();\n",
            "let d = (obj === null || obj === void 0 ? void 0 : obj.method).call(obj);\n",
            "let e = obj === null || obj === void 0 ? true : delete obj[key()];\n",
        ),
    );
}

#[test]
fn es2019_parameter_hoists_share_the_typed_function_scope_plan() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "function f({x}=foo()?.bar, y=g() ?? z) { return q()?.r; }\n",
                "const g = (x = (a?.b.c)()) => x;\n",
            ),
            ScriptTarget::ES2019,
        ),
        concat!(
            "function f(_a, y) { var _b, _c, _d; var { x } = _a === void 0 ? (_b = foo()) === null || _b === void 0 ? void 0 : _b.bar : _a; if (y === void 0) { y = (_c = g()) !== null && _c !== void 0 ? _c : z; } return (_d = q()) === null || _d === void 0 ? void 0 : _d.r; }\n",
            "const g = (x) => { var _a; if (x === void 0) { x = (a === null || a === void 0 ? void 0 : (_a = a.b).c).call(_a); } return x; };\n",
        ),
    );
}

#[test]
fn es2019_optional_chains_restore_erased_typescript_outer_expressions() {
    assert_eq!(
        transform_and_print_at_target(
            "let a = (value as Box)?.member;\nlet b = value!?.method?.();\n",
            ScriptTarget::ES2019,
        ),
        concat!(
            "var _a;\n",
            "let a = value === null || value === void 0 ? void 0 : value.member;\n",
            "let b = (_a = value === null || value === void 0 ? void 0 : value.method) === null || _a === void 0 ? void 0 : _a.call(value);\n",
        ),
    );
}

#[test]
fn es2019_optional_call_preserves_erased_instantiation_grammar_boundary() {
    assert_eq!(
        transform_and_print_at_target(
            "declare const value: any;\ntype Box = unknown;\nvalue<Box>?.();\n",
            ScriptTarget::ES2019,
        ),
        concat!(
            "var _a;\n",
            "(_a = (value)) === null || _a === void 0 ? void 0 : _a();\n",
        ),
    );
}

#[test]
fn es2019_optional_chain_composition_matches_the_pinned_transform() {
    let source = concat!(
        "declare let o: any, fn: any, key: any, fallback: any;\n",
        "declare function get(): any;\n",
        "o?.a.b;\n",
        "o.a?.b;\n",
        "o?.[key()].b;\n",
        "o[key()]?.b;\n",
        "o?.m();\n",
        "o.m?.();\n",
        "o[key()]?.();\n",
        "get().m?.();\n",
        "o?.m?.();\n",
        "o?.m().n?.();\n",
        "(o?.m)();\n",
        "(o?.m.n)();\n",
        "delete (o?.a.b);\n",
        "delete o.a?.[key()];\n",
        "(o.m as any)?.();\n",
        "o!?.m?.();\n",
        "(get() ?? fallback).m;\n",
        "o?.a ?? get();\n",
        "class B { m?(): any {} }\n",
        "class D extends B { f() { return super.m?.(); } }\n",
        "function p(x = o?.a, {y}: any = get()?.v, z = (o?.m.n)()) { return x; }\n",
        "function q(x = (() => get()?.a)()) { return x; }\n",
    );
    assert_eq!(
        transform_and_print_at_target(source, ScriptTarget::ES2019),
        concat!(
            "var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k, _l, _m, _o, _p, _q;\n",
            "o === null || o === void 0 ? void 0 : o.a.b;\n",
            "(_a = o.a) === null || _a === void 0 ? void 0 : _a.b;\n",
            "o === null || o === void 0 ? void 0 : o[key()].b;\n",
            "(_b = o[key()]) === null || _b === void 0 ? void 0 : _b.b;\n",
            "o === null || o === void 0 ? void 0 : o.m();\n",
            "(_c = o.m) === null || _c === void 0 ? void 0 : _c.call(o);\n",
            "(_d = o[key()]) === null || _d === void 0 ? void 0 : _d.call(o);\n",
            "(_f = (_e = get()).m) === null || _f === void 0 ? void 0 : _f.call(_e);\n",
            "(_g = o === null || o === void 0 ? void 0 : o.m) === null || _g === void 0 ? void 0 : _g.call(o);\n",
            "(_j = o === null || o === void 0 ? void 0 : (_h = o.m()).n) === null || _j === void 0 ? void 0 : _j.call(_h);\n",
            "(o === null || o === void 0 ? void 0 : o.m).call(o);\n",
            "(o === null || o === void 0 ? void 0 : (_k = o.m).n).call(_k);\n",
            "(o === null || o === void 0 ? true : delete o.a.b);\n",
            "(_l = o.a) === null || _l === void 0 ? true : delete _l[key()];\n",
            "(_m = o.m) === null || _m === void 0 ? void 0 : _m.call(o);\n",
            "(_o = o === null || o === void 0 ? void 0 : o.m) === null || _o === void 0 ? void 0 : _o.call(o);\n",
            "((_p = get()) !== null && _p !== void 0 ? _p : fallback).m;\n",
            "(_q = o === null || o === void 0 ? void 0 : o.a) !== null && _q !== void 0 ? _q : get();\n",
            "class B {\n",
            "    m() { }\n",
            "}\n",
            "class D extends B {\n",
            "    f() { var _a; return (_a = super.m) === null || _a === void 0 ? void 0 : _a.call(this); }\n",
            "}\n",
            "function p(x, _a, z) { var _b, _c; if (x === void 0) { x = o === null || o === void 0 ? void 0 : o.a; } var { y } = _a === void 0 ? (_b = get()) === null || _b === void 0 ? void 0 : _b.v : _a; if (z === void 0) { z = (o === null || o === void 0 ? void 0 : (_c = o.m).n).call(_c); } return x; }\n",
            "function q(x = (() => { var _a; return (_a = get()) === null || _a === void 0 ? void 0 : _a.a; })()) { return x; }\n",
        ),
    );
}

#[test]
fn native_standard_decorator_root_is_owned_after_typescript_erasure() {
    let parsed = parse_source_file(
        "decorator.ts",
        "@dec class Value { field: number; }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = UnavailableEmitResolver;
    let mut options = bootstrap_options();
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("native standard decorator syntax is owned by H2.4b");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print native standard decorator syntax");
    assert_eq!(printed.text(), "@dec\nclass Value {\n    field;\n}\n");
}

#[test]
fn es2022_using_scope_is_lowered_through_typed_disposal_state() {
    let parsed = parse_source_file(
        "using.ts",
        "{\n    using value = acquire();\n}\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = UnavailableEmitResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let transformers = get_script_transformers(&options, &resolver).unwrap();
    assert_eq!(
        transformers
            .iter()
            .map(|transformer| transformer.name())
            .collect::<Vec<_>>(),
        [
            "transformTypeScript",
            "transformESNext",
            "transformESDecorators",
            "transformClassFields",
            "transformECMAScriptModule",
        ]
    );
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("ES2022 using transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print lowered using scope");
    assert!(printed.text().starts_with("var __addDisposableResource ="));
    assert!(printed.text().contains("var __disposeResources ="));
    assert!(printed.text().ends_with(concat!(
        "{\n",
        "    const env_1 = { stack: [], error: void 0, hasError: false };\n",
        "    try {\n",
        "        const value = __addDisposableResource(env_1, acquire(), false);\n",
        "    }\n",
        "    catch (e_1) {\n",
        "        env_1.error = e_1;\n",
        "        env_1.hasError = true;\n",
        "    }\n",
        "    finally {\n",
        "        __disposeResources(env_1);\n",
        "    }\n",
        "}\n",
    )));
}

#[test]
fn es2022_disposal_names_follow_output_scope_ownership() {
    let parsed = parse_source_file(
        "using-scopes.ts",
        concat!(
            "using root = acquire();\n",
            "function nested() { using inner = acquire(); }\n",
            "{ using block = acquire(); }\n",
            "namespace N { using member = acquire(); }\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = InstantiatedModuleResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2022 nested using transforms");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print nested using scopes");
    let text = printed.text();

    let source_env = text.find("const env_1 =").expect("source disposal scope");
    let block_env = text.find("const env_2 =").expect("block disposal scope");
    let function_env = text
        .find("const env_3 =")
        .unwrap_or_else(|| panic!("function disposal scope missing from:\n{text}"));
    let namespace_env = text
        .find("const env_4 =")
        .expect("namespace disposal scope");
    assert!(function_env < source_env);
    assert!(source_env < block_env && block_env < namespace_env);
    for (catch_name, env_name) in [
        ("e_1", "env_2"),
        ("e_2", "env_1"),
        ("e_3", "env_3"),
        ("e_4", "env_4"),
    ] {
        assert!(text.contains(&format!("catch ({catch_name})")));
        assert!(text.contains(&format!("{env_name}.error = {catch_name};")));
    }
    assert!(!text.contains("using "));
}

#[test]
fn es2022_decorator_call_bindings_preserve_receivers_and_lexical_super() {
    let parsed = parse_source_file(
        "decorator-this.ts",
        concat!(
            "declare class DecoratorProvider {\n",
            "    decorate<T>(this: DecoratorProvider, value: T, context: DecoratorContext): T;\n",
            "}\n",
            "declare const instance: DecoratorProvider;\n",
            "class C {\n",
            "    @instance.decorate method1() {}\n",
            "    @(instance[\"decorate\"]) method2() {}\n",
            "    @((instance.decorate)) method3() {}\n",
            "}\n",
            "class D extends DecoratorProvider {\n",
            "    method() {\n",
            "        class Nested {\n",
            "            @(super.decorate) method1() {}\n",
            "            @(super[\"decorate\"]) method2() {}\n",
            "            @((super.decorate)) method3() {}\n",
            "        }\n",
            "    }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2022 standard decorator call bindings");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print standard decorator call bindings");
    let text = printed.text();

    assert!(
        text.contains("(_a = instance).decorate.bind(_a)"),
        "direct receiver binding missing from:\n{text}"
    );
    assert!(
        text.contains("((_b = instance)[\"decorate\"].bind(_b))"),
        "element receiver binding missing from:\n{text}"
    );
    assert!(
        text.contains("(((_c = instance).decorate.bind(_c)))"),
        "parenthesized receiver binding missing from:\n{text}"
    );
    assert_eq!(text.matches("let _outerThis = this;").count(), 1);
    assert!(text.contains("super.decorate.bind(_outerThis)"));
    assert!(text.contains("super[\"decorate\"].bind(_outerThis)"));
    assert!(text.contains("((super.decorate.bind(_outerThis)))"));
}

#[test]
fn detached_source_prefix_stays_before_standard_decorator_helpers() {
    let parsed = parse_source_file(
        "decorator-comment.ts",
        "// detached source comment\n\n@dec\nclass C {}\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("standard decorator transform with detached source trivia");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print standard decorator with detached source trivia");
    let text = printed.text();

    assert_eq!(text.matches("// detached source comment").count(), 1);
    assert!(
        text.find("// detached source comment").unwrap() < text.find("var __esDecorate =").unwrap(),
        "source-owned detached trivia must precede source helpers:\n{text}"
    );
}

#[test]
fn esnext_assignment_mode_static_auto_accessor_keeps_dynamic_this_receiver() {
    let parsed = parse_source_file(
        "decorator-static-accessor.ts",
        concat!(
            "const dec = (_value, _context) => {};\n",
            "class C { @dec static accessor value = 1; }\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.use_define_for_class_fields = Some(false);
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ESNext assignment-mode decorated static auto-accessor");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ESNext assignment-mode decorated static auto-accessor");
    let text = printed.text();

    assert!(text.contains("static get value() { return this.#value_accessor_storage; }"));
    assert!(text.contains("static set value(value) { this.#value_accessor_storage = value; }"));
    assert!(!text.contains("return C.#value_accessor_storage"));
}

#[test]
fn synthesized_jsx_import_owns_position_before_detached_source_prefix() {
    let parsed = parse_source_file(
        "jsx-comment.tsx",
        concat!(
            "/// <reference path=\"/.lib/react16.d.ts\" />\n",
            "\n",
            "export const tag = <div />;\n",
        ),
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.jsx = Some(4);
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("automatic JSX transform with detached source trivia");
    let root = result.arena().root(source).unwrap();
    let NodeData::SourceFile(data) = &result.arena().node(root).unwrap().data else {
        unreachable!();
    };
    let statements = result
        .arena()
        .node_array(
            result
                .arena()
                .node_array_ref(source, data.statements.unwrap())
                .unwrap(),
        )
        .unwrap();
    assert_eq!((statements.pos, statements.end), (u32::MAX, u32::MAX));

    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print automatic JSX with detached source trivia");
    let text = printed.text();
    let import = text.find("import { jsx as _jsx }").unwrap();
    let reference = text.find("/// <reference path=").unwrap();
    let export = text.find("export const tag").unwrap();
    assert!(
        import < reference && reference < export,
        "unexpected order:\n{text}"
    );
}

#[test]
fn synthesized_disposal_body_keeps_detached_prefix_inside_the_try() {
    let parsed = parse_source_file(
        "using-comment.ts",
        concat!(
            "/// <reference path=\"/resource.d.ts\" />\n",
            "\n",
            "using resource = acquire();\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("using transform with detached source trivia");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print using transform with detached source trivia");
    let text = printed.text();

    assert_eq!(text.matches("/// <reference path=").count(), 1);
    assert!(
        text.find("var __disposeResources =").unwrap() < text.find("/// <reference path=").unwrap()
    );
    assert!(text.contains("try {\n    /// <reference path=\"/resource.d.ts\" />\n"));
}

#[test]
fn es2022_auto_accessor_lowers_while_native_fields_remain_owned() {
    let parsed = parse_source_file(
        "accessor.ts",
        "class C { field = 1; accessor item = 3; #native = 4; static accessor total = 5; }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = UnavailableEmitResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2022 auto-accessor transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2022 auto accessor");
    assert_eq!(
        printed.text(),
        concat!(
            "class C {\n",
            "    field = 1;\n",
            "    #item_accessor_storage = 3;\n",
            "    get item() { return this.#item_accessor_storage; }\n",
            "    set item(value) { this.#item_accessor_storage = value; }\n",
            "    #native = 4;\n",
            "    static #total_accessor_storage = 5;\n",
            "    static get total() { return C.#total_accessor_storage; }\n",
            "    static set total(value) { C.#total_accessor_storage = value; }\n",
            "}\n",
        )
    );
}

#[test]
fn es2021_public_fields_use_typed_assignment_and_define_policies() {
    let source_text =
        "class Base {}\nclass C extends Base { field = 1; empty; static total = 2; }\n";
    let expected = [
        (
            false,
            concat!(
                "class Base {\n}\n",
                "class C extends Base {\n",
                "    constructor() {\n",
                "        super(...arguments);\n",
                "        this.field = 1;\n",
                "    }\n",
                "}\n",
                "C.total = 2;\n",
            ),
        ),
        (
            true,
            concat!(
                "class Base {\n}\n",
                "class C extends Base {\n",
                "    constructor() {\n",
                "        super(...arguments);\n",
                "        Object.defineProperty(this, \"field\", {\n",
                "            enumerable: true,\n",
                "            configurable: true,\n",
                "            writable: true,\n",
                "            value: 1\n",
                "        });\n",
                "        Object.defineProperty(this, \"empty\", {\n",
                "            enumerable: true,\n",
                "            configurable: true,\n",
                "            writable: true,\n",
                "            value: void 0\n",
                "        });\n",
                "    }\n",
                "}\n",
                "Object.defineProperty(C, \"total\", {\n",
                "    enumerable: true,\n",
                "    configurable: true,\n",
                "    writable: true,\n",
                "    value: 2\n",
                "});\n",
            ),
        ),
    ];
    for (use_define, expected) in expected {
        let parsed = parse_source_file("fields.ts", source_text, Default::default(), None);
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let resolver = UnavailableEmitResolver;
        let mut options = bootstrap_options();
        options.target = Some(ScriptTarget::ES2021.bits());
        options.always_strict = Some(false);
        options.use_define_for_class_fields = Some(use_define);
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&options, &resolver).unwrap(),
            false,
        )
        .expect("ES2021 public-field transform");
        let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(
                &mut result,
                PrintRequest::SourceFile(source),
                &mut DisabledSourceMapRecorder,
            )
            .expect("print ES2021 public fields");
        assert_eq!(
            printed.text(),
            expected,
            "useDefineForClassFields={use_define}"
        );
    }
}

#[test]
fn es2021_private_fields_use_owned_slots_and_helper_operations() {
    let parsed = parse_source_file(
        "private.ts",
        "class Cls { #x; m(){ this.#x ??= false ? neverThis() : 20; } }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2021 private-field transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2021 private fields");
    assert_eq!(
        printed.text(),
        concat!(
            "var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a getter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\");\n",
            "    return kind === \"m\" ? f : kind === \"a\" ? f.call(receiver) : f ? f.value : state.get(receiver);\n",
            "};\n",
            "var __classPrivateFieldSet = (this && this.__classPrivateFieldSet) || function (receiver, state, value, kind, f) {\n",
            "    if (kind === \"m\") throw new TypeError(\"Private method is not writable\");\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a setter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot write private member to an object whose class did not declare it\");\n",
            "    return (kind === \"a\" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;\n",
            "};\n",
            "var _Cls_x;\n",
            "class Cls {\n",
            "    constructor() {\n",
            "        _Cls_x.set(this, void 0);\n",
            "    }\n",
            "    m() { __classPrivateFieldSet(this, _Cls_x, __classPrivateFieldGet(this, _Cls_x, \"f\") ?? (false ? neverThis() : 20), \"f\"); }\n",
            "}\n",
            "_Cls_x = new WeakMap();\n",
        )
    );
}

#[test]
fn es2021_private_behavior_uses_a_shared_brand_and_named_functions() {
    let parsed = parse_source_file(
        "private-behavior.ts",
        concat!(
            "class C { #x=1; #m(){ return this.#x; } ",
            "get #a(){return this.#x;} set #a(v){this.#x=v;} ",
            "invoke(){ return this.#m() + this.#a; } }\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2021 private method/accessor transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2021 private behavior");
    assert!(printed.text().starts_with("var __classPrivateFieldGet ="));
    assert!(printed.text().contains("var __classPrivateFieldSet ="));
    assert!(printed.text().ends_with(concat!(
        "var _C_instances, _C_x, _C_m, _C_a_get, _C_a_set;\n",
        "class C {\n",
        "    constructor() {\n",
        "        _C_instances.add(this);\n",
        "        _C_x.set(this, 1);\n",
        "    }\n",
        "    invoke() { return __classPrivateFieldGet(this, _C_instances, \"m\", _C_m).call(this) + __classPrivateFieldGet(this, _C_instances, \"a\", _C_a_get); }\n",
        "}\n",
        "_C_x = new WeakMap(), _C_instances = new WeakSet(), _C_m = function _C_m() { return __classPrivateFieldGet(this, _C_x, \"f\"); }, _C_a_get = function _C_a_get() { return __classPrivateFieldGet(this, _C_x, \"f\"); }, _C_a_set = function _C_a_set(v) { __classPrivateFieldSet(this, _C_x, v, \"f\"); };\n",
    )));
}

#[test]
fn es2021_static_initializers_own_lexical_this_and_super_bindings() {
    let parsed = parse_source_file(
        "static-super.ts",
        "class C extends B { static x=super.y; static { super.m(); this.z=1; } }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2021 static lexical transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2021 static lexical bindings");
    assert_eq!(
        printed.text(),
        concat!(
            "var _a, _b;\n",
            "class C extends (_b = B) {\n",
            "}\n",
            "_a = C;\n",
            "Object.defineProperty(C, \"x\", {\n",
            "    enumerable: true,\n",
            "    configurable: true,\n",
            "    writable: true,\n",
            "    value: Reflect.get(_b, \"y\", _a)\n",
            "});\n",
            "(() => {\n",
            "    Reflect.get(_b, \"m\", _a).call(_a);\n",
            "    _a.z = 1;\n",
            "})();\n",
        )
    );
}

#[test]
fn es2021_auto_accessors_expand_into_downlevel_private_storage() {
    let parsed = parse_source_file(
        "auto-accessor.ts",
        "class C { accessor x=1; static accessor y=2; }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2021 auto-accessor transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2021 auto accessors");
    assert!(printed.text().starts_with("var __classPrivateFieldGet ="));
    assert!(printed.text().contains("var __classPrivateFieldSet ="));
    assert!(printed.text().ends_with(concat!(
        "var _a, _C_x_accessor_storage, _C_y_accessor_storage;\n",
        "class C {\n",
        "    constructor() {\n",
        "        _C_x_accessor_storage.set(this, 1);\n",
        "    }\n",
        "    get x() { return __classPrivateFieldGet(this, _C_x_accessor_storage, \"f\"); }\n",
        "    set x(value) { __classPrivateFieldSet(this, _C_x_accessor_storage, value, \"f\"); }\n",
        "    static get y() { return __classPrivateFieldGet(_a, _a, \"f\", _C_y_accessor_storage); }\n",
        "    static set y(value) { __classPrivateFieldSet(_a, _a, value, \"f\", _C_y_accessor_storage); }\n",
        "}\n",
        "_a = C, _C_x_accessor_storage = new WeakMap();\n",
        "_C_y_accessor_storage = { value: 2 };\n",
    )));
}

#[test]
fn es2021_private_auto_accessor_keeps_logical_and_backing_slots_distinct() {
    let parsed = parse_source_file(
        "private-auto-accessor.ts",
        "class C { accessor #x=1; m(){return this.#x;} }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2021 private auto-accessor transform");
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print ES2021 private auto accessor");
    assert!(printed.text().starts_with("var __classPrivateFieldGet ="));
    assert!(printed.text().contains("var __classPrivateFieldSet ="));
    assert!(printed.text().ends_with(concat!(
        "var _C_instances, _C_x_get, _C_x_set, _C_x_accessor_storage;\n",
        "class C {\n",
        "    constructor() {\n",
        "        _C_instances.add(this);\n",
        "        _C_x_accessor_storage.set(this, 1);\n",
        "    }\n",
        "    m() { return __classPrivateFieldGet(this, _C_instances, \"a\", _C_x_get); }\n",
        "}\n",
        "_C_instances = new WeakSet(), _C_x_accessor_storage = new WeakMap(), _C_x_get = function _C_x_get() { return __classPrivateFieldGet(this, _C_x_accessor_storage, \"f\"); }, _C_x_set = function _C_x_set(value) { __classPrivateFieldSet(this, _C_x_accessor_storage, value, \"f\"); };\n",
    )));
}

#[derive(Default)]
struct ReferencedAliasResolver {
    referenced_queries: Cell<usize>,
    value_queries: Cell<usize>,
}

impl EmitResolver for ReferencedAliasResolver {
    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.referenced_queries
            .set(self.referenced_queries.get() + 1);
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.value_queries.set(self.value_queries.get() + 1);
        Ok(true)
    }
}

#[test]
fn alias_elision_uses_the_borrowed_resolver_and_never_queries_type_only_specifiers() {
    let parsed = parse_source_file(
        "aliases.ts",
        concat!(
            "import Default, { type Shape, value as local } from \"./dep\";\n",
            "export { type Shape, local };\n",
        ),
        Default::default(),
        None,
    );
    let resolver = ReferencedAliasResolver::default();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(7)));
    {
        let result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&bootstrap_options(), &resolver).unwrap(),
            false,
        )
        .expect("alias transform with live resolver");

        let syntax = result.arena().source(source).unwrap().syntax();
        let root = result.arena().root(source).unwrap();
        let mut stack = vec![root.node()];
        let mut import_specifiers = 0;
        let mut export_specifiers = 0;
        while let Some(id) = stack.pop() {
            let node = syntax.arena.node(id);
            match &node.data {
                NodeData::ImportSpecifier(data) => {
                    assert!(!data.is_type_only);
                    import_specifiers += 1;
                }
                NodeData::ExportSpecifier(data) => {
                    assert!(!data.is_type_only);
                    export_specifiers += 1;
                }
                _ => {}
            }
            for_each_child(&syntax.arena, node, |child| {
                stack.push(child);
                false
            });
        }
        assert_eq!(import_specifiers, 1);
        assert_eq!(export_specifiers, 1);
    }
    assert_eq!(resolver.referenced_queries.get(), 2);
    assert_eq!(resolver.value_queries.get(), 1);
}

#[test]
fn changed_node_printer_uses_the_configured_newline_and_preserves_unicode_literals() {
    let parsed = parse_source_file(
        "zeta.ts",
        "export const zeta: string = \"雪\";\n",
        Default::default(),
        None,
    );
    let resolver = UnavailableEmitResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&bootstrap_options(), &resolver).unwrap(),
        false,
    )
    .unwrap();
    let printed = create_printer(PrinterOptions::new(NewLineKind::CarriageReturnLineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .unwrap();
    assert_eq!(
        printed.text(),
        emit_oracle_callback_text("ordered-multi-file-bom-crlf", "/project/src/zeta.js")
    );
    assert_eq!(printed.end().line(), 1);
    assert_eq!(printed.end().column(), 0);
}

#[test]
fn runtime_in_operator_remains_a_javascript_token() {
    let text = "const present = \"key\" in { key: 1 };\n";
    let parsed = parse_source_file("runtime-in.ts", text, Default::default(), None);
    let resolver = UnavailableEmitResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&bootstrap_options(), &resolver).unwrap(),
        false,
    )
    .expect("runtime in expression is not TypeScript variance syntax");
    assert_eq!(result.arena().root(source).unwrap().node(), parsed.root);

    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("identity JavaScript print");
    assert_eq!(printed.text(), text);
}

#[test]
fn h2_3a_javascript_print_preserves_shebang_directive_and_attached_comments() {
    let text = concat!(
        "#!/usr/bin/env node\n",
        "\"use strict\";\n",
        "/** retained JSDoc */\n",
        "// retained leading comment\n",
        "const answer = 42; // retained trailing comment\n",
    );
    let parsed = parse_source_file(
        "input.js",
        text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            javascript_file: true,
            js_doc_parsing_mode: JSDocParsingMode::ParseAll,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = UnavailableEmitResolver;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.allow_js = true;
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("H2.3a JavaScript transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("H2.3a JavaScript print");

    assert_eq!(printed.text(), text);
}
