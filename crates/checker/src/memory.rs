//! Report-only ownership traversal. Never called on the checker's hot paths.
use crate::{state::CheckerState, ProgramSnapshot};
use tsc_diagnostics::{MemoryUsage, TextSnapshot};

pub(crate) fn snapshot_heap_usage(
    snapshot: &ProgramSnapshot,
    usage: &mut MemoryUsage,
) -> (usize, tsc_binder::BinderMemory) {
    let mut syntax = tsc_syntax::SyntaxMemory::default();
    let mut binder = tsc_binder::BinderMemory::default();
    snapshot.add_memory_usage(usage);
    for document in snapshot.documents() {
        if usage.arc("BoundDocument Arc headers", document) {
            document.data.add_memory_usage(&mut binder);
            usage.diagnostics(&document.data.bind_diagnostics);
            let (names, initialized, requested) = document.data.classifiable_names.memory_usage();
            usage.add(
                "binder classifiable_names (estimate)",
                names,
                initialized,
                requested,
            );
        }
        usage.arc("ParsedDocument Arc headers", &document.parsed);
        let source = &document.parsed.source;
        if usage.arc("SourceFile Arc headers", source) {
            source.add_owned_memory_usage(usage);
            source.arena.add_memory_usage(&mut syntax);
        }
    }
    // Legacy symbol-owned bytes include declaration words in the mapped
    // store. Main-allocator records are observed from placement, not guessed
    // from address ranges or by subtracting all initialized record bytes.
    let legacy_heap =
        syntax.heap_record_bytes + binder.heap_record_bytes + binder.symbol_owned_bytes
            - binder.declaration_store_bytes
            + binder.tables.values().sum::<usize>()
            + binder.local_bytes
            + binder.flow_bytes
            + binder.flow_antecedent_bytes
            + binder.map_bytes;
    (legacy_heap, binder)
}

