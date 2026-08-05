use tsc_binder::bind_source_file;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions};
use tsc_types::{CompilerOptions, LiteralValue, TemplateText, TypeData};

use crate::relpin::find_probe_annotation;
use crate::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};
use crate::state::CheckerState;

fn probe(
    setup: &str,
    source: &str,
    target: &str,
    fresh: bool,
    relation: RelpinRelation,
) -> RelpinVerdict {
    let options = CompilerOptions::default();
    probe_relation(&RelpinQuery {
        setup,
        source,
        target,
        source_is_fresh: fresh,
        relation,
        options: &options,
    })
}

#[test]
fn slice_tuple_type_negative_end_counts_from_the_end() {
    // 61290 + JS Array.prototype.slice: endSkipCount beyond the
    // arity turns the slice end NEGATIVE and JS re-reads it from
    // the END — max(2*len - skip, 0). Reachable since 7.4's
    // impliedArity record (the 69114 both-variadic arm passes
    // endLength + sourceArity - impliedArity, which exceeds
    // sourceArity whenever impliedArity < endLength; fixture
    // corroborated against vendored tsc, scratchpad probe74k.mjs).
    // Pre-fix the port clamped the whole window to empty.
    crate::state::test_support::with_program_state(
        &[("a.ts", "var v: [string, number, boolean];\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("annotated var");
            let tuple = state
                .get_type_from_type_node(annotation)
                .expect("tuple type");
            // len 3, skip 4: JS slice(0, -1) → [0, 2).
            let sliced = state.slice_tuple_type(tuple, 0, 4).expect("slice succeeds");
            let elements = state.get_type_arguments(sliced).expect("elements");
            assert_eq!(
                elements.len(),
                2,
                "negative end counts from the end (2*3 - 4)"
            );
            // len 3, skip 7 (beyond 2*len): floored to empty.
            let floored = state.slice_tuple_type(tuple, 0, 7).expect("slice succeeds");
            let none = state.get_type_arguments(floored).expect("elements");
            assert_eq!(none.len(), 0, "max(2*len - skip, 0) floors at zero");
            // The inverted-range clamp is unchanged: skip 2 puts
            // the end (1) below the start (2) — still empty.
            let inverted = state.slice_tuple_type(tuple, 2, 2).expect("slice succeeds");
            let inv = state.get_type_arguments(inverted).expect("elements");
            assert_eq!(inv.len(), 0, "end before start clamps to empty");
        },
    );
}

#[test]
fn signature_display_parameter_name_expands_only_tuple_typed_rest_parameters() {
    crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "declare function tuple(...args: [unknown]): void;\n\
                 declare function array(...args: unknown[]): void;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let declarations: Vec<_> = state
                .binder
                .source(0)
                .arena
                .node_ids()
                .filter(|&node| state.kind_of(node) == tsc_syntax::SyntaxKind::FunctionDeclaration)
                .collect();
            assert_eq!(declarations.len(), 2);
            let tuple = state
                .get_signature_from_declaration(declarations[0])
                .expect("tuple-rest signature");
            let array = state
                .get_signature_from_declaration(declarations[1])
                .expect("array-rest signature");
            assert_eq!(
                state
                    .get_parameter_name_at_position(tuple, 0)
                    .expect("tuple label"),
                Some("args_0".to_owned())
            );
            assert_eq!(
                state
                    .get_parameter_name_at_position(array, 0)
                    .expect("rest name"),
                Some("args".to_owned())
            );
        },
    );
}

#[test]
fn excess_property_checks_fire_on_fresh_probe_sources() {
    assert!(matches!(
        probe(
            "",
            "{ a: number, b: number }",
            "{ a: number }",
            true,
            RelpinRelation::Assignable,
        ),
        RelpinVerdict::NotRelated
    ));
    // The same pair declared (non-fresh) is plain width subtyping.
    assert!(matches!(
        probe(
            "",
            "{ a: number, b: number }",
            "{ a: number }",
            false,
            RelpinRelation::Assignable,
        ),
        RelpinVerdict::Related
    ));
}

#[test]
fn empty_template_fragments_keep_empty_cooked_text() {
    // Regression: current_token_text's missing-token fallback used
    // to turn the empty tail of `a${string}` into the token NAME,
    // breaking template reduction and matching.
    let options = CompilerOptions::default();
    let source = parse_source_file(
        "template-regression.ts".to_owned(),
        "declare var c: `${string}`;\n".to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(source.parse_diagnostics.is_empty());
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    let annotation = find_probe_annotation(&source, "c").expect("annotation");
    let ty = state
        .get_type_from_type_node(annotation)
        .expect("template annotation resolves");
    assert_eq!(
        ty, state.tables.intrinsics.string,
        "`${{string}}` reduces to string (62075-62078)"
    );
}

#[test]
fn template_inference_splits_unpaired_surrogate_losslessly() {
    crate::state::test_support::with_program_state(
        &[("a.ts", "")],
        &CompilerOptions::default(),
        |state| {
            let source = state.tables.get_string_literal_type_from_utf16(&[0xD800]);
            let replacement = state.tables.get_string_literal_type_from_utf16(&[0xFFFD]);
            assert_ne!(source, replacement);

            // Two placeholders separated by an empty delimiter
            // make inferFromLiteralParts split after one UTF-16
            // code unit. tsc retains that unit even when it is an
            // unpaired surrogate.
            let empty = TemplateText::default();
            let number = state.tables.intrinsics.number;
            let target = state.get_template_literal_type_from_texts(
                &[empty.clone(), empty.clone(), empty],
                &[number, number],
            );
            let inferred = state
                .infer_types_from_template_literal_type(source, target)
                .expect("lossless split does not escape")
                .expect("literal matches the empty-delimiter shape");
            assert_eq!(inferred.len(), 2);
            assert_eq!(inferred[0], source);
            assert_eq!(inferred[1], state.tables.get_string_literal_type(""));
            assert_eq!(
                &state.tables.type_of(inferred[0]).data,
                &TypeData::Literal {
                    value: LiteralValue::String(TemplateText::from_utf16(&[0xD800])),
                }
            );
            let replacement_inferred = state
                .infer_types_from_template_literal_type(replacement, target)
                .expect("replacement split succeeds")
                .expect("replacement literal matches");
            assert_eq!(replacement_inferred[0], replacement);
            assert_ne!(replacement_inferred[0], inferred[0]);
        },
    );
}

#[test]
fn structural_relations_match_known_verdicts() {
    // Maybe-path recursion.
    let recursive = "interface A { next: B }\ninterface B { next: A }";
    assert!(matches!(
        probe(recursive, "A", "B", false, RelpinRelation::Assignable),
        RelpinVerdict::Related
    ));
    let divergent = "interface A { next: B; x: number }\ninterface B { next: A; x: string }";
    assert!(matches!(
        probe(divergent, "A", "B", false, RelpinRelation::Assignable),
        RelpinVerdict::NotRelated
    ));
    // Tuple arm.
    assert!(matches!(
        probe(
            "",
            "[number]",
            "[number, string?]",
            false,
            RelpinRelation::Assignable
        ),
        RelpinVerdict::Related
    ));
    assert!(matches!(
        probe(
            "",
            "[number, string]",
            "[number]",
            false,
            RelpinRelation::Assignable
        ),
        RelpinVerdict::NotRelated
    ));
    // Template matching.
    assert!(matches!(
        probe(
            "",
            "\"abc\"",
            "`a${string}`",
            false,
            RelpinRelation::Assignable
        ),
        RelpinVerdict::Related
    ));
    // Signatures: strictFunctionTypes contravariance is the
    // default-strict behavior.
    assert!(matches!(
        probe(
            "",
            "(x: 1) => void",
            "(x: number) => void",
            false,
            RelpinRelation::Assignable,
        ),
        RelpinVerdict::NotRelated
    ));
    // Index signatures.
    assert!(matches!(
        probe(
            "",
            "{ a: number }",
            "{ [k: string]: number }",
            false,
            RelpinRelation::Assignable,
        ),
        RelpinVerdict::Related
    ));
}

#[test]
fn conditional_source_and_target_relations_are_live() {
    crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "type C<X> = X extends string ? { a: string } : { b: number };\n\
                 type I<X> = X extends infer U ? [U] : never;\n\
                 type J<X> = X extends infer V ? [V] : never;\n\
                 function f<T>() {\n\
                 var wide: { a: string; b: number };\n\
                 var target: C<T>;\n\
                 var branch: C<T>;\n\
                 var union: { a: string } | { b: number };\n\
                 var inferredSource: I<T>;\n\
                 var inferredTarget: J<T>;\n\
                 }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = |state: &CheckerState, name: &str| {
                find_probe_annotation(state.binder.source(0), name)
                    .unwrap_or_else(|| panic!("annotation for {name}"))
            };
            let wide_node = annotation(state, "wide");
            let target_node = annotation(state, "target");
            let branch_node = annotation(state, "branch");
            let union_node = annotation(state, "union");
            let inferred_source_node = annotation(state, "inferredSource");
            let inferred_target_node = annotation(state, "inferredTarget");
            let wide = state.get_type_from_type_node(wide_node).expect("wide");
            let target = state
                .get_type_from_type_node(target_node)
                .expect("conditional target");
            let branch = state
                .get_type_from_type_node(branch_node)
                .expect("conditional source");
            let union = state.get_type_from_type_node(union_node).expect("union");
            let inferred_source = state
                .get_type_from_type_node(inferred_source_node)
                .expect("conditional source with infer");
            let inferred_target = state
                .get_type_from_type_node(inferred_target_node)
                .expect("conditional target with infer");
            let true_type = state
                .get_true_type_from_conditional_type(target)
                .expect("true branch");
            let false_type = state
                .get_false_type_from_conditional_type(target)
                .expect("false branch");
            assert!(state
                .is_type_assignable_to(wide, true_type)
                .expect("true branch relation"));
            assert!(state
                .is_type_assignable_to(wide, false_type)
                .expect("false branch relation"));
            let tsc_types::TypeData::Conditional(target_data) =
                state.tables.type_of(target).data.clone()
            else {
                panic!("target remains conditional");
            };
            assert!(
                !state
                    .is_distribution_dependent(target_data.root)
                    .expect("distribution dependence"),
                "neither branch references the distributive check parameter"
            );

            assert!(
                state
                    .is_type_assignable_to(wide, target)
                    .expect("conditional target relation"),
                "a source assignable to both branches relates to the conditional target"
            );
            assert!(
                state
                    .is_type_assignable_to(branch, union)
                    .expect("conditional source relation"),
                "the conditional source relates through its default constraint"
            );
            assert!(
                state
                    .is_type_assignable_to(inferred_source, inferred_target)
                    .expect("conditional-pair relation"),
                "matching infer parameters compare through the parked relation frame"
            );
        },
    );
}

