
use super::*;
use crate::prepared::PathMapping;
use tsc_types::ModuleSuffix;

fn key(path: &str) -> PathKey {
    PathKey::new(path.into(), true)
}

#[test]
fn path_key_containment_and_parent() {
    assert!(key("/p/a/b.ts").is_within(&key("/p/a")));
    assert!(key("/p/a/b.ts").is_within(&key("/")));
    assert!(!key("/p/ab.ts").is_within(&key("/p/a")));
    assert!(!key("/p/a").is_within(&key("/p/a")));
    assert_eq!(key("/p/a/b.ts").parent(), Some(key("/p/a")));
    assert_eq!(key("/p").parent(), Some(key("/")));
    assert_eq!(key("/").parent(), None);
}

#[test]
fn negative_file_dependency_is_violated_by_creation_and_parent_creation() {
    let mut dependencies = DependencySet::default();
    dependencies.entries.insert(Dependency::FileExists {
        path: key("/p/lib/index.ts"),
        exists: false,
    });
    let mut batch = ChangeBatch::default();
    assert!(batch.violated_dependency(&dependencies).is_none());
    batch.created_directories.insert(key("/p/lib"));
    assert!(batch.violated_dependency(&dependencies).is_some());
    let mut batch = ChangeBatch::default();
    batch.created_files.insert(key("/p/lib/index.ts"));
    assert!(batch.violated_dependency(&dependencies).is_some());
    let mut batch = ChangeBatch::default();
    batch.created_files.insert(key("/p/other.ts"));
    assert!(batch.violated_dependency(&dependencies).is_none());
}

fn memory_host() -> tsc_host::MemoryCompilerHost {
    tsc_host::MemoryCompilerHost::builder_js("/p")
        .file_js("/p/main.ts", b"import 'pkg';".to_vec())
        .file_js(
            "/p/node_modules/pkg/package.json",
            br#"{"name":"pkg","exports":{"a,b":"./joined.d.ts","a":"./split.d.ts"}}"#.to_vec(),
        )
        .file_js("/p/node_modules/pkg/joined.d.ts", b"export {};".to_vec())
        .file_js("/p/node_modules/pkg/split.d.ts", b"export {};".to_vec())
        .file_js("/p/other.ts", b"export {};".to_vec())
        .file_js("/p/util.ts", b"export {};".to_vec())
        .build()
        .expect("memory host")
}

fn nodenext(conditions: Option<&[&str]>) -> CompilerOptions {
    CompilerOptions {
        module_resolution: Some(99),
        module: Some(199),
        custom_conditions: conditions
            .map(|list| list.iter().map(|entry| JsString::from(*entry)).collect()),
        ..CompilerOptions::default()
    }
}

fn identity_of(options: &CompilerOptions, program_options: &ProgramOptions) -> OptionsIdentity {
    OptionsIdentity::new(options, program_options, &memory_host())
}

fn resolve_pkg(candidate: &mut Candidate<'_>, specifier: &str) -> Rc<CacheEntry> {
    candidate
        .resolve_module(
            "/p/main.ts".into(),
            specifier.into(),
            ResolutionMode::CommonJs,
            &CancellationToken::default(),
        )
        .expect("resolve")
}

#[test]
fn options_identity_is_structural_not_serialized() {
    let program = ProgramOptions::default();
    let joined = identity_of(&nodenext(Some(&["a,b"])), &program);
    let split = identity_of(&nodenext(Some(&["a", "b"])), &program);
    let reordered = identity_of(&nodenext(Some(&["b", "a"])), &program);
    let empty = identity_of(&nodenext(Some(&[])), &program);
    let absent = identity_of(&nodenext(None), &program);
    let identities = [&joined, &split, &reordered, &empty, &absent];
    for (left_index, left) in identities.iter().enumerate() {
        for (right_index, right) in identities.iter().enumerate() {
            assert_eq!(
                left_index == right_index,
                left == right,
                "identities {left_index} and {right_index}"
            );
            assert_eq!(
                left_index == right_index,
                left.digest() == right.digest(),
                "digests {left_index} and {right_index}"
            );
        }
    }
    assert_eq!(joined, identity_of(&nodenext(Some(&["a,b"])), &program));

    // Exact UTF-16 code units: a lone surrogate is not the replacement
    // character it would become under a lossy rendering.
    let lone = CompilerOptions {
        custom_conditions: Some(vec![JsString::from_code_units(&[0xD800])]),
        ..nodenext(None)
    };
    let replaced = CompilerOptions {
        custom_conditions: Some(vec![JsString::from("\u{FFFD}")]),
        ..nodenext(None)
    };
    assert_ne!(
        identity_of(&lone, &program),
        identity_of(&replaced, &program)
    );

    // moduleSuffixes element boundaries.
    let one = CompilerOptions {
        module_suffixes: Some(vec![ModuleSuffix::value(".a,b")]),
        ..nodenext(None)
    };
    let two = CompilerOptions {
        module_suffixes: Some(vec![ModuleSuffix::value(".a"), ModuleSuffix::value(".b")]),
        ..nodenext(None)
    };
    assert_ne!(identity_of(&one, &program), identity_of(&two, &program));

    // paths substitutions and patterns keep their boundaries.
    let single = ProgramOptions::default().with_config_paths(
        vec![PathMapping::new("@app/*", vec!["src,alt/*".into()])],
        "/p",
    );
    let pair = ProgramOptions::default().with_config_paths(
        vec![PathMapping::new(
            "@app/*",
            vec!["src/*".into(), "alt/*".into()],
        )],
        "/p",
    );
    let pattern_split = ProgramOptions::default().with_config_paths(
        vec![
            PathMapping::new("@app", vec!["src,alt/*".into()]),
            PathMapping::new("/*", vec![]),
        ],
        "/p",
    );
    let base = nodenext(None);
    assert_ne!(identity_of(&base, &single), identity_of(&base, &pair));
    assert_ne!(
        identity_of(&base, &single),
        identity_of(&base, &pattern_split)
    );

    // types, and the readable rendering, are distinct as well.
    let types_joined = ProgramOptions::default().with_types(vec!["a,b".into()]);
    let types_split = ProgramOptions::default().with_types(vec!["a".into(), "b".into()]);
    assert_ne!(
        identity_of(&base, &types_joined),
        identity_of(&base, &types_split)
    );
    assert_ne!(
        identity_of(&base, &types_joined).describe(),
        identity_of(&base, &types_split).describe()
    );
}