// Only commutative ownership sums; printed rows are ordered by BTreeMap.
#[allow(clippy::iter_over_hash_type)]
pub(crate) fn checker_owners(
    state: &CheckerState<'_>,
    usage: &mut MemoryUsage,
    include_shared: bool,
) {
    const TABLES: &str = "checker host tables (estimate)";
    const STRINGS: &str = "strings: stable owners";
    usage.hash_map(
        "memo: resolved_type_predicates",
        &state.resolved_type_predicates,
    );
    for predicate in state.resolved_type_predicates.values().flatten() {
        if let Some(name) = &predicate.parameter_name {
            usage.string(STRINGS, name);
        }
    }
    usage.hash_map("memo: effects signatures", &state.effects_signature_cache);
    usage.hash_map("memo: generic object flags", &state.generic_object_flags);
    usage.hash_map("memo: index symbols", &state.index_symbols);
    usage.hash_map("memo: evolving arrays", &state.evolving_array_types);
    usage.hash_map("memo: final arrays", &state.final_array_types);
    usage.hash_map("memo: undefined properties", &state.undefined_properties);
    usage.hash_map("memo: reverse mapped types", &state.reverse_mapped_cache);
    usage.hash_map(
        "memo: reverse homomorphic types",
        &state.reverse_homomorphic_mapped_cache,
    );
    usage.hash_map("memo: merged symbols", &state.merged_symbols);
    usage.hash_map("memo: merged symbol sources", &state.merged_symbol_sources);
    for sources in state.merged_symbol_sources.values() {
        usage.vec("memo: merged symbol sources", sources);
    }
    state.unaffected_members.add_memory_usage(usage);
    usage.hash_map("memo: contextual positions", &state.contextual_relevant);
    for (_, positions) in state.contextual_relevant.values() {
        usage.arc("memo: contextual position lists", positions);
    }
    usage.hash_map(
        "memo: recursion identities",
        &state.intersection_recursion_ids,
    );
    for identities in state.intersection_recursion_ids.values() {
        usage.arc("memo: recursion identity lists", identities);
    }
    usage.vec("checker widening contexts", &state.widening_contexts);
    for context in &state.widening_contexts {
        if let Some(siblings) = &context.siblings {
            usage.vec("checker widening context lists", siblings);
        }
        if let Some(properties) = &context.resolved_properties {
            usage.vec("checker widening context lists", properties);
        }
    }
    state.links.add_omitted_memory_usage(usage);
    for context in &state.inference_context_arena {
        usage.vec("inference context deep lists", &context.inferences);
        usage.vec("inference context deep lists", &context.mapper_sources);
        usage.vec("inference context deep lists", &context.mapper_infos);
        if let Some(values) = &context.inferred_type_parameters {
            usage.vec("inference context deep lists", values);
        }
        if let Some(values) = &context.intra_expression_inference_sites {
            usage.vec("inference context deep lists", values);
        }
    }
    for info in &state.inference_info_arena {
        if let Some(values) = &info.candidates {
            usage.vec("inference candidate lists", values);
        }
        if let Some(values) = &info.contra_candidates {
            usage.vec("inference candidate lists", values);
        }
    }
    state.add_inference_walker_memory_usage(usage);
    state.binder.add_projection_memory_usage(usage);
    if include_shared {
        if usage.arc("host fact Arc headers", &state.host_file_paths) {
            usage.hash_set(TABLES, &state.host_file_paths);
            for path in state.host_file_paths.iter() {
                usage.js_string(STRINGS, path);
            }
        }
        if usage.arc("host fact Arc headers", &state.host_input_snapshots) {
            usage.hash_map(TABLES, &state.host_input_snapshots);
            for (path, snapshot) in state.host_input_snapshots.iter() {
                usage.js_string(STRINGS, path);
                TextSnapshot::add_memory_usage(snapshot, usage);
            }
        }
        if usage.arc(
            "host fact Arc headers",
            &state.host_package_json_module_types,
        ) {
            usage.hash_map(TABLES, &state.host_package_json_module_types);
            for path in state.host_package_json_module_types.keys() {
                usage.js_string(STRINGS, path);
            }
        }
        if usage.arc("host fact Arc headers", &state.host_package_json_values) {
            usage.hash_map(TABLES, &state.host_package_json_values);
            for (path, value) in state.host_package_json_values.iter() {
                usage.js_string(STRINGS, path);
                if usage.arc("package JSON Arc headers", value) {
                    value.add_memory_usage(usage);
                }
            }
        }
        if usage.arc("host fact Arc headers", &state.host_package_json_names) {
            usage.hash_map(TABLES, &state.host_package_json_names);
            for (path, name) in state.host_package_json_names.iter() {
                usage.js_string(STRINGS, path);
                usage.js_string(STRINGS, name);
            }
        }
    }
    usage.js_string(STRINGS, &state.host_current_directory);
    usage.hash_map(TABLES, &state.program_path_index);
    for path in state.program_path_index.keys() {
        usage.js_string(STRINGS, path);
    }
    usage.vec(
        "checker authoritative file metadata",
        &state.authoritative_source_tokens,
    );
    usage.hash_map(TABLES, &state.authoritative_source_index_by_token);
    usage.vec(
        "checker authoritative file metadata",
        &state.authoritative_source_may_be_emitted,
    );
    usage.vec(
        "checker authoritative file metadata",
        &state.authoritative_implied_node_formats,
    );
    usage.vec(
        "checker authoritative file metadata",
        &state.authoritative_implied_node_formats_for_emit,
    );
    usage.vec(
        "checker authoritative file metadata",
        &state.authoritative_project_references,
    );
    state.diagnostics.add_memory_usage(usage);
    usage.diagnostics(&state.visible_global_diagnostics);
}

pub(crate) fn print_reserved_vec<T>(label: &str, name: &str, values: &Vec<T>) -> usize {
    let initialized = values.len() * std::mem::size_of::<T>();
    let requested = values.capacity() * std::mem::size_of::<T>();
    let resident = tsc_types::resident_vec_tail_bytes(values);
    let observed = resident.map_or_else(|| "unavailable".to_owned(), |bytes| format!("{bytes} B"));
    eprintln!("[memory] {label} reserved {name}: pointer={:p} len={} capacity={} initialized={} B requested={} B unused={} B complete_tail_resident={observed}",
        values.as_ptr(), values.len(), values.capacity(), initialized, requested, requested - initialized);
    resident.unwrap_or(0)
}

