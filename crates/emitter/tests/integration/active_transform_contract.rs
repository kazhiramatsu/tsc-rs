use std::cell::Cell;

use serde_json::Value;
use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, DisabledSourceMapRecorder,
    EmitResolver, EmitResolverError, EmitResolverNode, NewLineKind, PrintRequest, PrinterOptions,
    TransformArena, TransformRoot, UnavailableEmitResolver, UnsupportedTransformFeature,
};
use tsc_program::SourceFileId;
use tsc_syntax::{for_each_child, parse_source_file, NodeData, SyntaxKind};
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
        ..CompilerOptions::default()
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

    let resolver = UnavailableEmitResolver;
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
fn rejected_feature_roots_fail_before_a_partial_transform_is_returned() {
    let cases = [
        (
            "enum.ts",
            "export enum Direction { Up, Down }\n",
            UnsupportedTransformFeature::RuntimeEnums,
        ),
        (
            "namespace.ts",
            "namespace Runtime { export const value: number = 1; }\n",
            UnsupportedTransformFeature::RuntimeNamespaces,
        ),
        (
            "parameter-property.ts",
            "class Service { constructor(public value: number) {} }\n",
            UnsupportedTransformFeature::ParameterProperties,
        ),
        (
            "import-equals.ts",
            "import value = require('./value');\n",
            UnsupportedTransformFeature::ImportEquals,
        ),
    ];
    for (file_name, text, expected) in cases {
        let parsed = parse_source_file(file_name, text, Default::default(), None);
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let resolver = UnavailableEmitResolver;
        let error = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&bootstrap_options(), &resolver).unwrap(),
            false,
        )
        .err()
        .expect("profile control must fail closed");
        assert!(matches!(
            error,
            tsc_emitter::TransformError::UnsupportedSyntax { feature, .. }
                if feature == expected
        ));
    }
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
