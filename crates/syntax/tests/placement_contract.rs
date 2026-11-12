use std::sync::Arc;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::placement::{SyntaxStripe, SPACES};
use tsc_syntax::{
    parse_source_file, published_node, source_files_structurally_equal, JSDocParsingMode,
    ParseOptions, ScriptKind,
};
use tsc_types::IdentityDomain;

#[test]
fn composed_jsdoc_jsx_templates_and_texts_fit_exact_dense_rooms() {
    let cases = [
        (
            "composed.ts",
            r#"/// <reference path="other.ts" />
/** @typedef {{ value: string }} Alias */
export class Box { #value = "😀"; method<T>(x: T) { return [x, this.#value]; } }
export const f = <T>(x: T) => ({ x, text: `value:${x}` });"#,
            ScriptKind::Ts,
        ),
        (
            "hosted.js",
            r#"/** @typedef {{ value: string }} Alias */
/** @param {Alias} x @returns {string} */
export function value(x) { return x.value; }
/** @type {Alias} */ const x = { value: "\ud800" };"#,
            ScriptKind::Js,
        ),
        (
            "view.tsx",
            r#"export const view = <div data-label="😀">{[1, 2].map(x => <span>{x}</span>)}</div>;"#,
            ScriptKind::Tsx,
        ),
    ];
    let options = |kind| ParseOptions {
        script_kind: kind,
        js_doc_parsing_mode: JSDocParsingMode::ParseAll,
        ..ParseOptions::default()
    };
    let local: Vec<_> = cases
        .iter()
        .map(|(name, text, kind)| parse_source_file(*name, *text, options(*kind), None))
        .collect();
    let mut counts = [0u32; 4];
    for source in &local {
        for (total, count) in counts.iter_mut().zip([
            source.arena.nodes().len(),
            source.arena.node_arrays().len(),
            source.arena.array_item_count(),
            source.arena.text_word_count(),
        ]) {
            *total += u32::try_from(count).unwrap();
        }
    }
    let domain = IdentityDomain::new();
    let room = domain
        .lease_rooms(&[SPACES.into_iter().zip(counts).collect()])
        .unwrap()
        .pop()
        .unwrap();
    let mut stripe = SyntaxStripe::new(room);
    let mut placed = Vec::new();
    for ((name, text, kind), expected) in cases.iter().zip(&local) {
        let snapshot = TextSnapshot::new((*text).to_owned(), DocumentVersion::default());
        let mut source = stripe.parse((*name).into(), Arc::clone(&snapshot), options(*kind));
        assert!(source_files_structurally_equal(expected, &source), "{name}");
        assert_eq!(source.arena.symbol_bound(), expected.arena.symbol_bound());
        assert!(source.identity_owned_by(&domain));
        assert!(Arc::ptr_eq(source.snapshot(), &snapshot));
        for node in source.arena.node_ids() {
            assert!(std::ptr::eq(source.arena.node(node), published_node(node)));
        }
        let address = source.arena.node(source.root) as *const _;
        source
            .relocate_with_leases(source.lease_identities(&domain).unwrap())
            .unwrap();
        assert_eq!(address, source.arena.node(source.root) as *const _);
        placed.push(source);
    }
    let statistics = stripe.seal().unwrap();
    assert_eq!(statistics.direct_sources, cases.len());
    assert_eq!(statistics.fallbacks, 0);
    assert_eq!(statistics.holes, [0; 4]);
    assert_eq!(placed.len(), cases.len());
}
