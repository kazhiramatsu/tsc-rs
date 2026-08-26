use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, CommentRange, EmitConstantValue,
    EmitEnumMemberValue, EmitExportContainerMode, EmitMetadata, EmitResolver, EmitResolverError,
    EmitResolverNode, NewLineKind, PrintRequest, PrinterOptions, SourceByteRange,
    SourceFileTextMode, SourceMapRange, SourceRange, TransformArena, TransformRoot,
};
use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions};
use tsc_types::{CompilerOptions, ModuleKind, ScriptTarget};

fn canonical_identity(
    file_name: &str,
    source_text: &str,
    variant: LanguageVariant,
    remove_comments: bool,
) -> String {
    let parsed = parse_source_file(
        file_name,
        source_text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: variant,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("canonical identity print")
    .text()
    .to_owned()
}

fn print_at_target(source_text: &str, target: ScriptTarget) -> String {
    let parsed = parse_source_file(
        "target.ts",
        source_text,
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(target.bits()),
        // Keep this fixture on the ordinary explicit-format transformer
        // path. Leaving the module kind implicit correctly requires an emit
        // host, which is outside this cursor contract's scope.
        module: Some(ModuleKind::PRESERVE.bits()),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let transformers =
        get_script_transformers(&options, &NoConstantValueResolver).expect("target transformers");
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("target transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(target)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("target print")
    .text()
    .to_owned()
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

const COMMENTED_MODULES: &str = concat!(
    "import /* 取込KW😀 */ { foo /* 取込別称🍀 */ as bar } /* 取込FROM🌊 */ from /* 取込SPEC🧭 */ \"./dep.js\";\n",
    "export /* 公開KW🔥 */ { bar /* 公開別称🎋 */ as baz } /* 公開FROM🌙 */ from /* 公開SPEC🪐 */ \"./dep.js\";\n",
);

const MODULE_COMMENT_MARKERS: [&str; 8] = [
    "取込KW😀",
    "取込別称🍀",
    "取込FROM🌊",
    "取込SPEC🧭",
    "公開KW🔥",
    "公開別称🎋",
    "公開FROM🌙",
    "公開SPEC🪐",
];

#[test]
fn module_token_positions_preserve_unicode_comments_once() {
    let output = canonical_identity(
        "module.ts",
        COMMENTED_MODULES,
        LanguageVariant::Standard,
        false,
    );

    for marker in MODULE_COMMENT_MARKERS {
        let rendered_comment = format!("/* {marker} */");
        assert_eq!(
            output.matches(&rendered_comment).count(),
            1,
            "comment ownership changed for {marker}:\n{output}",
        );
    }
    assert_eq!(output, COMMENTED_MODULES);
}

#[test]
fn remove_comments_uses_the_same_position_cursor_without_comment_output() {
    let output = canonical_identity(
        "module.ts",
        COMMENTED_MODULES,
        LanguageVariant::Standard,
        true,
    );

    for marker in MODULE_COMMENT_MARKERS {
        assert!(!output.contains(marker), "{output}");
    }
    assert!(
        output.contains("import { foo as bar } from \"./dep.js\";"),
        "{output}"
    );
    assert!(
        output.contains("export { bar as baz } from \"./dep.js\";"),
        "{output}"
    );
}

#[test]
fn optional_catch_binding_advances_from_positions_without_token_search() {
    let output = print_at_target(
        "try { work(); } catch /* 本体前😀 */ { /* 本体内 */ recover(); }\n",
        ScriptTarget::ES2018,
    );

    assert_eq!(output.matches("本体前😀").count(), 1, "{output}");
    assert_eq!(output.matches("本体内").count(), 2, "{output}");
    assert!(
        output.contains("catch /* 本体前😀 */ ( /* 本体内 */_a)"),
        "{output}"
    );
}

#[test]
fn jsx_brace_positions_do_not_duplicate_unicode_comments() {
    let output = canonical_identity(
        "component.tsx",
        "const element = <div>{/* 空😀 */}{value/* 尾 */}</div>;\n",
        LanguageVariant::Jsx,
        false,
    );

    assert_eq!(output.matches("空😀").count(), 1, "{output}");
    assert_eq!(output.matches("尾").count(), 1, "{output}");
}

#[test]
fn detached_prefix_resumes_before_the_first_attached_comment() {
    let output = canonical_identity(
        "comments.ts",
        "// detached\n\n// attached\nconst value = 1;\n",
        LanguageVariant::Standard,
        false,
    );

    assert_eq!(output.matches("// detached").count(), 1, "{output}");
    assert_eq!(output.matches("// attached").count(), 1, "{output}");
    let detached = output.find("// detached").expect("detached comment");
    let attached = output.find("// attached").expect("attached comment");
    let statement = output.find("const value").expect("statement");
    assert!(detached < attached && attached < statement, "{output}");
}

#[test]
fn detached_prefix_resume_handles_crlf_and_jsdoc_boundaries() {
    let output = canonical_identity(
        "comments.ts",
        "// license\r\n\r\n/** attached */\r\nconst value = 1;\r\n",
        LanguageVariant::Standard,
        false,
    );

    assert_eq!(output.matches("// license").count(), 1, "{output}");
    assert_eq!(output.matches("/** attached */").count(), 1, "{output}");
    let license = output.find("// license").expect("license comment");
    let attached = output.find("/** attached */").expect("attached JSDoc");
    let statement = output.find("const value").expect("statement");
    assert!(license < attached && attached < statement, "{output}");
}

#[test]
fn transformed_child_does_not_claim_its_original_declarations_trailing_comment() {
    let output = print_at_target("enum E { A } // tail\n", ScriptTarget::ES2015);

    assert_eq!(output.matches("// tail").count(), 1, "{output}");
    let transformed_enum_end = output.rfind("})(E").expect("lowered enum closure");
    let trailing_comment = output.find("// tail").expect("trailing comment");
    assert!(transformed_enum_end < trailing_comment, "{output}");
    assert!(!output.contains("var E // tail"), "{output}");
}

#[test]
fn comment_and_source_map_ranges_are_independent_metadata() {
    let parsed = parse_source_file(
        "ranges.ts",
        "const value = 1;\n",
        ParseOptions::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let comment_bytes = SourceByteRange::new(0, 5, parsed.positions()).expect("source range");
    let comment_range = CommentRange::new(source, SourceRange::Original(comment_bytes));
    let source_map_range = SourceMapRange::new(source, SourceRange::Synthesized);
    let mut metadata = EmitMetadata::default();

    metadata.set_source_map_range(source_map_range);
    assert_eq!(metadata.source_map_range(), Some(source_map_range));
    assert_eq!(metadata.comment_range(), None);

    metadata.set_comment_range(comment_range);
    assert_eq!(metadata.comment_range(), Some(comment_range));
    assert_eq!(metadata.source_map_range(), Some(source_map_range));
}

#[test]
fn for_headers_advance_across_every_internal_comment_boundary() {
    let source = concat!(
        "/*0*/ for /*1*/ ( /*2*/ var /*3*/ x /*4*/ in /*5*/ a /*6*/) /*7*/ {}\n",
        "/*0*/ for /*1*/ ( /*2*/ var /*3*/ y /*4*/ of /*5*/ a /*6*/) /*7*/ {}\n",
        "/*0*/ for /*1*/ ( /*2*/ x /*3*/ in /*4*/ a /*5*/) /*6*/ {}\n",
        "/*0*/ for /*1*/ ( /*2*/ y /*3*/ of /*4*/ a /*5*/) /*6*/ {}\n",
        "/*0*/ for /*1*/ ( /*2*/ a /*3*/ ; /*4*/ a /*5*/ ; /*6*/ a /*7*/) /*8*/ {}\n",
        "/*0*/ for /*1*/ ( /*2*/ ; /*3*/ ; /*4*/ ) /*5*/ {}\n",
    );
    let expected = concat!(
        "/*0*/ for /*1*/ ( /*2*/var /*3*/ x /*4*/ in /*5*/ a /*6*/) /*7*/ { }\n",
        "/*0*/ for /*1*/ ( /*2*/var /*3*/ y /*4*/ of /*5*/ a /*6*/) /*7*/ { }\n",
        "/*0*/ for /*1*/ ( /*2*/x /*3*/ in /*4*/ a /*5*/) /*6*/ { }\n",
        "/*0*/ for /*1*/ ( /*2*/y /*3*/ of /*4*/ a /*5*/) /*6*/ { }\n",
        "/*0*/ for /*1*/ ( /*2*/a /*3*/; /*4*/ a /*5*/; /*6*/ a /*7*/) /*8*/ { }\n",
        "/*0*/ for /*1*/ ( /*2*/; /*3*/; /*4*/) /*5*/ { }\n",
    );

    let output = print_at_target(source, ScriptTarget::ES2015);

    assert_eq!(output, expected);
}

#[test]
fn keyword_expression_tokens_resume_into_their_operands() {
    let source = concat!(
        "/*1*/ new /*new operand*/ Array /*3*/;\n",
        "/*1*/ typeof /*typeof operand*/ Array /*3*/;\n",
        "/*1*/ void /*void operand*/ Array /*3*/;\n",
        "/*1*/ delete /*delete operand*/ Array.toString /*3*/;\n",
        "async function awaitValue() { await /*await operand*/ Array /*3*/; }\n",
    );

    let output = canonical_identity(
        "keyword-expression.ts",
        source,
        LanguageVariant::Standard,
        false,
    );

    for marker in [
        "new operand",
        "typeof operand",
        "void operand",
        "delete operand",
        "await operand",
    ] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
    assert_eq!(output, source);
}

#[test]
fn await_keyword_owns_the_frozen_inner_comment_boundary() {
    let source = concat!(
        "async function foo() {\n",
        "    /*comment1*/ await 1;\n",
        "    await /*comment2*/ 2;\n",
        "    await 3 /*comment3*/\n",
        "}\n",
    );
    let output = canonical_identity(
        "awaitExpressionInnerCommentEmit.ts",
        source,
        LanguageVariant::Standard,
        false,
    );

    assert_eq!(
        output,
        concat!(
            "async function foo() {\n",
            "    /*comment1*/ await 1;\n",
            "    await /*comment2*/ 2;\n",
            "    await 3; /*comment3*/\n",
            "}\n",
        )
    );
    for marker in ["comment1", "comment2", "comment3"] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
}

#[test]
fn nested_await_comments_resume_once_and_remove_cleanly() {
    let source = "async function f() { await /*outer*/ await /*inner*/ 0; }\n";
    let output = canonical_identity("nested-await.ts", source, LanguageVariant::Standard, false);

    assert_eq!(output, source);
    for marker in ["outer", "inner"] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
    assert_eq!(
        canonical_identity("nested-await.ts", source, LanguageVariant::Standard, true,),
        "async function f() { await await 0; }\n"
    );
}

#[test]
fn return_and_throw_tokens_resume_comments_into_their_expressions() {
    let source = concat!(
        "function value() { return /* @type {number} */ 42; }\n",
        "function fail() { throw /* reason */ Error(); }\n",
    );

    let output = canonical_identity(
        "keyword-statements.js",
        source,
        LanguageVariant::Standard,
        false,
    );

    assert_eq!(output.matches("@type {number}").count(), 1, "{output}");
    assert_eq!(output.matches("reason").count(), 1, "{output}");
    assert!(
        output.contains("return /* @type {number} */ 42;"),
        "{output}"
    );
    assert!(output.contains("throw /* reason */ Error();"), "{output}");
}

#[test]
fn loop_and_with_headers_advance_across_internal_comments() {
    let source = concat!(
        "/*a*/ while /*b*/ ( /*c*/false /*d*/) /*e*/ {}\n",
        "/*a*/ do /*b*/ {} /*c*/ while /*d*/ ( /*e*/true /*f*/);\n",
        "// @ts-ignore\n",
        "/*1*/ with /*2*/ ( /*3*/false /*4*/) /*5*/ {}\n",
    );
    let expected = concat!(
        "/*a*/ while /*b*/ ( /*c*/false /*d*/) /*e*/ { }\n",
        "/*a*/ do /*b*/ { } /*c*/ while /*d*/ ( /*e*/true /*f*/);\n",
        "// @ts-ignore\n",
        "/*1*/ with /*2*/ ( /*3*/false /*4*/) /*5*/ { }\n",
    );

    let output = canonical_identity(
        "statement-headers.ts",
        source,
        LanguageVariant::Standard,
        false,
    );

    assert_eq!(output, expected);
}

#[test]
fn yield_and_yield_star_resume_comments_into_their_operands() {
    let source = concat!(
        "function* generator() {\n",
        "    yield /*yield operand*/ 1;\n",
        "    yield* /*yield star operand*/ [2];\n",
        "    yield /*before star*/* [3];\n",
        "}\n",
    );

    let output = canonical_identity(
        "yield-comments.ts",
        source,
        LanguageVariant::Standard,
        false,
    );

    for marker in ["yield operand", "yield star operand", "before star"] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
    assert!(output.contains("yield /*yield operand*/ 1;"), "{output}");
    assert!(
        output.contains("yield* /*yield star operand*/ [2];"),
        "{output}"
    );
    assert!(output.contains("yield /*before star*/* [3];"), "{output}");
}

#[test]
fn chained_access_keeps_a_line_comment_with_the_preceding_call() {
    let source = concat!(
        "const value = first.call()\n",
        "    .second() // belongs to second\n",
        "    .third();\n",
    );

    let output = canonical_identity("chain.ts", source, LanguageVariant::Standard, false);

    assert_eq!(output, source);
}

#[test]
fn binary_separators_handoff_comments_once_across_layouts_and_in_keyword() {
    let source = concat!(
        "const sum = left/*binary left*/ + /*binary right*/right;\n",
        "const membership = key/*in left*/ in /*in right*/object;\n",
        "const broken = left // before line operator\n",
        "    + /*after line operator*/right;\n",
    );

    let output = canonical_identity(
        "binary-comments.ts",
        source,
        LanguageVariant::Standard,
        false,
    );

    for marker in [
        "binary left",
        "binary right",
        "in left",
        "in right",
        "before line operator",
        "after line operator",
    ] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
    assert!(output.contains(" in "), "{output}");
}

#[test]
fn optional_chain_lowering_repeats_only_source_trailing_receiver_comments() {
    let source = concat!(
        "/*optional root a*/Array/*optional edge a*/?./*optional name a*/toString/*optional tail a*/\n\n",
        "/*optional root b*/Array\n",
        "/*optional edge b*/?./*optional name b*/\n",
        "    // optional member b\n",
        "    toString/*optional tail b*/\n\n",
        "/*optional root c*/Array/*optional edge c*/?./*optional name c*/\n",
        "    // optional member c\n",
        "    toString/*optional tail c*/\n\n",
        "/*optional root d*/Array\n",
        "    // optional receiver d\n",
        "    /*optional edge d*/?./*optional name d*/toString/*optional tail d*/\n",
    );

    let output = print_at_target(source, ScriptTarget::ES2015);

    // The vendored printer's node-trailing phase runs for every reuse of a
    // parsed receiver. A comment after a source line break is instead owned
    // by the retained property token and is emitted only on that branch.
    for (marker, expected) in [
        ("optional edge a", 3),
        ("optional edge b", 1),
        ("optional edge c", 3),
        ("optional edge d", 1),
    ] {
        assert_eq!(
            output.matches(marker).count(),
            expected,
            "{marker}:\n{output}"
        );
    }
    for marker in [
        "optional root a",
        "optional root b",
        "optional root c",
        "optional root d",
        "optional tail a",
        "optional tail b",
        "optional tail c",
        "optional tail d",
        "optional member b",
        "optional member c",
        "optional receiver d",
    ] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
    // This is the fixture's documented upstream behavior: lowering writes a
    // one-byte `.` over the two-byte source `?.`, so the following comments
    // are not owned by the generated property token.
    for marker in [
        "optional name a",
        "optional name b",
        "optional name c",
        "optional name d",
    ] {
        assert!(!output.contains(marker), "{marker}:\n{output}");
    }
}

#[test]
fn nullish_lowering_reuses_the_binary_child_completion_contract() {
    let output = print_at_target(
        "/*nullish root*/value/*nullish left*/ ?? /*nullish right*/fallback;\n",
        ScriptTarget::ES2015,
    );

    let question = output.find('?').expect("lowered conditional operator");
    let condition = &output[..question];
    assert_eq!(condition.matches("nullish left").count(), 2, "{output}");
    assert_eq!(output.matches("nullish root").count(), 1, "{output}");
}

#[test]
fn property_token_handoff_remains_non_duplicating() {
    let output = canonical_identity(
        "property-comments.ts",
        "const access = Array/*property edge*/./*property name*/toString;\n",
        LanguageVariant::Standard,
        false,
    );

    for marker in ["property edge", "property name"] {
        assert_eq!(output.matches(marker).count(), 1, "{marker}:\n{output}");
    }
}
