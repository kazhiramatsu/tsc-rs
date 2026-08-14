use tsc_types::CompilerOptions;

use super::leading_jsx_pragmas;
use crate::state::test_support::with_program_state;

// `with_program_state` does not load the default library.  JSX child
// cardinality follows tsc's `isArrayOrTupleLikeType`, whose fallback needs the
// real Array/ReadonlyArray globals in order not to classify every object type
// (including call signatures) as array-like.
const JSX_ARRAY_GLOBALS: &str = "interface Array<T> { readonly length: number; [n: number]: T; }\n\
     interface ReadonlyArray<T> { readonly length: number; [n: number]: T; }\n";

/// Driver-level fixture check — oracle-pinned rows (tsc 6.0.3,
/// noLib, .tsx, options per test) — scratchpad j*.tsx probes,
/// 2026-07-13.
fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.tsx", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

fn checked_chain_codes_with(text: &str, options: &CompilerOptions) -> Vec<Vec<u32>> {
    fn flatten(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten(child, codes);
        }
    }

    with_program_state(&[("a.tsx", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file_name.is_some() && diagnostic.code() == 2322)
            .map(|diagnostic| {
                let mut codes = Vec::new();
                flatten(&diagnostic.message, &mut codes);
                codes
            })
            .collect()
    })
}

type DiagnosticChainRows = Vec<(u32, String)>;
type RelatedDiagnosticRows = Vec<(u32, String, u32, u32)>;
type JsxComponentDetails = (DiagnosticChainRows, RelatedDiagnosticRows);

fn checked_jsx_component_details_with(
    text: &str,
    options: &CompilerOptions,
) -> Vec<JsxComponentDetails> {
    fn flatten(chain: &tsc_diagnostics::MessageChain, rows: &mut DiagnosticChainRows) {
        rows.push((chain.code, chain.text.clone()));
        for child in &chain.next {
            flatten(child, rows);
        }
    }

    with_program_state(&[("a.tsx", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file_name.is_some() && diagnostic.code() == 2786)
            .map(|diagnostic| {
                let mut chain = Vec::new();
                flatten(&diagnostic.message, &mut chain);
                let related = diagnostic
                    .related
                    .iter()
                    .map(|related| {
                        (
                            related.message.code,
                            related.message.text.clone(),
                            related.start.unwrap_or(u32::MAX),
                            related.length.unwrap_or(u32::MAX),
                        )
                    })
                    .collect();
                (chain, related)
            })
            .collect()
    })
}

fn jsx(value: i32) -> CompilerOptions {
    CompilerOptions {
        jsx: Some(value),
        ..CompilerOptions::default()
    }
}

#[test]
fn jsx_attribute_elaboration_uses_2322_at_the_attribute_name() {
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element {} interface IntrinsicElements { x: { n: number } } }\n\
             (<x n=\"s\" />);\n\
             (<x q={1} />);\n",
            &jsx(1),
        ),
        [(2322, 100, 1), (2322, 115, 1)]
    );
}

#[test]
fn jsx_excess_properties_are_checked_against_empty_props() {
    let source =
        "declare namespace JSX { interface Element {} interface ElementChildrenAttribute { children: {} } }\n\
         declare function Tag(props: {}): JSX.Element;\n\
         (<Tag children=\"x\" />);\n\
         (<Tag key=\"1\">x</Tag>);\n";
    let rows = checked_rows_with(source, &jsx(1));
    assert_eq!(
        rows.iter()
            .filter(|row| row.0 == 2322)
            .map(|row| (row.0, row.2))
            .collect::<Vec<_>>(),
        [(2322, 8), (2322, 3)],
        "{rows:?}"
    );
    assert_eq!(
        checked_chain_codes_with(source, &jsx(1)),
        [vec![2322, 2339], vec![2322, 2339]]
    );
}

#[test]
fn multiple_jsx_children_elaborate_one_row_per_child() {
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element {} interface ElementChildrenAttribute { children: any } }\n\
             declare function Comp(p: { children: [string, string] }): JSX.Element;\n\
             (<Comp>{1}{2}</Comp>);\n",
            &jsx(1),
        ),
        [(2322, 178, 3), (2322, 181, 3)]
    );
}

