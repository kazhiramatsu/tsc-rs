use tsc_diagnostics::JsString;
use tsc_host::MemoryCompilerHost;
use tsc_program::{CancellationToken, ChangeBatch, CompilerOptions, ProgramOptions, ResolutionCache, ResolutionMode, RetentionLimits};

fn host() -> MemoryCompilerHost {
    MemoryCompilerHost::builder_js("/p")
        .file_js("/p/main.ts", b"import 'pkg';".to_vec())
        .file_js("/p/node_modules/pkg/package.json", br#"{"name":"pkg","exports":{"a,b":"./joined.d.ts","a":"./split.d.ts"}}"#.to_vec())
        .file_js("/p/node_modules/pkg/joined.d.ts", b"export {};".to_vec())
        .file_js("/p/node_modules/pkg/split.d.ts", b"export {};".to_vec())
        .file_js("/p/util.ts", b"export {};".to_vec())
        .build().unwrap()
}
fn options(conditions: &[&str]) -> CompilerOptions {
    CompilerOptions { module_resolution: Some(99), module: Some(199),
        custom_conditions: Some(conditions.iter().map(|s| JsString::from(*s)).collect()),
        ..CompilerOptions::default() }
}

#[test]
fn distinct_condition_arrays_must_not_reuse_the_same_resolution() {
    let host=host(); let program=ProgramOptions::default(); let token=CancellationToken::default();
    let joined=options(&["a,b"]); let split=options(&["a", "b"]);
    let mut cache=ResolutionCache::new(RetentionLimits::default());
    let mut first=cache.begin(ChangeBatch::default(), &host, &joined, &program);
    let original=first.resolve_module("/p/main.ts".into(), "pkg".into(), ResolutionMode::CommonJs, &token).unwrap();
    cache.publish(first).unwrap();
    let mut next=cache.begin(ChangeBatch::default(), &host, &split, &program);
    let reused=next.resolve_module("/p/main.ts".into(), "pkg".into(), ResolutionMode::CommonJs, &token).unwrap();
    let mut fresh_cache=ResolutionCache::new(RetentionLimits::default());
    let mut fresh=fresh_cache.begin(ChangeBatch::default(), &host, &split, &program);
    let expected=fresh.resolve_module("/p/main.ts".into(), "pkg".into(), ResolutionMode::CommonJs, &token).unwrap();
    eprintln!("original: {}\nreused after options change: {}\nfresh: {}", original.value().summary_json(), reused.value().summary_json(), expected.value().summary_json());
    assert_ne!(original.value(), expected.value(), "fixture must select different exports");
    assert_eq!(reused.value(), expected.value());
}

#[test]
fn dropping_a_candidate_must_not_mutate_a_held_published_entry() {
    let host=host(); let options=options(&[]); let program=ProgramOptions::default(); let token=CancellationToken::default();
    let mut cache=ResolutionCache::new(RetentionLimits::default());
    let mut first=cache.begin(ChangeBatch::default(), &host, &options, &program);
    let original=first.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs, &token).unwrap();
    let (held,_)=cache.publish(first).unwrap();
    let before=held.view().last_used(original.key()).unwrap();
    let mut next=cache.begin(ChangeBatch::default(), &host, &options, &program);
    next.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs, &token).unwrap();
    drop(next);
    assert_eq!(cache.published_id(), held.id());
    eprintln!("held entry last_used_generation before={before}, after={}", held.view().last_used(original.key()).unwrap());
    assert_eq!(held.view().last_used(original.key()).unwrap(), before);
}

#[test]
fn a_candidate_from_another_cache_must_be_rejected() {
    let host=host(); let options=options(&[]); let program=ProgramOptions::default(); let token=CancellationToken::default();
    let mut source=ResolutionCache::new(RetentionLimits::default());
    let mut destination=ResolutionCache::new(RetentionLimits::default());
    let mut candidate=source.begin(ChangeBatch::default(), &host, &options, &program);
    candidate.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs, &token).unwrap();
    let result=destination.publish(candidate);
    eprintln!("foreign publish accepted={}, destination generation={}", result.is_ok(), destination.published_id());
    assert!(result.is_err());
}

#[test]
fn shrinking_history_limit_applies_without_new_victims() {
 let host=host(); let options=options(&[]); let program=ProgramOptions::default(); let token=CancellationToken::default();
 let mut limits=RetentionLimits {max_entries:0,..RetentionLimits::default()};
 let mut cache=ResolutionCache::new(limits);
 let mut candidate=cache.begin(ChangeBatch::default(), &host, &options, &program);
 candidate.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs,&token).unwrap();
 cache.publish(candidate).unwrap();
 assert_eq!(cache.resident_stats().eviction_history_len,1);
 limits.max_eviction_history=0; cache.set_limits(limits);
 let candidate=cache.begin(ChangeBatch::default(), &host, &options, &program);
 cache.publish(candidate).unwrap();
 assert_eq!(cache.resident_stats().eviction_history_len,0);
}
#[test]
fn published_byte_limit_includes_retained_option_identity() {
 let host=host(); let options=options(&[&"a".repeat(65536)]); let program=ProgramOptions::default(); let token=CancellationToken::default();
 let mut cache=ResolutionCache::new(RetentionLimits {max_bytes:4096,..RetentionLimits::default()});
 let mut candidate=cache.begin(ChangeBatch::default(), &host, &options, &program);
 candidate.resolve_module("/p/main.ts".into(), "./util".into(), ResolutionMode::CommonJs,&token).unwrap();
 let (_,stats)=cache.publish(candidate).unwrap();
 eprintln!("published entries={} reported_bytes={}, single option text=65536 bytes",stats.entries,stats.bytes);
 assert_eq!(stats.entries,0,"oversize option identity is retained by published key");
}
