use crate::{check_program_with_libs_at, CheckResult, InputFile};
use tsc_types::CompilerOptions;

fn focused_default_library() -> InputFile {
    InputFile::new(
        "/lib.d.ts",
        concat!(
            "interface IArguments {}\n",
            "interface Array<T> {}\n",
            "interface Object {}\n",
            "interface Function {}\n",
            "interface CallableFunction extends Function {}\n",
            "interface NewableFunction extends Function {}\n",
            "interface String {}\n",
            "interface Number {}\n",
            "interface Boolean {}\n",
            "interface RegExp {}\n",
        ),
    )
}

fn check(files: &[InputFile], stable_type_ordering: Option<bool>) -> CheckResult {
    let libs = [focused_default_library()];
    let options = CompilerOptions {
        strict: Some(true),
        stable_type_ordering,
        ..CompilerOptions::default()
    };
    check_program_with_libs_at(&libs, files, &options, "/")
}

fn messages(result: &CheckResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic
                .message_text()
                .as_str()
                .expect("message text is UTF-8")
                .to_owned()
        })
        .collect()
}

const BOX: &str = "interface Box<out A> { readonly box: (_: never) => A }\n";

/// tsc 6.0.3's default order is the type creation order; `stableTypeOrdering`
/// (tsc 6.0.3's preview of the TypeScript 7 order, which tsc-rs's own
/// TypeScript 7.1 profile applies unless the option is set to false) ranks
/// the two references by their type arguments, `string` (TypeFlags 32)
/// before `number` (64), whichever was created first.
#[test]
fn stable_type_ordering_orders_union_members_by_content_not_by_creation() {
    for (declarations, default_order) in [
        (
            "declare const n: Box<number>\ndeclare const s: Box<string>\n",
            "Box<number> | Box<string>",
        ),
        (
            "declare const s: Box<string>\ndeclare const n: Box<number>\n",
            "Box<string> | Box<number>",
        ),
    ] {
        let text = format!(
            "{BOX}{declarations}declare const cond: boolean\nconst r: never = cond ? n : s\n"
        );
        let files = [InputFile::new("/main.ts", text)];

        let creation_order = messages(&check(&files, Some(false)));
        assert_eq!(
            creation_order,
            [format!(
                "Type '{default_order}' is not assignable to type 'never'."
            )],
            "creation order for {declarations:?}"
        );

        for configured in [Some(true), None] {
            let stable = messages(&check(&files, configured));
            assert_eq!(
                stable,
                [
                    "Type 'Box<string> | Box<number>' is not assignable to type 'never'."
                        .to_owned()
                ],
                "stable order for {declarations:?} with {configured:?}"
            );
        }
    }
}