// ---- m4-review A5: createUnionOrIntersectionProperty modifier /
// writeTypes propagation (tsc-probed rows, vendored 6.0.3 noLib,
// strict defaults) ----

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    rows_and_partials(text).0
}

fn checked_js_rows(text: &str) -> Vec<(u32, u32, u32)> {
    crate::state::test_support::with_program_state(
        &[("a.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.is_some()
                        && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
                })
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                    )
                })
                .collect()
        },
    )
}

#[test]
fn js_prototype_placeholder_is_a_prototype_property_override() {
    assert_eq!(
            checked_js_rows(
                "class Module {}\nModule.prototype.identifier = undefined;\nclass NormalModule extends Module { identifier() { return \"normal\"; } }\n"
            ),
            []
        );

    let ordinary =
        "class Base { constructor() { this.p = 1; } }\nclass Derived extends Base { p() {} }\n";
    // The ordinary number-property/function override owns two
    // independent tsc rows: 2416 from issueMemberSpecificError
    // and 2425 from checkKindsOfPropertyMemberOverrides. The JS
    // prototype placeholders above are special only to the latter
    // predicate and remain clean in both passes.
    assert_eq!(
        checked_js_rows(ordinary),
        [
            (
                2416,
                ordinary.find("p()").expect("derived method") as u32,
                1
            ),
            (
                2425,
                ordinary.find("p()").expect("derived method") as u32,
                1
            )
        ]
    );
}

/// The containment-aware face (7.5d review): a `(rows, 0)` pin
/// proves the path verdicts LIVE — a bare `checked_rows == []`
/// cannot distinguish a clean pass from an Err-contained
/// statement.
fn rows_and_partials(text: &str) -> (Vec<(u32, u32, u32)>, usize) {
    crate::state::test_support::with_program_state(
        &[("a.ts", text)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let rows = state
                .diagnostics
                .iter()
                .filter(|diag| {
                    diag.file_name.is_some()
                        && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
                })
                .map(|diag| {
                    (
                        diag.code(),
                        diag.start.unwrap_or(u32::MAX),
                        diag.length.unwrap_or(u32::MAX),
                    )
                })
                .collect();
            (rows, state.partial_check_records.len())
        },
    )
}

#[test]
fn relation_property_reports_use_target_symbol_to_string_faces() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, texts: &mut Vec<String>) {
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, texts);
        }
    }

    let texts = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "declare const sym: unique symbol;\n\
                 declare let quotedSource: { a: number };\n\
                 let quotedTarget: { 'a': string } = quotedSource;\n\
                 declare let identifierSource: { 'a': number };\n\
                 let identifierTarget: { a: string } = identifierSource;\n\
                 declare let numericSource: { 2: string };\n\
                 let numericTarget: { 2.0: number } = numericSource;\n\
                 declare let hyphenSource: { \"data-foo\": number };\n\
                 let hyphenTarget: { \"data-foo\": string } = hyphenSource;\n\
                 declare let computedStringSource: { [\"data-foo\"]: number };\n\
                 let computedStringTarget: { [\"data-foo\"]: string } = computedStringSource;\n\
                 declare let underscoreSource: { __typename: number };\n\
                 let underscoreTarget: { __typename: string } = underscoreSource;\n\
                 declare let symbolSource: { [sym]: number };\n\
                 let symbolTarget: { [sym]: string } = symbolSource;\n\
                 declare let empty: {};\n\
                 let requiredComputed: { [sym]: number } = empty;\n\
                 declare let indexedSource: { [sym]: number };\n\
                 let indexedTarget: { [key: symbol]: string } = indexedSource;\n\
                 interface Left { [sym]: number }\n\
                 interface Right { [sym]: string }\n\
                 interface Both extends Left, Right {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let mut texts = Vec::new();
            for diagnostic in &state.diagnostics {
                if diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error {
                    flatten(&diagnostic.message, &mut texts);
                }
            }
            texts
        },
    );

    for expected in [
        "Types of property ''a'' are incompatible.",
        "Types of property 'a' are incompatible.",
        "Types of property '2.0' are incompatible.",
        "Types of property '\"data-foo\"' are incompatible.",
        "Types of property '[\"data-foo\"]' are incompatible.",
        "Types of property '__typename' are incompatible.",
        "Types of property '[sym]' are incompatible.",
        "Property '[sym]' is missing in type '{}' but required in type '{ [sym]: number; }'.",
        "Property '[sym]' is incompatible with index signature.",
        "Named property '[sym]' of types 'Left' and 'Right' are not identical.",
    ] {
        assert!(
            texts.iter().any(|text| text == expected),
            "missing exact property-display row {expected:?}; got {texts:#?}"
        );
    }
    assert!(
        texts.iter().all(|text| !text.contains("__@sym@")),
        "internal late-bound names must never reach diagnostics: {texts:#?}"
    );
    assert!(
        texts
            .iter()
            .all(|text| !text.contains("Types of property '___typename'")),
        "escaped leading underscores must be read through symbolToString: {texts:#?}"
    );
}