#[test]
fn jsx_children_cardinality_selects_the_arity_specific_diagnostics() {
    let source = [
        JSX_ARRAY_GLOBALS,
        "declare namespace JSX { interface Element {} interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { scalar: { children: () => 'ok' }; many: { children: (() => 'ok')[] } } }\n\
         (<scalar>{() => 'ok'}{() => 'ok'}</scalar>);\n\
         (<many>{() => 'ok'}</many>);\n",
    ]
    .concat();
    let scalar_tag = source.find("<scalar>").expect("scalar opening tag") as u32 + 1;
    let many_tag = source.find("<many>").expect("many opening tag") as u32 + 1;
    let rows = checked_rows_with(&source, &jsx(1));
    assert_eq!(
        rows.into_iter()
            .filter(|row| matches!(row.0, 2745 | 2746))
            .collect::<Vec<_>>(),
        [(2746, scalar_tag, 6), (2745, many_tag, 4)]
    );
}

#[test]
fn single_jsx_child_elaborates_an_arrow_return_at_the_inner_expression() {
    let source = [
        JSX_ARRAY_GLOBALS,
        "declare namespace JSX { interface Element {} interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { leaf: { children: (x: number) => 'ok' } } }\n\
         (<leaf>{x => 'bad'}</leaf>);\n",
    ]
    .concat();
    let bad_literal = source.find("'bad'").expect("bad arrow return") as u32;
    let rows = checked_rows_with(&source, &jsx(1));
    assert_eq!(
        rows.into_iter()
            .filter(|row| row.0 == 2322)
            .collect::<Vec<_>>(),
        [(2322, bad_literal, 5)]
    );
}

#[test]
fn single_scalar_child_without_a_children_container_keeps_the_missing_property_head() {
    let source = [
        JSX_ARRAY_GLOBALS,
        "declare namespace JSX { interface Element {} interface IntrinsicElements { leaf: { children: () => string } } }\n\
         (<leaf>{() => 'ok'}</leaf>);\n",
    ]
    .concat();
    let rows = checked_rows_with(&source, &jsx(1));
    let filtered = rows
        .iter()
        .filter(|row| matches!(row.0, 2741 | 2745 | 2746))
        .map(|row| row.0)
        .collect::<Vec<_>>();
    assert_eq!(
        filtered,
        [2741],
        "unexpected full diagnostic rows: {rows:?}"
    );
}

#[test]
fn required_intrinsic_attribute_selects_the_missing_property_head() {
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element {} interface ElementClass { render: any } interface IntrinsicAttributes { key: string } interface IntrinsicClassAttributes<T> { ref: T } interface IntrinsicElements {} }\n\
             interface I { new(n: string): { x: number; render(): void } }\n\
             declare var E: I;\n\
             (<E x={10} />);\n",
            &jsx(1),
        ),
        [(2741, 294, 1)]
    );
}

#[test]
fn jsx_text_inside_a_string_is_not_a_pragma() {
    let rows = checked_rows_with(
        "declare namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } }\n\
         declare var React: any;\n\
         const marker = \"@jsx\";\n\
         (<div id={1} />);\n",
        &jsx(1),
    );
    assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
}

#[test]
fn jsx_pragma_collection_matches_multiline_and_precedence_rules() {
    let pragmas = leading_jsx_pragmas(
        "// @jsx Ignored.h\n\
         /** @jsx First.h */\n\
         /** @jsx Second.h\n\
             @jsxfrag First.Fragment\n\
             @jsxfrag Second.Fragment\n\
             @jsximportsource first\n\
             @jsximportsource second\n\
             @jsxruntime classic\n\
             @jsxruntime automatic */\n\
         const value = 1;\n\
         /** @jsx TooLate.h */",
    );
    assert_eq!(pragmas.factory.as_deref(), Some("First.h"));
    assert_eq!(pragmas.fragment_factory.as_deref(), Some("First.Fragment"));
    assert_eq!(pragmas.import_source.as_deref(), Some("second"));
    assert_eq!(pragmas.runtime.as_deref(), Some("automatic"));
}

