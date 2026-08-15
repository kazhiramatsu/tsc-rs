use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use tsc_emitter::{
    create_printer, get_script_transformers, get_script_transformers_for_source, transform_nodes,
    DisabledSourceMapRecorder, EmitConstantValue, EmitEnumMemberValue, EmitExportContainerMode,
    EmitFlags, EmitHost, EmitResolver, EmitResolverError, EmitResolverNode, EmitSource,
    EmitTypeReferenceSerializationKind, JavaScriptNumber, JavaScriptString, NewLineKind,
    PrintRequest, PrinterOptions, SourceFileTextMode, TransformArena, TransformRoot,
    UnavailableEmitResolver,
};
use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_child, parse_source_file, JSDocParsingMode, LanguageVariant, NodeData, NodeId,
    ParseOptions, SyntaxKind,
};
use tsc_types::{CompilerOptions, ModuleKind, NodeCheckFlags, ScriptTarget};

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

struct TransformContractHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    source_ids: [SourceFileId; 1],
}

impl EmitHost for TransformContractHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/")
    }

    fn config_file_path(&self) -> Option<&Path> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        (id == self.source_ids[0]).then(|| {
            let path = Path::new(&self.syntax.file_name);
            EmitSource::new(id, path, path, true, None, Some(self.syntax))
        })
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
    transform_and_print_at_target_with_resolver_and_mode(
        source_text,
        target,
        resolver,
        SourceFileTextMode::PreserveUnchanged,
        false,
    )
}

fn transform_and_print_canonical_at_target(source_text: &str, target: ScriptTarget) -> String {
    transform_and_print_at_target_with_resolver_and_mode(
        source_text,
        target,
        &NoConstantValueResolver,
        SourceFileTextMode::Canonical,
        false,
    )
}

fn transform_and_print_canonical_without_comments_at_target(
    source_text: &str,
    target: ScriptTarget,
) -> String {
    transform_and_print_at_target_with_resolver_and_mode(
        source_text,
        target,
        &NoConstantValueResolver,
        SourceFileTextMode::Canonical,
        true,
    )
}

fn transform_and_print_at_target_with_resolver_and_mode(
    source_text: &str,
    target: ScriptTarget,
    resolver: &dyn EmitResolver,
    source_file_text_mode: SourceFileTextMode,
    remove_comments: bool,
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
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(target)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(source_file_text_mode),
    )
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
fn erased_arrow_head_does_not_reemit_comments_before_the_arrow_token() {
    let output = transform_and_print_at_target(
        concat!(
            "var f1 = (x: string, y: string) /* before */ => { };\n",
            "var f2 = (x: number): number /*\n  recovery\n*/ => /* after */ x;\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(!output.contains("before"), "{output}");
    assert!(!output.contains("recovery"), "{output}");
    assert!(output.contains("(x, y) => { };"), "{output}");
    assert!(output.contains("(x) => /* after */ x;"), "{output}");
}

#[test]
fn synthesized_exponentiation_arguments_retain_recovery_leading_comments() {
    let output = transform_and_print_at_target(
        concat!(
            "var regex4 = /**// /**/asdf /;\n",
            "var regex5 = /**// asdf/**/ /;\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("var regex4 = /**/ Math.pow(/**/ / /, /asdf /);"),
        "{output}",
    );
    assert!(
        output.contains("var regex5 = /**/ Math.pow(/**/ / asdf/, / /);"),
        "{output}",
    );
}

#[test]
fn invalid_react_namespace_option_value_is_retained_by_recovery_emit() {
    let parsed = parse_source_file(
        "invalid-react-namespace.tsx",
        "const element = <foo data={true} />;\n",
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        jsx: Some(2),
        react_namespace: Some("my-React-Lib".to_owned()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("invalid reactNamespace recovery transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print invalid reactNamespace recovery output")
    .text()
    .to_owned();

    assert!(
        text.contains("my-React-Lib.createElement(\"foo\", { data: true })"),
        "{text}"
    );
}

#[test]
fn relocated_assignment_field_owns_its_leading_comment_once() {
    let parsed = parse_source_file(
        "assignment-field-comment.ts",
        concat!(
            "class C {\n",
            "    // field comment\n",
            "    field = 1;\n",
            "}\n",
        ),
        ParseOptions::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("assignment-mode class field transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print assignment-mode class field")
    .text()
    .to_owned();

    assert_eq!(text.matches("// field comment").count(), 1, "{text}");
    assert!(text.contains("this.field = 1;"), "{text}");
    assert!(!text.contains("this.\n"), "{text}");
}

fn transform_and_print_module(
    source_text: &str,
    target: ScriptTarget,
    module: ModuleKind,
) -> String {
    let parsed = parse_source_file("module.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source_id = SourceFileId::from_raw(0);
    let source = arena.add_source(&parsed, Some(source_id));
    let mut options = bootstrap_options();
    options.target = Some(target.bits());
    options.module = Some(module.bits());
    options.always_strict = Some(false);
    let host = TransformContractHost {
        options: &options,
        syntax: &parsed,
        source_ids: [source_id],
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers_for_source(&options, &SystemContractResolver, &host, source_id)
            .unwrap(),
        false,
    )
    .expect("module transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(target))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print module transform")
        .text()
        .to_owned()
}

fn transform_and_print_system_module(source_text: &str, target: ScriptTarget) -> String {
    transform_and_print_module(source_text, target, ModuleKind::SYSTEM)
}

#[test]
fn erased_export_under_a_label_keeps_the_module_transform_statement_slot() {
    let source = concat!(
        "export const box: string\n",
        "subTitle:\n",
        "export const title: string\n",
    );

    let common_js = transform_and_print_module(source, ScriptTarget::ES2015, ModuleKind::COMMON_JS);
    assert!(
        common_js.contains("exports.box = void 0;\nsubTitle: ;\n"),
        "{common_js}"
    );
    assert!(!common_js.contains("const title"), "{common_js}");

    let system = transform_and_print_system_module(source, ScriptTarget::ES2015);
    assert!(system.contains("var box, title;"), "{system}");
    assert!(system.contains("subTitle: ;"), "{system}");
    assert!(!system.contains("export const title"), "{system}");
}

#[test]
fn common_js_detached_reference_stays_between_strict_and_custom_prologues() {
    let output = transform_and_print_module(
        concat!(
            "#!/usr/bin/env node\n",
            "\n",
            "/// <reference path=\"f.d.ts\"/>\n",
            "\n",
            "declare function use(value: number): void;\n",
            "import { x } from \"test\";\n",
            "use(x);\n",
        ),
        ScriptTarget::ES2015,
        ModuleKind::COMMON_JS,
    );

    assert!(
        output.starts_with(concat!(
            "#!/usr/bin/env node\n",
            "\"use strict\";\n",
            "/// <reference path=\"f.d.ts\"/>\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        )),
        "{output}",
    );
    assert_eq!(
        output.matches("/// <reference path=").count(),
        1,
        "{output}"
    );
}

#[test]
fn system_module_hoists_nested_var_but_keeps_nested_lexical_bindings_local() {
    let text = transform_and_print_system_module(
        concat!(
            "export function read() { return hoisted; }\n",
            "for (let x = 0; x < 1; ++x) {\n",
            "    const y = x;\n",
            "    var hoisted = x + y;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let outer_hoists = text
        .lines()
        .take_while(|line| !line.contains("var __moduleName"))
        .filter_map(|line| line.trim().strip_prefix("var "))
        .collect::<Vec<_>>();
    assert_eq!(outer_hoists, ["hoisted;"]);
    assert!(text.contains("for (let x = 0; x < 1; ++x)"));
    assert!(text.contains("const y = x;"));
    assert!(text.contains("hoisted = x + y;"));
}

#[test]
fn system_module_keeps_import_hoists_in_source_order_when_setters_are_deduplicated() {
    let text = transform_and_print_system_module(
        concat!(
            "import { A } from \"f1\";\n",
            "import { B } from \"f2\";\n",
            "import { C } from \"f3\";\n",
            "import { D } from \"f2\";\n",
            "import { E } from \"f2\";\n",
            "import { F } from \"f1\";\n",
            "console.log(A + B + C + D + E + F);\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains("var f1_1, f2_1, f3_1, f2_2, f2_3, f1_2;"),
        "{text}",
    );
    assert_eq!(text.matches("function (f1_1_1)").count(), 1, "{text}");
    assert_eq!(text.matches("function (f2_1_1)").count(), 1, "{text}");
    assert_eq!(text.matches("function (f3_1_1)").count(), 1, "{text}");
}

#[test]
fn system_module_materializes_anonymous_default_declaration_bindings() {
    let function_text = transform_and_print_system_module(
        "export default function () { return true; }\n",
        ScriptTarget::ES2015,
    );
    assert!(
        function_text.contains("function default_1() { return true; }"),
        "{function_text}",
    );
    assert!(
        function_text.contains("exports_1(\"default\", default_1);"),
        "{function_text}",
    );

    let class_text = transform_and_print_system_module(
        "export default class { value = 1; }\n",
        ScriptTarget::ES2015,
    );
    assert!(class_text.contains("var default_1;"), "{class_text}");
    assert!(class_text.contains("default_1 = class {"), "{class_text}");
    assert!(
        class_text.contains("exports_1(\"default\", default_1);"),
        "{class_text}",
    );
}

#[test]
fn system_default_export_value_does_not_reown_asi_trailing_comments() {
    let text = transform_and_print_system_module(
        concat!(
            "const Home = {};\n",
            "export default Home\n",
            "// trailing export comment\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(text.contains("exports_1(\"default\", Home);"), "{text}");
    assert!(!text.contains("trailing export comment"), "{text}");
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

#[test]
fn arrow_object_body_is_parenthesized_after_type_erasure() {
    assert_eq!(
        transform_and_print_at_target("var v = a => <any>{};\n", ScriptTarget::ES2015),
        "var v = a => ({});\n",
    );
}

#[test]
fn assignment_to_an_erased_instantiation_expression_keeps_grammar_parentheses() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "let obj: { fn?: <T>() => T } = {};\n",
                "obj.fn<number> = () => 1234;\n",
                "let getValue: <T>() => T;\n",
                "getValue<number> = () => 1234;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "let obj = {};\n",
            "(obj.fn) = () => 1234;\n",
            "let getValue;\n",
            "(getValue) = () => 1234;\n",
        ),
    );
}

#[test]
fn invalid_constructor_parameter_modifiers_are_erased_after_diagnostics() {
    assert_eq!(
        transform_and_print_at_target(
            "class foo {\n    constructor(static a: number) {\n    }\n}\n",
            ScriptTarget::ES2015,
        ),
        "class foo {\n    constructor(a) {\n    }\n}\n",
    );
}

#[test]
fn setter_update_erases_parameter_types_but_preserves_recovery_signature_fields() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "class C {\n",
                "    set Valid(value: string) { }\n",
                "    set Invalid<T>(value: number): string { }\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "class C {\n",
            "    set Valid(value) { }\n",
            "    set Invalid<T>(value): string { }\n",
            "}\n",
        ),
    );
}

#[test]
fn unannotated_this_parameters_admit_typescript_erasure() {
    let text = transform_and_print_at_target(
        concat!(
            "const receiver = { set value(this, next) { this.value = next; } };\n",
            "const callback = function(this, value) { return value; };\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(text.contains("set value(next)"), "{text}");
    assert!(text.contains("function (value)"), "{text}");
    assert!(!text.contains("set value(this"), "{text}");
    assert!(!text.contains("function (this"), "{text}");
}

#[test]
fn constructor_and_getter_recovery_fields_follow_factory_update_boundaries() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "class GetterRecovery { get value<T>() { } }\n",
                "class ConstructorTypeParameters { constructor<T>() { } }\n",
                "class EmptyConstructorTypeParameters { constructor<>() { } }\n",
                "class ConstructorReturnType { constructor(): number { } }\n",
                "class StaticConstructor { static constructor() { } }\n",
                "class ExportConstructor { export constructor() { } }\n",
                "class UpdatedGetter { get value<T>(): string { } }\n",
                "class UpdatedConstructor { constructor<T>(value: string): number { } }\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "class GetterRecovery {\n    get value<T>() { }\n}\n",
            "class ConstructorTypeParameters {\n    constructor<T>() { }\n}\n",
            "class EmptyConstructorTypeParameters {\n    constructor() { }\n}\n",
            "class ConstructorReturnType {\n    constructor(): number { }\n}\n",
            "class StaticConstructor {\n    static constructor() { }\n}\n",
            "class ExportConstructor {\n    export constructor() { }\n}\n",
            "class UpdatedGetter {\n    get value<T>() { }\n}\n",
            "class UpdatedConstructor {\n    constructor<T>(value): number { }\n}\n",
        ),
    );
}

#[test]
fn isolated_accessor_constructor_retains_modifier_without_parent_typescript_visit() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "class C { accessor constructor() { } }\n",
            ScriptTarget::ES_NEXT,
        ),
        "class C {\n    accessor constructor() { }\n}\n",
    );
}

#[test]
fn abstract_class_forces_constructor_through_typescript_class_element_visitor() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "abstract class C { accessor constructor() { } }\n",
            ScriptTarget::ES_NEXT,
        ),
        "class C {\n    constructor() { }\n}\n",
    );
}

#[test]
fn typescript_parent_preserves_non_constructor_accessor_recovery_modifiers() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "abstract class C {\n",
                "    accessor i() { }\n",
                "    accessor get j() { return false; }\n",
                "    accessor set k(v) { }\n",
                "    accessor constructor() { }\n",
                "}\n",
            ),
            ScriptTarget::ES_NEXT,
        ),
        concat!(
            "class C {\n",
            "    accessor i() { }\n",
            "    accessor get j() { return false; }\n",
            "    accessor set k(v) { }\n",
            "    constructor() { }\n",
            "}\n",
        ),
    );
}

#[test]
fn typed_sibling_forces_constructor_through_typescript_class_element_visitor() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "class C { value: number; accessor constructor() { } }\n",
            ScriptTarget::ES_NEXT,
        ),
        "class C {\n    value;\n    constructor() { }\n}\n",
    );
}

#[test]
fn typed_class_expression_forces_constructor_through_typescript_class_element_visitor() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "const C = class { value: number; accessor constructor() { } };\n",
            ScriptTarget::ES_NEXT,
        ),
        "const C = class {\n    value;\n    constructor() { }\n};\n",
    );
}

#[test]
fn parser_recovery_statement_tokens_keep_tsc_canonical_spacing() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "debugger\n",
                "for (var of X) {\n}\n",
                "for (var of of) { }\n",
                "for (var in X) {\n}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "debugger;\n",
            "for (var  of X) {\n}\n",
            "for (var  of of) { }\n",
            "for (var  in X) {\n}\n",
        ),
    );
}

#[test]
fn labeled_enum_uses_the_surrounding_typescript_lexical_scope() {
    let output = transform_and_print_canonical_at_target(
        concat!(
            "sourceLabel: enum SourceEnum {}\n",
            "{\n",
            "    blockLabel: enum BlockEnum {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("sourceLabel: {\n    var SourceEnum;"),
        "{output}",
    );
    assert!(
        output.contains("blockLabel: {\n        let BlockEnum;"),
        "{output}",
    );
    assert!(!output.contains("let SourceEnum;"), "{output}");

    let namespace_output = transform_and_print_at_target_with_resolver(
        concat!(
            "sourceLabel: namespace SourceNamespace {}\n",
            "{\n",
            "    blockLabel: namespace BlockNamespace {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
        &InstantiatedModuleResolver,
    );
    assert!(
        namespace_output.contains("sourceLabel: {\n    var SourceNamespace;"),
        "{namespace_output}",
    );
    assert!(
        namespace_output.contains("blockLabel: {\n        let BlockNamespace;"),
        "{namespace_output}",
    );
}

#[test]
fn empty_case_block_uses_the_multiline_case_block_list_format_after_transform() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "switch ('single') { }\nswitch ('multi') {\n}\n",
            ScriptTarget::ES2015,
        ),
        "switch ('single') {\n}\nswitch ('multi') {\n}\n",
    );
}

#[test]
fn transformed_super_switch_retains_the_empty_case_block_line_in_crlf_output() {
    let source_text = concat!(
        "declare class Base {}\n",
        "const derived = [\n",
        "    class extends Base {\n",
        "        prop = true;\n",
        "        constructor() {\n",
        "            switch (super()) {}\n",
        "        }\n",
        "    },\n",
        "];\n",
    );
    let parsed = parse_source_file("target.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("transform derived-class super switch");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print derived-class super switch");

    assert!(
        printed
            .text()
            .contains("switch (super()) {\r\n            }"),
        "{printed:?}",
    );
}

#[test]
fn parsed_comma_computed_name_is_not_reparenthesized_by_the_printer() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "var x = {\n    [0, 1]: { }\n};\n",
            ScriptTarget::ES2015,
        ),
        "var x = {\n    [0, 1]: {}\n};\n",
    );
}

#[test]
fn erased_assertions_restore_expression_statement_and_numeric_access_grammar() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "(<any>{a: 0});\n",
                "(<any>1).foo;\n",
                "(<any>1.).foo;\n",
                "(<any>1.0).foo;\n",
                "(<any>12e+34).foo;\n",
                "(<any>0xff).foo;\n",
                "(<any>function named() { })();\n",
                "declare var A: any;\n",
                "(<any>new A).foo;\n",
                "(<any>typeof A).field;\n",
                "(<any>-A).field;\n",
                "new (<any>A());\n",
                "(<any><number><any>-A).field;\n",
                "(<any><number>(<any>-A)).field;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "({ a: 0 });\n",
            "1..foo;\n",
            "1..foo;\n",
            "1.0.foo;\n",
            "12e+34.foo;\n",
            "0xff.foo;\n",
            "(function named() { })();\n",
            "(new A).foo;\n",
            "(typeof A).field;\n",
            "(-A).field;\n",
            "new (A());\n",
            "(-A).field;\n",
            "(-A).field;\n",
        ),
    );
}

#[test]
fn es2015_exponentiation_uses_typed_lexical_temp_ownership() {
    let source = concat!(
        "\"custom\";\n",
        "let _a, _b;\n",
        "const basic = left ** right ** later;\n",
        "value **= exponent;\n",
        "getObject().value **= next();\n",
        "getObject()[getKey()] **= left ** right;\n",
        "function scoped(value = getObject()[getKey()] **= exponent) { return getObject().value **= value; }\n",
        "const concise = () => getObject()[getKey()] **= exponent;\n",
    );
    assert_eq!(
        transform_and_print_at_target(source, ScriptTarget::ES2015),
        concat!(
            "\"custom\";\n",
            "var _c, _d, _e;\n",
            "let _a, _b;\n",
            "const basic = Math.pow(left, Math.pow(right, later));\n",
            "value = Math.pow(value, exponent);\n",
            "(_c = getObject()).value = Math.pow(_c.value, next());\n",
            "(_d = getObject())[_e = getKey()] = Math.pow(_d[_e], Math.pow(left, right));\n",
            "function scoped(value) { var _c, _d, _e; if (value === void 0) { value = (_c = getObject())[_d = getKey()] = Math.pow(_c[_d], exponent); } return (_e = getObject()).value = Math.pow(_e.value, value); }\n",
            "const concise = () => { var _c, _d; return (_c = getObject())[_d = getKey()] = Math.pow(_c[_d], exponent); };\n",
        ),
    );
    assert_eq!(
        transform_and_print_at_target(source, ScriptTarget::ES2016),
        source,
    );
}

#[test]
fn es2015_exponentiation_keeps_statement_comments_outside_synthetic_arguments() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "var temp: any;\n",
                "// Error: incorrect type on left-hand side\n",
                "(! --temp) ** 3;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "var temp;\n",
            "// Error: incorrect type on left-hand side\n",
            "Math.pow((!--temp), 3);\n",
        ),
    );
}

#[test]
fn es2015_nullish_expansion_is_parenthesized_as_a_conditional_condition() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "declare const a: string | undefined;\n",
                "const value = a ?? 'fallback' ? 1 : 2;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!("const value = (a !== null && a !== void 0 ? a : 'fallback') ? 1 : 2;\n",),
    );
}

#[test]
fn conditional_separator_uses_its_source_anchor_after_a_synthetic_condition() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "declare const a: string | undefined;\n",
                "const value = a ?? 'fallback' /*outer-condition*/ ? /*outer-question*/ 1 /*outer-true*/ : /*outer-colon*/ 2 /*outer-false*/;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "const value = (a !== null && a !== void 0 ? a : 'fallback' /*outer-condition*/) ? /*outer-question*/ 1 /*outer-true*/ : /*outer-colon*/ 2 /*outer-false*/;\n",
        ),
    );
}

#[test]
fn conditional_expression_boundaries_retain_each_branch_comment_once() {
    let source = concat!(
        "function pick(condition: boolean, yes: number, no: number): number {\n",
        "    return condition /*boundary-condition*/ ? /*boundary-question*/ yes /*boundary-true*/ : /*boundary-colon*/ no /*boundary-false*/;\n",
        "}\n",
    );
    let output = transform_and_print_canonical_at_target(source, ScriptTarget::ES2015);

    assert_eq!(
        output,
        concat!(
            "function pick(condition, yes, no) {\n",
            "    return condition /*boundary-condition*/ ? /*boundary-question*/ yes /*boundary-true*/ : /*boundary-colon*/ no /*boundary-false*/;\n",
            "}\n",
        ),
    );
    for marker in [
        "boundary-condition",
        "boundary-question",
        "boundary-true",
        "boundary-colon",
        "boundary-false",
    ] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
}

#[test]
fn conditional_true_branch_line_comment_stays_before_the_colon() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "function pick(condition: boolean, yes: number, no: number): number {\n",
                "    return condition\n",
                "        ? yes // true-branch\n",
                "        : no; // false-branch\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function pick(condition, yes, no) {\n",
            "    return condition\n",
            "        ? yes // true-branch\n",
            "        : no; // false-branch\n",
            "}\n",
        ),
    );
}

#[test]
fn es2017_await_recovery_uses_the_non_top_level_transform_context() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "await 'top-level';\n",
                "function awaitString() { await 'literal'; }\n",
                "function awaitNumber() { await 1; }\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "await 'top-level';\n",
            "function awaitString() { yield 'literal'; }\n",
            "function awaitNumber() { yield 1; }\n",
        ),
    );
}

#[test]
fn es2017_async_rest_parameters_remain_in_the_outer_es2015_function() {
    let cases = [
        (
            "arrow/rest",
            "const value = async (...args: any[]) => { await 0; return args.length; };\n",
            "const value = (...args) => __awaiter(",
        ),
        (
            "arrow/no-rest",
            "const value = async (item: number) => { await 0; return item; };\n",
            "const value = (item) => __awaiter(",
        ),
        (
            "ordinary/rest",
            "async function value(...args: any[]) { await 0; return args.length; }\n",
            "function value(...args) {",
        ),
        (
            "ordinary/no-rest",
            "async function value(item: number) { await 0; return item; }\n",
            "function value(item) {",
        ),
    ];
    for (label, source_text, outer_signature) in cases {
        let text = transform_async_arguments_contract(source_text);
        assert!(text.contains(outer_signature), "{label}:\n{text}");
        assert!(
            text.contains("__awaiter(this, void 0, void 0, function* ()"),
            "{label}:\n{text}",
        );
        assert!(!text.contains("args_1"), "{label}:\n{text}");
        assert!(!text.contains("function* (...args)"), "{label}:\n{text}");
    }

    let default_parameter = transform_async_arguments_contract(
        "const value = async (item = 1) => { await 0; return item; };\n",
    );
    assert!(
        default_parameter.contains("(...args_1) => __awaiter(this, [...args_1]"),
        "a default initializer still needs an inner parameter scope:\n{default_parameter}",
    );
    assert!(
        default_parameter.contains("function* (item = 1)"),
        "{default_parameter}",
    );
}

#[test]
fn es2017_async_lexical_arguments_capture_respects_arrow_and_function_boundaries() {
    let direct_cases = [
        (
            "arrow/rest",
            "const value = async (...args: any[]) => { await 0; return arguments[0] + args.length; };\n",
        ),
        (
            "arrow/no-rest",
            "const value = async () => { await 0; return arguments[0]; };\n",
        ),
        (
            "ordinary/rest",
            "async function value(...args: any[]) { await 0; return arguments[0] + args.length; }\n",
        ),
        (
            "ordinary/no-rest",
            "async function value() { await 0; return arguments[0]; }\n",
        ),
    ];
    for (label, source_text) in direct_cases {
        let text = transform_async_arguments_contract(source_text);
        assert!(
            text.contains("var arguments_1 = arguments;"),
            "{label}:\n{text}",
        );
        assert!(
            text.contains("__awaiter(this, void 0, void 0, function* ()"),
            "{label}:\n{text}",
        );
        assert!(text.contains("arguments_1[0]"), "{label}:\n{text}");
        assert!(!text.contains("args_1"), "{label}:\n{text}");
    }

    for (label, source_text) in [
        (
            "arrow owner",
            concat!(
                "const value = async (...args: any[]) => {\n",
                "    const read = () => arguments[0];\n",
                "    await 0;\n",
                "    return read() + args.length;\n",
                "};\n",
            ),
        ),
        (
            "ordinary owner",
            concat!(
                "async function value(...args: any[]) {\n",
                "    const read = () => arguments[0];\n",
                "    await 0;\n",
                "    return read() + args.length;\n",
                "}\n",
            ),
        ),
    ] {
        let text = transform_async_arguments_contract(source_text);
        assert!(
            text.contains("var arguments_1 = arguments;"),
            "{label}:\n{text}",
        );
        assert!(text.contains("() => arguments_1[0]"), "{label}:\n{text}");
        assert!(!text.contains("args_1"), "{label}:\n{text}");
    }

    let nested_ordinary = transform_async_arguments_contract(concat!(
        "const value = async (...args: any[]) => {\n",
        "    function read() { return arguments[0]; }\n",
        "    await 0;\n",
        "    return read() + args.length;\n",
        "};\n",
    ));
    assert!(
        nested_ordinary.contains("return arguments[0];"),
        "{nested_ordinary}"
    );
    assert!(
        !nested_ordinary.contains("var arguments_1"),
        "{nested_ordinary}"
    );
    assert!(!nested_ordinary.contains("args_1"), "{nested_ordinary}");
}