#[test]
fn unconstrained_source_type_parameter_gets_the_constraint_hint() {
    let positive = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "function f<T>(x: T) { const y: { a: string } = x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the assignment mismatch is reported");
            diagnostic.related.clone()
        },
    );
    assert_eq!(positive.len(), 1);
    assert_eq!(positive[0].message.code, 2208);
    assert_eq!((positive[0].start, positive[0].length), (Some(11), Some(1)));
    assert_eq!(
        positive[0].message.text,
        "This type parameter might need an `extends { a: string; }` constraint."
    );

    let optional_property = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "function optional<T>(x: T) { const y: { a?: string } = x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the optional-property assignment mismatch is reported")
                .related
                .clone()
        },
    );
    assert_eq!(optional_property.len(), 1);
    assert_eq!(optional_property[0].message.code, 2208);
    assert_eq!(
        optional_property[0].message.text,
        "This type parameter might need an `extends { a?: string; }` constraint."
    );

    let target_parameter = crate::state::test_support::with_program_state(
        &[("a.ts", "function h<T, U>(x: T) { const y: U = x; }\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the type-parameter assignment mismatch is reported")
                .related
                .clone()
        },
    );
    assert_eq!(target_parameter.len(), 1);
    assert_eq!(target_parameter[0].message.code, 2208);
    assert_eq!(
        target_parameter[0].message.text,
        "This type parameter might need an `extends U` constraint."
    );

    let constrained = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "function g<T extends object>(x: T) { const y: { a: string } = x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the constrained assignment mismatch is reported")
                .related
                .iter()
                .map(|related| related.message.code)
                .collect::<Vec<_>>()
        },
    );
    assert!(
        !constrained.contains(&2208),
        "an existing constraint suppresses the hint: {constrained:?}"
    );
}

#[test]
fn tuple_relation_reports_tsc_arity_and_element_mismatch_chains() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let chains = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            r#"
declare let one: [number];
declare let two: [number, number];
declare let open: [number, ...number[]];
declare let optional: [number?];
const tooShort: [number, number] = one;
const tooLong: [number] = two;
const mayBeShort: [number, number] = open;
const mayBeLong: [number] = open;
const lacksRequired: [number] = optional;
"#,
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
                .map(|diagnostic| {
                    let mut codes = Vec::new();
                    flatten_codes(&diagnostic.message, &mut codes);
                    codes
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(
        chains,
        [
            vec![2322, 2618],
            vec![2322, 2619],
            vec![2322, 2620],
            vec![2322, 2621],
            vec![2322, 2623],
        ]
    );

    let ranged = crate::state::test_support::with_program_state(
            &[(
                "a.ts",
                "declare let source: [number, number, string];\ndeclare let target: [number, ...number[]];\ntarget = source;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == 2322)
                    .expect("the rest-element assignment mismatch is reported")
                    .message
                    .clone()
            },
        );
    assert_eq!(ranged.next[0].code, 2627);
    assert_eq!(
            ranged.next[0].text,
            "Type at positions 1 through 2 in source is not compatible with type at position 1 in target."
        );
    assert_eq!(ranged.next[0].next[0].code, 2322);

    let variadic = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            r#"
function targetVariadic<T extends unknown[]>(source: [number], target: [...T]) {
    target = source;
}
function sourceVariadic<T extends unknown[]>(source: [...T], target: [number]) {
    target = source;
}
"#,
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
                .map(|diagnostic| {
                    let mut codes = Vec::new();
                    flatten_codes(&diagnostic.message, &mut codes);
                    codes
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(variadic, [vec![2322, 2624], vec![2322, 2625]]);

    let single = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "declare let source: [number];\ndeclare let target: [string];\ntarget = source;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("the single-element assignment mismatch is reported")
                .message
                .clone()
        },
    );
    let mut single_codes = Vec::new();
    flatten_codes(&single, &mut single_codes);
    assert!(
        !single_codes.iter().any(|code| (2618..=2627).contains(code)),
        "the one-element sibling has no tuple-position wrapper: {single_codes:?}"
    );
}

#[test]
fn unmatched_property_reporting_preserves_tsc_control_flow() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let options = CompilerOptions {
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let chains = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            r#"
interface Array<T> {
    pop(): T | undefined;
    push(...items: T[]): number;
    concat(...items: T[]): T[];
    join(separator?: string): string;
}
declare let singleSource: { p: {} };
let singleTarget: { p: { a: string } } = singleSource;
declare let multiSource: { p: {} };
let multiTarget: { p: { a: string; b: number } } = multiSource;
declare let nonArray: { length: number };
let emptyTuple: [] = nonArray;
class PrivateSource { #x = 1 }
class PrivateTarget { #x = 1 }
let privateTarget: PrivateTarget = new PrivateSource();
class Base { p = {} }
interface Required { p: { a: string } }
class ImplementsViaBase extends Base implements Required {}
"#,
        )],
        &options,
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.is_some() && matches!(diagnostic.code(), 2322 | 2420)
                })
                .map(|diagnostic| {
                    let mut codes = Vec::new();
                    flatten_codes(&diagnostic.message, &mut codes);
                    codes
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(
        chains,
        [
            // reportUnmatchedProperty increments
            // overrideNextErrorInfo after the single and multi
            // details, replacing the immediately enclosing 2322.
            vec![2322, 2326, 2741],
            vec![2322, 2326, 2739],
            // tryElaborateArrayLikeErrors(false): a non-array
            // source against a tuple target declines the detail.
            vec![2322],
            // The private-identifier arm precedes missing-property
            // enumeration and never arms the override counter.
            vec![2322, 18015],
            // The class-implements closure head is the explicit
            // exception: its nested generic relation row remains.
            vec![2420, 2326, 2322, 2741],
        ]
    );
}

#[test]
fn no_matching_signature_reporting_preserves_tsc_control_flow() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let diagnostics = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            r#"
declare let plainCallSource: { x: number };
let callTarget: () => void = plainCallSource;
declare let plainConstructSource: { x: number };
let constructTarget: new () => object = plainConstructSource;
interface Overloaded {
    (x: string): string;
    (x: number): number;
}
declare let overloaded: Overloaded;
let laterMatch: (x: number) => number = overloaded;
"#,
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| {
                    let mut codes = Vec::new();
                    flatten_codes(&diagnostic.message, &mut codes);
                    let detail = diagnostic
                        .message
                        .next
                        .first()
                        .map(|child| child.text.clone());
                    (codes, detail)
                })
                .collect::<Vec<_>>()
        },
    );

    assert_eq!(
        diagnostics,
        [
            (
                vec![2322, 2658],
                Some(
                    "Type '{ x: number; }' provides no match for the signature '(): void'."
                        .to_owned()
                )
            ),
            (
                vec![2322, 2658],
                Some(
                    "Type '{ x: number; }' provides no match for the signature 'new (): object'."
                        .to_owned()
                )
            ),
        ]
    );
}

