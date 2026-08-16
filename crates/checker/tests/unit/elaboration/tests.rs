use tsc_diagnostics::gen as diagnostics;
use tsc_syntax::SyntaxKind;
use tsc_types::{CheckMode, CompilerOptions};

use crate::state::test_support::with_program_state;

fn jsx_attributes_optional_elaboration_codes(attribute: &str) -> Vec<u32> {
    let text = format!(
        "declare namespace JSX {{\n\
           interface Element {{}}\n\
           interface IntrinsicElements {{ x: {{ n: number }} }}\n\
         }}\n\
         declare const bad: {{ n: string }};\n\
         (<x {attribute} />);\n"
    );
    let options = CompilerOptions {
        jsx: Some(1),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.tsx", &text)], &options, |state| {
        state.check_source_file(0);
        let (attributes, target_node, opening) = {
            let source = state.binder.source(0);
            let attributes = source
                .arena
                .node_ids()
                .find(|&node| source.arena.node(node).kind == SyntaxKind::JsxAttributes)
                .expect("JSX attributes");
            let target_node = source
                .arena
                .node_ids()
                .find(|&node| source.arena.node(node).kind == SyntaxKind::TypeLiteral)
                .expect("intrinsic attributes target");
            let opening = source
                .arena
                .node(attributes)
                .parent
                .expect("opening element");
            (attributes, target_node, opening)
        };
        let source_type = state
            .check_expression_cached(attributes, CheckMode::NORMAL)
            .expect("JSX attributes source");
        let target_type = state
            .get_type_from_type_node(target_node)
            .expect("JSX attributes target");
        state.diagnostics.clear();
        state
            .check_type_assignable_to_and_optionally_elaborate(
                source_type,
                target_type,
                Some(opening),
                attributes,
                &diagnostics::Type_0_does_not_satisfy_the_expected_type_1,
            )
            .expect("optional elaboration");
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file_name.is_some())
            .map(|diagnostic| diagnostic.code())
            .collect()
    })
}

#[test]
fn jsx_attributes_optional_elaboration_reports_named_member() {
    assert_eq!(jsx_attributes_optional_elaboration_codes("n=\"s\""), [2322]);
}

#[test]
fn jsx_attributes_optional_elaboration_decline_reports_relation_head() {
    assert_eq!(
        jsx_attributes_optional_elaboration_codes("{...bad}"),
        [1360]
    );
}

#[test]
fn deferred_indexed_member_keeps_the_outer_object_relation() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, texts: &mut Vec<String>) {
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, texts);
        }
    }

    let source = "interface TextChannel { type: 'text'; phone: string }\n\
                  interface EmailChannel { type: 'email'; address: string }\n\
                  type Channel = TextChannel | EmailChannel;\n\
                  type ChannelType = Channel extends { type: infer R } ? R : never;\n\
                  type MyPick<T, K extends keyof T> = { [P in K]: T[P] };\n\
                  type MyPartial<T> = { [P in keyof T]?: T[P] };\n\
                  type MyOmit<T, K extends keyof T> = MyPick<T,\n\
                      ({ [P in keyof T]: P } & { [P in K]: never } &\n\
                       { [x: string]: never })[keyof T]>;\n\
                  type ChannelOfType<T extends ChannelType, A = Channel> =\n\
                      A extends { type: T } ? A : never;\n\
                  type NewChannel<T extends Channel> = MyPick<T, 'type'> &\n\
                      MyPartial<MyOmit<T, 'type'>> & { localChannelId: string };\n\
                  function make<T extends ChannelType>(type: T): NewChannel<ChannelOfType<T>> {\n\
                      return { type, localChannelId: '' };\n\
                  }\n";
    let return_start = source.find("return {").expect("return expression") as u32;
    with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2322)
            .expect("generic indexed member mismatch");
        assert_eq!(
            (diagnostic.start, diagnostic.length),
            (Some(return_start), Some(6))
        );
        let mut texts = Vec::new();
        flatten(&diagnostic.message, &mut texts);
        assert!(
                texts.first().is_some_and(|text| text.starts_with(
                    "Type '{ type: T; localChannelId: string; }' is not assignable to type 'NewChannel<"
                )),
                "{texts:#?}"
            );
        assert!(
            texts
                .iter()
                .any(|text| text == "Types of property 'type' are incompatible."),
            "{texts:#?}"
        );
    });
}