#[test]
fn es2015_await_lowering_restores_prefix_unary_operand_parentheses() {
    let output = transform_and_print_at_target(
        concat!(
            "async function f() {\n",
            "    <number> await 0;\n",
            "    typeof await 0;\n",
            "    void await 0;\n",
            "    await void <string> typeof <number> void await 0;\n",
            "    await await 0;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );
    assert!(output.contains("typeof (yield 0);"));
    assert!(output.contains("void (yield 0);"));
    assert!(output.contains("yield void typeof void (yield 0);"));
    assert!(output.contains("yield yield 0;"));
}

#[test]
fn tagged_template_optional_chain_recovery_omits_the_diagnostic_question_dot() {
    assert_eq!(
        transform_and_print_at_target(
            concat!("declare let a: any;\n", "a?.`b`;\n", "a?.`b${1}c`;\n",),
            ScriptTarget::ES2015,
        ),
        concat!("a `b`;\n", "a `b${1}c`;\n"),
    );
}

#[test]
fn erased_variable_types_preserve_declaration_boundary_comments() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "declare const chain: any;\n",
                "var z: any = chain.then(x => x)/*S*/.then(x => x)/*number*/;\n",
                "var first: any = one()/*first*/, second: any = two()/*second*/;\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "var z = chain.then(x => x) /*S*/.then(x => x) /*number*/;\n",
            "var first = one() /*first*/, second = two() /*second*/;\n",
        ),
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

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
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

struct ImportedTagResolver {
    import_specifier: NodeId,
    tag_reference: NodeId,
}

impl ImportedTagResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut import_specifier = None;
        let mut tag_reference = None;
        let mut pending = vec![source.root];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            match &record.data {
                NodeData::ImportSpecifier(_) => {
                    import_specifier.get_or_insert(node);
                }
                NodeData::TaggedTemplateExpression(data) => {
                    tag_reference = data.tag;
                }
                _ => {}
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        Self {
            import_specifier: import_specifier.expect("named import specifier"),
            tag_reference: tag_reference.expect("tag identifier"),
        }
    }
}

impl EmitResolver for ImportedTagResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok((node.node() == self.tag_reference)
            .then(|| EmitResolverNode::new(node.source(), self.import_specifier)))
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn common_js_named_import_tag_erases_the_substituted_property_receiver() {
    let parsed = parse_source_file(
        "tagged.ts",
        "import { css as tag } from \"react\";\ntag`color: red;`;\n",
        Default::default(),
        None,
    );
    let resolver = ImportedTagResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source_id = SourceFileId::from_raw(0);
    let source = arena.add_source(&parsed, Some(source_id));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let host = TransformContractHost {
        options: &options,
        syntax: &parsed,
        source_ids: [source_id],
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers_for_source(&options, &resolver, &host, source_id)
            .expect("CommonJS transformers"),
        false,
    )
    .expect("CommonJS named-import tagged-template transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print CommonJS named-import tagged template")
    .text()
    .to_owned();

    assert!(
        output.contains("(0, react_1.css) `color: red;`"),
        "{output}"
    );
}

struct AsyncArgumentsResolver {
    capture_functions: BTreeSet<NodeId>,
    argument_references: BTreeSet<NodeId>,
}

impl AsyncArgumentsResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut capture_functions = BTreeSet::new();
        let mut argument_references = BTreeSet::new();
        let mut pending = vec![(source.root, Vec::<(NodeId, bool)>::new())];
        while let Some((node, mut containers)) = pending.pop() {
            let record = source.arena.node(node);
            let function_is_arrow = match &record.data {
                NodeData::ArrowFunction(_) => Some(true),
                NodeData::FunctionDeclaration(_)
                | NodeData::FunctionExpression(_)
                | NodeData::MethodDeclaration(_)
                | NodeData::GetAccessor(_)
                | NodeData::SetAccessor(_)
                | NodeData::Constructor(_) => Some(false),
                _ => None,
            };
            if let Some(is_arrow) = function_is_arrow {
                containers.push((node, is_arrow));
            }
            if matches!(&record.data, NodeData::Identifier(identifier) if identifier.text == "arguments")
            {
                argument_references.insert(node);
                if let Some(mut index) = containers.len().checked_sub(1) {
                    capture_functions.insert(containers[index].0);
                    while containers[index].1 {
                        let Some(parent) = index.checked_sub(1) else {
                            break;
                        };
                        index = parent;
                        capture_functions.insert(containers[index].0);
                    }
                }
            }
            let mut children = Vec::new();
            for_each_child(&source.arena, record, |child| {
                children.push(child);
                false
            });
            for child in children {
                pending.push((child, containers.clone()));
            }
        }
        Self {
            capture_functions,
            argument_references,
        }
    }
}

impl EmitResolver for AsyncArgumentsResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(flag == NodeCheckFlags::CAPTURE_ARGUMENTS.bits() as u32
            && self.capture_functions.contains(&node.node()))
    }

    fn is_arguments_local_binding(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(self.argument_references.contains(&node.node()))
    }
}

fn transform_async_arguments_contract(source_text: &str) -> String {
    let parsed = parse_source_file("async-arguments.ts", source_text, Default::default(), None);
    let resolver = AsyncArgumentsResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("async arguments transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print async arguments transform")
        .text()
        .to_owned()
}

#[test]
fn invalid_bigint_enum_name_is_cloned_for_emit_after_diagnostics() {
    let output = transform_and_print_at_target("enum E { 0n = 0 }\n", ScriptTarget::ES2015);

    assert!(output.contains("E[E[0n] = 0] = 0n;"), "{output}");
}

struct NonQualifiedEnumMemberResolver {
    enum_declaration: NodeId,
    enum_member_references: BTreeSet<NodeId>,
}

impl EmitResolver for NonQualifiedEnumMemberResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .enum_member_references
            .contains(&node.node())
            .then(|| EmitResolverNode::new(node.source(), self.enum_declaration)))
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

#[test]
fn enum_member_substitution_respects_property_access_identifier_names() {
    let source_text = concat!(
        "enum Foo {\n",
        "    a = 2,\n",
        "    b = 3,\n",
        "    x = a.b,\n",
        "    y = b.a,\n",
        "    z = y.x * a.x,\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "enum-property-name.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut enum_declaration = None;
    let mut enum_member_references = BTreeSet::new();
    let mut stack = vec![parsed.root];
    while let Some(node) = stack.pop() {
        let record = parsed.arena.node(node);
        match &record.data {
            NodeData::EnumDeclaration(_) => enum_declaration = Some(node),
            NodeData::Identifier(identifier)
                if matches!(identifier.text.as_str(), "a" | "b" | "x" | "y") =>
            {
                enum_member_references.insert(node);
            }
            _ => {}
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    let resolver = NonQualifiedEnumMemberResolver {
        enum_declaration: enum_declaration.expect("Foo enum declaration"),
        enum_member_references,
    };
    let output =
        transform_and_print_at_target_with_resolver(source_text, ScriptTarget::ES2015, &resolver);

    assert!(
        output.contains("Foo[Foo[\"x\"] = Foo.a.b] = \"x\";"),
        "{output}"
    );
    assert!(
        output.contains("Foo[Foo[\"y\"] = Foo.b.a] = \"y\";"),
        "{output}"
    );
    assert!(
        output.contains("Foo[Foo[\"z\"] = Foo.y.x * Foo.a.x] = \"z\";"),
        "{output}"
    );
    assert!(!output.contains(".Foo."), "{output}");
}

struct ConstantValueAtNodeResolver {
    node: NodeId,
    value: f64,
}

impl EmitResolver for ConstantValueAtNodeResolver {
    fn get_constant_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok((node.node() == self.node)
            .then(|| EmitConstantValue::Number(JavaScriptNumber::from_f64(self.value))))
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

struct TypedConstantValueAtNodeResolver {
    node: NodeId,
    value: EmitConstantValue,
}

impl EmitResolver for TypedConstantValueAtNodeResolver {
    fn get_constant_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok((node.node() == self.node).then(|| self.value.clone()))
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

fn access_with_property_name(parsed: &tsc_syntax::SourceFile, expected: &str) -> NodeId {
    let mut stack = vec![parsed.root];
    while let Some(node) = stack.pop() {
        let record = parsed.arena.node(node);
        if let NodeData::PropertyAccessExpression(data) = &record.data {
            if data.name.is_some_and(|name| {
                matches!(
                    &parsed.arena.node(name).data,
                    NodeData::Identifier(identifier) if identifier.text == expected
                )
            }) {
                return node;
            }
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    panic!("missing property access named {expected}");
}

#[test]
fn const_enum_numeric_substitution_uses_javascript_number_spelling() {
    let source_text = "const enum Foo { A }\nlet value = Foo.A.toString();\n";
    let parsed = parse_source_file(
        "const-enum-number.ts",
        source_text,
        Default::default(),
        None,
    );
    let constant_access = access_with_property_name(&parsed, "A");
    for (value, expected) in [
        (100.0, "let value = 100 /* Foo.A */.toString();\n"),
        (0.5, "let value = 0.5 /* Foo.A */.toString();\n"),
        (-1.5, "let value = (-1.5 /* Foo.A */).toString();\n"),
        (-0.0, "let value = 0 /* Foo.A */.toString();\n"),
        (f64::NAN, "let value = NaN /* Foo.A */.toString();\n"),
        (
            f64::INFINITY,
            "let value = Infinity /* Foo.A */.toString();\n",
        ),
        (
            f64::NEG_INFINITY,
            "let value = (-Infinity /* Foo.A */).toString();\n",
        ),
        (1e21, "let value = 1e+21 /* Foo.A */.toString();\n"),
        (1e-7, "let value = 1e-7 /* Foo.A */.toString();\n"),
    ] {
        let resolver = ConstantValueAtNodeResolver {
            node: constant_access,
            value,
        };
        let output = transform_and_print_at_target_with_resolver(
            source_text,
            ScriptTarget::ES2015,
            &resolver,
        );
        assert_eq!(output, expected, "constant value: {value:?}");
    }
}

#[test]
fn const_enum_string_substitution_preserves_utf16_code_units() {
    let source_text = "const enum Foo { A }\nlet value = Foo.A.length;\n";
    let parsed = parse_source_file(
        "const-enum-string.ts",
        source_text,
        Default::default(),
        None,
    );
    let resolver = TypedConstantValueAtNodeResolver {
        node: access_with_property_name(&parsed, "A"),
        value: EmitConstantValue::String(JavaScriptString::from_code_units(vec![0xd800])),
    };
    let output =
        transform_and_print_at_target_with_resolver(source_text, ScriptTarget::ES2015, &resolver);

    assert_eq!(output, "let value = \"\\uD800\" /* Foo.A */.length;\n");
}

#[test]
fn const_enum_negative_access_comment_stays_inside_access_parentheses() {
    let source_text = "const enum Foo { A = -1 }\nlet value = Foo.A.toString();\n";
    let parsed = parse_source_file("const-enum.ts", source_text, Default::default(), None);
    let mut stack = vec![parsed.root];
    let mut constant_access = None;
    while let Some(node) = stack.pop() {
        let record = parsed.arena.node(node);
        if let NodeData::PropertyAccessExpression(data) = &record.data {
            let is_a = data.name.is_some_and(|name| {
                matches!(
                    &parsed.arena.node(name).data,
                    NodeData::Identifier(identifier) if identifier.text == "A"
                )
            });
            if is_a {
                constant_access = Some(node);
            }
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    let resolver = ConstantValueAtNodeResolver {
        node: constant_access.expect("Foo.A access"),
        value: -1.0,
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("const enum access transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print const enum access");

    assert_eq!(printed.text(), "let value = (-1 /* Foo.A */).toString();\n");
}

#[test]
fn const_enum_access_comment_escapes_closing_multiline_delimiter() {
    let source_text =
        "const enum Comments { Slash = 2 }\nswitch (value) { case Comments[\"*/\"]: break; }\n";
    let parsed = parse_source_file(
        "const-enum-comment.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut stack = vec![parsed.root];
    let mut constant_access = None;
    while let Some(node) = stack.pop() {
        let record = parsed.arena.node(node);
        if matches!(record.data, NodeData::ElementAccessExpression(_)) {
            constant_access = Some(node);
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    let resolver = ConstantValueAtNodeResolver {
        node: constant_access.expect("Comments[\"*/\"] access"),
        value: 2.0,
    };
    let output =
        transform_and_print_at_target_with_resolver(source_text, ScriptTarget::ES2015, &resolver);

    assert!(
        output.contains("case 2 /* Comments[\"*_/\"] */:"),
        "{output}"
    );
}

#[test]
fn commentless_const_enum_integer_access_keeps_the_property_dot_boundary() {
    let source_text = "const enum Foo { A = 100 }\nlet value = Foo.A.toString();\n";
    let parsed = parse_source_file("const-enum-dot.ts", source_text, Default::default(), None);
    let mut stack = vec![parsed.root];
    let mut constant_access = None;
    while let Some(node) = stack.pop() {
        let record = parsed.arena.node(node);
        if let NodeData::PropertyAccessExpression(data) = &record.data {
            let is_a = data.name.is_some_and(|name| {
                matches!(
                    &parsed.arena.node(name).data,
                    NodeData::Identifier(identifier) if identifier.text == "A"
                )
            });
            if is_a {
                constant_access = Some(node);
            }
        }
        for_each_child(&parsed.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    let resolver = ConstantValueAtNodeResolver {
        node: constant_access.expect("Foo.A access"),
        value: 100.0,
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.remove_comments = Some(true);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("commentless const enum access transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_remove_comments(true),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print commentless const enum access");

    assert_eq!(printed.text(), "let value = 100..toString();\n");
}

#[test]
fn commentless_const_enum_numeric_substitution_keeps_expression_grammar() {
    let source_text = "const enum Foo { A }\nlet value = Foo.A.toString();\n";
    for (value, expected) in [
        (0.5, "let value = 0.5.toString();\n"),
        (-1.5, "let value = (-1.5).toString();\n"),
        (-0.0, "let value = 0..toString();\n"),
        (f64::NAN, "let value = NaN.toString();\n"),
        (f64::INFINITY, "let value = Infinity.toString();\n"),
        (f64::NEG_INFINITY, "let value = (-Infinity).toString();\n"),
        (1e21, "let value = 1e+21..toString();\n"),
        (1e-7, "let value = 1e-7.toString();\n"),
    ] {
        let parsed = parse_source_file(
            "const-enum-no-comments.ts",
            source_text,
            Default::default(),
            None,
        );
        let resolver = ConstantValueAtNodeResolver {
            node: access_with_property_name(&parsed, "A"),
            value,
        };
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        let mut options = bootstrap_options();
        options.target = Some(ScriptTarget::ES2015.bits());
        options.remove_comments = Some(true);
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&options, &resolver).unwrap(),
            false,
        )
        .expect("commentless const enum transform");
        let printed = create_printer(
            PrinterOptions::new(NewLineKind::LineFeed)
                .with_target(ScriptTarget::ES2015)
                .with_remove_comments(true),
        )
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print commentless const enum value");

        assert_eq!(printed.text(), expected, "constant value: {value:?}");
    }
}

#[test]
fn chained_property_access_retains_a_comment_before_its_dot_token() {
    let output = transform_and_print_at_target(
        concat!(
            "interface Chain<T> { func<U>(callback: (value: T) => U): Chain<U>; }\n",
            "declare const value: Chain<number>;\n",
            "var result = value.func(x => x)\n",
            "    .func(x => x) // keep this boundary\n",
            "    .func(x => x);\n",
        ),
        ScriptTarget::ES2015,
    );

    let comment = output
        .find("// keep this boundary")
        .expect("boundary comment");
    let final_call = output[comment..]
        .find(".func(x => x)")
        .map(|offset| comment + offset)
        .expect("final chained call");
    assert!(comment < final_call, "{output}");
}

#[test]
fn property_access_cursor_comments_follow_the_token_line_scopes() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "/*lead*/Array\n",
                "/*before*/./*after*/\n",
                "    // name\n",
                "    toString/*tail*/\n",
                "\n",
                "/*optional*/Array\n",
                "/*qdot*/?./*lost*/\n",
                "    // optional name\n",
                "    toString/*optional tail*/\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "/*lead*/ Array\n",
            "    /*before*/ . /*after*/\n",
            "        // name\n",
            "        toString; /*tail*/\n",
            "/*optional*/ Array === null || Array === void 0 ? void 0 : Array\n",
            "/*qdot*/ .\n",
            "// optional name\n",
            "toString; /*optional tail*/\n",
        ),
    );
}

#[test]
fn optional_element_and_call_comments_follow_tsc_delimiter_ownership() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const obj: any;\n",
            "declare const elementKey: string;\n",
            "declare const callable: any;\n",
            "declare const callArg: any;\n",
            "obj?.[/* erased element prefix */\n",
            "// element argument\n",
            "elementKey];\n",
            "callable?.(/* call argument */ callArg);\n",
        ),
        ScriptTarget::ES2015,
    );

    let element_access = output.find("obj[").expect("lowered element access");
    let element_comment = output
        .find("// element argument")
        .expect("element argument comment");
    let element_name = output[element_comment..]
        .find("elementKey")
        .map(|offset| element_comment + offset)
        .expect("element argument");
    assert!(
        element_access < element_comment && element_comment < element_name,
        "{output}"
    );
    assert!(!output.contains("erased element prefix"), "{output}");

    let call_comment = output
        .find("/* call argument */")
        .expect("call argument comment");
    let call_open = output[..call_comment]
        .rfind("callable(")
        .expect("lowered optional call");
    let call_argument = output[call_comment..]
        .find("callArg")
        .map(|offset| call_comment + offset)
        .expect("call argument");
    assert!(
        call_open < call_comment && call_comment < call_argument,
        "{output}"
    );
    assert_eq!(output.matches("// element argument").count(), 1);
    assert_eq!(output.matches("/* call argument */").count(), 1);
}

#[test]
fn if_statement_tokens_own_each_internal_comment_boundary() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "/*1*/ if /*2*/ ( /*3*/ true /*4*/ ) /*5*/ {}\n",
                "/*1*/ if /*2*/ ( /*3*/ true /*4*/ ) /*5*/ {} /*6*/ else /*7*/ {}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "/*1*/ if /*2*/ ( /*3*/true /*4*/) /*5*/ { }\n",
            "/*1*/ if /*2*/ ( /*3*/true /*4*/) /*5*/ { } /*6*/\n",
            "else /*7*/ { }\n",
        ),
    );
}

#[test]
fn try_clause_boundaries_split_trailing_and_leading_comment_ownership() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "declare function work(): void;\n",
                "function f() {\n",
                "    try {\n",
                "        work();\n",
                "    } /* try tail */\n",
                "    // @ts-ignore\n",
                "    catch (error: number) {\n",
                "        work();\n",
                "    } /* catch tail */\n",
                "    // before finally\n",
                "    finally {\n",
                "        work();\n",
                "    }\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function f() {\n",
            "    try {\n",
            "        work();\n",
            "    } /* try tail */\n",
            "    // @ts-ignore\n",
            "    catch (error) {\n",
            "        work();\n",
            "    } /* catch tail */\n",
            "    // before finally\n",
            "    finally {\n",
            "        work();\n",
            "    }\n",
            "}\n",
        ),
    );
}

struct ExternalModuleIdentityResolver {
    expected_root: tsc_syntax::NodeId,
    observed_root: Cell<Option<tsc_syntax::NodeId>>,
}

impl EmitResolver for ExternalModuleIdentityResolver {
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

    fn is_external_or_common_js_module(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        assert_eq!(node.node(), self.expected_root);
        self.observed_root.set(Some(node.node()));
        Ok(false)
    }

    fn get_jsx_factory_import_declaration(
        &self,
        _node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_jsx_factory_export_container(
        &self,
        _node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
}

struct SystemContractResolver;

impl EmitResolver for SystemContractResolver {
    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration_at_location(
        &self,
        _node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_type_reference_serialization_kind(
        &self,
        _node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<EmitTypeReferenceSerializationKind, EmitResolverError> {
        Ok(EmitTypeReferenceSerializationKind::Unknown)
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

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
}

#[derive(Default)]
struct WrongContextImportEqualsResolver {
    referenced_alias_queries: Cell<usize>,
}

impl EmitResolver for WrongContextImportEqualsResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.referenced_alias_queries
            .set(self.referenced_alias_queries.get() + 1);
        Ok(true)
    }
}

#[test]
fn external_import_equals_recovery_respects_source_element_ownership() {
    let resolver = WrongContextImportEqualsResolver::default();
    let output = transform_and_print_at_target_with_resolver(
        concat!(
            "{\n",
            "    import rootBlock = require(\"root\");\n",
            "    import rootInternal = Other;\n",
            "}\n",
            "function f() {\n",
            "    import functionBody = require(\"function\");\n",
            "}\n",
            "namespace N {\n",
            "    import direct = require(\"direct\");\n",
            "    {\n",
            "        import namespaceBlock = require(\"nested\");\n",
            "        import namespaceInternal = Other;\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
        &resolver,
    );

    assert!(
        output.contains("import rootBlock = require(\"root\");"),
        "{output}"
    );
    assert!(output.contains("var rootInternal = Other;"), "{output}");
    assert!(
        output.contains("import functionBody = require(\"function\");"),
        "{output}"
    );
    assert!(!output.contains("direct = require"), "{output}");
    assert!(
        output.contains("import namespaceBlock = require(\"nested\");"),
        "{output}"
    );
    assert!(
        output.contains("var namespaceInternal = Other;"),
        "{output}"
    );
    assert_eq!(resolver.referenced_alias_queries.get(), 2, "{output}");
}

struct UninstantiatedModuleResolver;

impl EmitResolver for UninstantiatedModuleResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

#[test]
fn erased_type_only_namespace_anchors_source_trailing_comments() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "function y5c() { }\n",
                "namespace y5c { export interface I { foo(): void } } // erased with namespace\n",
                "\n",
                "// function then import, messes with other errors\n",
                "//function y6() { }\n",
                "//import y6 = require('');\n",
            ),
            ScriptTarget::ES2015,
            &UninstantiatedModuleResolver,
        ),
        concat!(
            "function y5c() { }\n",
            "// function then import, messes with other errors\n",
            "//function y6() { }\n",
            "//import y6 = require('');\n",
        ),
    );
}

#[test]
fn source_file_closes_an_eof_multiline_comment_like_tsc() {
    assert_eq!(
        transform_and_print_at_target(
            "const value: number = 1;\n/* retained at EOF */",
            ScriptTarget::ES2015,
        ),
        "const value = 1;\n/* retained at EOF */ \n",
    );
    assert_eq!(
        transform_and_print_at_target(
            "const value: number = 1;\n/* retained before newline */\n",
            ScriptTarget::ES2015,
        ),
        "const value = 1;\n/* retained before newline */\n",
    );
}

#[test]
fn multiline_comment_indentation_is_relative_to_its_source_line() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "class C {\n",
                "  /**\n",
                "   * Returns bar\n",
                "   */\n",
                "  public static foo(): string {\n",
                "    return \"bar\";\n",
                "  }\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "class C {\n",
            "    /**\n",
            "     * Returns bar\n",
            "     */\n",
            "    static foo() {\n",
            "        return \"bar\";\n",
            "    }\n",
            "}\n",
        ),
    );
}

#[test]
fn empty_delimited_lists_retain_their_node_array_boundary_comments() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "function foo(/** parameter list */) {\n",
                "}\n",
                "foo(/** argument list */);\n",
                "const values = [/** element list */];\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function foo( /** parameter list */) {\n",
            "}\n",
            "foo( /** argument list */);\n",
            "const values = [ /** element list */];\n",
        ),
    );
}

#[test]
fn array_element_end_comments_precede_their_delimiter() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const array = [\n",
                "    /* element 1*/\n",
                "    1\n",
                "    /* end of element 1 */,\n",
                "    2\n",
                "    /* end of element 2 */\n",
                "];\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "const array = [\n",
            "    /* element 1*/\n",
            "    1\n",
            "    /* end of element 1 */ ,\n",
            "    2\n",
            "    /* end of element 2 */\n",
            "];\n",
        ),
    );
}

#[test]
fn array_list_comment_boundaries_cover_trailing_commas_and_omitted_elements() {
    let cases = [
        (
            concat!(
                "const array = [\n",
                "    /* element 1*/\n",
                "    1 /* end of element 1 */,\n",
                "    2\n",
                "    /* end of element 2 */\n",
                "];\n",
            ),
            concat!(
                "const array = [\n",
                "    /* element 1*/\n",
                "    1 /* end of element 1 */,\n",
                "    2\n",
                "    /* end of element 2 */\n",
                "];\n",
            ),
        ),
        (
            concat!(
                "const array = [\n",
                "    /* element 1*/\n",
                "    1\n",
                "    /* end of element 1 */,\n",
                "    2\n",
                "    /* end of element 2 */, ,\n",
                "    /* extra comment */\n",
                "];\n",
            ),
            concat!(
                "const array = [\n",
                "    /* element 1*/\n",
                "    1\n",
                "    /* end of element 1 */ ,\n",
                "    2\n",
                "    /* end of element 2 */ ,\n",
                "    ,\n",
                "    /* extra comment */\n",
                "];\n",
            ),
        ),
        (
            "const array = [,, /* comment */];\n",
            "const array = [, , /* comment */];\n",
        ),
        (
            concat!("const array = [\n", "    ,, /* comment */\n", "];\n"),
            concat!(
                "const array = [\n",
                "    ,\n",
                "    , /* comment */\n",
                "];\n",
            ),
        ),
        (
            concat!(
                "const array = [\n",
                "    // comment start\n",
                "    1,\n",
                "    2,\n",
                "    // comment end\n",
                "];\n",
            ),
            concat!(
                "const array = [\n",
                "    // comment start\n",
                "    1,\n",
                "    2,\n",
                "    // comment end\n",
                "];\n",
            ),
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(
            transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
            expected,
            "source:\n{source}",
        );
    }
}

#[test]
fn binary_operator_token_boundaries_retain_leading_and_trailing_comments() {
    let source = concat!(
        "var a = 'some'\n",
        "    // comment\n",
        "    + 'text';\n",
        "\n",
        "var b = 'some'\n",
        "    /* comment */\n",
        "    + 'text';\n",
        "\n",
        "var c = 'some'\n",
        "    /* comment */\n",
        "    + /*comment1*/\n",
        "    'text';\n",
    );
    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
        concat!(
            "var a = 'some'\n",
            "    // comment\n",
            "    + 'text';\n",
            "var b = 'some'\n",
            "    /* comment */\n",
            "    + 'text';\n",
            "var c = 'some'\n",
            "    /* comment */\n",
            "    + /*comment1*/\n",
            "        'text';\n",
        ),
    );
    assert_eq!(
        transform_and_print_canonical_without_comments_at_target(source, ScriptTarget::ES2015),
        concat!(
            "var a = 'some'\n",
            "    + 'text';\n",
            "var b = 'some'\n",
            "    + 'text';\n",
            "var c = 'some'\n",
            "    +\n",
            "        'text';\n",
        ),
    );
}