#[test]
fn wrapper_object_to_primitive_reports_the_tsc_hint() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let diagnostics = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            r#"
interface String {}
interface Number {}
interface Boolean {}
interface Symbol {}
declare let boxedString: String;
declare let boxedNumber: Number;
declare let boxedBoolean: Boolean;
declare let boxedSymbol: Symbol;
let primitiveString: string = boxedString;
let primitiveNumber: number = boxedNumber;
let primitiveBoolean: boolean = boxedBoolean;
let primitiveSymbol: symbol = boxedSymbol;
"#,
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| {
                    let mut codes = Vec::new();
                    flatten_codes(&diagnostic.message, &mut codes);
                    let detail = diagnostic
                        .message
                        .next
                        .first()
                        .map(|child| child.text.clone());
                    (codes, detail)
                })
                .collect::<Vec<_>>()
        },
    );

    assert_eq!(
            diagnostics,
            [
                (
                    vec![2322, 2692],
                    Some(
                        "'string' is a primitive, but 'String' is a wrapper object. Prefer using 'string' when possible."
                            .to_owned()
                    )
                ),
                (
                    vec![2322, 2692],
                    Some(
                        "'number' is a primitive, but 'Number' is a wrapper object. Prefer using 'number' when possible."
                            .to_owned()
                    )
                ),
                (
                    vec![2322, 2692],
                    Some(
                        "'boolean' is a primitive, but 'Boolean' is a wrapper object. Prefer using 'boolean' when possible."
                            .to_owned()
                    )
                ),
                (
                    vec![2322, 2692],
                    Some(
                        "'symbol' is a primitive, but 'Symbol' is a wrapper object. Prefer using 'symbol' when possible."
                            .to_owned()
                    )
                ),
            ]
        );
}

#[test]
fn recursive_mapped_keyof_constraint_requires_a_definite_relation() {
    // 66144's `=== True` is intentional: stepping from `keyof T`
    // into T's self-referential mapped constraint revisits the
    // active relation and yields Maybe. That speculative result
    // cannot make number or string assignable to `keyof T`.
    assert_eq!(
            rows_and_partials(
                "function f4<T extends { [K in keyof T]: string }>(k: keyof T) {\n    k = 42;\n    k = \"hello\";\n}\n"
            ),
            (vec![(2322, 68, 1), (2322, 80, 1)], 0)
        );
}

#[test]
fn conflicting_private_union_property_bails_out() {
    // ContainsPrivate now folds per member (59148-59152): distinct
    // private declarations kill the union property -> 2339.
    assert_eq!(
            checked_rows(
                "class A { private x: number = 1; m() { return this.x } }\nclass B { private x: number = 2; m() { return this.x } }\ndeclare const u: A | B;\nu.x;\n"
            ),
            [(2339, 140, 1)]
        );
}

#[test]
fn conflicting_private_intersection_reduces_to_never() {
    // The never-reduction consumer reads CONTAINS_PRIVATE off the
    // synthetic: A & B collapses, so the never assignment is clean.
    assert_eq!(
            checked_rows(
                "class A { private x: number = 1; m() { return this.x } }\nclass B { private x: number = 2; m() { return this.x } }\ndeclare const i: A & B;\nconst n: never = i;\n"
            ),
            []
        );
}

#[test]
fn union_accessor_write_type_is_the_setter_union() {
    // writeTypes propagation (59209-59213/59237-59239): both
    // assignments target (number | string) | (number | boolean).
    assert_eq!(
            checked_rows(
                "class A { get p(): number { return 1 } set p(v: number | string) {} }\nclass B { get p(): number { return 2 } set p(v: number | boolean) {} }\ndeclare const u: A | B;\nu.p = true;\nu.p = 3;\n"
            ),
            []
        );
}

#[test]
fn same_declaration_private_instantiations_survive_the_bailout() {
    // getCommonDeclarationsOfSymbols carve-out (59172): C<string>
    // and C<number> share the one `x` declaration, so the union
    // property survives and in-class access stays legal.
    assert_eq!(
            checked_rows(
                "class C<T> { private x: T; constructor(v: T) { this.x = v } m(o: C<string> | C<number>) { return o.x; } }\n"
            ),
            []
        );
}

// ---- M6 7.5 B7: the compareTypePredicateRelatedTo decision
// table (64577-64628). All rows oracle-pinned 2026-07-21
// (scratchpad probe75.mjs / probe75b.mjs / probe75c.mjs,
// vendored 6.0.3 noLib, strict defaults). Verdict pins whose tsc
// head args sit behind the display curtain ride the
// @ts-expect-error band, which lives in the PROGRAM driver
// (directive filtering + 2578 synthesis + the S8 partial-check
// exemption) — those use program_rows, not checked_rows. ----

fn program_rows(text: &str) -> Vec<(u32, Option<u32>, Option<u32>)> {
    let result = crate::check_program(
        &[crate::InputFile {
            name: "a.ts".to_owned(),
            text: text.to_owned(),
        }],
        &CompilerOptions::default(),
    );
    result
        .diagnostics
        .iter()
        .map(|d| (d.code(), d.start, d.length))
        .collect()
}

#[test]
fn constructor_visibility_participates_in_static_side_relations() {
    let source = "class Pub { constructor() {} }\n\
                      class Prot { protected constructor() {} }\n\
                      class Priv { private constructor() {} }\n\
                      let p = Pub;\n\
                      p = Prot;\n\
                      p = Priv;\n\
                      let q = Prot;\n\
                      q = Pub;\n\
                      q = Priv;\n\
                      let r = Priv;\n\
                      r = Pub;\n\
                      r = Prot;\n";
    assert_eq!(
        program_rows(source),
        [
            (
                2322,
                Some(source.find("p = Prot").expect("protected to public") as u32),
                Some(1),
            ),
            (
                2322,
                Some(source.find("p = Priv").expect("private to public") as u32),
                Some(1),
            ),
            (
                2322,
                Some(source.find("q = Priv").expect("private to protected") as u32),
                Some(1),
            ),
        ]
    );
}

#[test]
fn predicate_both_sides_mismatched_types_fail_the_relation() {
    // Both sides carry identifier predicates; string vs number
    // fails compareTypePredicateRelatedTo's type compare. tsc
    // reports the 2322 head (1226 chain) — the head's
    // function-type args sit behind the display curtain
    // (typeToString 5.4 slice, T2/M8), so the verdict is pinned
    // via the @ts-expect-error band: a used directive is []
    // on both sides, a wrong TRUE verdict would surface 2578,
    // and the display-Err containment path stays exempt (S8).
    assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\n// @ts-expect-error\nconst f: (x: unknown) => x is number = isCat;\n"
            ),
            []
        );
}