#[test]
fn distinct_condition_arrays_never_reuse_a_resolution() {
    let host = memory_host();
    let program = ProgramOptions::default();
    let joined = nodenext(Some(&["a,b"]));
    let split = nodenext(Some(&["a", "b"]));
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut first = cache.begin(ChangeBatch::default(), &host, &joined, &program);
    let original = resolve_pkg(&mut first, "pkg");
    cache.publish(first).expect("publish joined");
    let mut next = cache.begin(ChangeBatch::default(), &host, &split, &program);
    let after = resolve_pkg(&mut next, "pkg");
    let mut fresh_cache = ResolutionCache::new(RetentionLimits::default());
    let mut fresh = fresh_cache.begin(ChangeBatch::default(), &host, &split, &program);
    let expected = resolve_pkg(&mut fresh, "pkg");
    assert_ne!(
        original.value(),
        expected.value(),
        "fixture selects different exports"
    );
    assert_eq!(after.value(), expected.value());
    assert!(matches!(
        next.trace().last().map(|row| &row.disposition),
        Some(Disposition::Fresh { .. })
    ));
}

#[test]
fn dropped_candidate_leaves_published_stamps_and_lru_untouched() {
    let host = memory_host();
    let program = ProgramOptions::default();
    let options = nodenext(None);
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut first = cache.begin(ChangeBatch::default(), &host, &options, &program);
    let other = resolve_pkg(&mut first, "./other");
    let util = resolve_pkg(&mut first, "./util");
    let (held, _) = cache.publish(first).expect("publish");
    let stamps_before = held.view().usage_stamps();
    assert_eq!(held.view().last_used(other.key()), Some(1));
    assert_eq!(held.view().last_used(util.key()), Some(1));

    // A candidate touches `./other`, then is dropped.
    let mut dropped = cache.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut dropped, "./other");
    assert_eq!(dropped.working_last_used(other.key()), Some(2));
    assert_eq!(
        held.view().last_used(other.key()),
        Some(1),
        "published stamp untouched"
    );
    drop(dropped);
    assert_eq!(cache.published_id(), held.id());
    assert_eq!(held.view().usage_stamps(), stamps_before);

    // A refused publish also leaves the stamps alone.
    cache.set_limits(RetentionLimits {
        max_live_generations: 1,
        ..RetentionLimits::default()
    });
    let mut refused = cache.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut refused, "./other");
    assert!(matches!(
        cache.publish(refused),
        Err(CacheError::LiveGenerationLimit { .. })
    ));
    assert_eq!(held.view().usage_stamps(), stamps_before);

    // LRU decision with the untouched stamps: both entries were last
    // used in generation 1, so the smaller key (`./other`) is evicted.
    // Had the dropped candidate leaked its stamp, `./util` would go.
    cache.set_limits(RetentionLimits {
        max_entries: 1,
        max_live_generations: 8,
        ..RetentionLimits::default()
    });
    let quiet = cache.begin(ChangeBatch::default(), &host, &options, &program);
    let (published, stats) = cache.publish(quiet).expect("publish quiet");
    assert_eq!(stats.evicted, 1);
    assert!(
        published.view().get(util.key()).is_some(),
        "./util retained"
    );
    assert!(
        published.view().get(other.key()).is_none(),
        "./other evicted"
    );
    assert_eq!(
        held.view().usage_stamps(),
        stamps_before,
        "old reader immutable"
    );
}