#[test]
fn element_access_brackets_own_each_internal_comment_boundary() {
    let source = concat!(
        "/*0*/ Array /*1*/[ /*2*/ \"toString\" /*3*/ ] /*4*/; /*5*/\n",
        "\n",
        "/*0*/ Array \n",
        "    // single line\n",
        "    /*1*/[ /*2*/ \"toString\"\n",
        "    // single line\n",
        "    /*3*/ ] /*4*/\n",
    );
    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
        concat!(
            "/*0*/ Array /*1*/[ /*2*/\"toString\" /*3*/] /*4*/; /*5*/\n",
            "/*0*/ Array\n",
            "// single line\n",
            "/*1*/ [ /*2*/\"toString\"\n",
            "// single line\n",
            "/*3*/ ]; /*4*/\n",
        ),
    );

    let nested = transform_and_print_canonical_at_target(
        "(Array[0] /* close bracket owner */);\n",
        ScriptTarget::ES2015,
    );
    assert_eq!(nested.matches("/* close bracket owner */").count(), 1);
}

#[test]
fn erased_declarations_remain_empty_embedded_statements() {
    let source = concat!(
        "if (1)\n",
        "    const enum A {}\n",
        "else\n",
        "    const enum B {}\n",
        "do\n",
        "    const enum C {}\n",
        "while (0);\n",
        "while (0)\n",
        "    const enum D {}\n",
        "for (;0;)\n",
        "    const enum E {}\n",
        "for (let _ in [])\n",
        "    const enum F {}\n",
        "for (let _ of [])\n",
        "    const enum G {}\n",
        "// @ts-ignore suppress `with` statement error\n",
        "with (window)\n",
        "    const enum H {}\n",
    );
    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
        concat!(
            "if (1)\n",
            "    ;\n",
            "else\n",
            "    ;\n",
            "do\n",
            "    ;\n",
            "while (0);\n",
            "while (0)\n",
            "    ;\n",
            "for (; 0;)\n",
            "    ;\n",
            "for (let _ in [])\n",
            "    ;\n",
            "for (let _ of [])\n",
            "    ;\n",
            "// @ts-ignore suppress `with` statement error\n",
            "with (window)\n",
            "    ;\n",
        ),
    );
}

#[test]
fn variable_initializers_retain_comments_around_erased_type_boundaries() {
    let source = concat!(
        "let a = /*[[${something}]]*/ {};\n",
        "let b: any = /*[[${something}]]*/ {};\n",
        "let c: { hoge: boolean } = /*[[${something}]]*/ { hoge: true };\n",
        "let d: any  /*[[${something}]]*/ = {};\n",
        "let e/*[[${something}]]*/: any   = {};\n",
        "let f = /* comment1 */ d(e);\n",
        "let g: any = /* comment2 */ d(e);\n",
    );
    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
        concat!(
            "let a = /*[[${something}]]*/ {};\n",
            "let b = /*[[${something}]]*/ {};\n",
            "let c = /*[[${something}]]*/ { hoge: true };\n",
            "let d /*[[${something}]]*/ = {};\n",
            "let e /*[[${something}]]*/ = {};\n",
            "let f = /* comment1 */ d(e);\n",
            "let g = /* comment2 */ d(e);\n",
        ),
    );
}

#[test]
fn parameter_lists_retain_comments_at_each_delimiter_boundary() {
    let cases = [
        (
            concat!(
                "function commentedParameters(\n",
                "/* Parameter a */\n",
                "a\n",
                "/* End of parameter a */\n",
                "/* Parameter b */\n",
                ",\n",
                "b\n",
                "/* End of parameter b */\n",
                "){}",
            ),
            concat!(
                "function commentedParameters(\n",
                "/* Parameter a */\n",
                "a\n",
                "/* End of parameter a */\n",
                "/* Parameter b */\n",
                ", b\n",
                "/* End of parameter b */\n",
                ") { }\n",
            ),
        ),
        (
            concat!(
                "function commentedParameters(\n",
                "/* Parameter a */\n",
                "a /* End of parameter a */\n",
                "/* Parameter b */\n",
                ",\n",
                "b\n",
                "/* End of parameter b */\n",
                "){}",
            ),
            concat!(
                "function commentedParameters(\n",
                "/* Parameter a */\n",
                "a /* End of parameter a */\n",
                "/* Parameter b */\n",
                ", b\n",
                "/* End of parameter b */\n",
                ") { }\n",
            ),
        ),
        (
            concat!(
                "function commentedParameters(\n",
                "a /* parameter a */, \n",
                "b /* parameter b */,\n",
                "/* extra comment */\n",
                ") { }",
            ),
            "function commentedParameters(a /* parameter a */, b /* parameter b */) { }\n",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(
            transform_and_print_canonical_at_target(source, ScriptTarget::ES2015),
            expected,
            "source:\n{source}",
        );
    }
}

#[test]
fn parenthesized_open_token_owns_its_same_line_trailing_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "var j;\nvar f: () => any;\n<any>( /* Preserve */ j = f());\n",
            ScriptTarget::ES2015,
        ),
        "var j;\nvar f;\n( /* Preserve */j = f());\n",
    );
}

#[test]
fn type_erasure_parentheses_do_not_reown_the_statement_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "// Must stay outside the generated parentheses\n(x + 1 as number) * 3;\n",
            ScriptTarget::ES2015,
        ),
        "// Must stay outside the generated parentheses\n(x + 1) * 3;\n",
    );
}

#[test]
fn nullish_lowering_parentheses_do_not_reown_the_statement_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "// Must stay outside the generated parentheses\na ?? b || c;\n",
            ScriptTarget::ES2015,
        ),
        "// Must stay outside the generated parentheses\n(a !== null && a !== void 0 ? a : b) || c;\n",
    );
}

#[test]
fn concise_arrow_body_retains_its_leading_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "function Foo(x: any)\n",
                "{\n",
                "}\n",
                " \n",
                "Foo(() =>\n",
                "    // do something\n",
                "    127);\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function Foo(x) {\n",
            "}\n",
            "Foo(() => \n",
            "// do something\n",
            "127);\n",
        ),
    );
}

#[test]
fn concise_arrow_body_retains_its_same_line_leading_comment_once() {
    let output = transform_and_print_canonical_at_target(
        "const f = (a: any) => /*here a should be any*/ a.toString();\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(
        output,
        "const f = (a) => /*here a should be any*/ a.toString();\n",
    );
    assert_eq!(
        output.matches("here a should be any").count(),
        1,
        "{output}"
    );
}

#[test]
fn retained_arrow_token_owns_its_same_line_trailing_comment_once() {
    let output = transform_and_print_canonical_at_target(
        "const f = a => /*here a should be any*/ a.toString();\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(
        output,
        "const f = a => /*here a should be any*/ a.toString();\n",
    );
    assert_eq!(
        output.matches("here a should be any").count(),
        1,
        "{output}"
    );
}

#[test]
fn simple_arrow_parameter_and_arrow_token_keep_distinct_trailing_comments() {
    let output = transform_and_print_canonical_at_target(
        "const f = a /*before*/ => /*after*/ a;\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(output, "const f = a /*before*/ => /*after*/ a;\n");
    assert_eq!(output.matches("before").count(), 1, "{output}");
    assert_eq!(output.matches("after").count(), 1, "{output}");
}

#[test]
fn simple_arrow_parameter_list_preserves_tsc_intervening_comment_replay() {
    let output = transform_and_print_canonical_at_target(
        "const f = /*head*/ a /*parameter*/ => a;\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(
        output,
        "const f = /*head*/ /*head*/ a /*parameter*/ => a;\n"
    );
    assert_eq!(output.matches("head").count(), 2, "{output}");
    assert_eq!(output.matches("parameter").count(), 1, "{output}");
}

#[test]
fn retained_arrow_line_comment_keeps_tsc_body_spacing() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "const f = a => // after\n    a;\n",
            ScriptTarget::ES2015,
        ),
        "const f = a => // after\n a;\n",
    );
}

#[test]
fn retained_arrow_token_owns_multiple_comments_before_a_block_body() {
    let output = transform_and_print_canonical_at_target(
        "const f = a => /* first */ /* second */ { return a; };\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(
        output,
        "const f = a => /* first */ /* second */ { return a; };\n"
    );
    assert_eq!(output.matches("first").count(), 1, "{output}");
    assert_eq!(output.matches("second").count(), 1, "{output}");
}

#[test]
fn retained_arrow_comment_resumes_before_a_parenthesized_concise_body() {
    let output = transform_and_print_canonical_at_target(
        "const f = a => /* before object */ ({ value: a });\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(
        output,
        "const f = a => /* before object */ ({ value: a });\n"
    );
    assert_eq!(output.matches("before object").count(), 1, "{output}");
}

#[test]
fn remove_comments_suppresses_simple_parameter_and_retained_arrow_comments() {
    assert_eq!(
        transform_and_print_canonical_without_comments_at_target(
            "const f = a /* parameter */ => /* first */ /* second */ a;\n",
            ScriptTarget::ES2015,
        ),
        "const f = a => a;\n",
    );
}

#[test]
fn erased_parenthesized_arrow_body_preserves_its_no_asi_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const x = (a: any[]) => (\n",
                "    // comment\n",
                "    undefined as number\n",
                ");\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!("const x = (a) => \n", "// comment\n", "undefined;\n",),
    );
}

#[test]
fn no_asi_projection_preserves_parsed_parenthesis_token_comments() {
    let source = concat!(
        "function open(a: any) { return (//open\n a as any); }\n",
        "function both(a: any) { return (/*open-block*/\n//lead\n a as any /*close*/); }\n",
    );

    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES_NEXT),
        concat!(
            "function open(a) {\n",
            "    return ( //open\n",
            "    a);\n",
            "}\n",
            "function both(a) {\n",
            "    return ( /*open-block*/\n",
            "    //lead\n",
            "    a /*close*/);\n",
            "}\n",
        ),
    );
}

#[test]
fn return_and_yield_no_asi_parentheses_follow_erased_left_edges() {
    let source = concat!(
        "function returns(a: any, b: any, c: any, k: any) {\n",
        "    return (\n        // r-direct\n        a as any);\n",
        "    return (\n        // r-property\n        a as any).b;\n",
        "    return (\n        // r-element\n        a as any)[k];\n",
        "    return (\n        // r-call\n        a as any)();\n",
        "    return (\n        // r-tagged\n        a as any)`x`;\n",
        "    return (\n        // r-binary\n        a as any) + b;\n",
        "    return (\n        // r-conditional\n        a as any) ? b : c;\n",
        "    return (\n        // r-nested-as\n        a as any) as unknown;\n",
        "    return (\n        // r-satisfies\n        a as any) satisfies unknown;\n",
        "    return (\n        // r-non-null\n        a as any)!;\n",
        "    return (/* r-block */ a as any).b;\n",
        "    return (\n        /* r-newline-block */\n        a as any).b;\n",
        "}\n",
        "function* yields(a: any, b: any, c: any, k: any) {\n",
        "    yield (\n        // y-direct\n        a as any);\n",
        "    yield (\n        // y-property\n        a as any).b;\n",
        "    yield (\n        // y-element\n        a as any)[k];\n",
        "    yield (\n        // y-call\n        a as any)();\n",
        "    yield (\n        // y-tagged\n        a as any)`x`;\n",
        "    yield (\n        // y-binary\n        a as any) + b;\n",
        "    yield (\n        // y-conditional\n        a as any) ? b : c;\n",
        "    yield (\n        // y-nested-as\n        a as any) as unknown;\n",
        "    yield (\n        // y-satisfies\n        a as any) satisfies unknown;\n",
        "    yield (\n        // y-non-null\n        a as any)!;\n",
        "    yield (/* y-block */ a as any).b;\n",
        "    yield (\n        /* y-newline-block */\n        a as any).b;\n",
        "}\n",
    );

    assert_eq!(
        transform_and_print_canonical_at_target(source, ScriptTarget::ES_NEXT),
        concat!(
            "function returns(a, b, c, k) {\n",
            "    return (\n    // r-direct\n    a);\n",
            "    return (\n    // r-property\n    a).b;\n",
            "    return (\n    // r-element\n    a)[k];\n",
            "    return (\n    // r-call\n    a)();\n",
            "    return (\n    // r-tagged\n    a) `x`;\n",
            "    return (\n    // r-binary\n    a) + b;\n",
            "    return (\n    // r-conditional\n    a) ? b : c;\n",
            "    return (\n    // r-nested-as\n    a);\n",
            "    return (\n    // r-satisfies\n    a);\n",
            "    return (\n    // r-non-null\n    a);\n",
            "    return /* r-block */ a.b;\n",
            "    return (\n    /* r-newline-block */\n    a).b;\n",
            "}\n",
            "function* yields(a, b, c, k) {\n",
            "    yield (\n    // y-direct\n    a);\n",
            "    yield (\n    // y-property\n    a).b;\n",
            "    yield (\n    // y-element\n    a)[k];\n",
            "    yield (\n    // y-call\n    a)();\n",
            "    yield (\n    // y-tagged\n    a) `x`;\n",
            "    yield (\n    // y-binary\n    a) + b;\n",
            "    yield (\n    // y-conditional\n    a) ? b : c;\n",
            "    yield (\n    // y-nested-as\n    a);\n",
            "    yield (\n    // y-satisfies\n    a);\n",
            "    yield (\n    // y-non-null\n    a);\n",
            "    yield /* y-block */ a.b;\n",
            "    yield (\n    /* y-newline-block */\n    a).b;\n",
            "}\n",
        ),
    );
}

#[test]
fn partially_emitted_satisfies_wrappers_preserve_inner_comments() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const a = (/*comm*/ 10 satisfies number);\n",
                "const b = ((/*comm*/ 10 satisfies number));\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!("const a = /*comm*/ 10;\n", "const b = /*comm*/ 10;\n",),
    );
}

#[test]
fn case_colon_token_owns_its_same_line_trailing_comment() {
    let output = transform_and_print_canonical_at_target(
        concat!(
            "function getSecurity(level) {\n",
            "    switch(level){\n",
            "        case 0: // Zero\n",
            "        case 1: // one\n",
            "        case 2: // two\n",
            "            return \"Hi\";\n",
            "        case 3: // three\n",
            "        case 4   : // four\n",
            "            return \"hello\";\n",
            "        case 5: // five\n",
            "        default:  // default\n",
            "            return \"world\";\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert_eq!(output.matches("//").count(), 7, "{output}");
    for expected in [
        "case 0: // Zero",
        "case 1: // one",
        "case 2: // two",
        "case 3: // three",
        "case 4: // four",
        "case 5: // five",
        "default: // default",
    ] {
        assert!(
            output.lines().any(|line| line.trim() == expected),
            "{output}"
        );
    }
    assert!(
        !output.lines().any(|line| line.trim() == "// two"),
        "{output}"
    );
    assert!(
        !output.lines().any(|line| line.trim() == "// four"),
        "{output}"
    );
    assert!(
        !output.lines().any(|line| line.trim() == "// default"),
        "{output}"
    );
}

#[test]
fn call_list_start_distinguishes_comment_first_from_newline_first() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "foo(\n",
                "    /*c4*/\n",
                "    () => { });\n",
                "foo(/*c7*/\n",
                "    () => { });\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "foo(\n",
            "/*c4*/\n",
            "() => { });\n",
            "foo(/*c7*/ () => { });\n",
        ),
    );
}

#[test]
fn property_initializer_retains_its_intervening_comment() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            "var v = {\n    f: /**own f*/ (a) => 0\n}\n",
            ScriptTarget::ES2015,
        ),
        "var v = {\n    f: /**own f*/ (a) => 0\n};\n",
    );
}

#[test]
fn inline_array_list_indent_is_retained_by_a_multiline_object_element() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const repro: any = {\n",
                "  dataType: {\n",
                "    fields: [{\n",
                "      key: 'bla', // retained\n",
                "      value: null,\n",
                "    }],\n",
                "  }\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "const repro = {\n",
            "    dataType: {\n",
            "        fields: [{\n",
            "                key: 'bla', // retained\n",
            "                value: null,\n",
            "            }],\n",
            "    }\n",
            "};\n",
        ),
    );
}

#[test]
fn multiline_object_drops_only_comments_on_the_opening_delimiter_line() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const value: any = { // dropped with the opening delimiter\n",
                "  // retained on its own line\n",
                "  property: 1\n",
                "};\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "const value = {\n",
            "    // retained on its own line\n",
            "    property: 1\n",
            "};\n",
        ),
    );
}

#[test]
fn preserved_jsx_expression_comments_follow_jsx_token_boundaries() {
    let source_text = concat!(
        "class Component {\n",
        "    render() {\n",
        "        return <div>\n",
        "            {/* missing */}\n",
        "            {null/* preserved */}\n",
        "            {\n",
        "                // ??? 1\n",
        "            }\n",
        "            { // ??? 2\n",
        "            }\n",
        "            {// ??? 3\n",
        "            }\n",
        "            {\n",
        "                // ??? 4\n",
        "            /* ??? 5 */}\n",
        "        </div>;\n",
        "    }\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "jsx-comments.tsx",
        source_text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(1);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("preserve JSX transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print preserved JSX comments")
    .text()
    .to_owned();

    assert!(output.contains("{null /* preserved */}"), "{output}");
    assert!(
        output.contains("            {// ??? 2\n            }"),
        "{output}"
    );
    assert!(
        output.contains("            {// ??? 3\n            }"),
        "{output}"
    );
    assert!(
        output.contains(concat!(
            "            {\n",
            "            // ??? 4\n",
            "            /* ??? 5 */ }",
        )),
        "{output}",
    );
}

#[test]
fn automatic_jsx_children_retain_expression_trailing_comments() {
    let parsed = parse_source_file(
        "jsx-child-comment.tsx",
        "const value = <div>{null/* preserved */}</div>;\n",
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = ExternalModuleIdentityResolver {
        expected_root: parsed.root,
        observed_root: Cell::new(None),
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(4);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("automatic JSX transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print automatic JSX child comment")
    .text()
    .to_owned();

    assert_eq!(resolver.observed_root.get(), Some(resolver.expected_root));
    assert!(
        output.contains("{ children: null /* preserved */ }"),
        "{output}"
    );
}

#[test]
fn classic_jsx_children_retain_expression_trailing_comments() {
    let parsed = parse_source_file(
        "classic-jsx-child-comment.tsx",
        "const value = <div>{null/* preserved */}</div>;\n",
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = ExternalModuleIdentityResolver {
        expected_root: parsed.root,
        observed_root: Cell::new(None),
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(2);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("classic JSX transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print classic JSX child comment")
    .text()
    .to_owned();

    assert!(
        output.contains("React.createElement(\"div\", null, null /* preserved */)"),
        "{output}",
    );
}

#[test]
fn classic_jsx_attribute_tail_is_not_owned_by_the_generated_property() {
    let parsed = parse_source_file(
        "jsx-attribute-tail.tsx",
        concat!(
            "declare const Widget: any;\n",
            "declare function use(value: any): any;\n",
            "const view = <Widget\n",
            "    value={x => use(x)/* initializer */} // attribute tail\n",
            "/>;\n",
        ),
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = ExternalModuleIdentityResolver {
        expected_root: parsed.root,
        observed_root: Cell::new(None),
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(2);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("classic JSX attribute-tail transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print classic JSX attribute-tail ownership")
    .text()
    .to_owned();

    assert_eq!(
        output,
        "const view = React.createElement(Widget, { value: x => use(x) /* initializer */ });\n",
    );
    assert!(!output.contains("attribute tail"), "{output}");
}

#[test]
fn parenthesized_jsx_line_comment_terminates_before_closing_delimiters() {
    let source_text = concat!(
        "const categories = ['Fruit'];\n",
        "const Component = () => (\n",
        "    <ul>{categories.map((category) => (\n",
        "        <li key={category}>{category}</li> // Error about 'key' only\n",
        "    ))}</ul>\n",
        ");\n",
    );
    let parsed = parse_source_file(
        "jsx-parenthesized-comment.tsx",
        source_text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = ExternalModuleIdentityResolver {
        expected_root: parsed.root,
        observed_root: Cell::new(None),
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(2);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("classic JSX parenthesized-comment transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print classic JSX parenthesized comment")
    .text()
    .to_owned();

    let marker = "// Error about 'key' only";
    let marker_end = output.find(marker).expect("retained JSX line comment") + marker.len();
    let after_comment = &output[marker_end..];
    assert!(after_comment.starts_with('\n'), "{output}");
    assert_eq!(
        after_comment.trim_start().chars().next(),
        Some(')'),
        "{output}"
    );
}

struct JsxNamespaceResolver {
    namespace_declaration: NodeId,
    tag_references: BTreeSet<NodeId>,
    factory_locations: BTreeSet<NodeId>,
}

impl JsxNamespaceResolver {
    fn new(source: &tsc_syntax::SourceFile) -> Self {
        let mut namespace_declaration = None;
        let mut tag_references = BTreeSet::new();
        let mut factory_locations = BTreeSet::new();
        let mut pending = vec![source.root];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            if record.kind == SyntaxKind::JsxOpeningFragment {
                factory_locations.insert(node);
            }
            match &record.data {
                NodeData::ModuleDeclaration(_) => {
                    namespace_declaration.get_or_insert(node);
                }
                NodeData::JsxOpeningElement(data) => {
                    factory_locations.insert(node);
                    tag_references.extend(data.tag_name);
                }
                NodeData::JsxSelfClosingElement(data) => {
                    factory_locations.insert(node);
                    tag_references.extend(data.tag_name);
                }
                NodeData::JsxClosingElement(data) => {
                    tag_references.extend(data.tag_name);
                }
                _ => {}
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        Self {
            namespace_declaration: namespace_declaration.expect("namespace declaration"),
            tag_references,
            factory_locations,
        }
    }
}

impl EmitResolver for JsxNamespaceResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn is_instantiated_module(&self, _node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .tag_references
            .contains(&node.node())
            .then(|| EmitResolverNode::new(node.source(), self.namespace_declaration)))
    }

    fn get_jsx_factory_import_declaration(
        &self,
        _node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_jsx_factory_export_container(
        &self,
        node: EmitResolverNode,
        name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(
            (name == "React" && self.factory_locations.contains(&node.node()))
                .then(|| EmitResolverNode::new(node.source(), self.namespace_declaration)),
        )
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

fn transform_jsx_namespace_contract(jsx: i32) -> String {
    let source_text = concat!(
        "namespace M {\n",
        "    export var React = { createElement() {} };\n",
        "    export var X = () => null;\n",
        "    var y = <X></X>;\n",
        "    var z = <X />;\n",
        "    var fragment = <></>;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "jsx-namespace.tsx",
        source_text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let resolver = JsxNamespaceResolver::new(&parsed);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.jsx = Some(jsx);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).expect("JSX namespace transformers"),
        false,
    )
    .expect("JSX namespace transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print JSX namespace transform")
    .text()
    .to_owned()
}

#[test]
fn preserved_jsx_tags_use_expression_namespace_substitution() {
    let output = transform_jsx_namespace_contract(1);

    assert!(output.contains("var y = <M.X></M.X>;"), "{output}");
}

#[test]
fn classic_jsx_factory_root_carries_typed_namespace_identity() {
    let output = transform_jsx_namespace_contract(2);

    assert!(
        output.contains("var y = M.React.createElement(M.X, null);"),
        "{output}",
    );
}

#[test]
fn classic_self_closing_jsx_factory_root_keeps_opening_node_identity() {
    let output = transform_jsx_namespace_contract(2);

    assert!(
        output.contains("var z = M.React.createElement(M.X, null);"),
        "{output}",
    );
}

#[test]
fn classic_jsx_fragment_roots_keep_opening_fragment_identity() {
    let output = transform_jsx_namespace_contract(2);

    assert!(
        output.contains("var fragment = M.React.createElement(M.React.Fragment, null);"),
        "{output}",
    );
}

#[test]
fn namespace_export_assignment_does_not_reown_the_declaration_comment() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace M {\n",
                "    export class C implements I {} // declaration comment\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var M;\n",
            "(function (M) {\n",
            "    class C {\n",
            "    } // declaration comment\n",
            "    M.C = C;\n",
            "})(M || (M = {}));\n",
        ),
    );
}

#[test]
fn namespace_exported_binding_patterns_publish_each_leaf() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace M {\n",
                "    export let [bar5] = [1];\n",
                "    export const [bar6] = [2];\n",
                "    export let { a: bar7 } = { a: 1 };\n",
                "    export const { a: bar8 } = { a: 1 };\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var M;\n",
            "(function (M) {\n",
            "    M.bar5 = [1][0];\n",
            "    M.bar6 = [2][0];\n",
            "    M.bar7 = { a: 1 }.a;\n",
            "    M.bar8 = { a: 1 }.a;\n",
            "})(M || (M = {}));\n",
        ),
    );
}

#[test]
fn namespace_destructuring_plan_hoists_temps_and_preserves_leaf_order() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace M {\n",
                "    export let [a, b] = source();\n",
                "    export let { x: { y = fallback() }, [key()]: z, ...rest } = obj();\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var __rest = (this && this.__rest) || function (s, e) {\n",
            "    var t = {};\n",
            "    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)\n",
            "        t[p] = s[p];\n",
            "    if (s != null && typeof Object.getOwnPropertySymbols === \"function\")\n",
            "        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {\n",
            "            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))\n",
            "                t[p[i]] = s[p[i]];\n",
            "        }\n",
            "    return t;\n",
            "};\n",
            "var M;\n",
            "(function (M) {\n",
            "    var _a, _b, _c, _d;\n",
            "    _a = source(), M.a = _a[0], M.b = _a[1];\n",
            "    _b = obj(), _c = _b.x.y, M.y = _c === void 0 ? fallback() : _c, _d = key(), M.z = _b[_d], M.rest = __rest(_b, [\"x\", typeof _d === \"symbol\" ? _d : _d + \"\"]);\n",
            "})(M || (M = {}));\n",
        ),
    );
}

#[test]
fn empty_namespace_body_does_not_reown_an_erased_statement_comment() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "class Foo { static x: number; }\n",
                "namespace Foo {\n",
                "    export var x: number; // erased trailing\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "class Foo {\n",
            "}\n",
            "(function (Foo) {\n",
            "})(Foo || (Foo = {}));\n",
        ),
    );
}

#[test]
fn empty_multiline_block_indents_its_node_array_comment() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "class C {\n",
                "    P(ii: number, j: number, k: number) {\n",
                "        for (var i = 0; i < arguments.length; i++) {\n",
                "            // WScript.Echo(\"param: \" + arguments[i]);\n",
                "        }\n",
                "    }\n",
                "}\n",
                "var c = new C();\n",
                "c.P(1, 2, 3);\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "class C {\n",
            "    P(ii, j, k) {\n",
            "        for (var i = 0; i < arguments.length; i++) {\n",
            "            // WScript.Echo(\"param: \" + arguments[i]);\n",
            "        }\n",
            "    }\n",
            "}\n",
            "var c = new C();\n",
            "c.P(1, 2, 3);\n",
        ),
    );
}

#[test]
fn empty_function_body_does_not_reown_the_open_brace_trailing_comment() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "function D7() {\n",
                "    return class T {\n",
                "        a(x = arguments) {  // ok\n",
                "        }\n",
                "    };\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function D7() {\n",
            "    return class T {\n",
            "        a(x = arguments) {\n",
            "        }\n",
            "    };\n",
            "}\n",
        ),
    );
}