#[test]
fn jsx_factory_option_selects_its_namespace() {
    let rows = checked_rows_with(
        "declare namespace Preact { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } function h(): any; }\n\
         (<div id={1} />);\n",
        &CompilerOptions {
            jsx: Some(2),
            jsx_factory: Some("Preact.h".to_owned()),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
    assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
}

#[test]
fn jsx_factory_option_uses_the_escaped_key_for_a_double_underscore_global() {
    let rows = checked_rows_with(
        "declare global {\n\
             function __make(params: object): any;\n\
         }\n\
         declare var __foot: any;\n\
         const thing = <__foot />;\n\
         export {};\n",
        &CompilerOptions {
            jsx: Some(2),
            jsx_factory: Some("__make".to_owned()),
            ..CompilerOptions::default()
        },
    );
    assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
}

#[test]
fn invalid_jsx_factory_option_falls_back_to_react_namespace() {
    let rows = checked_rows_with(
        "declare namespace React { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } }\n\
         declare var React: any;\n\
         (<div id={1} />);\n",
        &CompilerOptions {
            jsx: Some(2),
            jsx_factory: Some("Preact.!".to_owned()),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
    assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
}

#[test]
fn jsx_pragma_selects_its_namespace() {
    let rows = checked_rows_with(
        "/** @jsx Preact.h */\n\
         declare namespace Preact { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } function h(): any; }\n\
         (<div id={1} />);\n",
        &jsx(2),
    );
    assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
    assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
}

#[test]
fn automatic_jsx_runtime_reports_a_missing_runtime_module_once() {
    let rows = checked_rows_with("(<div />);\n(<span />);\n", &jsx(4));
    assert_eq!(
        rows.iter().filter(|row| row.0 == 2875).count(),
        1,
        "{rows:?}"
    );
    assert!(rows.iter().any(|row| row.0 == 7026), "{rows:?}");
}

#[test]
fn automatic_jsx_runtime_uses_exported_jsx_namespace() {
    let rows = checked_rows_with(
        "declare module \"react/jsx-runtime\" {\n\
           export namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } }\n\
         }\n\
         (<div id={1} />);\n",
        &jsx(4),
    );
    assert!(!rows.iter().any(|row| row.0 == 2875), "{rows:?}");
    assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
}

#[test]
fn global_import_equals_jsx_alias_uses_target_namespace_flags() {
    let source = "export {};\n\
                  declare namespace JSXInternal { interface Element {} interface IntrinsicElements { div: {} } }\n\
                  declare global { export import JSX = JSXInternal; }\n\
                  (<div />);\n";
    let rows = checked_rows_with(source, &jsx(1));
    assert!(
        !rows.iter().any(|row| row.0 == 7026),
        "alias-backed global JSX namespace must expose IntrinsicElements: {rows:?}"
    );
}

#[test]
fn namespaced_jsx_attribute_suggestion_uses_symbol_to_string_face() {
    let source = "declare namespace JSX {\n\
                    interface Element {}\n\
                    interface IntrinsicElements { \"ns:element\": { \"ns:attribute\": string } }\n\
                  }\n\
                  declare var React: any;\n\
                  (<ns:element attribute=\"x\" />);\n";
    with_program_state(&[("a.tsx", source)], &jsx(1), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.file_name.is_some() && diagnostic.code() == 2322)
            .expect("namespaced JSX attribute mismatch");
        assert!(
            diagnostic
                .message_text()
                .contains("Did you mean '\"ns:attribute\"'?"),
            "{}",
            diagnostic.message_text()
        );
    });
}

#[test]
fn no_jsx_option_reports_17004_and_empty_initializer_17000() {
    // Oracle (scratchpad j58.tsx probe, 2026-07-14): the FULL
    // 11-row set — 5.8a's checkVariableDeclaration recovered the
    // const-statement rows (7026/17004/17001/2695/18007) that the
    // 5.7c pin recorded as demand-caveat FN (risk §14.9 flip).
    assert_eq!(
        checked_rows_with(
            "declare var React: any;\nconst a = <div id=\"x\" id=\"y\" />;\nconst b = <span>{1, 2}</span>;\n(<p attr={} />);\n",
            &CompilerOptions::default(),
        ),
        [
            (17001, 46, 2),
            (17004, 34, 21),
            (7026, 34, 21),
            (17004, 67, 6),
            (7026, 67, 6),
            (18007, 74, 4),
            (2695, 74, 1),
            (7026, 79, 7),
            (17000, 97, 2),
            (17004, 89, 13),
            (7026, 89, 13),
        ]
    );
}

#[test]
fn duplicate_attribute_reports_17001_in_expression_position() {
    // Oracle: 7026 @25+21 + 17004 @25+21 + 17001 @37+2 — the FULL
    // oracle set (5.7c recovered the 7026 row).
    assert_eq!(
        checked_rows_with(
            "declare var React: any;\n(<div id=\"x\" id=\"y\" />);\n",
            &CompilerOptions::default(),
        ),
        [(17001, 37, 2), (17004, 25, 21), (7026, 25, 21)]
    );
}

#[test]
fn value_tag_without_signatures_reports_2604() {
    // Oracle (jsx: preserve, noLib + hand-declared Function so the
    // isUntypedFunctionCall globalFunctionType probe stays honest —
    // the degenerate noLib Function absorbs every object type):
    // 2604 @147+4 at the tag name.
    assert_eq!(
        checked_rows_with(
            "interface Function { $brand: 1 }\ndeclare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\ndeclare const Comp: { x: number };\n(<Comp />);\n",
            &jsx(1),
        ),
        [(2604, 147, 4)]
    );
}

#[test]
fn intrinsic_type_arguments_report_2558() {
    // Oracle (jsx: preserve): 2558 @136+6 on the typeArguments
    // range (the intrinsic fake signature expects 0).
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: { id?: string } } }\ndeclare var React: any;\n(<div<string> id=\"a\" />);\n",
            &jsx(1),
        ),
        [(2558, 136, 6)]
    );
}