/// The inference of `tapError`'s success type from the union
/// `Effect<A1, E1, R1> | Effect<void, never, never>` (effect's Stream.ts)
/// picks the first candidate. In the default order that is whichever type
/// was created first, so tsc itself errors when the `void` declaration is
/// checked after its use; the stable order ranks `void` before a type
/// parameter and the program checks the same way in either file order.
#[test]
fn stable_type_ordering_makes_inference_independent_of_file_order() {
    let effect = InputFile::new(
        "/effect.ts",
        concat!(
            // The elaboration of a failing arrow function body asks for the
            // global `Promise` (createPromiseType); this program has no
            // library, so it declares one.
            "interface Promise<T> { readonly promised: T }\n",
            "interface Variance<A, E, R> {\n",
            "  _A: (_: never) => A\n",
            "  _E: (_: never) => E\n",
            "  _R: (_: never) => R\n",
            "}\n",
            "interface Effect<out A, out E = never, out R = never> {\n",
            "  readonly effect: Variance<A, E, R>\n",
            "}\n",
            "interface Stream<out A, out E = never, out R = never> {\n",
            "  readonly stream: Variance<A, E, R>\n",
            "}\n",
            "declare function tapError<A, E, R, A2, E2, R2>(\n",
            "  self: Stream<A, E, R>,\n",
            "  f: (error: E) => Effect<A2, E2, R2>\n",
            "): Stream<A, E | E2, R | R2>\n",
        ),
    );
    let use_site = InputFile::new(
        "/use.ts",
        concat!(
            "const tapErrorIf = <A, E, R, A1, E1, R1>(\n",
            "  self: Stream<A, E, R>,\n",
            "  f: (e: E) => Effect<A1, E1, R1>,\n",
            "  p: (e: E) => boolean\n",
            "): Stream<A, E | E1, R | R1> =>\n",
            "  tapError(self, (error) => p(error) ? f(error) : voidEffect)\n",
        ),
    );
    let void_declaration = InputFile::new("/void.ts", "declare const voidEffect: Effect<void>\n");

    let void_first = [effect.clone(), void_declaration.clone(), use_site.clone()];
    let void_last = [effect, use_site, void_declaration];

    // tsc 6.0.3 without the option: clean with the declaration first, the
    // creation-order error with it last.
    assert_eq!(
        messages(&check(&void_first, Some(false))),
        Vec::<String>::new()
    );
    let default_void_last = messages(&check(&void_last, Some(false)));
    assert_eq!(default_void_last.len(), 1, "{default_void_last:?}");
    assert!(
        default_void_last[0].starts_with(
            "Type 'Effect<A1, E1, R1> | Effect<void, never, never>' is not assignable to type 'Effect<A1, E1, R1>'."
        ),
        "{default_void_last:?}"
    );

    // tsc 6.0.3 --stableTypeOrdering, and tsc-rs's default: clean in both
    // orders.
    for configured in [Some(true), None] {
        assert_eq!(
            messages(&check(&void_first, configured)),
            Vec::<String>::new()
        );
        assert_eq!(
            messages(&check(&void_last, configured)),
            Vec::<String>::new()
        );
    }
}

/// tsgo orders string literal types, tuple element labels and symbol names
/// with `strings.Compare` on its Go strings: byte (code point) order, not
/// JavaScript's UTF-16 order. The two differ for a character above U+FFFF
/// against one in U+E000..=U+FFFF; lone surrogates sort by their code point.
/// The expected texts are tsgo 7.1.0-dev `aa814927`'s messages.
#[test]
fn stable_type_ordering_compares_texts_in_code_point_order() {
    for (source, expected) in [
        (
            "const u: \"\\u{1F600}\" | \"\\uFFFF\" | \"\\uE000\" | \"a\" | \"\\u{10000}\" = 1;\n",
            "Type '1' is not assignable to type '\"a\" | \"\u{E000}\" | \"\u{FFFF}\" | \"\u{10000}\" | \"\u{1F600}\"'.",
        ),
        (
            "const u: \"\\uD800\" | \"\\uE000\" | \"\\uD7FF\" | \"\\u{1F600}\" | \"\\uDC00\" | \"\\uD83D\\uDE00\" | \"\\uFFFD\" = 1;\n",
            "Type '1' is not assignable to type '\"\u{D7FF}\" | \"\\uD800\" | \"\\uDC00\" | \"\u{E000}\" | \"\u{FFFD}\" | \"\u{1F600}\"'.",
        ),
        (
            "const t: [\u{1D465}: string] | [\u{FF41}: string] | [b: string] = 1;\n",
            "Type 'number' is not assignable to type '[b: string] | [\u{FF41}: string] | [\u{1D465}: string]'.",
        ),
    ] {
        let files = [InputFile::new("/main.ts", source)];
        assert_eq!(
            messages(&check(&files, None)),
            [expected.to_owned()],
            "{source:?}"
        );
    }
}