#[test]
fn predicate_relation_reporting_keeps_tsc_chains_and_related_info() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let diagnostics = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "class Guard {\n\
                     isA(): this is A { return true; }\n\
                     isB(): this is B { return true; }\n\
                 }\n\
                 class A extends Guard { a: number = 0; }\n\
                 class B extends Guard { b: number = 0; }\n\
                 let guard: Guard = new Guard();\n\
                 guard.isA = guard.isB;\n\
                 declare function plain(p1: unknown, p2: unknown): boolean;\n\
                 const targetOnly: (p1: unknown, p2: unknown) => p1 is A = plain;\n\
                 declare function shifted(p1: unknown, p2: unknown): p2 is A;\n\
                 const wrongPosition: (p1: unknown, p2: unknown) => p1 is A = shifted;\n\
                 declare function identifier(p: unknown): p is A;\n\
                 const wrongKind: { (p: unknown): this is Guard } = identifier;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
                .map(|diagnostic| (diagnostic.message.clone(), diagnostic.related.clone()))
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(diagnostics.len(), 4);

    let chains = diagnostics
        .iter()
        .map(|(message, _)| {
            let mut codes = Vec::new();
            flatten_codes(message, &mut codes);
            codes
        })
        .collect::<Vec<_>>();
    assert_eq!(
        chains,
        [
            vec![2322, 1226, 2741],
            vec![2322, 1224],
            vec![2322, 1226, 1227],
            vec![2322, 1226, 2518],
        ]
    );
    assert_eq!(
        diagnostics[0].0.next[0].text,
        "Type predicate 'this is B' is not assignable to 'this is A'."
    );
    assert_eq!(
        diagnostics[1].0.next[0].text,
        "Signature '(p1: unknown, p2: unknown): boolean' must be a type predicate."
    );
    assert_eq!(
        diagnostics[2].0.next[0].text,
        "Type predicate 'p2 is A' is not assignable to 'p1 is A'."
    );
    assert_eq!(
        diagnostics[2].0.next[0].next[0].text,
        "Parameter 'p2' is not in the same position as parameter 'p1'."
    );
    assert_eq!(
        diagnostics[3].0.next[0].text,
        "Type predicate 'p is A' is not assignable to 'this is Guard'."
    );
    assert_eq!(diagnostics[0].1.len(), 1);
    assert_eq!(diagnostics[0].1[0].message.code, 2728);
    assert_eq!(diagnostics[0].1[0].message.text, "'a' is declared here.");
    assert!(diagnostics[1].1.is_empty());
    assert!(diagnostics[2].1.is_empty());
    assert!(diagnostics[3].1.is_empty());
}

#[test]
fn predicate_expect_error_control_reports_unused_2578() {
    // CONTROL for the verdict pins above/below: when the
    // predicate relation SUCCEEDS, the directive goes unused and
    // the 2578 row fires — proving the [] pins observe verdicts,
    // not blanket suppression.
    assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\n// @ts-expect-error\nconst f: (x: unknown) => x is string = isCat;\n"
            ),
            [(2578, Some(49), Some(19))]
        );
}

#[test]
fn predicate_both_sides_equal_types_relate() {
    // Zero partials (7.5d): the entry cells verdict LIVE, not by
    // containment.
    assert_eq!(
            rows_and_partials(
                "declare function isCat(x: unknown): x is string;\nconst f: (x: unknown) => x is string = isCat;\n"
            ),
            (vec![], 0)
        );
}

#[test]
fn predicate_target_only_identifier_fails_the_relation() {
    // Target-only identifier predicate = the 1224-family cell: a
    // plain boolean source can never satisfy `x is string`
    // (verdict via the expect-error band; head display is T2/M8).
    assert_eq!(
            program_rows(
                "declare function plain(x: unknown): boolean;\n// @ts-expect-error\nconst g: (x: unknown) => x is string = plain;\n"
            ),
            []
        );
}

#[test]
fn predicate_target_only_asserts_falls_through_silently() {
    // Asserts-form target alone: the void-return early return
    // (64577-64579) catches it before the predicate arm — no error.
    assert_eq!(
            checked_rows(
                "declare function plain2(x: unknown): void;\nconst h: (x: unknown) => asserts x is string = plain2;\n"
            ),
            []
        );
}

#[test]
fn predicate_source_only_compares_as_boolean_return() {
    // Source-only is-predicate: plain return comparison — the
    // predicate signature's return type is boolean.
    assert_eq!(
            checked_rows(
                "declare function isNum(x: unknown): x is number;\nconst k: (x: unknown) => boolean = isNum;\n"
            ),
            []
        );
    assert_eq!(
            program_rows(
                "declare function isNum(x: unknown): x is number;\n// @ts-expect-error\nconst k2: (x: unknown) => string = isNum;\n"
            ),
            []
        );
}

#[test]
fn predicate_source_only_asserts_compares_as_void_return() {
    // Asserts-source vs plain boolean target: the plain return
    // comparison sees VOID (not boolean) and fails; a void target
    // takes the 64577-64579 early return instead.
    assert_eq!(
            program_rows(
                "declare function aStr(x: unknown): asserts x is string;\n// @ts-expect-error\nconst z: (x: unknown) => boolean = aStr;\n"
            ),
            []
        );
    assert_eq!(
            checked_rows(
                "declare function aStr2(x: unknown): asserts x is string;\nconst z2: (x: unknown) => void = aStr2;\n"
            ),
            []
        );
}

#[test]
fn predicate_parameter_index_mismatch_fails_the_relation() {
    // Identifier predicates on different parameter positions fail
    // the 64614 parameterIndex check (1227 chain, T2; verdict via
    // the expect-error band).
    assert_eq!(
            program_rows(
                "declare function isA(a: unknown, b: unknown): a is string;\n// @ts-expect-error\nconst m: (a: unknown, b: unknown) => b is string = isA;\n"
            ),
            []
        );
}

#[test]
fn predicate_kind_mismatch_fails_the_relation() {
    // Identifier source vs this-based target (and asserts vs
    // plain is-form) fail the 64607 kind check (2518 chain, T2;
    // verdicts via the expect-error band).
    assert_eq!(
            program_rows(
                "declare function isThis(this: object, x: unknown): boolean;\ndeclare const src: (x: unknown) => x is string;\n// @ts-expect-error\nconst n: { (x: unknown): this is object } = src;\n"
            ),
            []
        );
    assert_eq!(
            program_rows(
                "declare function assertIsStr2(x: unknown): asserts x is string;\n// @ts-expect-error\nconst q: (x: unknown) => x is string = assertIsStr2;\n"
            ),
            []
        );
}

#[test]
fn predicate_union_signature_matching_consults_no_predicate() {
    // findMatchingSignature runs ignoreReturnTypes=true — the
    // identical-path predicate consult sits INSIDE
    // !ignoreReturnTypes (67624-67628), so predicate-carrying
    // union members produce a callable union signature (the old
    // gate over-contained this cell).
    assert_eq!(
            checked_rows(
                "declare const u: ((x: unknown) => x is string) | ((x: unknown) => x is string);\nif (u(3)) {}\n"
            ),
            []
        );
    assert_eq!(
            checked_rows(
                "declare const u2: ((x: unknown) => x is string) | ((x: unknown) => boolean);\nif (u2(3)) {}\n"
            ),
            []
        );
}

#[test]
fn predicate_comparable_relation_fails() {
    // The comparable path reaches the predicate arm (boolean
    // return, not void): unrelated predicate types are not
    // comparable either way — tsc's 2352 head args are behind
    // the display curtain, so the verdict rides the expect-error
    // band.
    assert_eq!(
            program_rows(
                "declare const src2: (x: unknown) => x is string;\n// @ts-expect-error\nconst c0 = src2 as (x: unknown) => x is number;\n"
            ),
            []
        );
}

// ---- B7 positive movers: statements the old gate contained
// wholesale now check through and surface their OTHER rows
// (renderable args). Oracle-pinned 2026-07-21 (probe75c.mjs). ----

#[test]
fn predicate_relation_unblocks_sibling_declarator_row() {
    // Declarator 1's predicate relation now succeeds instead of
    // Err-containing the whole statement; declarator 2's plain
    // string→number mismatch surfaces.
    assert_eq!(
            checked_rows(
                "declare function isCat(x: unknown): x is string;\nconst f: (x: unknown) => x is string = isCat, bad: number = \"s\";\n"
            ),
            [(2322, 95, 3)]
        );
}

#[test]
fn predicate_union_call_result_row_surfaces() {
    // findMatchingSignature (ignoreReturnTypes=true) consults no
    // predicate: the union signature resolves and the boolean
    // call result fails the number annotation.
    assert_eq!(
            checked_rows(
                "declare const u2: ((x: unknown) => x is string) | ((x: unknown) => boolean);\nconst r: number = u2(3);\n"
            ),
            [(2322, 83, 1)]
        );
}