#[test]
fn react_fragment_with_untyped_react_stays_silent() {
    // Oracle (jsx: react): NO rows — React resolves as a value,
    // its exports carry no Fragment, the fragment type is
    // errorType and resolveErrorCall stays silent.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\n(<>x</>);\n",
            &jsx(2),
        ),
        []
    );
}

#[test]
fn react_fragment_without_react_reports_2874_and_2879() {
    // Oracle (jsx: react): 2874 @54+2 (markJsxAliasReferenced's
    // factory probe) + 2879 @54+2 (getJSXFragmentType's resolve).
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } }\n(<>x</>);\n",
            &jsx(2),
        ),
        [(2874, 54, 2), (2879, 54, 2)]
    );
}

#[test]
fn sfc_wrong_return_type_reports_2786() {
    // Oracle (jsx: preserve): 2786 @171+1 at the tag name (chain:
    // "Its return type 'number' is not a valid JSX element").
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } }\ndeclare var React: any;\ndeclare function F(props: { a: string }): number;\n(<F a=\"x\" />);\n",
            &jsx(1),
        ),
        [(2786, 171, 1)]
    );
}

#[test]
fn jsx_component_relation_keeps_missing_property_detail_and_related() {
    let text = "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } }\n\
                declare var React: any;\n\
                declare function F(props: { a: string }): { x: number };\n\
                (<F a=\"x\" />);\n";
    assert_eq!(
        checked_jsx_component_details_with(text, &jsx(1)),
        [(
            vec![
                (
                    2786,
                    "'F' cannot be used as a JSX component.".to_owned()
                ),
                (
                    2787,
                    "Its return type '{ x: number; }' is not a valid JSX element."
                        .to_owned()
                ),
                (
                    2741,
                    "Property 'e' is missing in type '{ x: number; }' but required in type 'Element'."
                        .to_owned()
                ),
            ],
            vec![(
                2728,
                "'e' is declared here.".to_owned(),
                text.find("e: 1").expect("Element.e") as u32,
                1,
            )],
        )]
    );
}

#[test]
fn primitive_jsx_return_does_not_fabricate_a_missing_property_detail() {
    let text = "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } }\n\
                declare var React: any;\n\
                declare function F(props: { a: string }): number;\n\
                (<F a=\"x\" />);\n";
    assert_eq!(
        checked_jsx_component_details_with(text, &jsx(1)),
        [(
            vec![
                (2786, "'F' cannot be used as a JSX component.".to_owned()),
                (
                    2787,
                    "Its return type 'number' is not a valid JSX element.".to_owned()
                ),
            ],
            Vec::new(),
        )]
    );
}

#[test]
fn class_component_wrong_instance_type_reports_2786() {
    // Oracle (jsx: preserve): 2786 @213+1 at the tag name (chain:
    // "Its instance type 'C' is not a valid JSX element") — the
    // Component ref kind + ElementAttributesProperty props path.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } interface ElementAttributesProperty { props: {} } }\ndeclare var React: any;\ndeclare class C { props: { a: string }; }\n(<C a=\"x\" />);\n",
            &jsx(1),
        ),
        [(2786, 213, 1)]
    );
}