/// createTypeofType (50137): the option sorts the `typeof` facts.
#[test]
fn stable_type_ordering_sorts_the_typeof_union() {
    let files = [InputFile::new(
        "/main.ts",
        "declare const value: unknown\nconst t: never = typeof value\n",
    )];
    assert_eq!(
        messages(&check(&files, Some(false))),
        ["Type '\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"' is not assignable to type 'never'.".to_owned()]
    );
    for configured in [Some(true), None] {
        assert_eq!(
            messages(&check(&files, configured)),
            ["Type '\"bigint\" | \"boolean\" | \"function\" | \"number\" | \"object\" | \"string\" | \"symbol\" | \"undefined\"' is not assignable to type 'never'.".to_owned()]
        );
    }
}

fn tied_symbols(state: &mut crate::state::CheckerState<'_>, n: usize) -> Vec<tsc_types::SymbolId> {
    (0..n)
        .map(|_| {
            state.binder.create_symbol(
                tsc_types::SymbolFlags::PROPERTY,
                tsc_types::known_name!("same"),
            )
        })
        .collect()
}

// These histories are pinned by Go 1.27.1's slices.SortFunc and
// SortStableFunc, with compareSymbols' left-before-right lazy ID fallback.
const GO_LAZY_13: [usize; 13] = [6, 3, 9, 1, 12, 11, 10, 8, 7, 2, 5, 4, 0];

#[test]
fn inferred_parameters_and_depth_targets_use_go_pdqsort_history() {
    use crate::state::test_support::with_program_state;
    for by_depth in [false, true] {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let symbols = tied_symbols(state, 13);
            let original: Vec<_> = symbols
                .iter()
                .map(|&symbol| {
                    let ty = state.tables.create_synthesized_type_parameter(None);
                    state.tables.type_mut(ty).symbol = Some(symbol);
                    ty
                })
                .collect();
            let mut types = original.clone();
            if by_depth {
                state.sort_types_by_depth(&mut types).unwrap();
            } else {
                super::order_ctx!(state).sort_types(&state.tables, &mut types);
            }
            let expected: Vec<_> = GO_LAZY_13.iter().map(|&i| original[i]).collect();
            assert_eq!(types, expected);
            // Rust's insertion history assigns ID 0 to input 1 and retains
            // 2..12. Go's pdqsort starts with its median-of-three probes.
            assert_ne!(
                types,
                [vec![original[1], original[0]], original[2..].to_vec()].concat()
            );
        });
    }
}

#[test]
fn symbol_sort_uses_lazy_ids_across_order_contexts_and_cached_keys() {
    use crate::state::test_support::with_program_state;
    use tsc_types::TypeOrderContext;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let original = tied_symbols(state, 13);
        let mut symbols = original.clone();
        state.sort_symbols(&mut symbols);
        assert_eq!(symbols, GO_LAZY_13.map(|i| original[i]));
        // Construct another context: ID allocation must not restart, and
        // direct comparisons must share the cached-key sort's IDs.
        assert_eq!(
            super::order_ctx!(state).compare_symbols(Some(original[6]), Some(original[0])),
            std::cmp::Ordering::Less
        );
        let mut reversed = original.iter().rev().copied().collect::<Vec<_>>();
        state.sort_symbols(&mut reversed);
        assert_eq!(reversed, symbols);
    });
}

#[test]
fn named_members_sort_contained_then_other_with_separate_go_histories() {
    use crate::state::test_support::with_program_state;
    use tsc_syntax::SyntaxKind;
    use tsc_types::SymbolFlags;
    with_program_state(
        &[("a.ts", "class C { x: number; }")],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let class = source
                .arena
                .node_ids()
                .find(|&node| source.arena.node(node).kind == SyntaxKind::ClassDeclaration)
                .unwrap();
            let member = source
                .arena
                .node_ids()
                .find(|&node| source.arena.node(node).kind == SyntaxKind::PropertyDeclaration)
                .unwrap();
            let container = state
                .binder
                .create_symbol(SymbolFlags::CLASS, tsc_types::known_name!("C"));
            state.binder.symbol_mut(container).declarations.push(class);
            let original = tied_symbols(state, 26);
            for &symbol in original.iter().step_by(2) {
                // Give each synthetic symbol a contained value declaration,
                // leaving its comparison declarations empty to exercise ties.
                state.binder.symbol_mut(symbol).value_declaration = Some(member);
            }
            let mut symbols = original.clone();
            state.order_named_members_if_stable(&mut symbols, Some(container));
            let expected: Vec<_> = GO_LAZY_13
                .iter()
                .map(|&i| original[2 * i])
                .chain(GO_LAZY_13.iter().map(|&i| original[2 * i + 1]))
                .collect();
            assert_eq!(symbols, expected);
        },
    );
}