// ---- M6 7.5 ripple audit: the steps-doc consumer list
// (contextual element inference, generic new, tagged templates,
// satisfies, the 2769 failure-path candidate choice) probed
// port-vs-oracle 11-for-11 (probe75f/probe75g.mjs, 2026-07-21) —
// representative rows pinned here. ----

#[test]
fn ripple_generic_new_and_tagged_template_infer() {
    assert_eq!(
            checked_rows(
                "declare class Box<T> { constructor(x: T); v: T; }\nconst b = new Box(\"s\");\nconst n: number = b.v;\n"
            ),
            [(2322, 80, 1)]
        );
    assert_eq!(
            checked_rows(
                "declare function tag2<T>(parts: unknown, x: T): T;\nconst t2: number = tag2`a${\"s\"}b`;\n"
            ),
            [(2322, 57, 2)]
        );
}

#[test]
fn ripple_satisfies_runs_the_live_relations() {
    // Generic source satisfies via the B8 arm; predicate faces via
    // the B7 table (the failing half rides the expect-error band).
    assert_eq!(
        checked_rows(
            "declare function id<T>(x: T): T;\nconst s = id satisfies (x: number) => number;\n"
        ),
        []
    );
    assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\nconst sp = isCat satisfies (x: unknown) => x is string;\n// @ts-expect-error\nconst sp2 = isCat satisfies (x: unknown) => x is number;\n"
            ),
            []
        );
}

#[test]
fn ripple_overload_failure_paths_report_oracle_rows() {
    // 2769 head at the callee; a generic candidate pair takes the
    // single-argument-error 2345 face (getCandidateForOverload-
    // Failure's candidate choice); bare arity keeps 2554.
    assert_eq!(
        checked_rows(
            "declare function f(x: string): void;\ndeclare function f(x: boolean): void;\nf(3);\n"
        ),
        [(2769, 77, 1)]
    );
    assert_eq!(
            checked_rows(
                "declare function g<T extends string>(x: T): T;\ndeclare function g<T extends boolean>(x: T, y: T): T;\ng(3);\n"
            ),
            [(2345, 103, 1)]
        );
    assert_eq!(
        checked_rows("declare function h<T>(x: T, y: T): T;\nh();\n"),
        [(2554, 38, 1)]
    );
}

// ---- M6 7.5 B8: compareSignaturesRelated head rebuild —
// generic-source instantiation (64505-64514), erase honoring
// (67069-67071), same-target pairwise arm (66952-66966). Rows
// oracle-pinned 2026-07-21 (probe75.mjs / probe75b.mjs /
// probe75e.mjs, vendored 6.0.3 noLib). ----

#[test]
fn generic_source_instantiates_against_concrete_target() {
    // <T>(x:T)=>T infers T:=number against (x:number)=>number.
    assert_eq!(
        checked_rows("declare function id3<T>(x: T): T;\nconst a1: (x: number) => number = id3;\n"),
        []
    );
    // The failing face: T[] return vs string — verdict via the
    // expect-error band (the 2322 head prints the generic
    // function type, display curtain T2/M8).
    assert_eq!(
            program_rows(
                "declare function id4<T>(x: T): T[];\n// @ts-expect-error\nconst a2: (x: number) => string = id4;\n"
            ),
            []
        );
}

#[test]
fn generic_source_constraint_clamp_compares_under_the_frame() {
    // T extends {id:number} satisfied — the clamp compare passes
    // LIVE (zero partials, 7.5d) and the relation holds.
    assert_eq!(
            rows_and_partials(
                "declare function pick<T extends { id: number }>(x: T): T;\nconst a4: (x: { id: number; name: string }) => { id: number; name: string } = pick;\n"
            ),
            (vec![], 0)
        );
    // T extends string violated by T:=number — the clamp's
    // RelationFrame compare rejects, the instantiated source
    // carries string params, and the param arm fails (verdict via
    // the expect-error band).
    assert_eq!(
            program_rows(
                "declare function pick2<T extends string>(x: T, y: T): T;\n// @ts-expect-error\nconst a5: (x: number, y: number) => number = pick2;\n"
            ),
            []
        );
}

#[test]
fn generic_to_generic_relates_via_canonical_target() {
    assert_eq!(
        checked_rows("declare function id5<T>(x: T): T;\nconst a3: <U>(x: U) => U = id5;\n"),
        []
    );
}

#[test]
fn canonical_signature_recanonicalizes_instantiated_methods() {
    // I<number>.m carries a cloned U (tp.target set): the
    // unconstrained clone re-canonicalizes to its target; the
    // constrained variant keeps the clone.
    assert_eq!(
            rows_and_partials(
                "interface I<T> { m<U>(x: T, y: U): void; }\ndeclare const i: I<number>;\nconst mf: (x: number, y: string) => void = i.m;\n"
            ),
            (vec![], 0)
        );
    assert_eq!(
            rows_and_partials(
                "interface I2<T> { m<U extends T>(x: T, y: U): U; }\ndeclare const i2: I2<number>;\nconst mf2: (x: number, y: 3) => 3 = i2.m;\n"
            ),
            (vec![], 0)
        );
    assert_eq!(
            program_rows(
                "interface I3<T> { m<U extends string>(x: T, y: U): U; }\ndeclare const i3: I3<number>;\n// @ts-expect-error\nconst mf3: (x: number, y: number) => number = i3.m;\n"
            ),
            []
        );
}

#[test]
fn comparable_relation_erases_generics() {
    // eraseGenerics (relation == comparable) now honors the erase
    // parameter: type parameters erase to any, so BOTH the benign
    // and the shape-mismatched as-assertions are comparable —
    // the unused directive (2578) pins the success where the old
    // gate contained the statement (S8-exempt silence).
    assert_eq!(
        checked_rows("declare function id<T>(x: T): T;\nconst c1 = id as (x: number) => number;\n"),
        []
    );
    assert_eq!(
            program_rows(
                "declare function id2<T extends string>(x: T): T;\n// @ts-expect-error\nconst c2 = id2 as (x: number) => boolean;\n"
            ),
            [(2578, Some(49), Some(19))]
        );
}

#[test]
fn same_target_instantiations_compare_pairwise() {
    // Box<number> vs Box<string>: index-to-index (s0 vs t0 fails
    // on number vs string) where the old N×M walk found s1 for
    // every target row and wrongly related.
    assert_eq!(
            checked_rows(
                "interface Box<T> {\n  m(x: T): T;\n  m(x: string): string;\n}\ndeclare const srcb: Box<number>;\nconst dstb: Box<string> = srcb;\n"
            ),
            [(2322, 98, 4)]
        );
    // Compatible instantiations still relate pairwise…
    assert_eq!(
            checked_rows(
                "interface Box<T> {\n  m(x: T): T;\n  m(x: string): string;\n}\ndeclare const src3: Box<number>;\nconst dst: Box<number | string> = src3;\n"
            ),
            []
        );
    // …and the structural twin (distinct targets) keeps N×M.
    assert_eq!(
            checked_rows(
                "interface BoxN {\n  m(x: number): number;\n  m(x: string): string;\n}\ninterface BoxS {\n  m(x: string): string;\n  m(x: string): string;\n}\ndeclare const srcn: BoxN;\nconst dstn: BoxS = srcn;\n"
            ),
            []
        );
}