#[test]
fn children_specified_twice_reports_2710() {
    // Oracle (jsx: preserve): 2710 @194+12 at the attributes node
    // (explicit `children` attribute + semantic children).
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\">text</div>);\n",
            &jsx(1),
        ),
        [(2710, 194, 12)]
    );
}

#[test]
fn non_object_jsx_spread_reports_2698() {
    // Oracle (jsx: preserve): 2698 @165+1 at the spread expression
    // + 2559 @157+3 at the tag. The 2559 head is the headless
    // reportRelationError face — T2-contained here (recorded FN);
    // the 2698 row emitted by the attributes worker survives the
    // containment.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: { id?: string } } }\ndeclare var React: any;\ndeclare const n: number;\n(<div {...n} />);\n",
            &jsx(1),
        ),
        [(2698, 165, 1)]
    );
}

#[test]
fn unknown_intrinsic_tag_reports_2339_on_opening_and_closing() {
    // Oracle (jsx: preserve): 2339 @118+5 (opening `<foo>`) +
    // 2339 @124+6 (closing `</foo>`) vs JSX.IntrinsicElements.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: {} } }\ndeclare var React: any;\n(<foo>x</foo>);\n",
            &jsx(1),
        ),
        [(2339, 118, 5), (2339, 124, 6)]
    );
}

#[test]
fn inline_whitespace_child_is_semantic_and_fires_2710() {
    // Oracle (jsx: preserve): 2710 @194+12 — `<div children="a"> `
    // has an INLINE-SPACE text child (no line break), which is a
    // SEMANTIC string child (scanJsxToken keeps firstNonWhitespace
    // at 0, NOT -1) — the children synthesis runs and the explicit
    // `children` attribute reports as overwritten.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\"> </div>);\n",
            &jsx(1),
        ),
        [(2710, 194, 12)]
    );
}

#[test]
fn line_break_whitespace_child_is_trivia_and_stays_silent() {
    // Oracle (jsx: preserve): NO rows — the same shape with a
    // LINE-BREAK whitespace child is JsxTextAllWhiteSpaces
    // (non-semantic), so no children synthesis and no 2710.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\">\n</div>);\n",
            &jsx(1),
        ),
        []
    );
}

#[test]
fn aliased_react_jsx_namespace_reports_2339() {
    // Oracle (jsx: preserve): 2339 @143+7 — tsc resolveSymbol()s
    // the `export import JSX = Inner` alias to the real container
    // and reports the unknown intrinsic. LIVE since 5.9d's
    // getSymbol alias arm (previously a recorded FN).
    assert_eq!(
        checked_rows_with(
            "declare namespace React { namespace Inner { interface Element { e: 1 } interface IntrinsicElements { div: {} } } export import JSX = Inner; }\n(<foo />);\n",
            &jsx(1),
        ),
        [(2339, 143, 7)]
    );
}

#[test]
fn factory_arity_reports_6229() {
    // Oracle (jsx: preserve): 6229 @229+4 at the tag name — Comp
    // requires 3 arguments but React.createElement's first-param
    // signatures provide at most 2.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } }\ndeclare namespace React { function createElement(tag: (a: string, b: string) => any, props: any): any; }\ndeclare function Comp(a: string, b: string, c: string): JSX.Element;\n(<Comp />);\n",
            &jsx(1),
        ),
        [(6229, 229, 4)]
    );
}

#[test]
fn class_without_props_property_reports_2607() {
    // Oracle (jsx: preserve): 2607 @159+11 at the opening element —
    // ElementAttributesProperty forces a `props` lookup the class
    // lacks, and attributes are present.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementAttributesProperty { props: {} } }\ndeclare var React: any;\ndeclare class C { m(): void; }\n(<C a=\"x\" />);\n",
            &jsx(1),
        ),
        [(2607, 159, 11)]
    );
}

#[test]
fn multi_property_children_container_reports_2608() {
    // Oracle (jsx: preserve): 2608 @61+24 at the container's NAME
    // (the ElementChildrenAttribute interface declaration).
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {}; kids: {} } interface IntrinsicElements { div: {} } }\ndeclare var React: any;\n(<div>text</div>);\n",
            &jsx(1),
        ),
        [(2608, 61, 24)]
    );
}