#[test]
fn accessible_chains_use_go_stable_history_instead_of_linear_minimum() {
    use crate::state::test_support::with_program_state;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let original = tied_symbols(state, 41);
        let mut chains: Vec<_> = original.iter().map(|&s| vec![s]).collect();
        super::order_ctx!(state).sort_symbol_chains(&mut chains);
        let mut go_order: Vec<_> = (0..41).collect();
        go_order.swap(0, 1);
        go_order.swap(20, 21);
        assert_eq!(
            chains,
            go_order
                .iter()
                .map(|&i| vec![original[i]])
                .collect::<Vec<_>>()
        );
        assert_eq!(chains[0], [original[1]]);
        assert_ne!(
            chains[0],
            [original[0]],
            "the old linear minimum assigns the first ID to input 0"
        );
    });
}

#[test]
fn ambient_modules_keep_the_cached_seen_map_order() {
    use crate::state::test_support::with_program_state;
    use tsc_types::{EscapedName, SymbolFlags};
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let symbols: Vec<_> = ["\"z\"", "\"a\"", "\"pattern*\""]
            .iter()
            .map(|name| {
                state.binder.create_symbol(
                    SymbolFlags::VALUE_MODULE,
                    EscapedName::from_escaped_text((*name).into()),
                )
            })
            .collect();
        std::sync::Arc::make_mut(&mut state.globals)
            .insert(tsc_types::known_name!("\"a\""), symbols[1]);
        std::sync::Arc::make_mut(&mut state.globals)
            .insert(tsc_types::known_name!("\"z\""), symbols[0]);
        for &symbol in &[symbols[1], symbols[2], symbols[2]] {
            state
                .pattern_ambient_modules
                .push(("".into(), "".into(), symbol));
        }
        let expected = vec![symbols[1], symbols[0], symbols[2]];
        assert_eq!(state.get_ambient_modules(), expected);
        state.pattern_ambient_modules.clear();
        assert_eq!(
            state.get_ambient_modules(),
            expected,
            "first result is cached like Go's sync.Once"
        );
    });
}

#[test]
fn symbol_tie_ids_respect_prior_hot_and_cold_link_reads() {
    use crate::state::test_support::with_program_state;
    use tsc_types::TypeOrderContext;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let symbols = tied_symbols(state, 5);
        // Link IDs use Go's high block namespace and are assigned on
        // reads, including absent cold fields, not just writes.
        state.links.value_symbol(symbols[1]);
        state.links.symbol_cold().write_type.get(symbols[0]);
        let order = super::order_ctx!(state);
        assert_eq!(
            order.compare_symbols(Some(symbols[0]), Some(symbols[1])),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            order.compare_symbols(Some(symbols[0]), Some(symbols[2])),
            std::cmp::Ordering::Greater,
            "fresh direct IDs sort below preallocated link IDs"
        );
        let direct = state.links.symbol_order_ids().get(symbols[2]);
        state.links.value_symbol(symbols[2]);
        assert_eq!(state.links.symbol_order_ids().get(symbols[2]), direct);
        state
            .links
            .read_symbol(symbols[4], |links| links.declared_type.get());
        state.links.symbol_cold().key_type.get(symbols[4]);
        assert!(
            state.links.symbol_order_ids().get(symbols[4]) < 0x1_0000_0000_0000,
            "pointer-keyed declared/mapped links do not allocate Go block IDs"
        );
        let before = state.links.symbol_order_ids().get(symbols[3]);
        assert_eq!(
            order.compare_symbols(Some(symbols[3]), Some(symbols[3])),
            std::cmp::Ordering::Equal
        );
        assert_eq!(state.links.symbol_order_ids().get(symbols[3]), before);
    });
}