#[test]
fn dotted_namespace_inherits_export_and_lexical_declaration_ownership() {
    let text = transform_and_print_at_target_with_resolver(
        concat!(
            "namespace Shape.Utils {\n",
            "    export function convert() { return null; }\n",
            "}\n",
            "namespace Explicit {\n",
            "    export namespace Nested {\n",
            "        export function convert() { return null; }\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
        &InstantiatedModuleResolver,
    );

    assert!(
        text.contains(concat!(
            "(function (Shape) {\n",
            "    var Utils;\n",
            "    (function (Utils) {",
        )),
        "{text}"
    );
    assert!(
        text.contains("})(Utils = Shape.Utils || (Shape.Utils = {}));"),
        "{text}"
    );
    assert!(
        text.contains(concat!(
            "(function (Explicit) {\n",
            "    let Nested;\n",
            "    (function (Nested) {",
        )),
        "{text}"
    );
    assert!(
        text.contains("})(Nested = Explicit.Nested || (Explicit.Nested = {}));"),
        "{text}"
    );
}

#[test]
fn dotted_namespace_iife_body_retains_module_block_closing_comments() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace hello.hi.world\n",
                "{\n",
                "    function foo() {}\n",
                "\n",
                "    // TODO, blah\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var hello;\n",
            "(function (hello) {\n",
            "    var hi;\n",
            "    (function (hi) {\n",
            "        var world;\n",
            "        (function (world) {\n",
            "            function foo() { }\n",
            "            // TODO, blah\n",
            "        })(world = hi.world || (hi.world = {}));\n",
            "    })(hi = hello.hi || (hello.hi = {}));\n",
            "})(hello || (hello = {}));\n",
        ),
    );
}

#[test]
fn namespace_body_retains_comments_after_its_last_runtime_statement() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace Foo {\n",
                "    // retained leading\n",
                "    class Helper {\n",
                "    }\n",
                "    // retained closing one\n",
                "    // retained closing two\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var Foo;\n",
            "(function (Foo) {\n",
            "    // retained leading\n",
            "    class Helper {\n",
            "    }\n",
            "    // retained closing one\n",
            "    // retained closing two\n",
            "})(Foo || (Foo = {}));\n",
        ),
    );
}

#[test]
fn namespace_body_does_not_reown_a_removed_tail_declaration_comment() {
    assert_eq!(
        transform_and_print_at_target_with_resolver(
            concat!(
                "namespace M {\n",
                "    var kept: number;\n",
                "    export var removed: string; // removed with declaration\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
            &InstantiatedModuleResolver,
        ),
        concat!(
            "var M;\n",
            "(function (M) {\n",
            "    var kept;\n",
            "})(M || (M = {}));\n",
        ),
    );
}

#[test]
fn namespace_parameter_names_reserve_descendant_bindings_not_member_names() {
    let text = transform_and_print_at_target_with_resolver(
        concat!(
            "namespace M { class C { set Z(M) { } } }\n",
            "namespace M { class D { set Z(value) { var M = value; } } }\n",
            "namespace M { class E { set M(value) { } } }\n",
            "namespace M { class F { get Z() { var M = 1; return M; } } }\n",
            "namespace M { class G { get M() { return 1; } } }\n",
        ),
        ScriptTarget::ES2015,
        &InstantiatedModuleResolver,
    );

    let parameters = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("(function ("))
        .filter_map(|line| line.strip_suffix(") {"))
        .collect::<Vec<_>>();
    assert_eq!(parameters, ["M_1", "M_2", "M", "M_3", "M"], "{text}");
}

#[test]
fn ambient_namespace_exports_keep_only_their_non_emitted_anchor() {
    let text = transform_and_print_at_target_with_resolver(
        concat!(
            "namespace N {\n",
            "    export declare class C { }\n",
            "    export declare function f(): void;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
        &InstantiatedModuleResolver,
    );

    assert!(
        text.contains("(function (N) {\n})(N || (N = {}));"),
        "{text}"
    );
    assert!(!text.contains("N.C = C"), "{text}");
    assert!(!text.contains("N.f = f"), "{text}");
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

    let (statement_count, not_emitted_statement_count) =
        match &result.arena().node(root).unwrap().data {
            NodeData::SourceFile(data) => result
                .arena()
                .node_array_ref(source, data.statements.unwrap())
                .map(|array| {
                    let statements = &result.arena().node_array(array).unwrap().nodes;
                    let not_emitted = statements
                        .iter()
                        .filter(|statement| {
                            result
                                .arena()
                                .node_ref(source, **statement)
                                .and_then(|statement| result.arena().node(statement).ok())
                                .is_some_and(|statement| {
                                    statement.kind == SyntaxKind::NotEmittedStatement
                                })
                        })
                        .count();
                    (statements.len() - not_emitted, not_emitted)
                })
                .unwrap(),
            _ => unreachable!(),
        };
    assert_eq!(not_emitted_statement_count, 2);
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
fn es2018_array_binding_defers_identifier_default_after_object_rest() {
    let output = transform_and_print_at_target(
        "let [{ ...a }, b = a]: any[] = [{ x: 1 }];\n",
        ScriptTarget::ES2015,
    );
    assert!(
        output
            .contains("let [_a, _b] = [{ x: 1 }], a = __rest(_a, []), b = _b === void 0 ? a : _b;"),
        "the later default must run after the earlier object-rest binding:\n{output}"
    );
}

#[test]
fn es2018_object_rest_classifies_literal_computed_keys_without_temporaries() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const obj: any;\n",
            "declare const key: PropertyKey;\n",
            "let { 'a': a3, ...r3 } = obj;\n",
            "let { ['a']: a4, ...r4 } = obj;\n",
            "let { ['\\u0061']: escaped, ...escapedRest } = obj;\n",
            "let { [1]: numeric, ...numericRest } = obj;\n",
            "let { [key]: dynamic, ...dynamicRest } = obj;\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("{ 'a': a3 } = obj, r3 = __rest(obj, ['a']);"),
        "{output}",
    );
    assert!(
        output.contains("{ ['a']: a4 } = obj, r4 = __rest(obj, ['a']);"),
        "{output}",
    );
    assert!(
        output.contains(concat!(
            "{ ['\\u0061']: escaped } = obj, ",
            "escapedRest = __rest(obj, ['\\u0061']);",
        )),
        "{output}",
    );
    assert!(
        output.contains("{ [1]: numeric } = obj, numericRest = __rest(obj, [\"1\"]);"),
        "{output}",
    );
    assert_eq!(output.matches("=== \"symbol\"").count(), 1, "{output}");
    assert!(!output.contains("= 'a', a4 ="), "{output}");
}

#[test]
fn es2018_object_rest_assignment_captures_a_rhs_identifier_reassigned_by_the_pattern() {
    let output = transform_and_print_at_target(
        concat!(
            "declare let o: any;\n",
            "declare let computed: PropertyKey;\n",
            "declare let computed2: PropertyKey;\n",
            "declare let first: any, second: any;\n",
            "({ [computed]: first, [computed2]: second, ...o } = o);\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(output.contains("var _a, _b, _c;"), "{output}");
    assert!(
        output.contains(concat!(
            "(_a = o, _b = computed, first = _a[_b], ",
            "_c = computed2, second = _a[_c], o = __rest(_a, ",
        )),
        "the RHS value must be captured before computed keys and the rest target: {output}",
    );
    assert!(!output.contains("first = o["), "{output}");
}

#[test]
fn es2018_non_final_declaration_rest_remains_in_the_final_rest_exclusion_set() {
    let output = transform_and_print_at_target(
        concat!(
            "var { ...a, x, ...b } = { x: 1 };\n",
            "({ ...a, x, ...b } = { x: 1 });\n",
        ),
        ScriptTarget::ES2015,
    );

    assert_eq!(output.matches("[\"a\", \"x\"]").count(), 1, "{output}");
    assert_eq!(output.matches("[\"x\"]").count(), 1, "{output}");
}

#[test]
fn es2018_empty_array_object_rest_target_materializes_only_its_value() {
    let output = transform_and_print_at_target("({...[]} = {});\n", ScriptTarget::ES2015);
    assert!(output.contains("var _a;"), "{output}");
    assert!(output.contains("(_a = __rest({}, []));"), "{output}");
    assert!(
        !output.contains("[] ="),
        "empty target must not be re-assigned: {output}"
    );
}

#[test]
fn es2018_discarded_outer_call_still_requires_object_rest_argument_values() {
    let output = transform_and_print_at_target(
        concat!(
            "declare function consume(value: unknown): void;\n",
            "declare let target: unknown;\n",
            "consume(({ ...target } = { x: 1 }));\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(output.contains("var _a;"), "{output}");
    assert!(
        output.contains("consume((_a = { x: 1 }, target = __rest(_a, []), _a));"),
        "the call result is discarded, but its argument value is required: {output}",
    );
}

#[test]
fn es2018_retained_object_binding_comment_has_one_list_owner() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const props: any;\n",
            "const { children, // here!\n",
            "active: _a, // rest boundary\n",
            "...rest } = props;\n",
        ),
        ScriptTarget::ES2015,
    );
    assert_eq!(output.matches("// here!").count(), 1, "{output}");
    assert!(
        output.contains("{ children, // here!\nactive: _a } = props"),
        "{output}"
    );
}

#[test]
fn compact_binding_pattern_does_not_indent_after_source_line_comment() {
    let output = transform_and_print_canonical_at_target(
        concat!(
            "const { a = 1, b = 2, c = b, // ok\n",
            "d = a, // ok\n",
            "e = f } = {};\n",
        ),
        ScriptTarget::ES2015,
    );
    assert!(
        output.contains("c = b, // ok\nd = a, // ok\ne = f"),
        "{output}"
    );
}

#[test]
fn erased_class_heritage_slot_keeps_the_name_boundary_comment() {
    let output = transform_and_print_at_target(
        "class ConnectionError /* extends Error */ { constructor(request: unknown) {} }\n",
        ScriptTarget::ES2015,
    );
    assert!(
        output.contains("class ConnectionError /* extends Error */ {"),
        "{output}"
    );
}

#[test]
fn es2015_single_object_literal_spread_retains_object_assign_call() {
    let text =
        transform_and_print_at_target("let i;\ni = { ...{ a: \"a\" } };\n", ScriptTarget::ES2015);

    assert_eq!(text, "let i;\ni = Object.assign({ a: \"a\" });\n");
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
fn es2015_static_class_expression_keeps_its_assigned_name_and_helper_order() {
    let output = transform_and_print_at_target(
        concat!(
            "const promise = (async () => { await 0; })();\n",
            "function f() { let y = class { static a = x; }; let x; }\n",
            "async function* g() { yield 1; }\n",
        ),
        ScriptTarget::ES2015,
    );
    let awaiter = output.find("var __awaiter").expect("async helper");
    let set_function_name = output
        .find("var __setFunctionName")
        .expect("static class expression named-evaluation helper");
    let await_value = output.find("var __await =").expect("async-yield helper");
    let async_generator = output
        .find("var __asyncGenerator")
        .expect("async-generator helper");
    assert!(awaiter < set_function_name);
    assert!(set_function_name < await_value);
    assert!(await_value < async_generator);
    assert!(output.contains("__setFunctionName(_a, \"y\")"));
}

#[test]
fn anonymous_class_receiver_precedes_its_computed_static_key_binding() {
    let parsed = parse_source_file(
        "static-field-binding-order.ts",
        concat!(
            "const key = \"x\";\n",
            "let value = class {\n",
            "    // static field\n",
            "    static [key] = 1;\n",
            "};\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("ES2015 computed static class-expression transform");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print computed static class expression")
    .text()
    .to_owned();

    assert!(output.contains("var _a, _b;"), "{output}");
    let receiver = output.find("_a = class").expect("class receiver");
    let key = output.find("_b = key").expect("computed-key cache");
    let named = output
        .find("__setFunctionName(_a, \"value\")")
        .expect("named evaluation");
    let comment = output.find("// static field").expect("field comment");
    let initializer = output.find("_a[_b] = 1").expect("static initializer");
    assert_eq!(output.matches("// static field").count(), 1, "{output}");
    assert!(
        receiver < key && key < named && named < comment && comment < initializer,
        "{output}"
    );
}

#[test]
fn private_static_class_expression_uses_its_hash_prefixed_assigned_name() {
    let output = transform_and_print_at_target(
        concat!(
            "class B {\n",
            "    static #anonymous = class { static value = 1; };\n",
            "    static #declared = class Named { static value = 2; };\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("__setFunctionName(_b, \"#anonymous\")"),
        "the private property name must survive named evaluation:\n{output}",
    );
    assert!(
        !output.contains("\"#declared\""),
        "a declared class name must not be replaced by the private property name:\n{output}",
    );
}

#[test]
fn computed_class_field_named_evaluation_reuses_the_class_key_cache() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const key: any;\n",
            "class Host {\n",
            "    [key] = class { static value = 1; };\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let setup = output
        .lines()
        .find_map(|line| line.trim().strip_suffix(" = key;"))
        .expect("computed class field owns one class-definition key cache");
    assert!(output.contains("var __propKey ="), "{output}");
    assert!(
        output.contains(&format!("Object.defineProperty(this, {setup}, {{")),
        "useDefineForClassFields owns a define-property write:\n{output}",
    );
    assert!(
        output.contains(&format!(", {setup})")),
        "named evaluation must read the same runtime key cache:\n{output}",
    );
    assert!(
        !output.contains("__setFunctionName(_a, \"key\")"),
        "{output}"
    );
    assert_eq!(output.matches(" = key;").count(), 1, "{output}");
}

#[test]
fn computed_object_property_named_evaluation_uses_prop_key_once() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const key: any;\n",
            "const value = { [key]: class { static field = 1; } };\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(output.contains("[_a = __propKey(key)]"), "{output}");
    assert!(output.contains("__setFunctionName(_b, _a)"), "{output}");
    assert_eq!(output.matches("__propKey(key)").count(), 1, "{output}");
    assert!(
        !output.contains("__setFunctionName(_b, \"key\")"),
        "{output}"
    );
}

#[test]
fn member_assignment_is_not_an_identifier_named_evaluation_source() {
    let output = transform_and_print_at_target(
        concat!(
            "declare const receiver: any;\n",
            "receiver.value = class { static field = 1; };\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(!output.contains("__setFunctionName"), "{output}");
}

#[test]
fn duplicate_private_fields_preserve_distinct_recovery_storage() {
    let output = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    #value = 1;\n",
            "    static #value = true; // duplicate\n",
            "                          // shared lexical scope\n",
            "                          // retained boundary\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(output.contains("var _a, _A_value, _A_value_1;"), "{output}");
    assert!(
        output.contains("_A_value_1 = { value: 1 };")
            && output.contains("_A_value_1 = { value: true };")
            && output.contains("_A_value = new WeakMap()"),
        "each declaration must retain its allocated recovery storage:\n{output}",
    );
    assert!(
        output.contains("#value = 1;") && output.contains("static #value = true;"),
        "declarations resolving to the invalid duplicate entry must remain for recovery:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "_A_value_1 = { value: 1 };\n",
            "        // shared lexical scope\n",
            "        // retained boundary\n",
            "    }\n",
            "    #value = 1;",
        )),
        "continuation comments at the class-member boundary must move with the synthetic constructor:\n{output}",
    );
    assert_eq!(
        output.matches("// shared lexical scope").count(),
        1,
        "{output}"
    );
    assert_eq!(
        output.matches("// retained boundary").count(),
        1,
        "{output}"
    );
}

#[test]
fn duplicate_private_static_then_instance_field_uses_the_effective_weak_map_slot() {
    let output = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    static #value = \"static\";\n",
            "    #value = \"instance\";\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("_A_value_1.set(A, \"static\");")
            && output.contains("_A_value_1.set(this, \"instance\");"),
        "execution order follows each member while both declarations resolve through the final instance-field slot:\n{output}",
    );
    assert!(
        !output.contains("_A_value_1 = { value:"),
        "the syntactically static declaration must not change the effective instance-field representation:\n{output}",
    );
    assert!(
        output.contains("static #value = \"static\";") && output.contains("#value = \"instance\";"),
        "invalid duplicate declarations remain in the recovery output:\n{output}",
    );
}

#[test]
fn duplicate_private_kinds_use_the_last_effective_slot_without_losing_allocations() {
    let output = transform_and_print_at_target(
        concat!(
            "class FieldThenMethod {\n",
            "    #value = 1;\n",
            "    #value() {}\n",
            "}\n",
            "class MethodThenField {\n",
            "    #value() {}\n",
            "    #value = 2;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains(concat!(
            "class FieldThenMethod {\n",
            "    constructor() {\n",
            "        _FieldThenMethod_instances.add(this);\n",
            "    }\n",
            "    #value = 1;\n",
            "    #value() { }\n",
            "}",
        )),
        "a final invalid method slot retains every duplicate member:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "_FieldThenMethod_value = new WeakMap(), ",
            "_FieldThenMethod_instances = new WeakSet();",
        )),
        "the overwritten field allocation and the method brand must survive:\n{output}",
    );
    assert!(
        !output.contains("_FieldThenMethod_value_1 = function"),
        "an invalid method declaration must not be externalized:\n{output}",
    );

    assert!(
        output.contains(concat!(
            "class MethodThenField {\n",
            "    constructor() {\n",
            "        _MethodThenField_instances.add(this);\n",
            "        _MethodThenField_value_1.set(this, 2);\n",
            "    }\n",
            "    #value() { }\n",
            "    #value = 2;\n",
            "}",
        )),
        "a final invalid field slot owns recovery initialization while members remain:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "_MethodThenField_value_1 = new WeakMap(), ",
            "_MethodThenField_instances = new WeakSet();",
        )),
        "declaration-order allocation must retain the second field storage:\n{output}",
    );
}

#[test]
fn private_accessor_pairing_follows_the_effective_slot_and_source_order() {
    let output = transform_and_print_at_target(
        concat!(
            "class SetterThenGetter {\n",
            "    set #value(value: number) {}\n",
            "    get #value() { return 1; }\n",
            "}\n",
            "class InterruptedPair {\n",
            "    get #value() { return 1; }\n",
            "    #value = 2;\n",
            "    set #value(value: number) {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let setter = output
        .find("_SetterThenGetter_value_set")
        .expect("setter allocation");
    let getter = output
        .find("_SetterThenGetter_value_get")
        .expect("getter allocation");
    assert!(
        setter < getter,
        "complementary accessor bindings must be allocated in source order:\n{output}",
    );
    let legal_pair_start = output
        .find("class SetterThenGetter")
        .expect("legal accessor-pair class");
    let legal_pair_end = output[legal_pair_start..]
        .find("class InterruptedPair")
        .map(|offset| legal_pair_start + offset)
        .expect("class following legal accessor pair");
    let legal_pair = &output[legal_pair_start..legal_pair_end];
    assert!(
        !legal_pair.contains("#value"),
        "the legal accessor pair must be externalized:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "_SetterThenGetter_value_set = function ",
            "_SetterThenGetter_value_set(value) { }, ",
            "_SetterThenGetter_value_get = function ",
            "_SetterThenGetter_value_get() { return 1; };",
        )),
        "accessor definitions must retain declaration order:\n{output}",
    );

    assert!(
        output.contains(concat!(
            "get #value() { return 1; }\n",
            "    #value = 2;\n",
            "    set #value(value) { }",
        )),
        "a field interrupts accessor pairing through the effective slot:\n{output}",
    );
    assert!(
        output.contains("_InterruptedPair_value = new WeakMap()")
            && output.contains("_InterruptedPair_instances = new WeakSet()"),
        "invalid duplicates must keep declaration-time storage and brand setup:\n{output}",
    );
}

#[test]
fn synthetic_constructor_does_not_reown_the_class_leading_comment() {
    let output = transform_and_print_at_target(
        "// class leader\nclass A { field = 1; }\n",
        ScriptTarget::ES2015,
    );

    assert_eq!(output.matches("// class leader").count(), 1, "{output}");
    assert!(
        output.starts_with("// class leader\nclass A {\n    constructor()"),
        "the generated constructor remains source-positioned without owning the class boundary:\n{output}",
    );
}

#[test]
fn es2015_class_bindings_created_in_parameters_move_defaults_to_the_body() {
    let output = transform_and_print_at_target(
        concat!(
            "function f(value = class { static field = later; }, later = 1) {\n",
            "    return value.field;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let signature = output
        .find("function f(value, later) {")
        .unwrap_or_else(|| panic!("parameter defaults were not lowered:\n{output}"));
    let declaration = output[signature..]
        .find("var _a;")
        .map(|offset| signature + offset)
        .unwrap_or_else(|| panic!("class binding was not body-owned:\n{output}"));
    let value_default = output[signature..]
        .find("if (value === void 0)")
        .map(|offset| signature + offset)
        .unwrap_or_else(|| panic!("class default prologue is missing:\n{output}"));
    let later_default = output[signature..]
        .find("if (later === void 0) { later = 1; }")
        .map(|offset| signature + offset)
        .unwrap_or_else(|| panic!("following default prologue is missing:\n{output}"));
    assert!(declaration < value_default && value_default < later_default);
    assert!(output.contains("__setFunctionName(_a, \"value\")"));
    assert!(!output.contains("function f(value ="));
}

#[test]
fn esnext_accesses_and_calls_distinguish_optional_chain_continuations_from_chain_breaks() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "const key = \"x\";\n",
                "const propertyAfterPropertyContinuation = this?.x.y;\n",
                "const elementAfterPropertyContinuation = this?.x[key];\n",
                "const propertyAfterElementContinuation = this?.[key].y;\n",
                "const elementAfterElementContinuation = this?.[key][key];\n",
                "const propertyContinuation = this?.x();\n",
                "const elementContinuation = this?.[key]();\n",
                "const optionalCall = this.x?.();\n",
                "const explicitPropertyAccessBreak = (this?.x).y;\n",
                "const explicitElementAccessBreak = (this?.x)[key];\n",
                "const explicitPropertyBreak = (this?.x)();\n",
                "const explicitElementBreak = (this?.[key])();\n",
                "const generatedPropertyAccessBreak = (this?.x as any).y;\n",
                "const generatedElementAccessBreak = (this?.[key] as any)[key];\n",
                "const generatedOrdinaryCall = (this?.x as any)();\n",
            ),
            ScriptTarget::ES_NEXT,
        ),
        concat!(
            "const key = \"x\";\n",
            "const propertyAfterPropertyContinuation = this?.x.y;\n",
            "const elementAfterPropertyContinuation = this?.x[key];\n",
            "const propertyAfterElementContinuation = this?.[key].y;\n",
            "const elementAfterElementContinuation = this?.[key][key];\n",
            "const propertyContinuation = this?.x();\n",
            "const elementContinuation = this?.[key]();\n",
            "const optionalCall = this.x?.();\n",
            "const explicitPropertyAccessBreak = (this?.x).y;\n",
            "const explicitElementAccessBreak = (this?.x)[key];\n",
            "const explicitPropertyBreak = (this?.x)();\n",
            "const explicitElementBreak = (this?.[key])();\n",
            "const generatedPropertyAccessBreak = (this?.x).y;\n",
            "const generatedElementAccessBreak = (this?.[key])[key];\n",
            "const generatedOrdinaryCall = (this?.x)();\n",
        ),
    );
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

fn transform_and_print_legacy_decorator_metadata(source_text: &str, module: i32) -> String {
    let parsed = parse_source_file("legacy-metadata.ts", source_text, Default::default(), None);
    transform_parsed_legacy_decorator_metadata(&parsed, module, &SystemContractResolver)
}

fn transform_parsed_legacy_decorator_metadata(
    parsed: &tsc_syntax::SourceFile,
    module: i32,
    resolver: &dyn EmitResolver,
) -> String {
    let mut arena = TransformArena::new();
    let source_id = SourceFileId::from_raw(0);
    let source = arena.add_source(parsed, Some(source_id));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(module),
        experimental_decorators: true,
        emit_decorator_metadata: Some(true),
        use_define_for_class_fields: Some(true),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let host = TransformContractHost {
        options: &options,
        syntax: parsed,
        source_ids: [source_id],
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers_for_source(&options, resolver, &host, source_id).unwrap(),
        false,
    )
    .expect("legacy decorator metadata transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print legacy decorator metadata transform")
        .text()
        .to_owned()
}

fn transform_parsed_class_declaration_correlation(
    parsed: &tsc_syntax::SourceFile,
    experimental_decorators: bool,
    resolver: &dyn EmitResolver,
) -> String {
    transform_parsed_class_declaration_correlation_with_mode(
        parsed,
        experimental_decorators,
        resolver,
        false,
    )
}

fn transform_parsed_class_declaration_correlation_with_mode(
    parsed: &tsc_syntax::SourceFile,
    experimental_decorators: bool,
    resolver: &dyn EmitResolver,
    use_define_for_class_fields: bool,
) -> String {
    transform_parsed_class_declaration_correlation_at_target_with_mode(
        parsed,
        experimental_decorators,
        resolver,
        ScriptTarget::ES2015,
        use_define_for_class_fields,
    )
}

fn transform_parsed_class_declaration_correlation_at_target_with_mode(
    parsed: &tsc_syntax::SourceFile,
    experimental_decorators: bool,
    resolver: &dyn EmitResolver,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
) -> String {
    let mut arena = TransformArena::new();
    let source = arena.add_source(parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(target.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        experimental_decorators,
        emit_decorator_metadata: Some(false),
        use_define_for_class_fields: Some(use_define_for_class_fields),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, resolver).unwrap(),
        false,
    )
    .expect("class declaration correlation transform");
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(target))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print class declaration correlation transform")
        .text()
        .to_owned()
}

struct DecoratedClassReferenceResolver {
    declaration: NodeId,
    constructor_references: BTreeSet<NodeId>,
}

struct NodeCheckFlagAtNodeResolver {
    node: NodeId,
    flag: u32,
}

impl NodeCheckFlagAtNodeResolver {
    fn first_kind(source: &tsc_syntax::SourceFile, kind: SyntaxKind, flag: NodeCheckFlags) -> Self {
        let mut pending = vec![source.root];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            if record.kind == kind {
                return Self {
                    node,
                    flag: flag.bits() as u32,
                };
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        panic!("missing {kind:?} resolver node")
    }
}

impl EmitResolver for NodeCheckFlagAtNodeResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(node.node() == self.node && flag == self.flag)
    }
}