#[test]
fn element_type_constraint_reports_2786() {
    // Oracle (jsx: preserve): 2786 @155+4 at the tag name — the
    // JSX.ElementType alias constrains tags to "div"; the "span"
    // string-literal tag type fails (chain: Its type '"span"' is
    // not a valid JSX element type).
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { e: 1 } type ElementType = \"div\"; interface IntrinsicElements { div: {}; span: {} } }\ndeclare var React: any;\n(<span />);\n",
            &jsx(1),
        ),
        [(2786, 155, 4)]
    );
}

#[test]
fn library_managed_attributes_drive_contextual_typing() {
    // Oracle (jsx: preserve): 2339 @237+3 — LibraryManagedAttributes
    // REPLACES the props type, so the callback parameter is
    // contextually typed V and `v.bad` misses. TS6205 covers the
    // independently unused C/P type-parameter list.
    assert_eq!(
        checked_rows_with(
            "interface V { m: number }\ndeclare namespace JSX { interface Element { e: 1 } type LibraryManagedAttributes<C, P> = { cb?: (v: V) => void }; }\ndeclare var React: any;\ndeclare function F(props: { a?: string }): JSX.Element;\n(<F cb={v => v.bad} />);\n",
            &jsx(1),
        ),
        [(2339, 237, 3), (6205, 106, 6)]
    );
}

#[test]
fn jsx_attribute_callback_is_contextually_typed() {
    // Oracle (jsx: preserve): 2339 @183+3 — the attribute value's
    // arrow parameter is contextually typed from the props member
    // (`v: V`), so `v.bad` misses.
    assert_eq!(
        checked_rows_with(
            "interface V { m: number }\ndeclare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\ndeclare function F(props: { cb?: (v: V) => void }): JSX.Element;\n(<F cb={v => v.bad} />);\n",
            &jsx(1),
        ),
        [(2339, 183, 3)]
    );
}

#[test]
fn rejected_jsx_overload_keeps_an_implicit_any_from_its_completed_contextual_check() {
    let source = "declare namespace JSX { interface Element {} }\n\
         interface ButtonProps { onClick: any; }\n\
         interface LinkProps { to: string; }\n\
         declare function MainButton(props: ButtonProps): JSX.Element;\n\
         declare function MainButton(props: LinkProps): JSX.Element;\n\
         (<MainButton to=\"/some/path\" onClick={e => {}} />);\n";
    let tag = source.find("<MainButton").expect("JSX opening tag") as u32 + 1;
    let parameter = source.find("e =>").expect("arrow parameter") as u32;
    let mut rows = checked_rows_with(
        source,
        &CompilerOptions {
            jsx: Some(1),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .filter(|row| matches!(row.0, 2769 | 7006))
    .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.1);
    assert_eq!(rows, [(2769, tag, 10), (7006, parameter, 1)]);
}

#[test]
fn declared_jsx_namespace_with_jsx_option_reports_no_intrinsics_7026() {
    // Oracle (jsx: preserve): 7026 @83+13 + 2339 @117+3 — the FULL
    // oracle set: the namespace resolves (no 17004), the div tag
    // misses JSX.IntrinsicElements (7026), the fragment rides the
    // untyped-call path (anyType at jsx=preserve) cleanly.
    assert_eq!(
        checked_rows_with(
            "declare namespace JSX { interface Element { x: number } }\ndeclare var React: any;\n(<div a=\"1\" />);\n(<>text</>);\n(\"x\".bad);\n",
            &jsx(1),
        ),
        [(2339, 117, 3), (7026, 83, 13)]
    );
}

#[test]
fn deprecated_contextual_jsx_attribute_reports_6385() {
    let text = "declare namespace JSX {\n\
                  interface Element {}\n\
                  interface IntrinsicElements { comp: Props }\n\
                }\n\
                interface Props {\n\
                  /** @deprecated */\n\
                  old?: string;\n\
                }\n\
                declare var React: any;\n\
                const value = <comp old=\"x\" />;\n";
    with_program_state(&[("a.tsx", text)], &jsx(1), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 6385)
            .expect("deprecated JSX attribute suggestion");
        assert_eq!(
            diagnostic.category(),
            tsc_diagnostics::DiagnosticCategory::Suggestion
        );
        assert_eq!(diagnostic.message_text(), "'old' is deprecated.");
        assert_eq!(
            diagnostic
                .related
                .iter()
                .map(|related| related.message.code)
                .collect::<Vec<_>>(),
            [2798]
        );
    });
}