pub(crate) fn print_reserved_arenas(state: &CheckerState<'_>, label: &str) -> usize {
    print_reserved_vec(label, "signatures", &state.signatures)
        + print_reserved_vec(label, "resolved_members", &state.members)
        + print_reserved_vec(label, "mappers", &state.mappers)
        + print_reserved_vec(label, "mapper_lists", &state.mapper_lists)
        + print_reserved_vec(label, "inference_context", &state.inference_context_arena)
        + print_reserved_vec(label, "inference_info", &state.inference_info_arena)
        + state.links.print_reserved_memory(label)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tsc_diagnostics::{DocumentVersion, MemoryUsage, SharedJsString, TextSnapshot};

    #[test]
    fn accounting_deduplicates_allocations_without_merging_equal_owners() {
        let first = Arc::new(42u64);
        let shared = Arc::clone(&first);
        let equal = Arc::new(42u64);
        let mut usage = MemoryUsage::default();
        assert!(usage.arc("headers", &first));
        assert!(!usage.arc("headers", &shared));
        assert!(usage.arc("headers", &equal));
        assert_eq!(usage.row("headers").count, 2);

        let path = SharedJsString::from(tsc_diagnostics::JsStr::from_str("/shared/path.ts"));
        let path_clone = path.clone();
        path.add_memory_usage(&mut usage);
        path_clone.add_memory_usage(&mut usage);
        assert_eq!(usage.row("strings: shared stable owners").count, 1);

        let snapshot = TextSnapshot::new("let value = '😀';", DocumentVersion::new("v1"));
        TextSnapshot::add_memory_usage(&snapshot, &mut usage);
        let before = usage.total().requested;
        TextSnapshot::add_memory_usage(&Arc::clone(&snapshot), &mut usage);
        assert_eq!(usage.total().requested, before);
        assert_eq!(usage.row("TextSnapshot Arc headers").count, 1);
        assert_eq!(usage.source_text_bytes, snapshot.text().len());
    }

    #[test]
    fn accounting_keeps_initialized_records_separate_from_reserved_room() {
        let mut values = Vec::<u32>::with_capacity(128);
        values.extend([1, 2, 3]);
        let mut usage = MemoryUsage::default();
        assert!(usage.vec("records", &values));
        assert!(!usage.vec("records", &values));
        let row = usage.row("records");
        assert_eq!(row.count, 3);
        assert_eq!(row.initialized, 12);
        assert_eq!(row.requested, values.capacity() * 4);
        assert!(row.requested >= 512);
    }

    #[test]
    fn accounting_keys_zero_sized_arcs_by_the_allocation_base() {
        let empty = Arc::new(());
        let payload = Arc::as_ptr(&empty) as usize;
        let mut usage = MemoryUsage::default();
        assert!(usage.arc("empty header", &empty));
        // The one-past payload address can identify a neighboring buffer;
        // recording the Arc must not reserve that address as its identity.
        assert!(usage.claim(payload));
        assert!(!usage.claim_arc(&Arc::clone(&empty)));
        assert_eq!(usage.row("empty header").count, 1);
        assert_eq!(
            usage.row("empty header").requested,
            2 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn resident_tail_observation_preserves_values_length_and_capacity() {
        let mut values = Vec::<u64>::with_capacity(32768);
        values.extend(0..256);
        let before = values.clone();
        let capacity = values.capacity();
        if let Some(bytes) = tsc_types::resident_vec_tail_bytes(&values) {
            assert!(bytes <= (capacity - values.len()) * std::mem::size_of::<u64>());
        }
        assert_eq!(values, before);
        assert_eq!(values.capacity(), capacity);
        let full = vec![1u64, 2, 3];
        assert_eq!(tsc_types::resident_vec_tail_bytes(&full), Some(0));
    }
}
