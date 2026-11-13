use super::{sort_func_like_go, sort_types_like_tsgo};

/// The tsgo stable sort port orders like a stable sort for a total order
/// on every block-size path (below, at and well above the 20-element
/// block) and keeps equal keys in their input order.
#[test]
fn the_tsgo_stable_sort_matches_a_stable_sort_on_total_orders() {
    for n in [0usize, 1, 2, 3, 12, 13, 19, 20, 21, 39, 40, 41, 100, 257] {
        // A deterministic pseudo-random sequence of (key, tag) pairs.
        let mut state = 0x9E37_79B9_u32.wrapping_add(n as u32);
        let items: Vec<(u32, usize)> = (0..n)
            .map(|index| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 7, index)
            })
            .collect();
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        let mut actual = items.clone();
        sort_types_like_tsgo(&mut actual, |a, b| a.0 < b.0);
        assert_eq!(actual, expected, "n = {n}");
    }
}

/// An inconsistent comparison (a cycle) must not panic and must give the
/// order tsgo's algorithm gives: for three elements, one insertion-sort
/// pass over the input order.
#[test]
fn the_tsgo_stable_sort_tolerates_a_cyclic_comparison() {
    // a < b, b < c, c < a.
    let less = |x: char, y: char| matches!((x, y), ('a', 'b') | ('b', 'c') | ('c', 'a'));
    let mut data = ['a', 'b', 'c'];
    sort_types_like_tsgo(&mut data, less);
    assert_eq!(data, ['a', 'b', 'c']);
    let mut data = ['c', 'b', 'a'];
    sort_types_like_tsgo(&mut data, less);
    assert_eq!(data, ['b', 'c', 'a']);
}

/// The Go `slices.SortFunc` port gives Go's order, equal elements included
/// (pdqsort is not stable), on each of its paths: insertion sort, the median
/// of three and Tukey's ninther, a decreasing run reversed, a nearly sorted
/// run finished by partial insertion sort, and partitions of many duplicates.
/// The expected orders are Go 1.27.1's output for the same inputs.
#[test]
fn the_go_sort_port_gives_go_s_order() {
    let mut keyed: Vec<i64> = (0..100).collect();
    sort_func_like_go(&mut keyed, |x, y| ((x * 37) % 10).cmp(&((y * 37) % 10)));
    assert_eq!(
        keyed,
        [
            10, 80, 0, 30, 60, 20, 40, 70, 90, 50, 93, 33, 63, 13, 3, 83, 73, 43, 53, 23, 66, 86,
            96, 36, 76, 56, 26, 16, 46, 6, 29, 79, 99, 89, 69, 9, 19, 59, 49, 39, 72, 62, 42, 2,
            92, 32, 12, 82, 52, 22, 25, 15, 95, 75, 85, 55, 45, 35, 65, 5, 8, 28, 38, 18, 78, 58,
            98, 88, 68, 48, 41, 51, 31, 11, 1, 71, 91, 61, 81, 21, 4, 64, 94, 44, 84, 54, 14, 24,
            74, 34, 37, 87, 7, 17, 77, 27, 67, 97, 57, 47,
        ]
    );

    let mut random = Vec::new();
    let mut state = 2_463_534_242_u32;
    for _ in 0..300 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        random.push(i64::from(state % 1000));
    }
    sort_func_like_go(&mut random, |x, y| (x / 10).cmp(&(y / 10)));
    assert_eq!(
        random,
        [
            4, 5, 14, 19, 29, 28, 23, 30, 35, 40, 55, 56, 65, 63, 70, 76, 82, 89, 87, 82, 86, 91,
            97, 95, 92, 92, 115, 119, 113, 115, 122, 129, 120, 126, 123, 133, 143, 148, 154, 159,
            153, 156, 164, 160, 162, 160, 182, 183, 184, 186, 195, 192, 194, 201, 201, 200, 200,
            216, 210, 218, 211, 218, 220, 226, 228, 224, 220, 236, 235, 233, 236, 244, 240, 243,
            246, 243, 249, 252, 266, 267, 262, 261, 272, 275, 274, 284, 280, 280, 282, 299, 293,
            304, 319, 319, 317, 325, 327, 333, 331, 335, 333, 349, 349, 344, 348, 360, 367, 366,
            368, 379, 373, 370, 376, 381, 392, 391, 401, 402, 407, 416, 410, 414, 417, 414, 418,
            424, 422, 422, 438, 443, 443, 448, 457, 455, 465, 467, 473, 489, 486, 491, 490, 491,
            502, 504, 517, 517, 522, 526, 535, 546, 540, 541, 545, 551, 554, 567, 568, 563, 571,
            571, 577, 585, 581, 592, 603, 607, 609, 606, 602, 614, 615, 623, 628, 632, 636, 630,
            639, 631, 638, 633, 646, 649, 640, 649, 655, 660, 660, 661, 676, 675, 681, 687, 690,
            698, 707, 701, 703, 707, 710, 717, 714, 715, 720, 725, 727, 734, 731, 738, 748, 744,
            748, 742, 747, 748, 756, 752, 754, 767, 764, 763, 769, 766, 767, 760, 772, 781, 784,
            788, 781, 793, 793, 790, 806, 806, 800, 819, 813, 815, 829, 824, 829, 821, 829, 822,
            837, 846, 840, 843, 853, 850, 859, 860, 863, 861, 863, 861, 867, 869, 872, 870, 879,
            884, 882, 880, 902, 906, 906, 909, 929, 925, 924, 924, 922, 936, 931, 935, 936, 943,
            942, 942, 940, 952, 958, 951, 952, 951, 968, 967, 968, 974, 979, 976, 970, 971, 985,
            985, 998, 991, 995, 999,
        ]
    );

    let mut decreasing: Vec<i64> = (0..120).rev().collect();
    sort_func_like_go(&mut decreasing, |x, y| (x / 4).cmp(&(y / 4)));
    assert_eq!(
        decreasing,
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 51, 55, 52, 53, 54, 58, 59, 57, 56, 61, 60, 62, 63, 67, 64, 65, 66,
            68, 71, 69, 70, 74, 72, 73, 75, 76, 77, 78, 79, 83, 80, 81, 82, 86, 87, 85, 84, 90, 88,
            89, 91, 92, 93, 94, 95, 99, 96, 97, 98, 100, 101, 102, 103, 107, 104, 105, 106, 108,
            109, 110, 111, 112, 113, 114, 115, 119, 116, 117, 118,
        ]
    );

    let mut nearly: Vec<i64> = (0..80).collect();
    nearly.swap(10, 11);
    nearly.swap(50, 53);
    sort_func_like_go(&mut nearly, |x, y| (x / 3).cmp(&(y / 3)));
    assert_eq!(
        nearly,
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 53, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
            68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
        ]
    );
}