#[test]
fn foreign_and_stale_candidates_are_rejected() {
    let host = memory_host();
    let program = ProgramOptions::default();
    let options = nodenext(None);
    let mut source = ResolutionCache::new(RetentionLimits::default());
    let mut destination = ResolutionCache::new(RetentionLimits::default());
    let mut foreign = source.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut foreign, "./util");
    let before = destination.current();
    assert!(matches!(
        destination.publish(foreign),
        Err(CacheError::ForeignCandidate)
    ));
    assert_eq!(destination.published_id(), 0);
    assert!(Rc::ptr_eq(&before.view, &destination.current().view));

    // A stale candidate: another generation was published after it began.
    let mut stale = source.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut stale, "./util");
    let mut newer = source.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut newer, "./other");
    let (published, _) = source.publish(newer).expect("publish newer");
    assert!(matches!(
        source.publish(stale),
        Err(CacheError::ParentMismatch {
            expected: 1,
            actual: 0
        })
    ));
    assert!(Rc::ptr_eq(&published.view, &source.current().view));

    // A candidate begun before evict_all is stale too.
    let older = source.begin(ChangeBatch::default(), &host, &options, &program);
    source.evict_all().expect("evict all");
    assert!(matches!(
        source.publish(older),
        Err(CacheError::ParentMismatch { .. })
    ));
    assert!(source.current().view().is_empty());
}

#[test]
fn eviction_history_is_bounded_and_reported() {
    let host = memory_host();
    let program = ProgramOptions::default();
    let options = nodenext(None);
    let limits = RetentionLimits {
        max_entries: 2,
        max_bytes: 1 << 20,
        max_live_generations: 4,
        max_eviction_history: 3,
    };
    let mut cache = ResolutionCache::new(limits);
    let mut evicted_total = 0;
    for generation in 0..20 {
        let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut candidate, &format!("./gen-{generation}"));
        let (_, stats) = cache.publish(candidate).expect("publish");
        evicted_total += stats.evicted;
        let resident = cache.resident_stats();
        assert!(resident.published_entries <= 2);
        assert!(resident.eviction_history_len <= 3, "{resident:?}");
        assert_eq!(resident.live_generation_records, 1, "no held readers");
    }
    let resident = cache.resident_stats();
    assert_eq!(evicted_total, 18);
    assert_eq!(resident.eviction_history_len, 3);
    assert_eq!(resident.eviction_history_dropped, 15);

    // A key evicted within the retained window is reported as such; one
    // forgotten beyond the window is not, and the candidate says so.
    let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
    resolve_pkg(&mut candidate, "./gen-17");
    resolve_pkg(&mut candidate, "./gen-0");
    let dispositions: Vec<_> = candidate
        .trace()
        .iter()
        .map(|row| row.disposition.clone())
        .collect();
    assert!(matches!(
        dispositions[0],
        Disposition::Fresh {
            evicted_before: true
        }
    ));
    assert!(matches!(
        dispositions[1],
        Disposition::Fresh {
            evicted_before: false
        }
    ));
    assert_eq!(candidate.to_json()["eviction_history"]["complete"], false);
    cache.publish(candidate).expect("publish");

    // evict_all respects the live bound and prunes dead records. The
    // newest held handle is also the current published view, so four
    // held handles are four live records (three older views plus the
    // published one).
    let held: Vec<GenerationHandle> = (0..4)
        .map(|_| {
            let candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
            cache.publish(candidate).map(|(handle, _)| handle)
        })
        .collect::<Result<_, _>>()
        .expect("publish held generations");
    assert_eq!(cache.live_generation_count(), 4);
    assert!(matches!(
        cache.evict_all(),
        Err(CacheError::LiveGenerationLimit { live: 5, limit: 4 })
    ));
    drop(held);
    for _ in 0..10 {
        cache.evict_all().expect("evict all");
        assert!(cache.live_generation_count() <= 2);
    }
    let resident = cache.resident_stats();
    assert_eq!(resident.published_entries, 0);
    assert!(resident.eviction_history_len <= 3);
    assert!(resident.live_generation_records <= 2);
}

#[test]
fn content_dependency_ignores_unrelated_changes() {
    let mut dependencies = DependencySet::default();
    dependencies.entries.insert(Dependency::FileContent {
        path: key("/p/node_modules/pkg/package.json"),
        digest: Some(1),
    });
    let mut batch = ChangeBatch::default();
    batch.changed_files.insert(key("/p/main.ts"));
    assert!(batch.violated_dependency(&dependencies).is_none());
    batch
        .changed_files
        .insert(key("/p/node_modules/pkg/package.json"));
    assert!(batch.violated_dependency(&dependencies).is_some());
    let mut batch = ChangeBatch::default();
    batch.deleted_directories.insert(key("/p/node_modules"));
    assert!(batch.violated_dependency(&dependencies).is_some());
}