#[test]
fn exported_alias_names_use_go_sort_history_and_identity_compaction() {
    use crate::links::LinkSlot;
    use crate::state::test_support::with_program_state;
    use tsc_types::{known_name, EscapedName, SymbolFlags};
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let parent = state
            .binder
            .create_symbol(SymbolFlags::VALUE_MODULE, known_name!("parent"));
        let target = state
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, known_name!("target"));
        let aliases: Vec<_> = (0..13)
            .map(|_| {
                state
                    .binder
                    .create_symbol(SymbolFlags::ALIAS, known_name!("same"))
            })
            .collect();
        for (i, &alias) in aliases.iter().enumerate() {
            state
                .links
                .set_fresh_symbol_alias_target(alias, LinkSlot::Resolved(target));
            let name = format!("export{i}");
            std::sync::Arc::make_mut(state.binder.symbol_mut(parent).exports_mut())
                .insert(EscapedName::escape(name.as_str().into()), alias);
        }
        // Duplicate export slots for one symbol compact by identity, with
        // the last name retained before sorting (Go's map[symbol]name).
        // On these 13 fresh ties Go first numbers alias 6; the old linear
        // minimum first numbered alias 0 and selected export0.
        std::sync::Arc::make_mut(state.binder.symbol_mut(parent).exports_mut())
            .insert(known_name!("last"), aliases[6]);
        assert_eq!(
            state.exported_name_of_chain_link(parent, target).unwrap(),
            Some("last".into())
        );
    });
}

#[test]
fn type_only_symbol_dispatch_leaves_lazy_ties_unallocated() {
    use crate::state::test_support::with_program_state;
    use tsc_types::TypeOrderContext;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let first = state.binder.create_symbol(
            tsc_types::SymbolFlags::TYPE_PARAMETER,
            tsc_types::known_name!("same"),
        );
        let second = state.binder.create_symbol(
            tsc_types::SymbolFlags::TYPE_PARAMETER,
            tsc_types::known_name!("same"),
        );
        assert_eq!(
            state.get_type_of_symbol(first).unwrap(),
            state.tables.intrinsics.error
        );
        // Go's getTypeOfSymbol tail returns errorType without value links.
        // The first comparator therefore numbers its left operand first.
        assert!(super::order_ctx!(state)
            .compare_symbols(Some(first), Some(second))
            .is_lt());
    });
}

#[test]
fn require_fast_path_keeps_go_value_link_id_history() {
    use crate::state::test_support::with_program_state;
    use tsc_types::TypeOrderContext;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let symbols = tied_symbols(state, 2);
        state.require_symbol = symbols[0];
        assert_eq!(
            state.get_type_of_symbol(symbols[0]).unwrap(),
            state.tables.intrinsics.any
        );
        // Go enters the value-link getter before its synthetic-require
        // shortcut. Its block ID sorts above a fresh direct comparison ID.
        assert!(super::order_ctx!(state)
            .compare_symbols(Some(symbols[0]), Some(symbols[1]))
            .is_gt());
    });
}

#[test]
fn export_target_links_defer_ids_until_value_links() {
    use crate::links::LinkSlot;
    use crate::state::test_support::with_program_state;
    use tsc_types::TypeOrderContext;
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let symbols = tied_symbols(state, 2);
        state
            .links
            .set_symbol_export_target(state.speculation_depth, symbols[0], symbols[1]);
        // cloneTypeAsModuleType resolves members between its pointer-keyed
        // export target write and its value-link resolvedType write.
        state.links.value_symbol(symbols[1]);
        state
            .links
            .set_fresh_symbol_type(symbols[0], LinkSlot::Resolved(state.tables.intrinsics.any));
        assert!(super::order_ctx!(state)
            .compare_symbols(Some(symbols[0]), Some(symbols[1]))
            .is_gt());
    });
}