/// A comparison that is no total order (tsgo's auto-import fix comparison
/// compares some fixes only against their own kind) must not panic, as the
/// standard sorts may, and must give Go's order: here rock-paper-scissors on
/// the value modulo 3, Go 1.27.1's output.
#[test]
fn the_go_sort_port_tolerates_a_comparison_with_no_total_order() {
    let mut cyclic: Vec<i64> = (0..60).map(|index| (index * 7) % 60).collect();
    sort_func_like_go(&mut cyclic, |x, y| match (x % 3 - y % 3 + 3) % 3 {
        1 => std::cmp::Ordering::Less,
        2 => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    assert_eq!(
        cyclic,
        [
            27, 39, 18, 21, 57, 36, 42, 15, 54, 3, 33, 12, 24, 51, 30, 45, 0, 9, 6, 48, 23, 35, 53,
            41, 32, 14, 2, 59, 11, 50, 38, 20, 44, 29, 8, 5, 17, 56, 26, 47, 52, 10, 19, 1, 58, 49,
            22, 31, 43, 37, 4, 28, 40, 16, 55, 25, 13, 7, 46, 34,
        ]
    );
}

/// Hit every member and miss at every insertion rank, including empty and
/// singleton slices and the algorithm cutoffs used by the sort ports.
#[test]
fn go_binary_search_returns_the_lower_bound() {
    use crate::goslices::binary_search_func;
    for n in [0, 1, 2, 3, 8, 12, 13, 20, 21, 64, 256, 257] {
        let values: Vec<i32> = (0..n).map(|i| 2 * i).collect();
        for target in -1..=2 * n {
            let expected = values
                .iter()
                .position(|&v| v >= target)
                .unwrap_or(values.len());
            assert_eq!(
                binary_search_func(&values, &target, i32::cmp),
                (expected, values.get(expected) == Some(&target)),
                "n={n}, target={target}"
            );
        }
    }
    let values = [1, 1, 2, 2, 2, 5, 5];
    for (target, expected) in [
        (0, (0, false)),
        (1, (0, true)),
        (2, (2, true)),
        (3, (5, false)),
        (5, (5, true)),
        (6, (7, false)),
    ] {
        assert_eq!(binary_search_func(&values, &target, i32::cmp), expected);
    }
    // The element and target types can differ, and the target can be unsized.
    assert_eq!(
        binary_search_func(&[("ant", 1), ("cat", 2)], "cat", |row, target| row
            .0
            .cmp(target)),
        (1, true)
    );
}

/// Probe fixtures from the pinned local Go 1.27.1 slices.BinarySearchFunc.
/// The final equality call is deliberate, even for a singleton hit.
#[test]
fn go_binary_search_matches_go_probe_history() {
    use crate::goslices::binary_search_func;
    type SearchFixture<'a> = (&'a [i32], i32, (usize, bool), &'a [usize]);
    let fixtures: &[SearchFixture<'_>] = &[
        (&[], 2, (0, false), &[]),
        (&[2], 1, (0, false), &[0, 0]),
        (&[2], 2, (0, true), &[0, 0]),
        (&[2], 3, (1, false), &[0]),
        (&[0, 2, 4], 2, (1, true), &[1, 0, 1]),
        (&[0, 2, 4], 3, (2, false), &[1, 2, 2]),
        (&[0, 2, 4], 5, (3, false), &[1, 2]),
        (&[0, 2, 4, 6], 0, (0, true), &[2, 1, 0, 0]),
        (&[1, 2, 2, 2, 4], 2, (1, true), &[2, 1, 0, 1]),
    ];
    for &(values, target, expected, probes) in fixtures {
        let indexed: Vec<_> = values.iter().enumerate().collect();
        let mut actual_probes = Vec::new();
        let actual = binary_search_func(&indexed, &target, |&(index, value), target| {
            actual_probes.push(index);
            value.cmp(target)
        });
        assert_eq!(actual, expected);
        assert_eq!(actual_probes, probes, "{values:?}, target={target}");
    }
}

#[derive(Default)]
struct TestOrderContext {
    symbol_comparisons: std::cell::RefCell<Vec<(crate::SymbolId, crate::SymbolId)>>,
}

impl super::TypeOrderContext for TestOrderContext {
    fn compare_symbols(
        &self,
        a: Option<crate::SymbolId>,
        b: Option<crate::SymbolId>,
    ) -> std::cmp::Ordering {
        a.cmp(&b)
    }
    fn compare_symbol_names(&self, a: crate::SymbolId, b: crate::SymbolId) -> std::cmp::Ordering {
        self.symbol_comparisons.borrow_mut().push((a, b));
        a.cmp(&b)
    }
    fn compare_nodes(&self, a: u32, b: u32) -> std::cmp::Ordering {
        a.cmp(&b)
    }
    fn compare_element_labels(&self, a: Option<u32>, b: Option<u32>) -> std::cmp::Ordering {
        a.cmp(&b)
    }
    fn deferred_node(&self, _: crate::TypeId) -> Option<u32> {
        None
    }
    fn object_mapper(&self, _: crate::TypeId) -> Option<crate::MapperId> {
        None
    }
    fn compare_mappers(
        &self,
        _: &crate::TypeTables,
        a: Option<crate::MapperId>,
        b: Option<crate::MapperId>,
    ) -> std::cmp::Ordering {
        a.cmp(&b)
    }
    fn first_declaration(&self, _: Option<crate::SymbolId>) -> Option<u32> {
        None
    }
    fn mapped_instantiation_mapper(
        &self,
        mapper: Option<crate::MapperId>,
    ) -> Option<crate::MapperId> {
        mapper
    }
}

/// Reconstruct the old ID-insert/GST pipeline using the explicit ID path.
/// Compare the whole ordered vector, flags, and interned union identity
/// (including origin and alias keys), with nullable filtering on and off.
#[test]
fn append_first_union_collection_preserves_members_flags_and_keys() {
    use crate::{IdentityDomain, SymbolId, TypeFlags, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    for strict in [false, true] {
        let mut tables = TypeTables::new(strict, true, &IdentityDomain::new());
        let z = tables.get_string_literal_type("z");
        let a = tables.get_string_literal_type("a");
        let m = tables.get_string_literal_type("m");
        let named = tables.get_union_type(Some(&ctx), &[z, a], UnionReduction::None);
        tables.set_alias(named, Some(SymbolId::new(10)), None);
        let origin = tables.create_origin_union_or_intersection_type(TypeFlags::UNION, vec![a, m]);
        let with_origin = tables.get_union_type_with_origin(
            Some(&ctx),
            &[m, a],
            UnionReduction::None,
            Some(origin),
        );
        let i = &tables.intrinsics;
        let cases = [
            vec![],
            vec![z],
            vec![z, z, a, m, z, a],
            vec![a, z, m],
            vec![named, named, m, named, a, with_origin],
            vec![with_origin, named, with_origin, a, m, z],
            vec![
                i.never,
                i.null_widening,
                i.undefined,
                z,
                i.null,
                i.undefined_widening,
                z,
                a,
            ],
            vec![i.wildcard, i.error, i.never, named, a],
        ];
        for input in cases {
            let initial_includes = TypeFlags::INCLUDES_INSTANTIABLE.bits();
            // Also exercise accumulation into an existing member set.
            for initial in [vec![], vec![m]] {
                let mut old = initial.clone();
                let old_includes =
                    tables.add_types_to_union(None, &mut old, initial_includes, &input);
                sort_types_like_tsgo(&mut old, |a, b| {
                    super::compare_types(&tables, &ctx, a, b).is_lt()
                });
                old.dedup();
                let mut actual = initial;
                let includes =
                    tables.add_types_to_union(Some(&ctx), &mut actual, initial_includes, &input);
                assert_eq!(actual, old, "strict={strict}, input={input:?}");
                assert_eq!(includes, old_includes);
                let alias = Some(SymbolId::new(20));
                let arguments = [a, m];
                let expected = tables.finish_union_type_set(
                    Some(&ctx),
                    old,
                    old_includes,
                    &input,
                    alias,
                    Some(&arguments),
                    None,
                );
                let actual = tables.finish_union_type_set(
                    Some(&ctx),
                    actual,
                    includes,
                    &input,
                    alias,
                    Some(&arguments),
                    None,
                );
                assert_eq!(actual, expected, "union key, origin, and alias");
                if tables.flags_of(actual).intersects(TypeFlags::UNION) {
                    assert_eq!(tables.alias_symbol(actual), alias);
                    assert_eq!(
                        tables.alias_type_arguments(actual),
                        Some(arguments.as_slice())
                    );
                }
            }
        }
    }
}

/// Adjacent identical source unions are suppressed, while another
/// occurrence after a filtered never still contributes its members.
#[test]
fn union_merge_preserves_duplicate_source_results() {
    use crate::{IdentityDomain, SymbolId, TypeTables, UnionReduction};
    let mut tables = TypeTables::new(true, false, &IdentityDomain::new());
    let ctx = TestOrderContext::default();
    let literals: Vec<_> = ["z", "m", "a"]
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let ty = tables.get_string_literal_type(text);
            tables.set_alias(ty, Some(SymbolId::new(2 - index as u32)), None);
            ty
        })
        .collect();
    let (z, m, a) = (literals[0], literals[1], literals[2]);
    let left = tables.get_union_type(Some(&ctx), &[a, z], UnionReduction::None);
    let source = [left, left, tables.intrinsics.never, left, m, a, m];
    let mut expected = vec![a, z, a, z, m, a, m];
    ctx.symbol_comparisons.borrow_mut().clear();
    sort_types_like_tsgo(&mut expected, |a, b| {
        super::compare_types(&tables, &ctx, a, b).is_lt()
    });
    expected.dedup();
    let mut actual = Vec::new();
    ctx.symbol_comparisons.borrow_mut().clear();
    tables.add_types_to_union(Some(&ctx), &mut actual, 0, &source);
    assert_eq!(actual, expected);
    let actual_trace = ctx.symbol_comparisons.take();
    let mut without_adjacent_duplicate = Vec::new();
    tables.add_types_to_union(
        Some(&ctx),
        &mut without_adjacent_duplicate,
        0,
        &[left, tables.intrinsics.never, left, m, a, m],
    );
    assert_eq!(without_adjacent_duplicate, actual);
    assert_eq!(ctx.symbol_comparisons.take(), actual_trace);
}

/// Independent flatten/ID-insert oracle, followed by the previous sorter
/// for content order. Include raw nested origin lists and cross-order runs,
/// so merging cannot silently assume every interned list uses this order.
#[test]
fn union_runs_match_flatten_insert_and_sort_in_both_orders() {
    use crate::{
        IdentityDomain, SymbolId, TypeData, TypeFlags, TypeId, TypeTables, UnionReduction,
    };

    fn old_collect(
        t: &mut TypeTables,
        out: &mut Vec<TypeId>,
        mut flags: i32,
        src: &[TypeId],
    ) -> i32 {
        let mut last = None;
        for &ty in src {
            if Some(ty) == last {
                continue;
            }
            if let TypeData::Union { types, origin, .. } = t.type_of(ty).data {
                if t.alias_symbol(ty).is_some() || origin.is_some() {
                    flags |= TypeFlags::UNION.bits();
                }
                let members = t.list(types).to_vec();
                flags = old_collect(t, out, flags, &members);
            } else {
                flags = t.add_type_to_union(None, out, flags, ty);
            }
            last = Some(ty);
        }
        flags
    }

    let ctx = TestOrderContext::default();
    for strict in [false, true] {
        let mut t = TypeTables::new(strict, false, &IdentityDomain::new());
        let literals: Vec<_> = (0..65)
            .map(|n| t.get_string_literal_type(&format!("s{:03}", 64 - n)))
            .collect();
        let content = t.get_union_type(Some(&ctx), &literals[..32], UnionReduction::None);
        let ids = t.get_union_type(None, &literals[16..], UnionReduction::None);
        t.set_alias(ids, Some(SymbolId::new(10)), None);
        let nested = t.create_origin_union_or_intersection_type(
            TypeFlags::UNION,
            vec![content, content, t.intrinsics.never, ids, literals[0], ids],
        );
        let intersection = t.create_origin_union_or_intersection_type(
            TypeFlags::INTERSECTION,
            vec![t.intrinsics.string, t.intrinsics.number],
        );
        t.type_mut(intersection).object_flags = crate::ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE;
        let parameter = t.create_synthesized_type_parameter(None);
        let mut pool = literals.clone();
        pool.extend([
            content,
            ids,
            nested,
            t.intrinsics.never,
            t.intrinsics.undefined,
            t.intrinsics.null,
            t.intrinsics.null_widening,
            t.intrinsics.wildcard,
            t.intrinsics.error,
            intersection,
            parameter,
        ]);
        for order in [None, Some(&ctx as &dyn super::TypeOrderContext)] {
            for n in [0, 1, 2, 3, 19, 20, 21, 41, 100, 257, 1024] {
                let mut seed = 0x9e37_79b9_u32;
                let mut source = Vec::new();
                for _ in 0..n {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let ty = pool[seed as usize % pool.len()];
                    source.extend([ty, ty]);
                }
                for initial in [vec![], vec![literals[33]]] {
                    let initial_flags = TypeFlags::INCLUDES_INSTANTIABLE.bits();
                    let mut expected = initial.clone();
                    let expected_flags = old_collect(&mut t, &mut expected, initial_flags, &source);
                    if let Some(ctx) = order {
                        crate::goslices::sort_union_types(&mut expected, |a, b| {
                            super::compare_types(&t, ctx, a, b).is_lt()
                        });
                        expected.dedup();
                    }
                    let mut actual = initial;
                    let actual_flags =
                        t.add_types_to_union(order, &mut actual, initial_flags, &source);
                    assert_eq!(
                        actual,
                        expected,
                        "strict={strict}, content={}, n={n}",
                        order.is_some()
                    );
                    assert_eq!(actual_flags, expected_flags);
                }
            }
        }
    }
}

#[test]
fn union_member_order_is_total_with_id_only_ties() {
    use crate::{IdentityDomain, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut t = TypeTables::new(true, false, &IdentityDomain::new());
    let a = t.get_string_literal_type("a");
    let z = t.get_string_literal_type("z");
    let fresh = t.get_fresh_type_of_literal_type(a);
    let one = t.get_number_literal_type(1.0);
    let zero = t.get_number_literal_type(0.0);
    let union = t.get_union_type(Some(&ctx), &[a, z], UnionReduction::None);
    let parameter = t.create_synthesized_type_parameter(None);
    let i = &t.intrinsics;
    let types = [
        a,
        z,
        fresh,
        one,
        zero,
        union,
        parameter,
        i.any,
        i.unknown,
        i.string,
        i.number,
        i.boolean,
        i.bigint,
        i.es_symbol,
        i.void,
        i.undefined,
        i.null,
        i.never,
        i.non_primitive,
    ];
    for &a in &types {
        for &b in &types {
            let ab = super::compare_types(&t, &ctx, a, b);
            assert_eq!(ab.is_eq(), a == b);
            assert_eq!(ab, super::compare_types(&t, &ctx, b, a).reverse());
            for &c in &types {
                if ab.is_le() && super::compare_types(&t, &ctx, b, c).is_le() {
                    assert!(super::compare_types(&t, &ctx, a, c).is_le());
                }
            }
        }
    }
}

#[test]
fn content_search_and_single_insertion_share_the_go_lower_bound() {
    use crate::{IdentityDomain, TypeTables};
    let mut tables = TypeTables::new(true, false, &IdentityDomain::new());
    let ctx = TestOrderContext::default();
    let c = tables.get_string_literal_type("c");
    let a = tables.get_string_literal_type("a");
    let b = tables.get_string_literal_type("b");
    let order = Some(&ctx as &dyn super::TypeOrderContext);
    let mut members = vec![a, c];
    assert_eq!(super::search_type(&tables, order, &members, b), Err(1));
    tables.add_type_to_union(order, &mut members, 0, b);
    assert_eq!(members, [a, b, c]);
    assert!(super::contains_type(&tables, order, &members, b));
    assert!(!super::insert_type(&tables, order, &mut members, b));
    tables.add_type_to_union(order, &mut members, 0, a);
    assert_eq!(members, [a, b, c]);
    assert_eq!(super::search_type(&tables, order, &[a, a, a, c], a), Ok(0));
    let ids = [c, c, a, b];
    assert_eq!(
        super::search_type(&tables, None, &ids, c),
        ids.binary_search(&c)
    );
}

/// Tagged equal keys check stability as well as ordering, including existing
/// runs, small tails, duplicate runs and the recursive merge path.
#[test]
fn union_run_merge_matches_the_go_stable_sort() {
    for n in [0, 1, 2, 19, 20, 21, 39, 40, 41, 100, 257, 1024] {
        for run in [1, 2, 7, 20, 31, 257, 1024] {
            let mut data: Vec<_> = (0..n).map(|i| ((i * 37 + 11) % 23, i)).collect();
            for chunk in data.chunks_mut(run) {
                chunk.sort_by_key(|item| item.0);
            }
            let mut expected = data.clone();
            sort_types_like_tsgo(&mut expected, |a, b| a.0 < b.0);
            crate::goslices::sort_union_types(&mut data, |a, b| a.0 < b.0);
            assert_eq!(data, expected, "n={n}, run={run}");
        }
    }
}

#[test]
fn plain_string_order_preserves_wtf8_fresh_id_ties_and_alias_precedence() {
    let mut tables = crate::TypeTables::new(true, true, &crate::IdentityDomain::new());
    let ctx = TestOrderContext::default();
    let bmp = tables.get_string_literal_type("\u{e000}");
    let supplementary = tables.get_string_literal_type("\u{10000}");
    assert!(super::compare_types(&tables, &ctx, bmp, supplementary).is_lt());
    let fresh = tables.get_fresh_type_of_literal_type(bmp);
    assert_eq!(
        super::compare_types(&tables, &ctx, bmp, fresh),
        bmp.index().cmp(&fresh.index())
    );
    let z = tables.get_string_literal_type("z");
    let a = tables.get_string_literal_type("a");
    tables.set_alias(z, Some(crate::SymbolId::new(1)), None);
    tables.set_alias(a, Some(crate::SymbolId::new(2)), None);
    assert!(super::compare_types(&tables, &ctx, z, a).is_lt());
}

#[test]
fn named_union_membership_retains_search_behavior_across_ordering_modes() {
    let mut tables = crate::TypeTables::new(true, false, &crate::IdentityDomain::new());
    let ctx = TestOrderContext::default();
    let z = tables.get_string_literal_type("z");
    let a = tables.get_string_literal_type("a");
    let named = tables.get_union_type(None, &[z, a], crate::UnionReduction::None);
    tables.set_alias(named, Some(crate::SymbolId::new(1)), None);
    let m = tables.get_string_literal_type("m");
    let order = Some(&ctx as &dyn super::TypeOrderContext);
    let members = match tables.type_of(named).data {
        crate::TypeData::Union { types, .. } => tables.list(types),
        _ => panic!("union"),
    };
    let legacy: Vec<_> = [a, m, z]
        .into_iter()
        .filter(|&t| !super::contains_type(&tables, order, members, t))
        .collect();
    assert_eq!(
        legacy,
        [m, z],
        "retain the existing search, including a differently ordered list"
    );
    let outer = tables.get_union_type(order, &[named, m], crate::UnionReduction::None);
    let crate::TypeData::Union { types, origin, .. } = tables.type_of(outer).data else {
        panic!("union");
    };
    assert_eq!(tables.list(types), [a, m, z]);
    assert!(
        origin.is_none(),
        "the original overlap count did not create an origin"
    );

    let content_named = tables.get_union_type(order, &[z, a], crate::UnionReduction::None);
    tables.set_alias(content_named, Some(crate::SymbolId::new(2)), None);
    let outer = tables.get_union_type(None, &[content_named, m], crate::UnionReduction::None);
    let crate::TypeData::Union { types, origin, .. } = tables.type_of(outer).data else {
        panic!("union");
    };
    assert_eq!(tables.list(types), [z, a, m]);
    assert!(
        origin.is_none(),
        "also retain content-ordered members searched by ID"
    );
}
/// The subset test of two unions walks the target once when both lists were
/// built in the asked order, and answers what a search per member answers:
/// subsets and near-subsets of every size, a source larger than its target,
/// two members looked for in a very large target (the searches stay), and
/// lists built in the other order (the searches stay).
#[test]
fn union_subset_walk_answers_what_a_search_per_member_answers() {
    use crate::{IdentityDomain, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut t = TypeTables::new(true, false, &IdentityDomain::new());
    // Ids ascend while the texts descend: the two member orders differ.
    let literals: Vec<_> = (0..1400)
        .map(|n| t.get_string_literal_type(&format!("s{:04}", 9999 - n)))
        .collect();
    let content = Some(&ctx as &dyn super::TypeOrderContext);
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    let mut cases = Vec::new();
    for round in 0..200 {
        let target_len = [2, 3, 8, 33, 64, 350][round % 6];
        let start = next(literals.len() - target_len - 40);
        let target: Vec<_> = literals[start..start + target_len].to_vec();
        // A subset of the target, sometimes with one foreign member.
        let mut source: Vec<_> = target.iter().copied().filter(|_| next(3) != 0).collect();
        if round % 4 == 1 {
            source.push(literals[start + target_len + next(40)]);
        }
        if round % 4 == 2 {
            source.insert(0, literals[start.saturating_sub(1 + next(8))]);
        }
        if source.len() >= 2 {
            cases.push((source, target));
        }
    }
    // A source larger than its target, an equal pair, and a few members in
    // a target large enough to keep the searches.
    cases.push((literals[10..20].to_vec(), literals[12..18].to_vec()));
    cases.push((literals[10..20].to_vec(), literals[10..20].to_vec()));
    cases.push((
        vec![literals[100], literals[900]],
        literals[..1300].to_vec(),
    ));
    cases.push((
        vec![literals[100], literals[1350]],
        literals[..1300].to_vec(),
    ));
    cases.push((
        vec![literals[1350], literals[100]],
        literals[..1300].to_vec(),
    ));
    let mut walked = 0;
    for (source, target) in &cases {
        let truth = source.iter().all(|ty| target.contains(ty));
        for (order, other) in [(content, None), (None, content)] {
            let source_union = t.get_union_type(order, source, UnionReduction::None);
            let target_union = t.get_union_type(order, target, UnionReduction::None);
            let searched = t
                .members_of(source_union)
                .iter()
                .all(|&ty| super::contains_type(&t, order, t.members_of(target_union), ty));
            assert_eq!(searched, truth);
            assert_eq!(
                t.union_members_subset(order, source_union, target_union),
                truth,
                "{} members in {}",
                source.len(),
                target.len()
            );
            walked += 1;
            // Lists built in the other order keep the searches of the
            // asked order, whatever those answer.
            let crossed = t
                .members_of(source_union)
                .iter()
                .all(|&ty| super::contains_type(&t, other, t.members_of(target_union), ty));
            assert_eq!(
                t.union_members_subset(other, source_union, target_union),
                crossed
            );
        }
    }
    assert!(walked > 300);
}

/// Exercise the large-input path and every fallback against an independent
/// flatten/ID-insert oracle, followed by the base content sorter.
#[test]
fn large_union_inputs_match_the_base_in_both_orders() {
    use crate::{
        IdentityDomain, ObjectFlags, SymbolId, TypeData, TypeFlags, TypeId, TypeTables,
        UnionReduction,
    };
    fn collect(
        t: &mut TypeTables,
        set: &mut Vec<TypeId>,
        mut includes: i32,
        src: &[TypeId],
    ) -> i32 {
        let mut last = None;
        for &ty in src {
            if last == Some(ty) {
                continue;
            }
            last = Some(ty);
            if let TypeData::Union { types, origin, .. } = t.type_of(ty).data {
                if t.alias_symbol(ty).is_some() || origin.is_some() {
                    includes |= TypeFlags::UNION.bits();
                }
                let members = t.list(types).to_vec();
                includes = collect(t, set, includes, &members);
            } else {
                includes = t.add_type_to_union(None, set, includes, ty);
            }
        }
        includes
    }
    let ctx = TestOrderContext::default();
    for strict in [false, true] {
        let mut t = TypeTables::new(strict, false, &IdentityDomain::new());
        let literals: Vec<_> = (0..130)
            .map(|n| t.get_string_literal_type(&format!("s{:03}", 129 - n)))
            .collect();
        let content = t.get_union_type(Some(&ctx), &literals[..64], UnionReduction::None);
        let content2 = t.get_union_type(Some(&ctx), &literals[32..96], UnionReduction::None);
        let ids = t.get_union_type(None, &literals[..64], UnionReduction::None);
        let ids2 = t.get_union_type(None, &literals[32..96], UnionReduction::None);
        t.set_alias(content, Some(SymbolId::new(7)), None);
        t.set_alias(ids, Some(SymbolId::new(8)), None);
        let unknown = t.get_union_type_from_sorted_list(
            &literals[64..128],
            ObjectFlags::NONE,
            None,
            None,
            None,
        );
        let nested = t.create_origin_union_or_intersection_type(
            TypeFlags::UNION,
            vec![content, content, ids, literals[128]],
        );
        let parameter = t.create_synthesized_type_parameter(None);
        let intersection = t.create_origin_union_or_intersection_type(
            TypeFlags::INTERSECTION,
            vec![t.intrinsics.string, t.intrinsics.number],
        );
        t.type_mut(intersection).object_flags = ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE;
        let mut sources = vec![
            vec![content, content2],
            vec![content2, content],
            vec![ids, ids2],
            vec![ids2, ids],
            vec![content, ids],
            vec![content, content],
            vec![ids, ids],
            vec![content, unknown],
            vec![ids, nested],
            vec![nested, content],
            vec![content, literals[128], content2],
        ];
        let small_content = t.get_union_type(Some(&ctx), &literals[96..104], UnionReduction::None);
        let small_ids = t.get_union_type(None, &literals[96..104], UnionReduction::None);
        let filtered_content = t.filter_type(content, |_, ty| ty != literals[0]);
        let filtered_ids = t.filter_type(ids, |_, ty| ty != literals[0]);
        for (large, small) in [
            (content, small_content),
            (ids, small_ids),
            (filtered_content, small_content),
            (filtered_ids, small_ids),
        ] {
            for len in [3, 7, 8, 9, 16, 50] {
                let mut source = vec![
                    large,
                    large,
                    small,
                    literals[129],
                    t.intrinsics.never,
                    small,
                    t.intrinsics.undefined,
                    literals[128],
                ];
                source.resize(len, small);
                sources.push(source);
            }
        }
        sources.push(vec![
            content,
            content2,
            unknown,
            small_content,
            literals[129],
            content,
            content,
            small_content,
        ]);
        sources.push(vec![
            ids,
            ids2,
            nested,
            small_ids,
            literals[129],
            ids,
            ids,
            small_ids,
        ]);
        for u in [content, ids] {
            for other in [
                literals[0],
                literals[33],
                literals[129],
                t.intrinsics.never,
                t.intrinsics.undefined,
                t.intrinsics.null,
                t.intrinsics.null_widening,
                t.intrinsics.wildcard,
                t.intrinsics.error,
                parameter,
                intersection,
            ] {
                sources.push(vec![u, other]);
                sources.push(vec![other, u]);
            }
        }
        for order in [None, Some(&ctx as &dyn super::TypeOrderContext)] {
            for source in &sources {
                for initial in [vec![], vec![literals[129]]] {
                    let initial_flags = TypeFlags::INCLUDES_INSTANTIABLE.bits();
                    let mut expected = initial.clone();
                    let expected_flags = collect(&mut t, &mut expected, initial_flags, source);
                    if let Some(ctx) = order {
                        crate::goslices::sort_union_types(&mut expected, |a, b| {
                            super::compare_types(&t, ctx, a, b).is_lt()
                        });
                        expected.dedup();
                    }
                    let mut actual = initial;
                    let actual_flags =
                        t.add_types_to_union(order, &mut actual, initial_flags, source);
                    assert_eq!(
                        actual,
                        expected,
                        "strict={strict}, content={}, source={source:?}",
                        order.is_some()
                    );
                    assert_eq!(actual_flags, expected_flags);
                }
            }
        }
    }
}

/// A known large run plus an earlier singleton needs only logarithmically many
/// boundary comparisons. Cross-order and unknown lists still use the base sorter.
#[test]
fn large_union_merge_does_not_scan_members_for_order() {
    use crate::{IdentityDomain, SymbolId, TypeData, TypeFlags, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut t = TypeTables::new(true, false, &IdentityDomain::new());
    let literals: Vec<_> = (0..65)
        .map(|n| {
            let ty = t.get_string_literal_type(&format!("s{n:03}"));
            t.set_alias(ty, Some(SymbolId::new(n)), None);
            ty
        })
        .collect();
    let union = t.get_union_type(Some(&ctx), &literals[1..], UnionReduction::None);
    ctx.symbol_comparisons.borrow_mut().clear();
    let mut actual = Vec::new();
    t.add_types_to_union(Some(&ctx), &mut actual, 0, &[union, literals[0]]);
    assert_eq!(actual, literals);
    assert!(ctx.symbol_comparisons.borrow().len() <= 7);
    let raw = t.create_origin_union_or_intersection_type(TypeFlags::UNION, &literals[1..]);
    ctx.symbol_comparisons.borrow_mut().clear();
    actual.clear();
    t.add_types_to_union(Some(&ctx), &mut actual, 0, &[raw, literals[0]]);
    assert_eq!(actual, literals);
    assert!(ctx.symbol_comparisons.borrow().len() > 1);
    let TypeData::Union { types, .. } = t.type_of(union).data else {
        panic!("union")
    };
    assert_eq!(t.list(types), &literals[1..]);
}

/// tsgo removeType: a member leaves a union by its position in the member
/// list, and the result is the union the filter interns — in both member
/// orders, for a denormalized origin that names the member (the filter
/// rebuilds it) and for one that hides the member inside a named union.
#[test]
fn remove_type_gives_the_union_the_filter_gives() {
    use crate::{IdentityDomain, SymbolId, TypeData, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut t = TypeTables::new(true, false, &IdentityDomain::new());
    // Ids ascend while the texts descend: the two member orders differ.
    let literals: Vec<_> = (0..48)
        .map(|n| t.get_string_literal_type(&format!("s{:03}", 99 - n)))
        .collect();
    let content = Some(&ctx as &dyn super::TypeOrderContext);
    for (order, other) in [(content, None), (None, content)] {
        let union = t.get_union_type(order, &literals[..32], UnionReduction::None);
        for &member in &literals[..32] {
            let removed = t.remove_type(order, union, member);
            let filtered = t.filter_type(union, |_, ty| ty != member);
            assert_eq!(removed, filtered);
            assert_eq!(t.members_of(removed).len(), 31);
            assert!(!t.members_of(removed).contains(&member));
            assert_eq!(t.object_flags_of(removed), t.object_flags_of(union));
            // A search in the other order may miss the member; it never
            // answers with another union.
            let crossed = t.remove_type(other, union, member);
            assert!(crossed == union || crossed == filtered);
        }
        assert_eq!(t.remove_type(order, union, literals[40]), union);

        let pair = t.get_union_type(order, &literals[33..35], UnionReduction::None);
        assert_eq!(t.remove_type(order, pair, literals[33]), literals[34]);
        assert_eq!(t.remove_type(order, pair, literals[34]), literals[33]);

        let never = t.intrinsics.never;
        assert_eq!(t.remove_type(order, literals[0], literals[0]), never);
        assert_eq!(t.remove_type(order, literals[0], literals[1]), literals[0]);

        let named = t.get_union_type(order, &literals[35..37], UnionReduction::None);
        t.set_alias(
            named,
            Some(SymbolId::new(if order.is_some() { 1 } else { 2 })),
            None,
        );
        let outer = t.get_union_type(
            order,
            &[named, literals[37], literals[38]],
            UnionReduction::None,
        );
        let TypeData::Union { origin, .. } = t.type_of(outer).data else {
            panic!("union")
        };
        let origin = origin.expect("a named union and two literals keep an origin");
        assert!(t.members_of(origin).contains(&named));

        // The origin names the literal: the result keeps a rebuilt origin.
        let removed = t.remove_type(order, outer, literals[37]);
        assert_eq!(removed, t.filter_type(outer, |_, ty| ty != literals[37]));
        assert_eq!(t.members_of(removed).len(), 3);
        let TypeData::Union { origin, .. } = t.type_of(removed).data else {
            panic!("union")
        };
        let rebuilt = origin.expect("the filter rebuilds the origin");
        assert_eq!(t.members_of(rebuilt).len(), 2);
        assert!(t.members_of(rebuilt).contains(&named));

        // The origin hides the literal inside the named union: no origin.
        let removed = t.remove_type(order, outer, literals[35]);
        assert_eq!(removed, t.filter_type(outer, |_, ty| ty != literals[35]));
        assert_eq!(t.members_of(removed).len(), 3);
        let TypeData::Union { origin, .. } = t.type_of(removed).data else {
            panic!("union")
        };
        assert!(origin.is_none());
    }
}

#[test]
fn small_stored_union_runs_preserve_members_origins_and_memoization() {
    use crate::{IdentityDomain, SymbolId, TypeData, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut tables = TypeTables::new(true, false, &IdentityDomain::new());
    let literals: Vec<_> = (0..6)
        .map(|i| {
            let ty = tables.get_string_literal_type(&format!("s{i}"));
            tables.set_alias(ty, Some(SymbolId::new(100 + i)), None);
            ty
        })
        .collect();
    let inner = tables.get_union_type(Some(&ctx), &literals[1..5], UnionReduction::None);
    tables.set_alias(inner, Some(SymbolId::new(1000)), None);
    let sources = [literals[0], inner, literals[5]];
    ctx.symbol_comparisons.borrow_mut().clear();
    let actual = tables.get_union_type(Some(&ctx), &sources, UnionReduction::None);
    assert_eq!(tables.members_of(actual), literals);
    assert_eq!(
        &*ctx.symbol_comparisons.borrow(),
        &[
            (SymbolId::new(101), SymbolId::new(100)),
            (SymbolId::new(105), SymbolId::new(104)),
        ],
        "only the two source boundaries need content comparisons"
    );
    let TypeData::Union { origin, .. } = tables.type_of(actual).data else {
        panic!("union")
    };
    assert_eq!(tables.alias_symbol(actual), None);
    assert_eq!(
        tables.members_of(origin.expect("named origin")),
        &[literals[0], literals[5], inner]
    );
    assert_eq!(
        tables.get_union_type(Some(&ctx), &sources, UnionReduction::None),
        actual
    );
}

#[test]
fn balanced_union_merges_search_skewed_runs_and_keep_duplicate_occurrences() {
    use crate::{IdentityDomain, TypeTables};
    let mut tables = TypeTables::new(true, false, &IdentityDomain::new());
    let ids: Vec<_> = (0..130)
        .map(|i| tables.get_string_literal_type(&format!("s{i:03}")))
        .collect();
    for reverse in [false, true] {
        let long = ids[..128].to_vec();
        let short = vec![ids[5], ids[17], ids[129]];
        let (left, right) = if reverse {
            (short, long)
        } else {
            (long, short)
        };
        let mut actual = left.clone();
        actual.extend_from_slice(&right);
        let mut expected = actual.clone();
        expected.sort();
        let mut starts = vec![0, left.len()];
        let mut probes = Vec::new();
        super::merge_union_runs(&mut actual, &mut starts, |a, b| {
            probes.push((a, b));
            a < b
        });
        assert_eq!(actual, expected);
        assert!(
            probes.len() <= 24,
            "three insertions into a 128-element run"
        );
        assert!(probes.iter().all(|(a, b)| a != b));
    }
    let mut actual = ids[..64].to_vec();
    actual.extend_from_slice(&ids[..64]);
    let mut expected = actual.clone();
    expected.sort();
    let mut starts = vec![0, 64];
    super::merge_union_runs(&mut actual, &mut starts, |_, _| {
        panic!("equal heads already determine the next two occurrences")
    });
    assert_eq!(
        actual, expected,
        "deduplication remains the caller's operation"
    );
}

#[test]
fn named_union_finishing_reuses_markers_and_checks_unmarked_lists() {
    use crate::{IdentityDomain, SymbolId, TypeFlags, TypeTables, UnionReduction};
    let ctx = TestOrderContext::default();
    let mut tables = TypeTables::new(true, false, &IdentityDomain::new());
    let literals: Vec<_> = (0..6)
        .map(|i| {
            let ty = tables.get_string_literal_type(&format!("s{i}"));
            tables.set_alias(ty, Some(SymbolId::new(200 + i)), None);
            ty
        })
        .collect();
    let known = tables.get_union_type(Some(&ctx), &literals[1..5], UnionReduction::None);
    tables.set_alias(known, Some(SymbolId::new(1000)), None);
    let raw = tables.create_origin_union_or_intersection_type(TypeFlags::UNION, &literals[1..5]);
    tables.set_alias(raw, Some(SymbolId::new(1001)), None);
    for inner in [known, raw] {
        let sources = [inner, literals[0], literals[5]];
        let mut collected = Vec::new();
        let includes = tables.add_types_to_union(None, &mut collected, 0, &sources);
        ctx.symbol_comparisons.borrow_mut().clear();
        let actual = tables.finish_union_type_set(
            Some(&ctx),
            literals.clone(),
            includes,
            &sources,
            None,
            None,
            None,
        );
        assert_eq!(tables.members_of(actual), literals);
        if inner == known {
            assert!(ctx.symbol_comparisons.borrow().is_empty());
        } else {
            assert_eq!(
                &*ctx.symbol_comparisons.borrow(),
                &[
                    (SymbolId::new(201), SymbolId::new(202)),
                    (SymbolId::new(202), SymbolId::new(203)),
                    (SymbolId::new(203), SymbolId::new(204)),
                ],
                "a raw origin still uses the original adjacent scan"
            );
        }
    }
}
