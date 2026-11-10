use super::*;
use crate::inference::create_inference_info;
use crate::state::test_support::with_program_state;
use tsc_types::CompilerOptions;

#[test]
fn older_variance_measurement_cannot_publish_a_reusable_outcome() {
    use crate::links::LinkSlot;
    use crate::variance::VarianceStackEntry;
    use tsc_types::{SymbolFlags, VarianceFlags};

    let parameters = (0..20).map(|n| format!("A{n}")).collect::<Vec<_>>();
    let arguments = (0..20).map(|n| format!("x{n}: A{n}")).collect::<Vec<_>>();
    let source_args = vec!["string"; 20].join(",");
    let target_args = vec!["T"; 20].join(",");
    let text = format!("type Fn<{}> = ({}) => void; function f<T>() {{ let source: Fn<{source_args}>; let target: Fn<{target_args}>; }}",
        parameters.join(","), arguments.join(","));
    with_program_state(
        &[("a.ts", &text)],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            let source_node =
                crate::relpin::find_probe_annotation(state.binder.source(0), "source").unwrap();
            let target_node =
                crate::relpin::find_probe_annotation(state.binder.source(0), "target").unwrap();
            let symbol = state
                .resolve_file_scope_name("Fn", SymbolFlags::TYPE_ALIAS)
                .unwrap();
            let parameter = state
                .resolve_name(
                    Some(target_node),
                    "T",
                    SymbolFlags::TYPE_PARAMETER,
                    None,
                    false,
                    false,
                )
                .unwrap()
                .unwrap();
            let t = state.get_declared_type_of_type_parameter(parameter);
            let source = state.get_type_from_type_node(source_node).unwrap();
            let target = state.get_type_from_type_node(target_node).unwrap();
            let string = state.tables.intrinsics.string;
            // Reproduce variance.rs's circularity answer while an older measuring
            // frame remains alive. That answer is replaced when it completes.
            state.links.set_symbol_variances(
                state.speculation_depth,
                symbol,
                LinkSlot::Resolved(crate::state::SharedList::new()),
            );
            state.variance_stack.push(VarianceStackEntry {
                symbol,
                type_parameters: Box::new([]),
            });
            let stores = state.infer_memo.stats.stores;
            for _ in 0..4 {
                let info = state.alloc_inference_info(create_inference_info(t));
                state
                    .infer_types(&[info], source, target, InferencePriority::NONE, false)
                    .unwrap();
                assert_eq!(
                    state.inference_info(info).candidates.as_deref(),
                    Some(&[string][..])
                );
            }
            assert_eq!(state.infer_memo.stats.stores, stores);
            assert!(state.infer_memo.stats.rejected[16] > 0);
            state.variance_stack.pop();
            state.links.set_symbol_variances(
                state.speculation_depth,
                symbol,
                LinkSlot::Resolved(vec![VarianceFlags::CONTRAVARIANT; 20].into()),
            );
            for _ in 0..4 {
                let info = state.alloc_inference_info(create_inference_info(t));
                state
                    .infer_types(&[info], source, target, InferencePriority::NONE, false)
                    .unwrap();
                assert_eq!(
                    state.inference_info(info).contra_candidates.as_deref(),
                    Some(&[string][..])
                );
                assert!(state.inference_info(info).candidates.is_none());
            }
            assert!(state.infer_memo.stats.hits > 0);
        },
    );
}

#[test]
fn full_keys_prevent_hash_collision_hits_and_table_is_bounded() {
    let mut memo = InferMemo::new();
    for index in 0..(TABLE_SLOTS * 2) {
        memo.store(Entry {
            hash: index as u64,
            key: vec![index as u32].into_boxed_slice(),
            infos: Box::new([]),
            cleared: false,
            height: 0,
            flags: 0,
            steps: 16,
        });
    }
    assert_eq!(memo.slots.len(), TABLE_SLOTS);
    assert!(memo
        .lookup(TABLE_SLOTS as u64, &[TABLE_SLOTS as u32])
        .is_some());
    // Same full hash, different full key: still a miss.
    assert!(memo.lookup(TABLE_SLOTS as u64, &[0]).is_none());
    assert!(memo.lookup(0, &[0]).is_none());
    assert!(InferMemo::new().long_targets.is_empty());
}

#[test]
fn key_preserves_optional_state_and_rejects_aliasing_and_large_info_lists() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let t = state.tables.create_synthesized_type_parameter(None);
        let id = state.alloc_inference_info(create_inference_info(t));
        let source = state.tables.intrinsics.string;
        let key = |state: &CheckerState<'_>| {
            let mut key = Vec::new();
            assert!(state.infer_memo_key(
                &mut key,
                &[id],
                source,
                t,
                InferencePriority::NONE,
                false
            ));
            key
        };
        let absent = key(state);
        state.inference_info_mut(id).candidates = Some(Vec::new());
        assert_ne!(key(state), absent);
        state.inference_info_mut(id).candidates = None;
        state.inference_info_mut(id).priority = Some(InferencePriority::NONE);
        assert_ne!(key(state), absent);
        state.inference_info_mut(id).priority = None;
        state.inference_info_mut(id).implied_arity = Some(0);
        assert_ne!(key(state), absent);
        let mut key = Vec::new();
        assert!(!state.infer_memo_key(
            &mut key,
            &[id, id],
            source,
            t,
            InferencePriority::NONE,
            false
        ));
        assert!(!state.infer_memo_key(
            &mut key,
            &[id; 9],
            source,
            t,
            InferencePriority::NONE,
            false
        ));
    });
}

/// Oracle: pinned tsgo aa814927, `-p scratch/oracle --noEmit --pretty false`.
/// Fixture is tsrs's 24-member Extract union with eight intentional errors.
#[test]
fn repeated_extract_walks_match_pinned_tsgo_and_hit_the_memo() {
    let text = include_str!("../../fixtures/infer-memo-repeated-walks.ts");
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs/lib.es5.d.ts"))
        .expect("vendored ES5 lib");
    with_program_state(
        &[("a.ts", text), ("lib.es5.d.ts", &lib)],
        &CompilerOptions {
            strict: Some(true),
            skip_lib_check: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            let mut diagnostics: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| {
                    d.file_name
                        .as_ref()
                        .is_some_and(|name| name.as_js().as_str() == Some("a.ts"))
                })
                .collect();
            diagnostics.sort_by_key(|d| (d.start, d.code()));
            assert_eq!(diagnostics.len(), 8);
            let mut actual = String::new();
            for diagnostic in diagnostics {
                let start = diagnostic.start.expect("diagnostic location") as usize;
                let prefix = &text[..start]; // The oracle fixture is ASCII.
                let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                let column = start - prefix.rfind('\n').map_or(0, |n| n + 1) + 1;
                let message = tsc_diagnostics::flattened_message(&diagnostic.message, "\n", None);
                actual.push_str(&format!(
                    "a.ts({line},{column}): error TS{}: {}\n",
                    diagnostic.code(),
                    message.as_js().as_str().expect("ASCII fixture message")
                ));
            }
            assert_eq!(
                actual,
                include_str!("../../fixtures/infer-memo-repeated-walks.expected.txt")
            );
            assert!(
                state.infer_memo.stats.hits > 0,
                "{:?}",
                state.infer_memo.stats
            );
            assert!(state.infer_memo.stats.skipped_steps > 100_000);
            assert_eq!(state.flow_memo.frame_depth(), 0);
            assert!(state.inference_visited.is_empty());
        },
    );
}