#[test]
fn generic_predicate_source_infers_through_the_predicate_arm() {
    // applyToReturnTypes' predicate arm (68224-68237) feeds the
    // arm's iSICO: T[] ⇐ number[] under the predicate types.
    assert_eq!(
            checked_rows(
                "declare function isArr<T>(x: unknown): x is T[];\nconst gp: (x: unknown) => x is number[] = isArr;\n"
            ),
            []
        );
}

#[test]
fn predicate_argument_passthrough_row_surfaces() {
    // The argument check's predicate relation succeeds; the call
    // types and the return-annotation mismatch surfaces.
    assert_eq!(
            checked_rows(
                "declare function isCat(x: unknown): x is string;\ndeclare function take(p: (x: unknown) => x is string): number;\nconst ok: string = take(isCat);\n"
            ),
            [(2322, 118, 2)]
        );
}

// ---- M6 7.5d review fixes: regression pins. Every fixture
// oracle-probed against vendored 6.0.3 noLib (2026-07-21,
// scratchpad probe-review.cjs / probe-port). ----

#[test]
fn forward_constraint_generic_resolves_through_the_parked_frame() {
    // Blocker fix: <T extends U, U extends string> — resolving
    // slot T instantiates its constraint through the DEFERRED
    // non-fixing mapper, re-entering slot U (and U's clamp)
    // MID-iSICO; pre-7.5d the parameter-threaded loan missed the
    // thunk path and the RelationFrame dispatch panicked. Pass
    // face: tsc clean — zero containment proves the re-entrant
    // path completes live.
    assert_eq!(
            rows_and_partials(
                "declare function f<T extends U, U extends string>(x: T, y: U): void;\nconst g: (x: \"a\", y: \"a\") => void = f;\n"
            ),
            (vec![], 0)
        );
    // Fail face: 2322 renders at the 9.3b2 signature rung
    // (oracle-probed byte row).
    assert_eq!(
            rows_and_partials(
                "declare function f<T extends U, U extends string>(x: T, y: U): void;\nconst g: (x: \"a\", y: \"b\") => void = f;\n"
            ),
            (vec![(2322, 75, 1)], 0)
        );
}

#[test]
fn forward_constraint_object_member_re_enters_during_the_clamp() {
    // The InFlight face: T's clamp WALKS the instantiated
    // { u: U } constraint, whose lazy member resolution re-enters
    // slot U while the frame is checked out — the fresh-sub-walk
    // fallback (engine.rs RelationFrameLoan::InFlight) carries
    // it. tsc: clean / 2322-behind-the-curtain.
    assert_eq!(
            rows_and_partials(
                "declare function f2<T extends { u: U }, U extends string>(x: T, y: U): void;\nconst g2: (x: { u: \"b\" }, y: \"b\") => void = f2;\n"
            ),
            (vec![], 0)
        );
    // The fail face renders at the 9.3b2 signature rung
    // (oracle-probed byte row).
    assert_eq!(
            rows_and_partials(
                "declare function f2<T extends { u: U }, U extends string>(x: T, y: U): void;\nconst g2: (x: { u: \"a\" }, y: \"b\") => void = f2;\n"
            ),
            (vec![(2322, 83, 2)], 0)
        );
}

#[test]
fn this_parameter_blocks_the_body_inferred_predicate() {
    // tsc iterates func.parameters INCLUDING the this-parameter
    // (79049's forEach index feeds createTypePredicate), so a
    // leading `this: object` yields no USABLE predicate for x —
    // overload 2 (boolean → string) wins and the 2322 reports.
    // tsc-probed q8 (vendored 6.0.3 noLib): port row-identical,
    // no off-by-one divergence.
    assert_eq!(
            rows_and_partials(
                "function isStr(this: object, x: unknown) { return typeof x === \"string\"; }\ndeclare function take(p: (this: object, x: unknown) => x is string): number;\ndeclare function take(p: (this: object, x: unknown) => boolean): string;\nconst n: number = take(isStr);\n",
            ),
            (vec![(2322, 231, 1)], 0)
        );
}

#[test]
fn body_inferred_predicates_decide_for_real() {
    // m6 7.6 flip of the 7.5d containment pins: the body-
    // inference arm (getTypePredicateFromBody, 79019-79074) is
    // LIVE, so these faces DECIDE. tsc-probed q1a/q1b/q1c
    // (vendored 6.0.3 noLib).
    // Overloads: `x is string` is inferred from isStr's body and
    // overload 1 resolves — clean, no containment.
    assert_eq!(
            rows_and_partials(
                "function isStr(x: unknown) { return typeof x === \"string\"; }\ndeclare function take(p: (x: unknown) => x is string): number;\ndeclare function take(p: (x: unknown) => boolean): string;\nconst n: number = take(isStr);\n"
            ),
            (vec![], 0)
        );
    // Override compat: body-inferred predicates on BOTH sides —
    // tsc reports (2416, 82, 3); the row's args are function
    // displays, so the port renders or contains by the display
    // slice, never fabricates.
    assert_eq!(
            rows_and_partials(
                "class A { isS(x: unknown) { return typeof x === \"string\"; } }\nclass B extends A { isS(x: unknown) { return typeof x === \"number\"; } }\n"
            ),
            (vec![(2416, 82, 3)], 0)
        );
    // The inferred source predicate satisfies the annotated
    // target — clean.
    assert_eq!(
            rows_and_partials(
                "function isStr(x: unknown) { return typeof x === \"string\"; }\nconst f: (x: unknown) => x is string = isStr;\n"
            ),
            (vec![], 0)
        );
}

#[test]
fn body_inferred_guard_leaves_plain_boolean_helpers_live() {
    // The related arm consults the source only under a
    // target-side predicate (tsc order), so an unannotated
    // boolean helper against a plain boolean target never
    // reaches the guard — zero containment.
    assert_eq!(
            rows_and_partials(
                "function isPos(x: number) { return x > 0; }\nconst p: (x: number) => boolean = isPos;\n"
            ),
            (vec![], 0)
        );
}

#[test]
fn instantiated_generic_parameter_suppresses_callback_treatment() {
    // Major fix: 64549-64550's SECOND suppression disjunct —
    // I<(x: string) => void>'s m keeps signature.target whose
    // v-position is T (generic), so the position takes the plain
    // bivariant compare (the fewer-params source leg passes),
    // NOT the callback recursion whose arity check wrongly
    // Falsed. tsc: clean.
    assert_eq!(
            rows_and_partials(
                "interface I<T> { m(v: T): void }\ninterface J { m(v: (x: string, y: number) => void): void }\ndeclare const a: I<(x: string) => void>;\nconst b: J = a;\n"
            ),
            (vec![], 0)
        );
    // Control: annotation-derived positions carry no
    // signature.target, so callback treatment SURVIVES — the
    // arity-incompatible pair still Falses, and the 2322 renders
    // at the 9.3b2 signature rung (oracle-probed byte row).
    assert_eq!(
            rows_and_partials(
                "declare function on(cb: (x: string) => void): void;\ndeclare const h: (x: string, y: number) => void;\nconst c: (cb: (x: string, y: number) => void) => void = on;\n"
            ),
            (vec![(2322, 107, 1)], 0)
        );
}

#[test]
fn predicate_type_compare_arm_relates_live() {
    // The both-Some UNEQUAL-types cell ('a' vs string) — the
    // compareTypes arm proper, not the ty == ty shortcut; zero
    // containment proves the verdict is live (the pre-7.5d 2578
    // control only ever exercised the shortcut).
    assert_eq!(
            rows_and_partials(
                "declare function isLit(x: unknown): x is \"a\";\nconst cf: (x: unknown) => x is string = isLit;\n"
            ),
            (vec![], 0)
        );
}