impl DecoratedClassReferenceResolver {
    fn new(source: &tsc_syntax::SourceFile, class_name: &str) -> Self {
        let mut declaration = None;
        let mut pending = vec![source.root];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            if let NodeData::ClassDeclaration(data) = &record.data {
                if data.name.is_some_and(|name| {
                    matches!(
                        &source.arena.node(name).data,
                        NodeData::Identifier(data) if data.text == class_name
                    )
                }) {
                    declaration = Some(node);
                    break;
                }
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        let declaration = declaration.expect("decorated class declaration");
        let declaration_name = match &source.arena.node(declaration).data {
            NodeData::ClassDeclaration(data) => data.name,
            _ => None,
        };
        let mut constructor_references = BTreeSet::new();
        let mut pending = vec![declaration];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            if Some(node) != declaration_name
                && matches!(
                    &record.data,
                    NodeData::Identifier(data) if data.text == class_name
                )
            {
                constructor_references.insert(node);
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        Self {
            declaration,
            constructor_references,
        }
    }
}

impl EmitResolver for DecoratedClassReferenceResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .constructor_references
            .contains(&node.node())
            .then(|| EmitResolverNode::new(node.source(), self.declaration)))
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(
            flag == NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32
                && node.node() == self.declaration
                || flag == NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32
                    && self.constructor_references.contains(&node.node()),
        )
    }
}

#[test]
fn legacy_decorated_static_initializers_use_invalid_lexical_receivers() {
    let parsed = parse_source_file(
        "decorated-static-this.ts",
        concat!(
            "declare const dec: any;\n",
            "@dec class C { static a = 1; static b = this.a + 1; }\n",
            "@dec class D extends C {\n",
            "    static c = 2;\n",
            "    static d = this.c + 1;\n",
            "    static e = super.a + this.c + 1;\n",
            "    static f = () => this.c + 1;\n",
            "    static ff = function () { return this.c + 1; };\n",
            "}\n",
        ),
        Default::default(),
        None,
    );

    for use_define in [false, true] {
        let output = transform_parsed_class_declaration_correlation_with_mode(
            &parsed,
            true,
            &NoConstantValueResolver,
            use_define,
        );
        assert!(output.contains("let C = class C"), "{output}");
        assert!(output.contains("let D = class D extends C"), "{output}");
        assert!(!output.contains("let C = _"), "{output}");
        assert!(!output.contains("let D = _"), "{output}");
        assert!(output.contains("(void 0).a + 1"), "{output}");
        assert!(output.contains("(void 0).c + 1"), "{output}");
        assert!(output.contains("(void 0).a + (void 0).c + 1"), "{output}",);
        assert!(output.contains("() => (void 0).c + 1"), "{output}");
        assert!(
            output.contains("function () { return this.c + 1; }"),
            "{output}",
        );
    }
}

