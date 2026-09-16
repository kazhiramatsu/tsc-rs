
#[cfg(test)]
mod review_retention_boundaries {
    use super::*;
    use tsc_host::MemoryCompilerHost;

    #[test]
    fn changing_option_identities_must_not_accumulate_unbounded_eviction_keys() {
        let host = MemoryCompilerHost::builder_js("/p")
            .file_js("/p/util.ts", b"export {};".to_vec()).build().unwrap();
        let program = ProgramOptions::default();
        let token = CancellationToken::default();
        let limits = RetentionLimits { max_entries:1, max_bytes:4096, max_live_generations:2 };
        let mut cache = ResolutionCache::new(limits);
        for i in 0..1000 {
            let options = CompilerOptions { module_resolution:Some(99), module:Some(199),
                custom_conditions:Some(vec![JsString::from(format!("condition{i}"))]),
                ..CompilerOptions::default() };
            let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
            candidate.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs, &token).unwrap();
            let (_, stats) = cache.publish(candidate).unwrap();
            if i == 99 || i == 999 {
                let identities: BTreeMap<_, _> = cache.evicted_keys.iter().map(|key|
                    (Rc::as_ptr(&key.identity), key.identity.as_str().len())).collect();
                let identity_bytes: usize = identities.values().sum();
                eprintln!("generation={} published_entries={} reported_bytes={} evicted_keys={} history_option_string_bytes={}",
                    i + 1, stats.entries, stats.bytes, cache.evicted_keys.len(), identity_bytes);
            }
        }
        let identities: BTreeMap<_, _> = cache.evicted_keys.iter().map(|key|
            (Rc::as_ptr(&key.identity), key.identity.as_str().len())).collect();
        let minimum_owned_bytes: usize = identities.values().sum();
        // This lower bound excludes all map allocation, keys and live values.
        assert!(minimum_owned_bytes <= limits.max_bytes * limits.max_live_generations,
            "retired option strings alone remain unbounded: {minimum_owned_bytes} bytes");
    }

    #[test]
    fn repeated_evict_all_must_prune_dead_generation_bookkeeping() {
        let limits = RetentionLimits { max_entries:1, max_bytes:4096, max_live_generations:2 };
        let mut cache = ResolutionCache::new(limits);
        for _ in 0..1000 { cache.evict_all(); }
        let actual_live = cache.live.iter().filter(|weak| weak.strong_count() > 0).count();
        eprintln!("evict_all calls=1000 actual_live={} weak_records={}", actual_live, cache.live.len());
        assert!(cache.live.len() <= limits.max_live_generations);
    }
}