#[test]
fn predicate_parameter_index_match_relates_live() {
    // The nonzero-index positive twin of the mismatch pin:
    // index 1 == 1 passes the 64614 check and the relation
    // completes with zero containment.
    assert_eq!(
            rows_and_partials(
                "declare function isB(a: unknown, b: unknown): b is string;\nconst m2: (a: unknown, b: unknown) => b is string = isB;\n"
            ),
            (vec![], 0)
        );
}

#[test]
fn relation_error_state_generic_mapped_cleanup_preserves_the_tsc_boundary() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, out: &mut Vec<(u32, String)>) {
        out.push((chain.code, chain.text.clone()));
        for child in &chain.next {
            flatten(child, out);
        }
    }

    fn error_chains(text: &str, options: &CompilerOptions) -> Vec<Vec<(u32, String)>> {
        crate::state::test_support::with_program_state(&[("a.ts", text)], options, |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.is_some()
                        && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
                })
                .map(|diagnostic| {
                    let mut chain = Vec::new();
                    flatten(&diagnostic.message, &mut chain);
                    chain
                })
                .collect()
        })
    }

    let options = CompilerOptions {
        strict_null_checks: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };

    assert_eq!(
        error_chains(
            "type Partial<T> = { [P in keyof T]?: T[P] };\n\
                 type Thing = { a: string, b: string };\n\
                 function f<T extends Thing>(x: Partial<Thing>, y: Partial<T>) {\n\
                     y = x;\n\
                 }\n",
            &options,
        ),
        [vec![(
            2322,
            "Type 'Partial<Thing>' is not assignable to type 'Partial<T>'.".to_owned(),
        )]],
        "a non-generic mapped source cleans up the speculative detail"
    );

    assert_eq!(
        error_chains(
            "function g<T, U extends T>(\n\
                     x: { [P in keyof T]: T[P] },\n\
                     y: { [P in keyof T]: U[P] },\n\
                 ) {\n\
                     y = x;\n\
                 }\n",
            &options,
        ),
        [vec![
            (
                2322,
                "Type '{ [P in keyof T]: T[P]; }' is not assignable to type \
                     '{ [P in keyof T]: U[P]; }'."
                    .to_owned(),
            ),
            (
                2322,
                "Type 'T[P]' is not assignable to type 'U[P]'.".to_owned(),
            ),
            (2322, "Type 'T' is not assignable to type 'U'.".to_owned(),),
            (
                5082,
                "'U' could be instantiated with an arbitrary type which could be unrelated \
                     to 'T'."
                    .to_owned(),
            ),
        ]],
        "a generic mapped source bypasses cleanup and keeps its detail"
    );
}

#[test]
fn relation_reporting_keeps_union_keyof_and_class_member_failure_levels() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, out: &mut Vec<(u32, String)>) {
        out.push((chain.code, chain.text.clone()));
        for child in &chain.next {
            flatten(child, out);
        }
    }

    fn error_chains(files: &[(&str, &str)], options: &CompilerOptions) -> Vec<Vec<(u32, String)>> {
        crate::state::test_support::with_program_state(files, options, |state| {
            for index in 0..files.len() {
                state.check_source_file(index);
            }
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.is_some()
                        && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
                })
                .map(|diagnostic| {
                    let mut chain = Vec::new();
                    flatten(&diagnostic.message, &mut chain);
                    chain
                })
                .collect()
        })
    }

    let strict = CompilerOptions {
        strict: Some(true),
        strict_null_checks: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    assert_eq!(
        error_chains(
            &[(
                "a.ts",
                "interface Array<T> { [index: number]: T }\n\
                     let x = [{ a: 1, b: 2 }, { a: \"abc\" }, {}][0];\n\
                     x = { a: \"value\", b: 1 };\n",
            )],
            &strict,
        ),
        [vec![
            (
                2322,
                "Type '{ a: string; b: number; }' is not assignable to type \
                     '{ a: number; b: number; } | { a: string; b?: undefined; } | \
                     { a?: undefined; b?: undefined; }'."
                    .to_owned(),
            ),
            (2326, "Types of property 'b' are incompatible.".to_owned(),),
            (
                2322,
                "Type 'number' is not assignable to type 'undefined'.".to_owned(),
            ),
        ]]
    );

    assert_eq!(
        error_chains(
            &[(
                "a.ts",
                "interface Array<T> { [index: number]: T }\n\
                     function f<T extends string[]>(k: keyof [1, 2, ...T]) {\n\
                         k = '2';\n\
                     }\n",
            )],
            &strict,
        ),
        [vec![
            (
                2322,
                "Type 'string' is not assignable to type 'keyof [1, 2, ...T]'.".to_owned(),
            ),
            // This noLib pin declares only a numeric Array index,
            // so its known-key face expands to number | "0" |
            // "1". The vendored-lib conformance fixture retains
            // the canonical `"0" | "1" | keyof T[]` face.
            (
                2322,
                "Type '\"2\"' is not assignable to type 'number | \"0\" | \"1\"'.".to_owned(),
            ),
        ]]
    );

    assert_eq!(
        error_chains(
            &[(
                "a.ts",
                "interface Array<T> { [index: number]: T }\n\
                     class Base {\n\
                         load(supplies?: any[]): void {}\n\
                         static circle(wagons?: Base[]): number { return 0; }\n\
                     }\n\
                     class DerivedInstance extends Base {\n\
                         load(files: string[], format: \"csv\" | \"json\"): void {}\n\
                     }\n\
                     class DerivedStatic extends Base {\n\
                         static circle(others: (typeof Base)[]): number { return 0; }\n\
                     }\n",
            )],
            &strict,
        ),
        [
            vec![
                (
                    2416,
                    "Property 'load' in type 'DerivedInstance' is not assignable to the same \
                         property in base type 'Base'."
                        .to_owned(),
                ),
                (
                    2322,
                    "Type '(files: string[], format: \"csv\" | \"json\") => void' is not \
                         assignable to type '(supplies?: any[] | undefined) => void'."
                        .to_owned(),
                ),
                (
                    2849,
                    "Target signature provides too few arguments. Expected 2 or more, but got \
                         1."
                    .to_owned(),
                ),
            ],
            vec![
                (
                    2417,
                    "Class static side 'typeof DerivedStatic' incorrectly extends base class \
                         static side 'typeof Base'."
                        .to_owned(),
                ),
                (
                    2326,
                    "Types of property 'circle' are incompatible.".to_owned(),
                ),
                (
                    2322,
                    "Type '(others: (typeof Base)[]) => number' is not assignable to type \
                         '(wagons?: Base[] | undefined) => number'."
                        .to_owned(),
                ),
                (
                    2328,
                    "Types of parameters 'others' and 'wagons' are incompatible.".to_owned(),
                ),
                (
                    2322,
                    "Type 'Base[] | undefined' is not assignable to type \
                         '(typeof Base)[]'."
                        .to_owned(),
                ),
                (
                    2322,
                    "Type 'undefined' is not assignable to type '(typeof Base)[]'.".to_owned(),
                ),
            ],
        ]
    );

    assert_eq!(
        error_chains(
            &[(
                "a.ts",
                "let okUnion: { a: number } | { a: string };\n\
                     okUnion = { a: \"value\" };\n\
                     function okKey<T extends string[]>(k: keyof [1, 2, ...T]) {\n\
                         k = '0';\n\
                     }\n\
                     class OkBase { load(supplies?: any[]): void {} }\n\
                     class OkDerived extends OkBase { load(supplies?: any[]): void {} }\n",
            )],
            &strict,
        ),
        Vec::<Vec<(u32, String)>>::new(),
        "non-firing siblings remain clean"
    );
}