#[test]
fn class_fields_keep_legacy_decorated_declarations_statement_expandable() {
    let parsed = parse_source_file(
        "decorated-class-fields.ts",
        concat!(
            "declare const dec: any;\n",
            "class A {}\n",
            "@dec class B extends A {\n",
            "    static x = 1;\n",
            "    static y = B.x;\n",
            "    method() { return B.x; }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = DecoratedClassReferenceResolver::new(&parsed, "B");
    let output = transform_parsed_class_declaration_correlation(&parsed, true, &resolver);

    assert!(output.contains("var B_1;"), "{output}");
    assert!(
        output.contains("let B = B_1 = class B extends A"),
        "{output}",
    );
    assert!(output.contains("return B_1.x;"), "{output}");
    let class = output.find("let B = B_1 = class").expect("class");
    let first_field = output.find("B.x = 1;").expect("first static field");
    let second_field = output.find("B.y = B_1.x;").expect("second static field");
    let decorate = output
        .find("B = B_1 = __decorate")
        .expect("decoration assignment");
    assert!(class < first_field && first_field < second_field && second_field < decorate);
    assert!(!output.contains("let B = ("), "{output}");
    assert!(!output.contains("(() =>"), "{output}");
}

#[test]
fn legacy_decorated_class_alias_storage_is_a_custom_prologue() {
    let parsed = parse_source_file(
        "decorated-class-alias-prologue.ts",
        "declare const dec: any;\n@dec class C { static value = C; }\n",
        Default::default(),
        None,
    );
    let resolver = DecoratedClassReferenceResolver::new(&parsed, "C");
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        experimental_decorators: true,
        emit_decorator_metadata: Some(false),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("legacy class alias transform");
    let root = result.arena().root(source).unwrap();
    let NodeData::SourceFile(data) = &result.arena().node(root).unwrap().data else {
        unreachable!();
    };
    let statements = result
        .arena()
        .node_array_ref(source, data.statements.unwrap())
        .unwrap();
    let statements = result.arena().node_array(statements).unwrap();
    let custom_prologues = statements
        .nodes
        .iter()
        .filter(|statement| {
            result
                .arena()
                .node_ref(source, **statement)
                .and_then(|statement| result.arena().metadata(statement))
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
        })
        .collect::<Vec<_>>();
    assert_eq!(custom_prologues.len(), 1);
    assert_eq!(
        result
            .arena()
            .node(
                result
                    .arena()
                    .node_ref(source, *custom_prologues[0])
                    .unwrap()
            )
            .unwrap()
            .kind,
        SyntaxKind::VariableStatement,
    );
}

#[test]
fn legacy_decorated_class_exports_follow_lowered_static_fields() {
    for (label, declaration, publication) in [
        ("local", "class C", None),
        ("named export", "export class C", Some("export { C };")),
        (
            "default export",
            "export default class C",
            Some("export default C;"),
        ),
    ] {
        let parsed = parse_source_file(
            "decorated-export-class-fields.ts",
            &format!(
                "declare const dec: any;\n@dec\n{declaration} {{ static x() {{ return C.y; }} static y = 1; }}\n"
            ),
            Default::default(),
            None,
        );
        let resolver = DecoratedClassReferenceResolver::new(&parsed, "C");
        let output = transform_parsed_class_declaration_correlation(&parsed, true, &resolver);
        assert!(
            output.contains("let C = C_1 = class C"),
            "{label}:\n{output}",
        );
        let field = output.find("C.y = 1;").expect("static field");
        let decorate = output
            .find("C = C_1 = __decorate")
            .expect("decoration assignment");
        assert!(field < decorate, "{label}:\n{output}");
        if let Some(publication) = publication {
            let publication = output.find(publication).expect("class publication");
            assert!(decorate < publication, "{label}:\n{output}");
        }
    }
}

#[test]
fn es_module_class_publications_follow_declaration_owned_operations() {
    let parsed = parse_source_file(
        "external-module-classes.ts",
        concat!(
            "export class C { static s = 0; p = 1; method() {} }\n",
            "export { C as C2 };\n",
            "declare const dec: any;\n",
            "@dec export class D { static s = 0; p = 1; method() {} }\n",
            "export { D as D2 };\n",
            "class E {}\n",
            "export { E };\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, true, &SystemContractResolver);

    let c = output.find("export class C {").expect("exported class C");
    let c_static = output.find("C.s = 0;").expect("C static field");
    let c_alias = output.find("export { C as C2 };").expect("C alias export");
    assert!(c < c_static && c_static < c_alias, "{output}");

    let d = output.find("let D = class D {").expect("decorated class D");
    let d_static = output.find("D.s = 0;").expect("D static field");
    let decorate = output.find("D = __decorate([").expect("D decoration");
    let d_export = output.find("export { D };").expect("D publication");
    let d_alias = output.find("export { D as D2 };").expect("D alias export");
    assert!(
        d < d_static && d_static < decorate && decorate < d_export && d_export < d_alias,
        "{output}",
    );
    assert!(!output.contains("let D = ("), "{output}");
    assert!(!output.contains("(() =>"), "{output}");

    let e = output.find("class E {").expect("class E");
    let e_export = output.find("export { E };").expect("E publication");
    assert!(e < e_export, "{output}");
}

#[test]
fn es_module_default_classes_publish_after_class_field_and_decorator_operations() {
    let plain = parse_source_file(
        "plain-default-class.ts",
        "export default class C { static s = 0; p = 1; method() {} }\n",
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&plain, false, &NoConstantValueResolver);
    let class = output.find("class C {").expect("default class local");
    let static_field = output.find("C.s = 0;").expect("default class static field");
    let publication = output
        .find("export default C;")
        .expect("default publication");
    assert!(
        class < static_field && static_field < publication,
        "{output}"
    );
    assert!(!output.contains("export default class C"), "{output}");

    let decorated = parse_source_file(
        "decorated-default-class.ts",
        concat!(
            "declare const dec: any;\n",
            "@dec export default class D { static s = 0; p = 1; method() {} }\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&decorated, true, &NoConstantValueResolver);
    let class = output
        .find("let D = class D {")
        .expect("decorated default local");
    let static_field = output
        .find("D.s = 0;")
        .expect("decorated default static field");
    let decorate = output.find("D = __decorate([").expect("default decoration");
    let publication = output
        .find("export default D;")
        .expect("default publication");
    assert!(
        class < static_field && static_field < decorate && decorate < publication,
        "{output}",
    );
    assert!(!output.contains("export default class D"), "{output}");
    assert!(!output.contains("let D = ("), "{output}");
}

#[test]
fn anonymous_default_class_fields_share_declaration_and_runtime_names() {
    let plain = parse_source_file(
        "anonymous-default.ts",
        concat!(
            "export default class {\n",
            "    static z: string = \"Foo\";\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&plain, false, &NoConstantValueResolver);
    assert_eq!(
        output,
        concat!(
            "class default_1 {\n",
            "}\n",
            "default_1.z = \"Foo\";\n",
            "export default default_1;\n",
        ),
    );

    let decorated = parse_source_file(
        "decorated-anonymous-default.ts",
        concat!(
            "declare const dec: any;\n",
            "@dec\n",
            "export default class { static y = 1; }\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&decorated, true, &NoConstantValueResolver);
    assert!(output.contains("var _a;"), "{output}");
    assert!(output.contains("let default_1 = _a = class"), "{output}");
    assert!(
        output.contains("__setFunctionName(_a, \"default\");"),
        "{output}",
    );
    assert!(output.contains("default_1.y = 1;"), "{output}");
    assert!(output.contains("export default default_1;"), "{output}");
    assert!(!output.contains("\"default_1\""), "{output}");
}

#[test]
fn empty_legacy_decorated_anonymous_default_does_not_request_named_evaluation() {
    let parsed = parse_source_file(
        "empty-decorated-anonymous-default.ts",
        concat!(
            "declare const dec: any;\n",
            "@dec\n",
            "export default class {}\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

    assert!(output.contains("let default_1 = class"), "{output}");
    assert!(!output.contains("__setFunctionName"), "{output}");
    assert!(!output.contains("var _a;"), "{output}");
    assert!(output.contains("export default default_1;"), "{output}");
}

#[test]
fn legacy_decorated_anonymous_default_reserves_definition_before_computed_key() {
    let parsed = parse_source_file(
        "computed-decorated-anonymous-default.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function key(): string;\n",
            "@dec\n",
            "export default class { static [key()] = 1; }\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

    assert!(output.contains("var _a, _b;"), "{output}");
    let definition = output
        .find("let default_1 = _a = class")
        .expect("class-definition binding");
    let key = output.find("_b = key();").expect("computed-key binding");
    let named = output
        .find("__setFunctionName(_a, \"default\");")
        .unwrap_or_else(|| panic!("missing named evaluation:\n{output}"));
    let field = output
        .find("default_1[_b] = 1;")
        .expect("public static-field receiver");
    assert!(definition < key && key < named && named < field, "{output}");
    assert!(!output.contains("\"default_1\""), "{output}");
}

#[test]
fn legacy_decorated_named_evaluation_does_not_allocate_for_control_classes() {
    for (label, declaration) in [
        ("empty", "export default class {}"),
        (
            "static method",
            "export default class { static method() {} }",
        ),
        (
            "uninitialized static field",
            "export default class { static value: number; }",
        ),
        (
            "named class",
            "export default class Named { static value = 1; }",
        ),
    ] {
        let parsed = parse_source_file(
            "decorated-named-evaluation-control.ts",
            &format!("declare const dec: any;\n@dec\n{declaration}\n"),
            Default::default(),
            None,
        );
        let output =
            transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

        assert!(!output.contains("var _a;"), "{label}:\n{output}");
        assert!(!output.contains("__setFunctionName"), "{label}:\n{output}");
    }
}

#[test]
fn legacy_decorated_definition_binding_serves_static_this_but_not_super() {
    let parsed = parse_source_file(
        "decorated-anonymous-default-static-lexical.ts",
        concat!(
            "declare const dec: any;\n",
            "declare class B { static x: number; }\n",
            "@dec\n",
            "export default class extends B {\n",
            "    static y = super.x;\n",
            "    static z = this;\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

    assert!(output.contains("var _a;"), "{output}");
    let definition = output
        .find("let default_1 = _a = class extends B")
        .expect("class-definition binding");
    let named = output
        .find("__setFunctionName(_a, \"default\");")
        .unwrap_or_else(|| panic!("missing named evaluation:\n{output}"));
    let super_field = output
        .find("default_1.y = (void 0).x;")
        .expect("legacy static-super recovery");
    let this_field = output
        .find("default_1.z = _a;")
        .expect("class-definition static-this receiver");
    let decorate = output
        .find("default_1 = __decorate([")
        .expect("decoration assignment");
    assert!(
        definition < named
            && named < super_field
            && super_field < this_field
            && this_field < decorate,
        "{output}"
    );
    assert!(!output.contains("default_1.z = default_1;"), "{output}");
}

#[test]
fn semantic_class_expression_identity_in_a_loop_is_iteration_scoped() {
    let parsed = parse_source_file(
        "loop-semantic-class-identity.ts",
        concat!(
            "declare const values: readonly unknown[];\n",
            "declare function consume(value: unknown): void;\n",
            "for (const value of values) {\n",
            "    consume(class { static #identity = 1; });\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NodeCheckFlagAtNodeResolver::first_kind(
        &parsed,
        SyntaxKind::ClassExpression,
        NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP,
    );
    let output = transform_parsed_class_declaration_correlation(&parsed, false, &resolver);

    assert!(
        output.contains("for (const value of values) {\n    let _a;"),
        "the semantic constructor identity belongs to the current iteration:\n{output}",
    );
    assert!(output.contains("consume((_a = class"), "{output}");
    assert!(output.contains("_identity = { value: 1 }"), "{output}");
    assert!(!output.contains("var _a;"), "{output}");
}

#[test]
fn fallback_class_expression_result_in_a_loop_is_iteration_scoped() {
    let parsed = parse_source_file(
        "loop-fallback-class-result.ts",
        concat!(
            "declare const values: readonly unknown[];\n",
            "declare function consume(value: unknown): void;\n",
            "for (const value of values) {\n",
            "    consume(class Named { static value = 1; });\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NodeCheckFlagAtNodeResolver::first_kind(
        &parsed,
        SyntaxKind::ClassExpression,
        NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP,
    );
    let output = transform_parsed_class_declaration_correlation(&parsed, false, &resolver);

    assert!(
        output.contains("for (const value of values) {\n    let _a;"),
        "the late sequencing result belongs to the current iteration:\n{output}",
    );
    assert!(output.contains("consume((_a = class Named"), "{output}");
    assert!(output.contains("_a.value = 1"), "{output}");
    assert!(!output.contains("var _a;"), "{output}");
}

#[test]
fn class_binding_allocations_follow_tsc_lexical_environment_phases() {
    let parsed = parse_source_file(
        "class-binding-allocation-phases.ts",
        concat!(
            "declare class B { static x: number; }\n",
            "const X = class extends B {\n",
            "    #method() {}\n",
            "    static value = super.x;\n",
            "};\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, false, &NoConstantValueResolver);

    assert!(
        output.contains("var _X_instances, _a, _b, _X_method;"),
        "instance brand, constructor identity, heritage capture, and private method must retain tsc allocation order:\n{output}",
    );
    assert!(
        output.contains("const X = (_a = class extends (_b = B)"),
        "{output}",
    );
    assert!(output.contains("Reflect.get(_b, \"x\", _a)"), "{output}",);
}

#[test]
fn private_auto_accessor_provenance_does_not_invent_a_constructor_fact() {
    let parsed = parse_source_file(
        "private-auto-accessor-class-facts.ts",
        "class C { accessor #x = C; }\n",
        Default::default(),
        None,
    );
    let resolver = NodeCheckFlagAtNodeResolver::first_kind(
        &parsed,
        SyntaxKind::PropertyDeclaration,
        NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE,
    );
    let output = transform_parsed_class_declaration_correlation(&parsed, false, &resolver);

    assert!(
        output.contains("var _C_instances, _C_x_get, _C_x_set, _C_x_accessor_storage;"),
        "source auto-accessor facts must not be re-read from its generated redirectors:\n{output}",
    );
    assert!(!output.contains("var _a"), "{output}");
    assert!(
        output.contains("_C_x_accessor_storage.set(this, C);"),
        "{output}",
    );
}

#[test]
fn legacy_invalid_super_calls_preserve_only_an_available_definition_receiver() {
    let with_identity = parse_source_file(
        "decorated-default-super-call.ts",
        concat!(
            "declare const dec: any;\n",
            "declare class B { static f(): any; }\n",
            "@dec export default class extends B {\n",
            "    static call = super.f();\n",
            "    static tag = super.f`x`;\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let with_identity = transform_parsed_class_declaration_correlation(
        &with_identity,
        true,
        &NoConstantValueResolver,
    );
    assert!(
        with_identity.contains("default_1.call = (void 0).f.call(_a);"),
        "{with_identity}",
    );
    assert!(
        with_identity.contains("default_1.tag = (void 0).f.bind(_a) `x`;"),
        "{with_identity}",
    );

    let without_identity = parse_source_file(
        "decorated-named-super-call.ts",
        concat!(
            "declare const dec: any;\n",
            "declare class B { static f(): any; }\n",
            "@dec class C extends B {\n",
            "    static call = super.f();\n",
            "    static tag = super.f`x`;\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let without_identity = transform_parsed_class_declaration_correlation(
        &without_identity,
        true,
        &NoConstantValueResolver,
    );
    assert!(
        without_identity.contains("C.call = (void 0).f();"),
        "{without_identity}",
    );
    assert!(
        without_identity.contains("C.tag = (void 0).f `x`;"),
        "{without_identity}",
    );
    assert!(
        !without_identity.contains(".call(_a)"),
        "{without_identity}"
    );
    assert!(
        !without_identity.contains(".bind(_a)"),
        "{without_identity}"
    );
}

#[test]
fn decorated_static_private_and_auto_accessor_facts_supply_static_this_identity() {
    for (label, member, storage_fragment) in [
        (
            "private field",
            "static #value = 1;",
            "_C_value = { value: 1 };",
        ),
        (
            "auto accessor",
            "static accessor value = 1;",
            "_C_value_accessor_storage = { value: 1 };",
        ),
    ] {
        let parsed = parse_source_file(
            "decorated-static-private-this.ts",
            &format!(
                "declare const dec: any;\n@dec class C {{ {member} static identity = this; }}\n"
            ),
            Default::default(),
            None,
        );
        let output =
            transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

        assert!(output.contains("var _a,"), "{label}:\n{output}");
        assert!(
            output.contains("let C = _a = class C"),
            "{label}:\n{output}"
        );
        assert!(output.contains(storage_fragment), "{label}:\n{output}");
        assert!(output.contains("C.identity = _a;"), "{label}:\n{output}");
        assert!(
            !output.contains("C.identity = (void 0);"),
            "{label}:\n{output}"
        );
    }
}

#[test]
fn relocated_class_field_operation_owns_initializer_trailing_comment_once() {
    let output = transform_and_print_at_target(
        concat!(
            "class A { get p() { return 'base'; } }\n",
            "class B extends A { p = 'value' // error\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );
    assert_eq!(output.matches("// error").count(), 1, "{output}");
    assert!(
        output.contains("value: 'value'\n        }); // error"),
        "{output}"
    );
}

struct MetadataSerializationResolver {
    type_name: NodeId,
    class_scope: NodeId,
    kind: EmitTypeReferenceSerializationKind,
    queries: Cell<usize>,
}

impl MetadataSerializationResolver {
    fn new(source: &tsc_syntax::SourceFile, kind: EmitTypeReferenceSerializationKind) -> Self {
        let mut type_name = None;
        let mut class_scope = None;
        let mut pending = vec![source.root];
        while let Some(node) = pending.pop() {
            let record = source.arena.node(node);
            if matches!(&record.data, NodeData::ClassDeclaration(_)) {
                class_scope.get_or_insert(node);
            }
            if let NodeData::TypeReference(data) = &record.data {
                if let Some(name) = data.type_name {
                    type_name.get_or_insert(name);
                }
            }
            for_each_child(&source.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        Self {
            type_name: type_name.expect("metadata type reference"),
            class_scope: class_scope.expect("metadata class scope"),
            kind,
            queries: Cell::new(0),
        }
    }
}

impl EmitResolver for MetadataSerializationResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration_at_location(
        &self,
        _node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_type_reference_serialization_kind(
        &self,
        node: EmitResolverNode,
        location: EmitResolverNode,
    ) -> Result<EmitTypeReferenceSerializationKind, EmitResolverError> {
        assert_eq!(node.node(), self.type_name);
        assert_eq!(location.node(), self.class_scope);
        self.queries.set(self.queries.get() + 1);
        Ok(self.kind)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }

    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }
}

#[test]
fn legacy_method_overload_signature_is_not_a_decoration_owner() {
    let output = transform_and_print_legacy_decorator_metadata(
        concat!(
            "declare function dec(target: any, key: string, descriptor: any): any;\n",
            "class C {\n",
            "    @dec\n",
            "    method(): void;\n",
            "    method() {}\n",
            "}\n",
        ),
        ModuleKind::PRESERVE.bits(),
    );
    assert!(!output.contains("__decorate"), "{output}");
    assert!(!output.contains("__metadata"), "{output}");
    assert!(output.contains("method() { }"), "{output}");
}

#[test]
fn legacy_parameter_decorator_index_excludes_the_erased_this_parameter() {
    let output = transform_and_print_legacy_decorator_metadata(
        concat!(
            "declare function dec(target: any, key: string, index: number): void;\n",
            "class C { method(this: C, @dec value: number) {} }\n",
        ),
        ModuleKind::PRESERVE.bits(),
    );

    assert!(output.contains("__param(0, dec)"), "{output}");
    assert!(!output.contains("__param(1, dec)"), "{output}");
}

#[test]
fn legacy_member_decorator_expressions_use_the_typescript_erased_tree() {
    let parsed = parse_source_file(
        "legacy-decorator-expression.ts",
        concat!(
            "declare function y(...args: any[]): any;\n",
            "type T = number;\n",
            "@y(1 as T, () => C)\n",
            "class C<T> {\n",
            "    @y(null as T)\n",
            "    method(@y x: T, y: T) {}\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let output =
        transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

    assert!(output.contains("y(null)"), "{output}");
    assert!(output.contains("__param(0, y)"), "{output}");
    assert!(output.contains("y(1, () => C)"), "{output}");
    assert!(!output.contains("null as T"), "{output}");
    assert!(!output.contains("1 as T"), "{output}");
}

#[test]
fn legacy_metadata_unwraps_jsdoc_nullable_and_non_nullable_types() {
    let output = transform_and_print_legacy_decorator_metadata(
        concat!(
            "declare const decorator: any;\n",
            "class X {\n",
            "    @decorator() a?: string?;\n",
            "    @decorator() b?: string!;\n",
            "    @decorator() c?: *;\n",
            "}\n",
        ),
        ModuleKind::PRESERVE.bits(),
    );
    assert_eq!(
        output
            .matches("__metadata(\"design:type\", String)")
            .count(),
        2,
        "{output}",
    );
    assert_eq!(
        output
            .matches("__metadata(\"design:type\", Object)")
            .count(),
        1,
        "{output}",
    );
}

#[test]
fn legacy_metadata_unknown_conditional_branches_do_not_hoist_fallback_temps() {
    let output = transform_and_print_legacy_decorator_metadata(
        concat!(
            "declare function d(): PropertyDecorator;\n",
            "abstract class BaseEntity<T> {\n",
            "    @d()\n",
            "    public attributes: T extends { attributes: infer A } ? (A) : undefined;\n",
            "}\n",
        ),
        ModuleKind::PRESERVE.bits(),
    );

    assert!(!output.contains("var _a;"), "{output}");
    assert!(
        output.contains("__metadata(\"design:type\", Object)"),
        "{output}",
    );
}

#[test]
fn legacy_import_metadata_uses_the_class_name_scope_for_value_resolution() {
    let cases = [
        (
            "default value import",
            "import Service from \"./service\";\n",
            EmitTypeReferenceSerializationKind::TypeWithConstructSignatureAndValue,
            "__metadata(\"design:paramtypes\", [Service])",
        ),
        (
            "type-only import",
            "import type { Service } from \"./service\";\n",
            EmitTypeReferenceSerializationKind::TypeWithCallSignature,
            "__metadata(\"design:paramtypes\", [Function])",
        ),
    ];
    for (label, import, kind, expected) in cases {
        let parsed = parse_source_file(
            "legacy-import-metadata.ts",
            &format!(
                "{import}declare const decorator: any;\n@decorator\nclass MyComponent {{ constructor(public Service: Service) {{}} }}\n"
            ),
            Default::default(),
            None,
        );
        let resolver = MetadataSerializationResolver::new(&parsed, kind);
        let output = transform_parsed_legacy_decorator_metadata(
            &parsed,
            ModuleKind::PRESERVE.bits(),
            &resolver,
        );
        assert!(output.contains(expected), "{label}:\n{output}");
        assert_eq!(resolver.queries.get(), 1, "{label}:\n{output}");
    }
}

#[test]
fn legacy_class_parameter_metadata_requires_an_explicit_constructor_body() {
    let absent_cases = [
        (
            "implicit constructor",
            "declare const dec: any;\n@dec\nclass C {}\n",
        ),
        (
            "overload signature only",
            concat!(
                "declare const dec: any;\n",
                "@dec\n",
                "class C { constructor(value: number); }\n",
            ),
        ),
        (
            "ambient declaration",
            concat!(
                "declare const dec: any;\n",
                "@dec\n",
                "declare class C { constructor(value: number); }\n",
            ),
        ),
    ];
    for (label, source_text) in absent_cases {
        let text =
            transform_and_print_legacy_decorator_metadata(source_text, ModuleKind::PRESERVE.bits());
        assert!(!text.contains("design:paramtypes"), "{label}:\n{text}");
        assert!(!text.contains("var __metadata"), "{label}:\n{text}");
    }

    let explicit_zero = transform_and_print_legacy_decorator_metadata(
        "declare const dec: any;\n@dec\nclass C { constructor() {} }\n",
        ModuleKind::PRESERVE.bits(),
    );
    assert!(
        explicit_zero.contains("__metadata(\"design:paramtypes\", [])"),
        "{explicit_zero}",
    );
    assert!(explicit_zero.contains("var __metadata"), "{explicit_zero}");

    let implemented_overload = transform_and_print_legacy_decorator_metadata(
        concat!(
            "declare const dec: any;\n",
            "@dec\n",
            "class C {\n",
            "    constructor(value: number);\n",
            "    constructor(value: number) {}\n",
            "}\n",
        ),
        ModuleKind::PRESERVE.bits(),
    );
    assert!(
        implemented_overload.contains("__metadata(\"design:paramtypes\", [Number])"),
        "{implemented_overload}",
    );
    assert!(
        implemented_overload.contains("var __metadata"),
        "{implemented_overload}",
    );

    let member_only = transform_and_print_legacy_decorator_metadata(
        "declare const dec: any;\nclass C { @dec method() {} }\n",
        ModuleKind::PRESERVE.bits(),
    );
    assert!(member_only.contains("var __metadata"), "{member_only}");
    assert!(
        member_only.contains("__metadata(\"design:paramtypes\", [])"),
        "{member_only}",
    );
}

#[test]
fn exported_legacy_decorated_classes_without_constructors_skip_metadata_in_modules() {
    let source_text = "declare const dec: any;\n@dec\nexport class Testing123 {}\n";
    for (label, module, wrapper) in [
        (
            "CommonJS",
            ModuleKind::COMMON_JS.bits(),
            "Object.defineProperty(exports",
        ),
        ("System", ModuleKind::SYSTEM.bits(), "System.register"),
    ] {
        let text = transform_and_print_legacy_decorator_metadata(source_text, module);
        assert!(text.contains(wrapper), "{label}:\n{text}");
        assert!(text.contains("__decorate"), "{label}:\n{text}");
        assert!(!text.contains("design:paramtypes"), "{label}:\n{text}");
        assert!(!text.contains("var __metadata"), "{label}:\n{text}");
    }
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
fn standard_decorator_fallback_erases_invalid_declaration_modifiers_only() {
    let cases = [
        (
            "@dec\nenum E {}\n",
            concat!("var E;\n", "(function (E) {\n", "})(E || (E = {}));\n"),
        ),
        ("@dec\nfunction F() {}\n", "function F() { }\n"),
        ("@dec\nvar x: number;\n", "var x;\n"),
    ];

    for (source, expected) in cases {
        assert_eq!(
            transform_and_print_at_target(source, ScriptTarget::ES2015),
            expected,
            "invalid decorator recovery changed its declaration owner for {source:?}",
        );
    }
}

#[test]
fn standard_decorator_fallback_preserves_invalid_static_block_owner() {
    assert_eq!(
        transform_and_print_at_target(
            concat!(
                "class C {\n",
                "    @decorator\n",
                "    static {\n",
                "        // something\n",
                "    }\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "class C {\n",
            "}\n",
            "(() => {\n",
            "    // something\n",
            "})();\n",
        ),
    );
}

#[test]
fn standard_decorator_fallback_erases_parameter_marker_inside_decorated_class() {
    let text = transform_and_print_at_target(
        concat!(
            "declare var dec: any;\n",
            "class C {\n",
            "    @dec x: any;\n",
            "    constructor(@dec x: any) {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(text.contains("constructor(x)"), "{text}");
    assert!(text.contains("_x_decorators = [dec]"), "{text}");
    assert_eq!(text.matches("[dec]").count(), 1, "{text}");
    assert!(!text.contains("@dec"), "{text}");
}

#[test]
fn standard_decorator_metadata_definition_is_a_single_line_if_statement() {
    let text = transform_and_print_at_target(
        concat!(
            "declare var dec: any;\n",
            "export class C {\n",
            "    @dec x: any;\n",
            "    constructor(@dec x: any) {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains("if (_metadata) Object.defineProperty"),
        "the SingleLine emit flag belongs to the generated if statement:\n{text}",
    );
    assert!(!text.contains("if (_metadata)\n"), "{text}");
}

#[test]
fn standard_decorator_pending_initializers_flow_through_ordinary_instance_fields() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function before(): number;\n",
            "declare function middle(): number;\n",
            "declare function after(): number;\n",
            "class C {\n",
            "    first = before();\n",
            "    @dec method() {}\n",
            "    @dec decorated = middle();\n",
            "    last = after();\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let method_extra = text
        .find("value: (__runInitializers(this, _instanceExtraInitializers), before())")
        .expect("method extra initializers belong to the first ordinary instance field");
    let decorated_field = text
        .find("value: __runInitializers(this, _decorated_initializers, middle())")
        .expect("the decorated field keeps its value initializer");
    let field_extra = text
        .find("value: (__runInitializers(this, _decorated_extraInitializers), after())")
        .expect("decorated-field extra initializers belong to the next ordinary field");
    assert!(
        method_extra < decorated_field && decorated_field < field_extra,
        "pending initializer ownership changed:\n{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(this, _decorated_extraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_pending_method_initializer_owns_uninitialized_first_field() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "class C {\n",
            "    first: number;\n",
            "    @dec method() {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains("Object.defineProperty(this, \"first\", {"),
        "{text}"
    );
    assert!(
        text.contains("value: __runInitializers(this, _instanceExtraInitializers)"),
        "an uninitialized field still owns the pending method initializers:\n{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_pending_method_initializer_flows_into_es2015_parameter_property() {
    // tsc-port: transformTypeScript/transformClassMembers @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    // tsc-port: transformPropertyWorker @6.0.3
    // tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    // tsc-span: _tsc.js:97501-97575
    let parsed = parse_source_file(
        "parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function makeValue(): number;\n",
            "declare function body(value: number): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor(public value = makeValue()) { body(value); }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NoConstantValueResolver;

    for (target_label, target) in [
        ("es2015", ScriptTarget::ES2015),
        ("es2021", ScriptTarget::ES2021),
    ] {
        for (mode_label, use_define_for_class_fields, expected_assignment) in [
            (
                "define",
                true,
                "value: (__runInitializers(this, _instanceExtraInitializers), value)",
            ),
            (
                "assignment",
                false,
                "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
            ),
        ] {
            let label = format!("{target_label}-{mode_label}");
            let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
                &parsed,
                false,
                &resolver,
                target,
                use_define_for_class_fields,
            );

            let constructor = text
                .find("constructor(value = makeValue()) {")
                .unwrap_or_else(|| {
                    panic!("{label}: parameter initializer was not preserved:\n{text}")
                });
            let assignment = text.find(expected_assignment).unwrap_or_else(|| {
                panic!("{label}: parameter-property value lost its pending initializer:\n{text}")
            });
            let body = text
                .find("body(value);")
                .unwrap_or_else(|| panic!("{label}: constructor body was not preserved:\n{text}"));
            assert!(
                constructor < assignment && assignment < body,
                "{label}: pending initializers must run before assigning the parameter local:\n{text}",
            );
            assert_eq!(
                text.matches("__runInitializers(this, _instanceExtraInitializers)")
                    .count(),
                1,
                "{label}:\n{text}",
            );
            assert_eq!(text.matches("makeValue()").count(), 1, "{label}:\n{text}");
            if !use_define_for_class_fields {
                assert!(
                    !text.contains("this.value = value;"),
                    "{label}: the original plain parameter-property assignment must be replaced:\n{text}",
                );
            }
        }
    }
}

#[test]
fn standard_decorator_helper_does_not_claim_same_spelling_source_call() {
    // tsc-port: createRunInitializersHelper @6.0.3
    // tsc-hash: ac7241f25e6f4d82e533ae048fbe9de24149093224ff8713b1483e39c8798e68
    // tsc-span: _tsc.js:25715-25723
    // tsc-port: isCallToHelper @6.0.3
    // tsc-hash: 65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef
    // tsc-span: _tsc.js:26566-26568
    // tsc-port: transformPropertyWorker @6.0.3
    // tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    // tsc-span: _tsc.js:97501-97575
    let parsed = parse_source_file(
        "same-spelling-run-initializers.ts",
        concat!(
            "declare const dec: any;\n",
            "declare const receiver: any;\n",
            "declare const userInitializers: any[];\n",
            "declare function __runInitializers(receiver: any, initializers: any[]): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor(public value = (__runInitializers(receiver, userInitializers), void 0)) {}\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
        &parsed,
        false,
        &NoConstantValueResolver,
        ScriptTarget::ES2015,
        false,
    );

    let user_call = text
        .find("constructor(value = (__runInitializers(receiver, userInitializers), void 0)) {")
        .unwrap_or_else(|| panic!("the parsed same-spelling call was rewritten:\n{text}"));
    let helper_call = text
        .find("this.value = (__runInitializers(this, _instanceExtraInitializers), value);")
        .unwrap_or_else(|| panic!("the typed decorator helper was not normalized:\n{text}"));
    assert!(user_call < helper_call, "{text}");
    assert_eq!(
        text.matches("__runInitializers(receiver, userInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_pending_method_initializer_flows_into_es2022_parameter_property() {
    // tsc-port: transformTypeScript/transformClassMembers @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    // tsc-port: transformPropertyWorker @6.0.3
    // tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    // tsc-span: _tsc.js:97501-97575
    let parsed = parse_source_file(
        "parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function makeValue(): number;\n",
            "declare function body(value: number): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor(public value = makeValue()) { body(value); }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NoConstantValueResolver;

    let define_text = transform_parsed_class_declaration_correlation_at_target_with_mode(
        &parsed,
        false,
        &resolver,
        ScriptTarget::ES2022,
        true,
    );
    let field = define_text
        .find("value = __runInitializers(this, _instanceExtraInitializers);")
        .unwrap_or_else(|| {
            panic!("define: the synthetic field must own method extra initializers:\n{define_text}")
        });
    let constructor = define_text
        .find("constructor(value = makeValue()) {")
        .unwrap_or_else(|| {
            panic!("define: parameter initializer was not preserved:\n{define_text}")
        });
    let assignment = define_text
        .find("this.value = value;")
        .unwrap_or_else(|| panic!("define: parameter local was not assigned:\n{define_text}"));
    let body = define_text
        .find("body(value);")
        .unwrap_or_else(|| panic!("define: constructor body was not preserved:\n{define_text}"));
    assert!(
        field < constructor && constructor < assignment && assignment < body,
        "define: native field initialization must precede the parameter-local assignment:\n{define_text}",
    );
    assert_eq!(
        define_text
            .matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{define_text}",
    );
    assert_eq!(
        define_text.matches("makeValue()").count(),
        1,
        "{define_text}"
    );

    let assignment_text = transform_parsed_class_declaration_correlation_at_target_with_mode(
        &parsed,
        false,
        &resolver,
        ScriptTarget::ES2022,
        false,
    );
    let constructor = assignment_text
        .find("constructor(value = makeValue()) {")
        .unwrap_or_else(|| {
            panic!("assignment: parameter initializer was not preserved:\n{assignment_text}")
        });
    let assignment = assignment_text
        .find("this.value = (__runInitializers(this, _instanceExtraInitializers), value);")
        .unwrap_or_else(|| {
            panic!("assignment: parameter-property value lost its pending initializer:\n{assignment_text}")
        });
    let body = assignment_text.find("body(value);").unwrap_or_else(|| {
        panic!("assignment: constructor body was not preserved:\n{assignment_text}")
    });
    assert!(
        constructor < assignment && assignment < body,
        "assignment: pending initializers must run before assigning the parameter local:\n{assignment_text}",
    );
    assert_eq!(
        assignment_text
            .matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{assignment_text}",
    );
    assert_eq!(
        assignment_text.matches("makeValue()").count(),
        1,
        "{assignment_text}",
    );
    assert!(
        !assignment_text.contains("this.value = value;"),
        "the original plain parameter-property assignment must be replaced:\n{assignment_text}",
    );
}

#[test]
fn standard_decorator_parameter_property_bridge_is_inactive_in_esnext_define_mode() {
    // tsc-port: transformTypeScript/transformClassMembers @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: transformConstructorBody/transformParameterWithPropertyAssignment @6.0.3
    // tsc-hash: 2ce5fcc7bd977aa385985e7b3a327adca6fbd1947fedb992bebac736cc8d7383
    // tsc-span: _tsc.js:94835-94910
    // tsc-port: transformClassFields @6.0.3
    // tsc-hash: d91b924e40f595971a329f76bfa713b825bf9c1b48d047143bfa6abc10cef9ff
    // tsc-span: _tsc.js:95852-95904
    // tsc-port: getScriptTransformers @6.0.3
    // tsc-hash: 97b0afa45b94123122c16d49af7e5dc40164e1add85910ee1abd6a55f614cc63
    // tsc-span: _tsc.js:115903-115923
    let parsed = parse_source_file(
        "esnext-define-parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function makeValue(): number;\n",
            "declare function body(value: number): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor(public value = makeValue()) { body(value); }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
        &parsed,
        false,
        &NoConstantValueResolver,
        ScriptTarget::ES_NEXT,
        true,
    );

    let field = text.find("class C {\n    value;").unwrap_or_else(|| {
        panic!("the projected parameter-property field was not retained:\n{text}")
    });
    let decorator = text
        .find("@dec")
        .unwrap_or_else(|| panic!("native decorator syntax was not retained:\n{text}"));
    let constructor = text
        .find("constructor(value = makeValue()) {")
        .unwrap_or_else(|| panic!("the parameter initializer was not retained:\n{text}"));
    let assignment = text
        .find("this.value = value;")
        .unwrap_or_else(|| panic!("the TypeScript parameter assignment was not retained:\n{text}"));
    let body = text
        .find("body(value);")
        .unwrap_or_else(|| panic!("the constructor body was not retained:\n{text}"));
    assert!(
        field < decorator
            && decorator < constructor
            && constructor < assignment
            && assignment < body,
        "ESNext define mode must leave the bridge inactive and preserve native ownership:\n{text}",
    );
    assert_eq!(text.matches("makeValue()").count(), 1, "{text}");
    assert_eq!(text.matches("this.value = value;").count(), 1, "{text}");
    assert!(!text.contains("__esDecorate"), "{text}");
    assert!(!text.contains("__runInitializers"), "{text}");
}

#[test]
fn standard_decorator_pending_method_initializer_only_owns_first_parameter_property() {
    // tsc-port: transformTypeScript/transformClassMembers @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    // tsc-port: transformConstructorBody @6.0.3
    // tsc-hash: 6ab03601cab55c7af832a1cec8e17a822e21aa330f32a65b2b79637c4765c9f3
    // tsc-span: _tsc.js:97329-97431
    // tsc-port: transformPropertyWorker @6.0.3
    // tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    // tsc-span: _tsc.js:97501-97575
    let parsed = parse_source_file(
        "multiple-parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function first(): number;\n",
            "declare function second(): string;\n",
            "declare function body(a: number, b: string): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor(public a = first(), public b = second()) { body(a, b); }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NoConstantValueResolver;

    for (label, use_define_for_class_fields, first_assignment, second_assignment) in [
        (
            "define",
            true,
            "value: (__runInitializers(this, _instanceExtraInitializers), a)",
            "value: b",
        ),
        (
            "assignment",
            false,
            "this.a = (__runInitializers(this, _instanceExtraInitializers), a);",
            "this.b = b;",
        ),
    ] {
        let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
            &parsed,
            false,
            &resolver,
            ScriptTarget::ES2015,
            use_define_for_class_fields,
        );

        let constructor = text
            .find("constructor(a = first(), b = second()) {")
            .unwrap_or_else(|| panic!("{label}: parameter locals were not preserved:\n{text}"));
        let first_assignment = text.find(first_assignment).unwrap_or_else(|| {
            panic!("{label}: the first parameter property lost the pending helper:\n{text}")
        });
        let second_assignment = text.find(second_assignment).unwrap_or_else(|| {
            panic!("{label}: the second parameter-property local was not retained:\n{text}")
        });
        let body = text
            .find("body(a, b);")
            .unwrap_or_else(|| panic!("{label}: constructor body was not preserved:\n{text}"));
        assert!(
            constructor < first_assignment
                && first_assignment < second_assignment
                && second_assignment < body,
            "{label}: parameter properties must retain declaration order before the body:\n{text}",
        );
        assert_eq!(
            text.matches("__runInitializers(this, _instanceExtraInitializers)")
                .count(),
            1,
            "{label}: only the first parameter property may consume the pending queue:\n{text}",
        );
        assert_eq!(text.matches("first()").count(), 1, "{label}:\n{text}");
        assert_eq!(text.matches("second()").count(), 1, "{label}:\n{text}");
        if !use_define_for_class_fields {
            assert!(!text.contains("this.a = a;"), "{label}:\n{text}");
            assert_eq!(text.matches("this.b = b;").count(), 1, "{label}:\n{text}");
        }
    }
}

#[test]
fn standard_decorator_parameter_property_initializer_follows_direct_super_call() {
    // tsc-port: transformTypeScript/transformClassMembers @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    // tsc-port: transformConstructorBodyWorker @6.0.3
    // tsc-hash: 37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a
    // tsc-span: _tsc.js:97290-97328
    // tsc-port: transformPropertyWorker @6.0.3
    // tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    // tsc-span: _tsc.js:97501-97575
    let parsed = parse_source_file(
        "derived-parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare class Base { constructor(tag: string); }\n",
            "declare function makeValue(): number;\n",
            "declare function body(value: number): void;\n",
            "class C extends Base {\n",
            "    @dec method() {}\n",
            "    constructor(public value = makeValue()) {\n",
            "        super(\"base\");\n",
            "        body(value);\n",
            "    }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NoConstantValueResolver;

    for (label, use_define_for_class_fields, expected_assignment) in [
        (
            "define",
            true,
            "value: (__runInitializers(this, _instanceExtraInitializers), value)",
        ),
        (
            "assignment",
            false,
            "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
        ),
    ] {
        let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
            &parsed,
            false,
            &resolver,
            ScriptTarget::ES2015,
            use_define_for_class_fields,
        );

        let constructor = text
            .find("constructor(value = makeValue()) {")
            .unwrap_or_else(|| panic!("{label}: parameter initializer was not preserved:\n{text}"));
        let super_call = text
            .find("super(\"base\");")
            .unwrap_or_else(|| panic!("{label}: direct super call was not preserved:\n{text}"));
        let assignment = text.find(expected_assignment).unwrap_or_else(|| {
            panic!("{label}: parameter-property value lost its pending initializer:\n{text}")
        });
        let body = text
            .find("body(value);")
            .unwrap_or_else(|| panic!("{label}: constructor body was not preserved:\n{text}"));
        assert!(
            constructor < super_call && super_call < assignment && assignment < body,
            "{label}: derived initialization must follow super and precede the body:\n{text}",
        );
        assert_eq!(
            text.matches("__runInitializers(this, _instanceExtraInitializers)")
                .count(),
            1,
            "{label}:\n{text}",
        );
        assert_eq!(text.matches("makeValue()").count(), 1, "{label}:\n{text}");
        if !use_define_for_class_fields {
            assert!(!text.contains("this.value = value;"), "{label}:\n{text}");
        }
    }
}

#[test]
fn standard_decorator_parameter_property_initializer_follows_nested_try_super_path() {
    // tsc-port: transformClassMembers parameter-property projection @6.0.3
    // tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    // tsc-span: _tsc.js:94564-94598
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: transformConstructorBodyWorker @6.0.3
    // tsc-hash: 37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a
    // tsc-span: _tsc.js:97290-97328
    let parsed = parse_source_file(
        "nested-derived-parameter-property-decorator.ts",
        concat!(
            "declare const dec: any;\n",
            "declare class Base {}\n",
            "declare function makeValue(): number;\n",
            "declare function body(value: number): void;\n",
            "class C extends Base {\n",
            "    @dec method() {}\n",
            "    constructor(public value = makeValue()) {\n",
            "        try { super(); } catch (error) { throw error; }\n",
            "        body(value);\n",
            "    }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let resolver = NoConstantValueResolver;

    for (label, target, use_define_for_class_fields, expected_assignment) in [
        (
            "es2015-define",
            ScriptTarget::ES2015,
            true,
            "value: (__runInitializers(this, _instanceExtraInitializers), value)",
        ),
        (
            "es2015-assignment",
            ScriptTarget::ES2015,
            false,
            "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
        ),
        (
            "es2022-assignment",
            ScriptTarget::ES2022,
            false,
            "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
        ),
        (
            "esnext-assignment",
            ScriptTarget::ES_NEXT,
            false,
            "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
        ),
    ] {
        let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
            &parsed,
            false,
            &resolver,
            target,
            use_define_for_class_fields,
        );

        let try_block = text
            .find("try {")
            .unwrap_or_else(|| panic!("{label}: try block was not preserved:\n{text}"));
        let super_call = text
            .find("super();")
            .unwrap_or_else(|| panic!("{label}: nested super call was not preserved:\n{text}"));
        let assignment = text.find(expected_assignment).unwrap_or_else(|| {
            panic!("{label}: parameter-property value lost its pending initializer:\n{text}")
        });
        let catch_clause = text
            .find("catch (error)")
            .unwrap_or_else(|| panic!("{label}: catch clause was not preserved:\n{text}"));
        let body = text
            .find("body(value);")
            .unwrap_or_else(|| panic!("{label}: constructor body was not preserved:\n{text}"));
        assert!(
            try_block < super_call
                && super_call < assignment
                && assignment < catch_clause
                && catch_clause < body,
            "{label}: initialization must be inserted inside try, immediately after super:\n{text}",
        );
        assert_eq!(
            text.matches("__runInitializers(this, _instanceExtraInitializers)")
                .count(),
            1,
            "{label}: the pending queue must be consumed exactly once:\n{text}",
        );
        assert_eq!(text.matches("makeValue()").count(), 1, "{label}:\n{text}");
        if !use_define_for_class_fields {
            assert!(
                !text.contains("this.value = value;"),
                "{label}: the original parameter-property assignment must be replaced:\n{text}",
            );
        }
    }

    let define_text = transform_parsed_class_declaration_correlation_at_target_with_mode(
        &parsed,
        false,
        &resolver,
        ScriptTarget::ES2022,
        true,
    );
    let field = define_text
        .find("value = __runInitializers(this, _instanceExtraInitializers);")
        .unwrap_or_else(|| {
            panic!("es2022-define: the native field must own the pending helper:\n{define_text}")
        });
    let constructor = define_text
        .find("constructor(value = makeValue()) {")
        .unwrap_or_else(|| {
            panic!("es2022-define: the constructor was not preserved:\n{define_text}")
        });
    let super_call = define_text
        .find("super();")
        .unwrap_or_else(|| panic!("es2022-define: nested super was not preserved:\n{define_text}"));
    let assignment = define_text.find("this.value = value;").unwrap_or_else(|| {
        panic!("es2022-define: the parameter assignment was not preserved:\n{define_text}")
    });
    let catch_clause = define_text
        .find("catch (error)")
        .unwrap_or_else(|| panic!("es2022-define: catch was not preserved:\n{define_text}"));
    let body = define_text
        .find("body(value);")
        .unwrap_or_else(|| panic!("es2022-define: body was not preserved:\n{define_text}"));
    assert!(
        field < constructor
            && constructor < super_call
            && super_call < assignment
            && assignment < catch_clause
            && catch_clause < body,
        "es2022-define: field helper and constructor-local assignment crossed scopes:\n{define_text}",
    );
    assert_eq!(
        define_text
            .matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{define_text}",
    );
}

#[test]
fn standard_decorator_parameter_property_initializer_follows_parenthesized_super_paths() {
    // tsc-port: findSuperStatementIndexPath/getSuperCallFromStatement @6.0.3
    // tsc-hash: 14dfc2d8ccf6dcb0d10be798e055c204560e8561e32e5851d32e1a18703f2201
    // tsc-span: _tsc.js:93070-93093
    // tsc-port: transformConstructorBodyWorker @6.0.3
    // tsc-hash: 37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a
    // tsc-span: _tsc.js:97290-97328
    for (shape, constructor_body) in [
        ("direct", "        ((super()));\n        body(value);\n"),
        (
            "nested-try",
            concat!(
                "        try { ((super())); } catch (error) { throw error; }\n",
                "        body(value);\n",
            ),
        ),
    ] {
        let parsed = parse_source_file(
            "parenthesized-super-parameter-property-decorator.ts",
            &format!(
                concat!(
                    "declare const dec: any;\n",
                    "declare class Base {{}}\n",
                    "declare function makeValue(): number;\n",
                    "declare function body(value: number): void;\n",
                    "class C extends Base {{\n",
                    "    @dec method() {{}}\n",
                    "    constructor(public value = makeValue()) {{\n",
                    "{}",
                    "    }}\n",
                    "}}\n",
                ),
                constructor_body,
            ),
            Default::default(),
            None,
        );
        let resolver = NoConstantValueResolver;

        for (target_label, target) in [
            ("es2022", ScriptTarget::ES2022),
            ("esnext", ScriptTarget::ES_NEXT),
        ] {
            let text = transform_parsed_class_declaration_correlation_at_target_with_mode(
                &parsed, false, &resolver, target, false,
            );
            let label = format!("{target_label}-{shape}");
            let constructor = text
                .find("constructor(value = makeValue()) {")
                .unwrap_or_else(|| {
                    panic!("{label}: parameter initializer was not preserved:\n{text}")
                });
            let super_call = text
                .find("super()")
                .unwrap_or_else(|| panic!("{label}: parenthesized super call was lost:\n{text}"));
            let assignment = text
                .find(
                    "this.value = (__runInitializers(this, _instanceExtraInitializers), value);",
                )
                .unwrap_or_else(|| {
                    panic!(
                        "{label}: parameter-property initializer was not placed after super:\n{text}"
                    )
                });
            let body = text
                .find("body(value);")
                .unwrap_or_else(|| panic!("{label}: constructor body was not preserved:\n{text}"));
            assert!(
                constructor < super_call && super_call < assignment && assignment < body,
                "{label}: initialization must follow the parenthesized super path:\n{text}",
            );
            if shape == "nested-try" {
                let catch_clause = text.find("catch (error)").unwrap_or_else(|| {
                    panic!("{label}: nested try/catch was not preserved:\n{text}")
                });
                assert!(
                    assignment < catch_clause,
                    "{label}: initialization must remain inside the try block:\n{text}",
                );
            }
            assert_eq!(
                text.matches("__runInitializers(this, _instanceExtraInitializers)")
                    .count(),
                1,
                "{label}: the pending queue must be consumed exactly once:\n{text}",
            );
            assert!(
                !text.contains("this.value = value;"),
                "{label}: the original parameter-property assignment must be replaced:\n{text}",
            );
        }
    }
}

#[test]
fn standard_decorator_residual_initializer_replays_base_constructor_prologue() {
    // tsc-port: copyPrologue/copyStandardPrologue/copyCustomPrologue @6.0.3
    // tsc-hash: 555445a3fd02a4b53bbc05f05e48729ca0f7208892d66dbc7985f51f3e897a8e
    // tsc-span: _tsc.js:24827-24869
    // tsc-port: visitConstructorDeclaration @6.0.3
    // tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    // tsc-span: _tsc.js:99788-99823
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function body(): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor() {\n",
            "        \"standard\";\n",
            "        body();\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let constructor = text.find("constructor() {").expect("constructor");
    let directives = text
        .match_indices("\"standard\";")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let initializer = text
        .find("__runInitializers(this, _instanceExtraInitializers);")
        .expect("residual initializer");
    let body = text.find("body();").expect("constructor body");
    assert_eq!(directives.len(), 2, "{text}");
    assert!(
        constructor < directives[0]
            && directives[0] < initializer
            && initializer < directives[1]
            && directives[1] < body,
        "the no-super path must emit prefix, initializer, then the complete original body:\n{text}",
    );
}

#[test]
fn standard_decorator_residual_initializer_replays_custom_variable_prologue() {
    // tsc-port: copyPrologue/copyStandardPrologue/copyCustomPrologue @6.0.3
    // tsc-hash: 555445a3fd02a4b53bbc05f05e48729ca0f7208892d66dbc7985f51f3e897a8e
    // tsc-span: _tsc.js:24827-24869
    // tsc-port: visitConstructorDeclaration @6.0.3
    // tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    // tsc-span: _tsc.js:99788-99823
    let parsed = parse_source_file(
        "decorator-custom-constructor-prologue.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function customPrologue(): void;\n",
            "declare function body(): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor() {\n",
            "        const marker = customPrologue(); /*between*/\n",
            "        body();\n",
            "    }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("source file root");
    };
    let source_statements = parsed
        .arena
        .node_array(source_file.statements.expect("source statements"));
    let class = source_statements
        .nodes
        .iter()
        .copied()
        .find(|node| {
            matches!(
                &parsed.arena.node(*node).data,
                NodeData::ClassDeclaration(_)
            )
        })
        .expect("class declaration");
    let NodeData::ClassDeclaration(class) = &parsed.arena.node(class).data else {
        unreachable!();
    };
    let members = parsed
        .arena
        .node_array(class.members.expect("class members"));
    let constructor = members
        .nodes
        .iter()
        .copied()
        .find(|node| matches!(&parsed.arena.node(*node).data, NodeData::Constructor(_)))
        .expect("constructor");
    let NodeData::Constructor(constructor) = &parsed.arena.node(constructor).data else {
        unreachable!();
    };
    let NodeData::Block(body) = &parsed
        .arena
        .node(constructor.body.expect("constructor body"))
        .data
    else {
        panic!("constructor body block");
    };
    let body_statements = parsed
        .arena
        .node_array(body.statements.expect("constructor statements"));
    let custom_prologue_id = body_statements.nodes[0];
    assert!(matches!(
        &parsed.arena.node(custom_prologue_id).data,
        NodeData::VariableStatement(_)
    ));

    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let custom_prologue = arena
        .node_ref(source, custom_prologue_id)
        .expect("mounted custom prologue");
    arena
        .metadata_mut(custom_prologue)
        .add_flags(EmitFlags::CUSTOM_PROLOGUE);
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2022.bits());
    options.always_strict = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("standard decorator custom-prologue transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2022),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print standard decorator custom-prologue transform")
    .text()
    .to_owned();

    let markers = text
        .match_indices("const marker = customPrologue();")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let initializer = text
        .find("__runInitializers(this, _instanceExtraInitializers);")
        .expect("residual initializer");
    let body = text.find("body();").expect("constructor body");
    assert_eq!(markers.len(), 2, "{text}");
    assert!(
        markers[0] < initializer && initializer < markers[1] && markers[1] < body,
        "the no-super path must replay a non-expression custom prologue:\n{text}",
    );
    assert_eq!(text.matches("/*between*/").count(), 2, "{text}");
}

#[test]
fn standard_decorator_residual_initializer_keeps_derived_prologue_single() {
    // tsc-port: transformConstructorBodyWorker/visitConstructorDeclaration @6.0.3
    // tsc-hash: 0cc717d67cc49994f53a0d9e2c8f1451a32980ea8cabfd66266908365c221911
    // tsc-span: _tsc.js:99759-99823
    for (shape, constructor_body) in [
        ("direct", "        super();\n        body();\n"),
        (
            "nested-try",
            concat!(
                "        try { super(); } catch (error) { throw error; }\n",
                "        body();\n",
            ),
        ),
    ] {
        let source_text = format!(
            concat!(
                "declare const dec: any;\n",
                "declare class Base {{}}\n",
                "declare function body(): void;\n",
                "class C extends Base {{\n",
                "    @dec method() {{}}\n",
                "    constructor() {{\n",
                "        \"standard\";\n",
                "{}",
                "    }}\n",
                "}}\n",
            ),
            constructor_body,
        );
        let text = transform_and_print_at_target(&source_text, ScriptTarget::ES2022);
        let directive = text
            .find("\"standard\";")
            .unwrap_or_else(|| panic!("{shape}: constructor prologue was not retained:\n{text}"));
        let super_call = text
            .find("super();")
            .unwrap_or_else(|| panic!("{shape}: super call was not retained:\n{text}"));
        let initializer = text
            .find("__runInitializers(this, _instanceExtraInitializers);")
            .unwrap_or_else(|| panic!("{shape}: residual initializer was not emitted:\n{text}"));
        let body = text
            .find("body();")
            .unwrap_or_else(|| panic!("{shape}: constructor body was not retained:\n{text}"));
        assert_eq!(text.matches("\"standard\";").count(), 1, "{shape}:\n{text}");
        assert!(
            directive < super_call && super_call < initializer && initializer < body,
            "{shape}: the super path must retain one prefix and initialize after super:\n{text}",
        );
        if shape == "nested-try" {
            let catch_clause = text
                .find("catch (error)")
                .unwrap_or_else(|| panic!("{shape}: catch clause was not retained:\n{text}"));
            assert!(initializer < catch_clause, "{shape}:\n{text}");
        }
    }
}

#[test]
fn standard_decorator_residual_instance_initializer_follows_parenthesized_super_paths() {
    // tsc-port: findSuperStatementIndexPath/getSuperCallFromStatement @6.0.3
    // tsc-hash: 14dfc2d8ccf6dcb0d10be798e055c204560e8561e32e5851d32e1a18703f2201
    // tsc-span: _tsc.js:93070-93093
    // tsc-port: prepareConstructor @6.0.3
    // tsc-hash: 2a79ab99613abecdfd7e854650bbaac5f5b831bde37c6c0a45fd71d923d79954
    // tsc-span: _tsc.js:99747-99758
    // tsc-port: transformConstructorBodyWorker @6.0.3
    // tsc-hash: aaf0c5324b33bbc52730bda4f4a77db2c952a35f0f18f78dafe9750923fd9c12
    // tsc-span: _tsc.js:99759-99787
    // tsc-port: visitConstructorDeclaration @6.0.3
    // tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    // tsc-span: _tsc.js:99788-99823
    for (shape, constructor_body) in [
        ("direct", "        ((super()));\n        body();\n"),
        (
            "nested-try",
            concat!(
                "        try { ((super())); } catch (error) { throw error; }\n",
                "        body();\n",
            ),
        ),
    ] {
        let source_text = format!(
            concat!(
                "declare const dec: any;\n",
                "declare class Base {{}}\n",
                "declare function body(): void;\n",
                "class C extends Base {{\n",
                "    @dec method() {{}}\n",
                "    constructor() {{\n",
                "{}",
                "    }}\n",
                "}}\n",
            ),
            constructor_body,
        );

        for (target_label, target) in [
            ("es2015", ScriptTarget::ES2015),
            ("es2022", ScriptTarget::ES2022),
        ] {
            let label = format!("{target_label}-{shape}");
            let text = transform_and_print_at_target(&source_text, target);
            let constructor = text
                .find("constructor() {")
                .unwrap_or_else(|| panic!("{label}: constructor was not retained:\n{text}"));
            let super_call = text
                .find("super()")
                .unwrap_or_else(|| panic!("{label}: parenthesized super call was lost:\n{text}"));
            let initializer = text
                .find("__runInitializers(this, _instanceExtraInitializers);")
                .unwrap_or_else(|| {
                    panic!("{label}: the residual instance queue was not consumed:\n{text}")
                });
            let body = text
                .find("body();")
                .unwrap_or_else(|| panic!("{label}: constructor body was not retained:\n{text}"));
            assert!(
                constructor < super_call && super_call < initializer && initializer < body,
                "{label}: the standard-decorator residual queue must run after super:\n{text}",
            );
            if shape == "nested-try" {
                let catch_clause = text.find("catch (error)").unwrap_or_else(|| {
                    panic!("{label}: nested try/catch was not retained:\n{text}")
                });
                assert!(
                    initializer < catch_clause,
                    "{label}: the initializer must remain inside the try block:\n{text}",
                );
            }
            assert_eq!(
                text.matches("__runInitializers(this, _instanceExtraInitializers)")
                    .count(),
                1,
                "{label}: the residual queue must be consumed exactly once:\n{text}",
            );
        }
    }
}

#[test]
fn standard_decorator_terminal_initializer_follows_nested_try_super_path() {
    // tsc-port: transformConstructorBodyWorker @6.0.3
    // tsc-hash: aaf0c5324b33bbc52730bda4f4a77db2c952a35f0f18f78dafe9750923fd9c12
    // tsc-span: _tsc.js:99759-99787
    // tsc-port: visitConstructorDeclaration @6.0.3
    // tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    // tsc-span: _tsc.js:99788-99823
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare class Base {}\n",
            "declare function body(): void;\n",
            "class C extends Base {\n",
            "    @dec method() {}\n",
            "    constructor() {\n",
            "        try { super(); } catch (error) { throw error; }\n",
            "        body();\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let try_block = text.find("try {").expect("try block is retained");
    let super_call = text
        .find("super();")
        .expect("nested super call is retained");
    let initializer = text
        .find("__runInitializers(this, _instanceExtraInitializers);")
        .expect("the residual instance queue is consumed");
    let catch_clause = text
        .find("catch (error)")
        .expect("catch clause is retained");
    let body = text.find("body();").expect("constructor body is retained");
    assert!(
        try_block < super_call
            && super_call < initializer
            && initializer < catch_clause
            && catch_clause < body,
        "the residual initializer must be inserted inside try, immediately after super:\n{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_pending_method_initializer_falls_back_to_constructor_without_fields() {
    // tsc-port: prepareConstructor @6.0.3
    // tsc-hash: 2a79ab99613abecdfd7e854650bbaac5f5b831bde37c6c0a45fd71d923d79954
    // tsc-span: _tsc.js:99747-99758
    // tsc-port: visitConstructorDeclaration @6.0.3
    // tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    // tsc-span: _tsc.js:99788-99823
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function body(): void;\n",
            "class C {\n",
            "    @dec method() {}\n",
            "    constructor() { body(); }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let constructor = text
        .find("constructor() {")
        .expect("constructor is retained");
    let pending = text
        .find("__runInitializers(this, _instanceExtraInitializers);")
        .expect("the constructor drains method extra initializers when no field can own them");
    let body = text.find("body();").expect("constructor body is retained");
    assert!(constructor < pending && pending < body, "{text}");
    assert_eq!(
        text.matches("__runInitializers(this, _instanceExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_pending_static_method_initializer_owns_first_ordinary_field() {
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function before(): number;\n",
            "class C {\n",
            "    static first = before();\n",
            "    @dec static method() {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains("value: (__runInitializers(")
            && text.contains("_staticExtraInitializers), before())"),
        "static method extra initializers belong inside the first ordinary static field:\n{text}",
    );
}

#[test]
fn standard_decorator_pending_static_method_initializer_creates_uninitialized_field_value() {
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "class C {\n",
            "    @dec static method() {}\n",
            "    static first: number;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let property = text
        .rfind("Object.defineProperty(")
        .expect("the uninitialized static field is emitted");
    // Below ES2022 class-fields lowering owns this as a class-pending comma
    // operand, so the call is followed by `}),` rather than a statement
    // terminator.  Inspect the final property operand without assuming either
    // statement or comma-list ownership.
    let property = &text[property..];
    assert!(property.contains("\"first\""), "{text}");
    assert!(
        property.contains("value: __runInitializers(")
            && property.contains("_staticExtraInitializers"),
        "the uninitialized static field creates a value that drains the pending queue:\n{text}",
    );
}

#[test]
fn standard_decorator_pending_static_method_initializer_precedes_static_block_body() {
    // tsc-port: visitClassStaticBlockDeclaration @6.0.3
    // tsc-hash: 5ba6f2d5e5b218a418e3ca67a6714022b5a77e460c16e042d950b765f0a6504a
    // tsc-span: _tsc.js:100005-100040
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function boundary(): void;\n",
            "class C {\n",
            "    @dec static method() {}\n",
            "    static { boundary(); }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let decorate = text
        .rfind("__esDecorate(")
        .expect("the static method is decorated");
    let pending = text
        .rfind("__runInitializers(")
        .expect("the static block boundary drains pending initializers");
    let pending_end = text[pending..]
        .find(';')
        .map(|offset| pending + offset)
        .expect("pending initializer call is a statement");
    let pending_statement = &text[pending..pending_end];
    let boundary = text
        .find("boundary();")
        .expect("static block body is retained");
    assert!(
        pending_statement.contains("_staticExtraInitializers"),
        "the boundary drains the static method queue:\n{text}",
    );
    assert!(decorate < pending && pending < boundary, "{text}");
}

#[test]
fn standard_decorator_residual_static_method_initializer_stays_in_leading_block() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: 7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df
    // tsc-span: _tsc.js:99319-99616
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "class C {\n",
            "    @dec static method() {}\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let decorate = text
        .rfind("__esDecorate(")
        .expect("the static method is decorated");
    let residual = text
        .rfind("__runInitializers(this, _staticExtraInitializers);")
        .expect("the residual static method queue is consumed");
    let method = text
        .find("static method()")
        .expect("the decorated static method is retained");
    assert!(
        decorate < residual && residual < method,
        "without a pre-existing static initializer, the residual queue belongs to the leading decoration block:\n{text}",
    );
    assert_eq!(text.matches("static {").count(), 1, "{text}");
    assert_eq!(
        text.matches("__runInitializers(this, _staticExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
}

#[test]
fn standard_decorator_class_extra_follows_static_method_extra_in_leading_block() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: 7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df
    // tsc-span: _tsc.js:99319-99616
    // Class-plan allocation within the following owner.
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: cc63e7e5a08da6f16ac6b79dece72553e777487b91623ff131a7383595bb36d0
    // tsc-span: _tsc.js:99344-99364
    // Class-decorate production and final placement within the following owner.
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: e7c279d7ef714e4c9b18693ddd733b1473d97bff6f71b3e894a60b9ba36ffe91
    // tsc-span: _tsc.js:99488-99528
    let text = transform_and_print_at_target(
        concat!(
            "declare const classDec: any;\n",
            "declare const memberDec: any;\n",
            "@classDec\n",
            "class C {\n",
            "    @memberDec static method() {}\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let class_decorate = text
        .find("__esDecorate(null, _classDescriptor")
        .expect("the class is decorated");
    let method_extra = text
        .find("__runInitializers(_classThis, _staticExtraInitializers);")
        .expect("the residual static method queue is consumed");
    let class_extra = text
        .find("__runInitializers(_classThis, _classExtraInitializers);")
        .expect("the class extra initializer queue is consumed");
    let method = text
        .find("static method()")
        .expect("the decorated static method is retained");
    assert!(
        class_decorate < method_extra && method_extra < class_extra && class_extra < method,
        "class extra initializers must follow static method extras in the leading block:\n{text}",
    );
    assert_eq!(text.matches("static {").count(), 2, "{text}");
    assert_eq!(
        text.matches("__runInitializers(_classThis, _staticExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(_classThis, _classExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("null, _classExtraInitializers);").count(),
        1,
        "the class decorator must populate the class-finalizer lane exactly once:\n{text}",
    );
}

#[test]
fn standard_decorator_residual_static_field_initializer_uses_trailing_block() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: 7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df
    // tsc-span: _tsc.js:99319-99616
    // tsc-port: visitPropertyDeclaration @6.0.3
    // tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    // tsc-span: _tsc.js:100041-100150
    // tsc-port: injectPendingInitializers @6.0.3
    // tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    // tsc-span: _tsc.js:100535-100545
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any;\n",
            "declare function before(): number;\n",
            "class C {\n",
            "    @dec static field = before();\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let decorate = text
        .rfind("__esDecorate(")
        .expect("the static field is decorated");
    let field = text
        .find("static field = __runInitializers(this, _static_field_initializers, before());")
        .expect("the decorated static field initializer is retained");
    let residual = text
        .rfind("__runInitializers(this, _static_field_extraInitializers);")
        .expect("the residual static field queue is consumed");
    assert!(
        decorate < field && field < residual,
        "a pre-existing static initializer keeps the residual queue in a trailing block:\n{text}",
    );
    assert_eq!(text.matches("static {").count(), 2, "{text}");
    assert_eq!(
        text.matches("__runInitializers(this, _static_field_extraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(text.matches("before()").count(), 1, "{text}");
}

#[test]
fn standard_decorator_class_extra_follows_static_field_extra_in_trailing_block() {
    // tsc-port: createClassInfo @6.0.3
    // tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
    // tsc-span: _tsc.js:99241-99318
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: 7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df
    // tsc-span: _tsc.js:99319-99616
    // Class-plan allocation within the following owner.
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: cc63e7e5a08da6f16ac6b79dece72553e777487b91623ff131a7383595bb36d0
    // tsc-span: _tsc.js:99344-99364
    // Class-decorate production and final placement within the following owner.
    // tsc-port: transformClassLike @6.0.3
    // tsc-hash: e7c279d7ef714e4c9b18693ddd733b1473d97bff6f71b3e894a60b9ba36ffe91
    // tsc-span: _tsc.js:99488-99528
    let text = transform_and_print_at_target(
        concat!(
            "declare const classDec: any;\n",
            "declare const fieldDec: any;\n",
            "@classDec\n",
            "class C {\n",
            "    @fieldDec static field;\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let class_decorate = text
        .find("__esDecorate(null, _classDescriptor")
        .expect("the class is decorated");
    let field = text
        .find("static field = __runInitializers(_classThis, _static_field_initializers, void 0);")
        .expect("the native decorated static field is retained");
    let field_extra = text
        .find("__runInitializers(_classThis, _static_field_extraInitializers);")
        .expect("the residual static field queue is consumed");
    let class_extra = text
        .find("__runInitializers(_classThis, _classExtraInitializers);")
        .expect("the class extra initializer queue is consumed");
    assert!(
        class_decorate < field && field < field_extra && field_extra < class_extra,
        "class extra initializers must follow static field extras in the trailing block:\n{text}",
    );
    assert_eq!(text.matches("static {").count(), 3, "{text}");
    assert_eq!(
        text.matches("__runInitializers(_classThis, _static_field_initializers, void 0)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(_classThis, _static_field_extraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("__runInitializers(_classThis, _classExtraInitializers)")
            .count(),
        1,
        "{text}",
    );
    assert_eq!(
        text.matches("null, _classExtraInitializers);").count(),
        1,
        "the class decorator must populate the class-finalizer lane exactly once:\n{text}",
    );
}

#[test]
fn standard_decorator_class_evaluation_precedes_pending_field_keys() {
    // tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
    // tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
    // tsc-span: _tsc.js:97049-97129
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any, base: number, exponent: number;\n",
            "declare const later: unique symbol;\n",
            "declare function first(): PropertyKey;\n",
            "@dec class Value {\n",
            "    [first()]() {}\n",
            "    [later] = base ** exponent;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let class_key = text
        .find("[first()]()")
        .expect("retained method key is evaluated with the class expression");
    let pending_field_key = text
        .find("_a = later;")
        .expect("erased field key becomes a following statement");
    assert!(
        class_key < pending_field_key,
        "the class expression must run before its pending field key:\n{text}",
    );
    assert!(text.contains("var Value = _classThis = class"), "{text}");
}

#[test]
fn standard_decorator_pending_class_operations_keep_source_order_and_statement_ownership() {
    // tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
    // tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
    // tsc-span: _tsc.js:97049-97129
    // The producer/drain identities are pinned separately by
    // E-CLASS-PENDING-G in emitter-architecture.md.
    let text = transform_and_print_at_target(
        concat!(
            "declare const dec: any, base: number, exponent: number;\n",
            "declare const before: unique symbol, after: unique symbol;\n",
            "@dec class Value {\n",
            "    [before] = base ** exponent;\n",
            "    #method() {}\n",
            "    [after] = base ** exponent;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let class = text
        .find("var Value = _classThis = class")
        .expect("standard decorator class assignment");
    let brand = text
        .find("_Value_instances = new WeakSet();")
        .expect("private-method instance brand");
    let before = text.find("_a = before;").expect("first computed field key");
    let method = text
        .find("_Value_method = function _Value_method() { };")
        .expect("private method definition");
    let after = text.find("_b = after;").expect("second computed field key");
    let named = text
        .find("__setFunctionName(_classThis, \"Value\");")
        .expect("standard named evaluation");
    assert!(
        class < brand && brand < before && before < method && method < after && after < named,
        "pending class operations must retain tsc's ordered stream:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "_Value_instances = new WeakSet();\n",
            "    _a = before;\n",
            "    _Value_method = function _Value_method() { };\n",
            "    _b = after;\n",
        )),
        "statement-owned pending expressions must not collapse into a comma expression:\n{text}",
    );
}

#[test]
fn class_pending_prefix_and_member_walk_drain_into_one_retained_computed_name() {
    // tsc-port: injectPendingExpressions @6.0.3
    // tsc-hash: 5ba282b28c8f6b724f359b12b848c573fa0c2218cd12f4619475d3d22596d54e
    // tsc-span: _tsc.js:96167-96179
    // The setup/member-walk producer identities are pinned separately by
    // E-CLASS-PENDING-G in emitter-architecture.md.
    let text = transform_and_print_at_target(
        concat!(
            "declare const before: unique symbol, after: unique symbol;\n",
            "class C {\n",
            "    accessor left = 0;\n",
            "    #one = 1;\n",
            "    [before] = 2;\n",
            "    #method() {}\n",
            "    accessor right = 0;\n",
            "    #two = 3;\n",
            "    [after]() {}\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let computed_name = text
        .lines()
        .find(|line| line.contains("after)]()"))
        .unwrap_or_else(|| panic!("missing retained computed member drain:\n{text}"));
    let positions = [
        "_C_one = new WeakMap()",
        "_C_two = new WeakMap()",
        "_C_instances = new WeakSet()",
        "_C_left_accessor_storage = new WeakMap()",
        "_C_right_accessor_storage = new WeakMap()",
        "_a = before",
        "_C_method = function _C_method() { }",
        "after)]()",
    ]
    .map(|fragment| {
        computed_name
            .find(fragment)
            .unwrap_or_else(|| panic!("missing {fragment:?} from computed-name drain:\n{text}"))
    });
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the retained member must drain private fields, the shared brand, auto-accessor storage, and member-walk entries in tsc order:\n{text}",
    );
}

#[test]
fn ordinary_class_expression_keeps_pending_and_static_work_as_ordered_operands() {
    // tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
    // tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
    // tsc-span: _tsc.js:97049-97129
    let text = transform_and_print_at_target(
        concat!(
            "declare const key: unique symbol;\n",
            "const Value = class Named {\n",
            "    #field = 1;\n",
            "    #method() {}\n",
            "    accessor item = 2;\n",
            "    [key] = 3;\n",
            "    static tail = 4;\n",
            "};\n",
        ),
        ScriptTarget::ES2015,
    );

    let initializer_start = text
        .find("const Value = (")
        .unwrap_or_else(|| panic!("missing lowered class-expression initializer:\n{text}"));
    let initializer_end = text[initializer_start..]
        .find("_b);")
        .map(|offset| initializer_start + offset + "_b);".len())
        .unwrap_or_else(|| panic!("missing final class-expression value operand:\n{text}"));
    let initializer = &text[initializer_start..initializer_end];
    let positions = [
        "_b = class Named",
        "_Named_field = new WeakMap()",
        "_Named_instances = new WeakSet()",
        "_Named_item_accessor_storage = new WeakMap()",
        "_Named_method = function _Named_method() { }",
        "_a = key",
        "Object.defineProperty(_b, \"tail\"",
        "_b);",
    ]
    .map(|fragment| {
        initializer.find(fragment).unwrap_or_else(|| {
            panic!("missing {fragment:?} from class-expression operands:\n{text}")
        })
    });
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "ordinary class expressions must sequence class evaluation, pending work, static work, and the result temp in that order:\n{text}",
    );
    for operand in [
        "_Named_field = new WeakMap()",
        "_Named_instances = new WeakSet()",
        "_Named_item_accessor_storage = new WeakMap()",
        "_Named_method = function _Named_method() { }",
        "_a = key",
    ] {
        let line = initializer
            .lines()
            .find(|line| line.contains(operand))
            .unwrap_or_else(|| panic!("missing operand line for {operand:?}:\n{text}"));
        assert!(
            line.trim_end().ends_with(','),
            "pending work must remain an operand of the class-expression comma sequence: {line:?}\n{text}",
        );
    }
    assert!(initializer.trim_end().ends_with("_b);"), "{text}");
}

#[test]
fn legacy_decorated_class_materializes_pending_work_as_individual_statements() {
    // tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
    // tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
    // tsc-span: _tsc.js:97049-97129
    let parsed = parse_source_file(
        "legacy-decorated-pending-statements.ts",
        concat!(
            "declare const dec: any;\n",
            "declare const before: unique symbol, after: unique symbol;\n",
            "@dec class Value {\n",
            "    [before] = 1;\n",
            "    #method() {}\n",
            "    [after] = 2;\n",
            "    static tail = 3;\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let text =
        transform_parsed_class_declaration_correlation(&parsed, true, &NoConstantValueResolver);

    let class = text
        .find("let Value = class Value")
        .expect("legacy decorated class initializer");
    let brand = text
        .find("_Value_instances = new WeakSet();")
        .expect("private-method instance brand statement");
    let before = text.find("_a = before;").expect("first key statement");
    let method = text
        .find("_Value_method = function _Value_method() { };")
        .expect("private method definition statement");
    let after = text.find("_b = after;").expect("second key statement");
    let static_field = text
        .find("Value.tail = 3;")
        .expect("static field statement");
    let decorate = text
        .find("Value = __decorate([")
        .expect("legacy decoration assignment");
    assert!(
        class < brand
            && brand < before
            && before < method
            && method < after
            && after < static_field
            && static_field < decorate,
        "legacy statement owners must retain the ordered pending stream before static and decorator work:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "_Value_instances = new WeakSet();\n",
            "_a = before;\n",
            "_Value_method = function _Value_method() { };\n",
            "_b = after;\n",
            "Value.tail = 3;\n",
        )),
        "legacy decorated pending entries must be adjacent individual statements, not a comma expression:\n{text}",
    );
}

#[test]
fn decorated_class_pending_comma_keys_preserve_tsc_operand_boundaries() {
    // tsc-port: flattenCommaListWorker @6.0.3
    // tsc-hash: a879551d103899488f8f2dbe2ca28ab980ecb860b8871916a3ad6958c3d274d2
    // tsc-span: _tsc.js:28218-28231
    let parsed = parse_source_file(
        "decorated-pending-comma-key.ts",
        concat!(
            "declare const dec: any;\n",
            "declare function a(): PropertyKey;\n",
            "declare function b(): PropertyKey;\n",
            "@dec class C {\n",
            "    [a(), b()]: unknown;\n",
            "    #method() {}\n",
            "}\n",
        ),
        Default::default(),
        None,
    );

    for experimental_decorators in [false, true] {
        let assign = transform_parsed_class_declaration_correlation_with_mode(
            &parsed,
            experimental_decorators,
            &NoConstantValueResolver,
            false,
        );
        let assign_lines = assign.lines().map(str::trim).collect::<Vec<_>>();
        let first = assign_lines
            .iter()
            .position(|line| *line == "a();")
            .unwrap_or_else(|| panic!("missing first flattened key operand:\n{assign}"));
        assert_eq!(
            assign_lines.get(first + 1),
            Some(&"b();"),
            "an uncached comma key must become adjacent individual pending statements:\n{assign}",
        );
        assert!(
            !assign_lines.iter().any(|line| *line == "(a(), b());"),
            "the statement owner must not recover a flattened key as one comma statement:\n{assign}",
        );

        let define = transform_parsed_class_declaration_correlation_with_mode(
            &parsed,
            experimental_decorators,
            &NoConstantValueResolver,
            true,
        );
        let define_lines = define.lines().map(str::trim).collect::<Vec<_>>();
        assert!(
            define_lines
                .iter()
                .any(|line| line.ends_with("= (a(), b());")),
            "a captured key assignment remains one pending event:\n{define}",
        );
        assert!(
            !define_lines
                .iter()
                .any(|line| *line == "a();" || *line == "b();"),
            "flattening must not split the comma expression inside a cache assignment:\n{define}",
        );
    }
}

#[test]
fn downlevel_standard_decorator_metadata_uses_the_class_receiver() {
    let parsed = parse_source_file(
        "parameterDecoratorsEmitCrash.ts",
        concat!(
            "declare var dec: any;\n",
            "export class C {\n",
            "    @dec x: any;\n",
            "    constructor(@dec x: any) {}\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.module = Some(ModuleKind::COMMON_JS.bits());
    options.always_strict = Some(false);
    let source_id = SourceFileId::from_raw(0);
    let host = TransformContractHost {
        options: &options,
        syntax: &parsed,
        source_ids: [source_id],
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers_for_source(&options, &SystemContractResolver, &host, source_id)
            .unwrap(),
        false,
    )
    .expect("ES2015 CommonJS standard-decorator pipeline");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print downlevel standard-decorator metadata")
        .text()
        .to_owned();

    assert!(
        text.contains("if (_metadata) Object.defineProperty(_a, Symbol.metadata"),
        "the relocated static block must retain its class receiver:\n{text}",
    );
    assert!(
        !text.contains("if (_metadata) Object.defineProperty(this, Symbol.metadata"),
        "the wrapper's lexical this is not the decorated class:\n{text}",
    );
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
fn es2022_disposal_names_follow_retained_class_member_order() {
    let text = transform_and_print_at_target(
        concat!(
            "class C {\n",
            "    field = () => { using fieldResource = acquire(); };\n",
            "    constructor() { using constructorResource = acquire(); }\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    let field_scope = text
        .find("field = () => { const env_1 =")
        .unwrap_or_else(|| panic!("retained field disposal scope missing from:\n{text}"));
    let constructor_scope = text
        .find("constructor() { const env_2 =")
        .unwrap_or_else(|| panic!("constructor disposal scope missing from:\n{text}"));
    assert!(field_scope < constructor_scope);
}

#[test]
fn es2021_class_lowering_finalizes_outer_bindings_before_nested_static_evaluation() {
    let text = transform_and_print_at_target(
        concat!(
            "(class Reflect {\n",
            "    static { class C extends B { static value = super.w(); } }\n",
            "});\n",
        ),
        ScriptTarget::ES2021,
    );

    assert!(
        text.starts_with("var _a;\n"),
        "outer class binding missing from:\n{text}"
    );
    assert!(
        text.contains("(() => {\n        var _b, _c;"),
        "nested static bindings did not reserve the outer identity:\n{text}"
    );
    assert!(!text.contains("(() => {\n        var _a, _b;"));
}

#[test]
fn es2015_base_class_does_not_hoist_nested_static_block_super_base() {
    let text = transform_and_print_at_target(
        concat!(
            "(class Reflect {\n",
            "    static { class C extends B { static { super.w(); } } }\n",
            "});\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.starts_with("var _a;\n"),
        "base class hoisted an unused nested super binding:\n{text}"
    );
    assert!(!text.starts_with("var _a, _b;\n"), "{text}");
    assert!(
        text.contains("(() => {\n        var _b, _c;"),
        "nested class did not own its receiver and super base:\n{text}"
    );
}

#[test]
fn assignment_mode_parameter_properties_precede_field_initializers() {
    let parsed = parse_source_file(
        "parameter-property.ts",
        concat!(
            "class Helper { create() { return true; } }\n",
            "class Broken {\n",
            "    constructor(readonly facade: Helper) { use(this.bug); }\n",
            "    bug = this.facade.create();\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2021.bits());
    options.use_define_for_class_fields = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("assignment-mode parameter-property transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print assignment-mode parameter property")
        .text()
        .to_owned();

    let parameter_property = text
        .find("this.facade = facade;")
        .unwrap_or_else(|| panic!("parameter-property assignment missing from:\n{text}"));
    let field_initializer = text
        .find("this.bug = this.facade.create();")
        .unwrap_or_else(|| panic!("field initializer missing from:\n{text}"));
    assert!(parameter_property < field_initializer);
}

#[test]
fn parameter_property_assignments_preserve_escaped_identifier_spelling() {
    let text = transform_and_print_at_target(
        r"class C { constructor(public arg\u0032: string, public arg\u0033: boolean) {} }",
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains(r"value: arg\u0032"),
        "escaped parameter-property spelling was not retained:\n{text}"
    );
    assert!(
        text.contains(r"value: arg\u0033"),
        "escaped parameter-property spelling was not retained:\n{text}"
    );
}

#[test]
fn relocated_field_initializer_does_not_reown_constructor_header_comments() {
    let text = transform_and_print_at_target(
        concat!(
            "class Base {}\n",
            "class Derived extends Base {\n",
            "    field = 1;\n",
            "    constructor(value: number) { // constructor header\n",
            "        super();\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let super_call = text.find("super();").expect("derived super call");
    let field_initializer = text
        .find("Object.defineProperty(this, \"field\"")
        .expect("relocated field initializer");
    assert!(super_call < field_initializer, "{text}");
    assert!(
        !text.contains("constructor header"),
        "a generated statement outside the source body cannot anchor its trailing trivia:\n{text}",
    );
}

#[test]
fn function_body_detached_comment_precedes_relocated_field_initializer() {
    let parsed = parse_source_file(
        "detached-constructor-comment.ts",
        concat!(
            "class Event {\n",
            "    private _listeners: any[] = [];\n",
            "    constructor() {\n",
            "        // TODO: remove\n",
            "\n",
            "        this._listeners = [];\n",
            "    }\n",
            "}\n",
        ),
        ParseOptions::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        use_define_for_class_fields: Some(false),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("assignment-mode class field transform");
    let text = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2015),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("print constructor detached comments")
    .text()
    .to_owned();

    let comment = text.find("// TODO: remove").expect("detached comment");
    let assignments = text
        .match_indices("this._listeners = [];")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    assert_eq!(assignments.len(), 2, "{text}");
    assert!(comment < assignments[0], "{text}");
    assert!(assignments[0] < assignments[1], "{text}");
    assert_eq!(text.matches("// TODO: remove").count(), 1, "{text}");
}

#[test]
fn es2022_decorated_auto_accessor_comment_belongs_to_getter_only() {
    let text = transform_and_print_at_target(
        concat!(
            "const dec = (_value, _context) => {};\n",
            "class C {\n",
            "    // accessor comment\n",
            "    @dec static accessor value = 1;\n",
            "}\n",
        ),
        ScriptTarget::ES2022,
    );

    assert_eq!(text.matches("// accessor comment").count(), 1);
    let comment = text.find("// accessor comment").unwrap();
    let backing = text.find("static #value_accessor_storage").unwrap();
    let getter = text.find("static get value()").unwrap();
    assert!(
        backing < comment && comment < getter,
        "unexpected comment owner:\n{text}"
    );
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
fn remove_comments_keeps_only_the_top_detached_pinned_group() {
    let output = transform_and_print_canonical_without_comments_at_target(
        concat!(
            "/* ordinary top comment */\n",
            "/*! keep top pinned comment */\n",
            "\n",
            "var x = 10;\n",
            "\n",
            "/*! remove non-top pinned comment */\n",
            "\n",
            "var y = 20;\n",
        ),
        ScriptTarget::ES2015,
    );

    let pinned = output
        .find("/*! keep top pinned comment */")
        .expect("top detached pinned comment");
    let first_statement = output.find("var x = 10;").expect("first statement");
    assert!(pinned < first_statement, "{output}");
    assert_eq!(output.matches("/*! keep top pinned comment */").count(), 1);
    assert!(!output.contains("ordinary top comment"), "{output}");
    assert!(
        !output.contains("remove non-top pinned comment"),
        "{output}"
    );
}

#[test]
fn erased_source_leader_preserves_only_recognized_triple_slash_comments() {
    let output = transform_and_print_at_target(
        concat!(
            "/// <reference path=\"/.lib/react.d.ts\" />\n",
            "// belongs to erased syntax\n",
            "interface A { value: string }\n",
            "interface B { value: number }\n",
            "const value = 1;\n",
        ),
        ScriptTarget::ES2015,
    );
    let reference = output
        .find("/// <reference path=\"/.lib/react.d.ts\" />")
        .expect("recognized triple-slash pragma");
    let value = output.find("const value = 1;").expect("runtime statement");
    assert!(reference < value, "{output}");
    assert!(!output.contains("belongs to erased syntax"), "{output}");

    let type_only_output = transform_and_print_at_target(
        concat!(
            "/// <reference lib=\"dom\" />\n",
            "\n",
            "interface Thenable<T> { then(value: T): void }\n",
            "type AwaitedValue = Awaited<Thenable<string>>;\n",
        ),
        ScriptTarget::ES2015,
    );
    assert_eq!(
        type_only_output
            .matches("/// <reference lib=\"dom\" />")
            .count(),
        1,
        "{type_only_output}"
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
fn erased_class_member_keeps_its_same_line_comment_out_of_the_next_member() {
    let source_text = concat!(
        "// this should be an error\n",
        "class C {\n",
        "    public x = null;// error at x\n",
        "    public x1: string  // belongs to erased x1\n",
        "\n",
        "    constructor(c1, c2, c3: string) { }  // constructor comment\n",
        "    funcOfC(f1, f2, f3: number) { }     // method comment\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "elided-class-member-comment.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.use_define_for_class_fields = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("ES2015 erased class-member transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print erased class-member comments")
        .text()
        .to_owned();

    assert!(!text.contains("belongs to erased x1"), "{text}");
    assert_eq!(text.matches("// error at x").count(), 1, "{text}");
    assert_eq!(text.matches("// constructor comment").count(), 1, "{text}");
    assert_eq!(text.matches("// method comment").count(), 1, "{text}");
}

#[test]
fn class_field_key_evaluation_drains_into_the_next_computed_member() {
    let source_text = concat!(
        "class A {\n",
        "    static readonly p1 = \"x\";\n",
        "    static readonly p2 = \"y\";\n",
        "    static readonly [A.p1] = 0;\n",
        "    static [A.p2]() { return 0 }\n",
        "    [A.p1]() { }\n",
        "    [A.p2] = 0;\n",
        "}\n",
    );
    let parsed = parse_source_file(
        "computed-field-order.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.always_strict = Some(false);
    options.use_define_for_class_fields = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2015 computed class-field transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print computed class-field ordering")
        .text()
        .to_owned();

    assert!(
        text.contains("static [(_a = A.p1, A.p2)]() { return 0; }"),
        "pending static-field key was not attached to the next computed member:\n{text}"
    );
    assert!(text.contains("\n_b = A.p2;\n"), "{text}");
    assert!(!text.contains("_a = A.p1, _b = A.p2"), "{text}");
    let method = text.find("static [(_a = A.p1, A.p2)]").unwrap();
    let instance_key = text.find("_b = A.p2;").unwrap();
    let first_static_initializer = text.find("A.p1 = \"x\";").unwrap();
    assert!(
        method < instance_key && instance_key < first_static_initializer,
        "{text}"
    );
}

#[test]
fn case_block_end_comments_survive_class_field_relocation() {
    let source_text = concat!(
        "class Example {\n",
        "    insideClass = function (value) {\n",
        "        switch (value) {\n",
        "            case 1:\n",
        "                return \"1\";\n",
        "            // forgot inner case\n",
        "        }\n",
        "    };\n",
        "}\n",
        "const outsideClass = function (value) {\n",
        "    switch (value) {\n",
        "        case 1:\n",
        "            return \"1\";\n",
        "        // forgot outer case\n",
        "    }\n",
        "};\n",
    );
    let parsed = parse_source_file(
        "class-field-case-comment.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let resolver = NoConstantValueResolver;
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.always_strict = Some(false);
    options.use_define_for_class_fields = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &resolver).unwrap(),
        false,
    )
    .expect("ES2015 class-field comment transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print class-field case comments")
        .text()
        .to_owned();

    assert_eq!(text.matches("// forgot inner case").count(), 1, "{text}");
    assert_eq!(text.matches("// forgot outer case").count(), 1, "{text}");
    assert!(
        text.find("this.insideClass = function")
            .is_some_and(|assignment| assignment < text.find("// forgot inner case").unwrap()),
        "{text}"
    );
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
fn private_field_destructuring_targets_use_setter_wrappers() {
    let parsed = parse_source_file(
        "private-target.ts",
        concat!(
            "class A {\n",
            "    readonly #x: string;\n",
            "    constructor(arg: { key: string }, public exposed: number) {\n",
            "        ({ key: this.#x } = arg);\n",
            "    }\n",
            "}\n",
        ),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let mut options = bootstrap_options();
    options.target = Some(ScriptTarget::ES2015.bits());
    options.use_define_for_class_fields = Some(false);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &NoConstantValueResolver).unwrap(),
        false,
    )
    .expect("private destructuring target transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("print private destructuring target")
        .text()
        .to_owned();

    assert!(text.contains("var __classPrivateFieldSet ="), "{text}");
    let generated_declaration = text.find("var _a;").expect("receiver declaration");
    let parameter_property = text
        .find("this.exposed = exposed;")
        .expect("parameter-property assignment");
    let private_initializer = text
        .find("_A_x.set(this, void 0);")
        .expect("private-field initializer");
    assert!(
        generated_declaration < parameter_property && parameter_property < private_initializer,
        "constructor prelude and initializers are out of order:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "(_a = this, { key: ({ set value(_b) { ",
            "__classPrivateFieldSet(_a, _A_x, _b, \"f\"); } }).value } = arg);",
        )),
        "{text}",
    );
    assert!(
        !text.contains("{ key: __classPrivateFieldGet("),
        "a destructuring write target was lowered as a read:\n{text}",
    );
}

#[test]
fn private_method_updates_preserve_tsc_value_and_comment_boundaries() {
    let text = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    #m() {}\n",
            "    update(b: any) {\n",
            "        this.#m = () => {} // assignment tail\n",
            "        ({ x: this.#m } = { x: () => {} });\n",
            "        b.#m++ // update tail\n",
            "        return b.#m++;\n",
            "    }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(text.contains("var _a, _b, _c, _d, _e, _f;"), "{text}");
    assert!(
        text.contains(concat!(
            "__classPrivateFieldSet(this, _A_instances, () => { }, \"m\"); ",
            "// assignment tail",
        )),
        "the source assignment must own its trailing comment:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "(_a = this, { x: ({ set value(_b) { ",
            "__classPrivateFieldSet(_a, _A_instances, _b, \"m\"); } }).value }",
        )),
        "an earlier nested setter may shadow outer temps allocated later:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "__classPrivateFieldSet(_b = b, _A_instances, ",
            "(_c = __classPrivateFieldGet(_b, _A_instances, \"m\", _A_m), ",
            "_c++, _c), \"m\"); // update tail",
        )),
        "a discarded postfix update must set the incremented value once:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "return __classPrivateFieldSet(_d = b, _A_instances, ",
            "(_f = __classPrivateFieldGet(_d, _A_instances, \"m\", _A_m), ",
            "_e = _f++, _f), \"m\"), _e;",
        )),
        "a value-producing postfix update must return the pre-update value:\n{text}",
    );
    assert_eq!(text.matches("// assignment tail").count(), 1, "{text}");
    assert_eq!(text.matches("// update tail").count(), 1, "{text}");
}

#[test]
fn private_postfix_update_value_is_grouped_in_a_variable_initializer() {
    let text = transform_and_print_at_target(
        concat!(
            "class C {\n",
            "    static #value = 0;\n",
            "    readThenIncrement() { const before = C.#value++; return before; }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    let declaration_start = text
        .find("const before =")
        .unwrap_or_else(|| panic!("missing transformed declaration:\n{text}"));
    let declaration_end = text[declaration_start..]
        .find("; return before;")
        .map(|offset| declaration_start + offset + 1)
        .unwrap_or_else(|| panic!("missing declaration boundary:\n{text}"));
    let declaration = &text[declaration_start..declaration_end];
    assert!(
        declaration.starts_with("const before = (") && declaration.ends_with(");"),
        "the comma sequence must remain one variable initializer:\n{text}",
    );
}

#[test]
fn private_method_calls_and_tags_preserve_receiver_binding() {
    let text = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    #m(...args: any[]) {}\n",
            "    test(xs: any[]) {\n",
            "        this.#m`simple`;\n",
            "        this.getInstance().#m`complex${1}`;\n",
            "        this.getInstance().#m(...xs);\n",
            "        new (this.getInstance().#m)(...xs);\n",
            "    }\n",
            "    getInstance() { return this; }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(text.contains("var _a, _b;"), "{text}");
    assert!(
        text.contains(
            "__classPrivateFieldGet(this, _A_instances, \"m\", _A_m).bind(this) `simple`;"
        ),
        "a private tag must bind its simple receiver:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "__classPrivateFieldGet((_a = this.getInstance()), _A_instances, ",
            "\"m\", _A_m).bind(_a) `complex${1}`;",
        )),
        "a private tag must evaluate its complex receiver once:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "__classPrivateFieldGet((_b = this.getInstance()), _A_instances, ",
            "\"m\", _A_m).call(_b, ...xs);",
        )),
        "a private call must share its captured receiver with `.call`:\n{text}",
    );
    assert!(
        text.contains(concat!(
            "new (__classPrivateFieldGet(this.getInstance(), _A_instances, ",
            "\"m\", _A_m))(...xs);",
        )),
        "construction must not introduce call receiver binding:\n{text}",
    );
}

#[test]
fn private_static_optional_call_preserves_the_receiver_binding() {
    let text = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    static #f = function () {};\n",
            "    test() { A.#f?.(); }\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains(concat!(
            "(_b = __classPrivateFieldGet(A, _a, \"f\", _A_f)) === null || ",
            "_b === void 0 ? void 0 : _b.call(A);",
        )),
        "an optional private call must test the private value, then call it with the receiver:\n{text}",
    );
    assert!(
        !text.contains(")).call) === null"),
        "optionality must not move to the synthesized `.call` function:\n{text}",
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
fn static_super_assignment_value_stays_grouped_in_a_define_property_initializer() {
    let output = transform_and_print_at_target(
        concat!(
            "class C extends B {\n",
            "    static value = super.a = 0;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains(concat!(
            "    value: (Reflect.set(_b, \"a\", _c = 0, _a), _c)\n",
        )),
        "a comma expression is one descriptor value, not an adjacent object property:\n{output}",
    );
}

#[test]
fn es2018_visits_a_synthetic_descriptor_that_contains_object_rest_in_its_value() {
    let output = transform_and_print_at_target(
        concat!(
            "class C extends B {\n",
            "    static value = { ...super.a } = { x: 0 };\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains(concat!(
            "Object.defineProperty(C, \"value\", Object.assign({ ",
            "enumerable: true, configurable: true, writable: true, ",
            "value: (_a = { x: 0 }, "
        )),
        "a synthesized descriptor inherits the object-rest subtree flag and follows tsc's one-chunk Object.assign path:\n{output}",
    );
    assert!(
        output.contains(concat!("}).value = __rest(_a, []), _a) }));\n",)),
        "the lowered rest assignment must remain the descriptor's grouped value:\n{output}",
    );
}

#[test]
fn unresolved_private_method_access_uses_tsc_diagnostic_recovery_output() {
    let text = transform_and_print_at_target(
        concat!(
            "class A {\n",
            "    #method() { return \"\"; }\n",
            "    inside() { return this.#method(); }\n",
            "}\n",
            "new A().#method();\n",
            "function outside() { return new A().#method(); }\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        text.contains(
            "return __classPrivateFieldGet(this, _A_instances, \"m\", _A_method).call(this);"
        ),
        "declared private access must retain the required slot invariant:\n{text}",
    );
    assert_eq!(
        text.matches("new A().();").count(),
        2,
        "unknown private names must follow tsc's empty-identifier recovery:\n{text}",
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
fn es2015_static_auto_accessor_redirectors_use_the_class_constructor_identity() {
    let output = transform_and_print_at_target(
        "class C { accessor item = 1; static accessor value = 2; }\n",
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains(concat!(
            "get item() { return __classPrivateFieldGet(this, ",
            "_C_item_accessor_storage, \"f\"); }"
        )),
        "instance redirectors retain their dynamic receiver:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "static get value() { return __classPrivateFieldGet(_a, _a, \"f\", ",
            "_C_value_accessor_storage); }"
        )),
        "static getter must use the class constructor as receiver and brand:\n{output}",
    );
    assert!(
        output.contains(concat!(
            "static set value(value) { __classPrivateFieldSet(_a, _a, value, \"f\", ",
            "_C_value_accessor_storage); }"
        )),
        "static setter must use the class constructor as receiver and brand:\n{output}",
    );
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

#[test]
fn auto_accessor_backing_names_follow_member_identity_in_runtime_scope() {
    let output = transform_and_print_at_target(
        concat!(
            "class C1 {\n",
            "    accessor \"w\": any;\n",
            "    accessor 0: any;\n",
            "    static accessor [\"x\"]: any;\n",
            "}\n",
            "declare const key: any;\n",
            "class C2 { accessor [key]: any; }\n",
            "class C3 { accessor a: any; }\n",
            "class C4 { accessor #a: any; }\n",
        ),
        ScriptTarget::ES2015,
    );

    for storage in [
        "_C1__a_accessor_storage",
        "_C1__b_accessor_storage",
        "_C1__c_accessor_storage",
        "_C2__d_accessor_storage",
        "_C3_a_accessor_storage",
        "_C4_a_1_accessor_storage",
    ] {
        assert!(output.contains(storage), "missing {storage}:\n{output}");
    }
    assert!(!output.contains("_accessor_accessor_storage"), "{output}");
}

#[test]
fn computed_auto_accessor_pairs_share_one_class_definition_key_evaluation() {
    let output = transform_and_print_at_target(
        concat!(
            "class C1 {\n",
            "    accessor [\"w\"]: any;\n",
            "    accessor [\"x\"] = 1;\n",
            "    static accessor [\"y\"]: any;\n",
            "    static accessor [\"z\"] = 2;\n",
            "}\n",
            "declare var f: any;\n",
            "class C2 {\n",
            "    accessor [f()] = 1;\n",
            "}\n",
        ),
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains(concat!(
            "get [(_C1__a_accessor_storage = new WeakMap(), ",
            "_C1__b_accessor_storage = new WeakMap(), \"w\")]()",
        )),
        "{output}",
    );
    assert!(output.contains("_b = f())]()"), "{output}",);
    assert!(output.contains("set [_b](value)"), "{output}");
    assert!(output.contains("_b;\nclass C1"), "{output}");
    assert_eq!(output.matches("f()").count(), 1, "{output}");
}

#[test]
fn generated_binding_in_concise_arrow_keeps_synthetic_body_single_line() {
    let output = transform_and_print_at_target(
        "const Printable = (base: any) => class extends base { static message = 'hello'; };\n",
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("const Printable = (base) => { var _a; return"),
        "{output}",
    );
    assert!(
        !output.contains("const Printable = (base) => {\n"),
        "{output}"
    );
}

#[test]
fn parsed_arrow_directive_body_remains_multiline_after_type_erasure() {
    let output = transform_and_print_at_target(
        "const value = { charAt: (x: number) => { '' } };\n",
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("charAt: (x) => {\n        '';\n    }"),
        "{output}",
    );
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

#[test]
fn object_rest_binding_pattern_comments_keep_declaration_indentation() {
    let output = transform_and_print_canonical_at_target(
        concat!(
            "const getType = <P>(params: P) => {\n",
            "    const {\n",
            "        // Omit\n",
            "        foo,\n",
            "\n",
            "        ...rest\n",
            "    } = params;\n",
            "    return rest;\n",
            "};\n",
        ),
        ScriptTarget::ES2015,
    );
    let body = output
        .find("const getType")
        .map(|start| &output[start..])
        .expect("transformed object-rest declaration");
    assert_eq!(
        body,
        concat!(
            "const getType = (params) => {\n",
            "    const { \n",
            "    // Omit\n",
            "    foo } = params, rest = __rest(params, [\"foo\"]);\n",
            "    return rest;\n",
            "};\n",
        ),
    );
}

#[test]
fn array_literal_elements_preserve_parsed_sibling_line_breaks() {
    assert_eq!(
        transform_and_print_canonical_at_target(
            concat!(
                "function collect<T>(a: T, b: T, c: T, d: T): T[] {\n",
                "    return [a, b,\n",
                "        c, d];\n",
                "}\n",
            ),
            ScriptTarget::ES2015,
        ),
        concat!(
            "function collect(a, b, c, d) {\n",
            "    return [a, b,\n",
            "        c, d];\n",
            "}\n",
        ),
    );
}

#[test]
fn es2017_hoists_for_await_names_derived_from_a_simple_async_parameter() {
    let output = transform_and_print_canonical_at_target(
        "async function f(c: any) { for await (const x of c) {} }\n",
        ScriptTarget::ES2015,
    );

    assert!(
        output.contains("var _a, c_1, c_1_1; var _b, e_1, _c, _d;"),
        "source-derived iterator names must join the __awaiter generator's hoisted scope:\n{output}",
    );
    assert!(
        output.contains("for (_a = true, c_1 = __asyncValues(c); c_1_1 = yield c_1.next()"),
        "the rewritten loop head must assign the hoisted names:\n{output}",
    );
    assert!(
        !output.contains("for (var _a = true"),
        "a generated name derived from parameter c collides semantically and cannot stay local",
    );
}
