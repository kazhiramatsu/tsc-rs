use super::*;

/// A shard's map files a key under a seven-bit tag, the top seven bits of
/// the key's hash, and compares the keys of the entries whose tag is the
/// one it looks for. The shard of a key comes from other bits of the same
/// hash, so the keys of one shard keep their tags apart. (A shard chosen by
/// the top six bits leaves two tags to a shard, and a lookup compares the
/// paths of half the entries it passes.)
#[test]
fn the_keys_of_a_shard_keep_their_tags_apart() {
    type Table = Sharded<JsString, ()>;
    let mut tags = vec![std::collections::BTreeSet::new(); Table::SHARDS];
    for index in 0..8192 {
        let key = JsString::from(format!(
            "/work/packages/core/src/services/module-{index}/index.ts"
        ));
        let mut hasher = rustc_hash::FxHasher::default();
        key.hash(&mut hasher);
        tags[Table::shard_index(&key)].insert(hasher.finish() >> 57);
    }
    // 8,192 keys in 64 shards: about 128 keys to a shard and 128 tags to
    // draw from, of which about 80 come up.
    let fewest = tags.iter().map(|tags| tags.len()).min().unwrap();
    assert!(fewest >= 32, "a shard with {fewest} tags");
}

/// A directory's resolutions are found by a request's specifier and mode:
/// the same specifier in another mode is another resolution, a later
/// resolution of the same request takes the earlier one's place, and the
/// walk's map gets each of them under (directory, specifier, mode).
#[test]
fn a_directorys_resolutions_are_kept_by_specifier_and_mode() {
    let table = Arc::new(DirectoryResolutions::<u32>::default());
    let model = JsString::from("./model");
    let event = JsString::from("vs/base/common/event");
    assert_eq!(table.get(model.as_js(), ResolutionMode::EsNext), None);
    table.insert(model.as_js(), ResolutionMode::EsNext, 1);
    table.insert(model.as_js(), ResolutionMode::CommonJs, 2);
    table.insert(event.as_js(), ResolutionMode::Unspecified, 3);
    assert_eq!(table.get(model.as_js(), ResolutionMode::EsNext), Some(1));
    assert_eq!(table.get(model.as_js(), ResolutionMode::CommonJs), Some(2));
    assert_eq!(table.get(model.as_js(), ResolutionMode::Unspecified), None);
    assert_eq!(table.get(event.as_js(), ResolutionMode::EsNext), None);
    table.insert(model.as_js(), ResolutionMode::EsNext, 4);
    assert_eq!(table.get(model.as_js(), ResolutionMode::EsNext), Some(4));

    let directory = LexicalPath::from_owned(JsString::from("/work/src"));
    let mut walk = FxHashMap::default();
    DirectoryResolutions::append_to(Arc::default(), &directory, &mut walk);
    assert!(walk.is_empty());
    DirectoryResolutions::append_to(table, &directory, &mut walk);
    let additional = Arc::new(DirectoryResolutions::default());
    additional.insert(model.as_js(), ResolutionMode::CommonJs, 7);
    let other = LexicalPath::from_owned("/work/other".into());
    DirectoryResolutions::append_to(Arc::clone(&additional), &other, &mut walk);
    additional.insert(event.as_js(), ResolutionMode::EsNext, 5);
    DirectoryResolutions::append_to(additional, &directory, &mut walk);
    assert_eq!(walk.len(), 2);
    let table = &walk[&directory];
    assert_eq!(table.get(model.as_js(), ResolutionMode::CommonJs), Some(&7));
    assert_eq!(table.get(model.as_js(), ResolutionMode::EsNext), Some(&4));
    assert_eq!(
        table.get(event.as_js(), ResolutionMode::Unspecified),
        Some(&3)
    );
    assert_eq!(table.get(event.as_js(), ResolutionMode::EsNext), Some(&5));
    assert_eq!(
        walk[&other].get(event.as_js(), ResolutionMode::EsNext),
        None
    );
}

/// A path's symbol is made when the table first sees the path and is the
/// same for every later call, whichever of them queues: the read-ahead takes
/// the symbol of a target from the call that queued it and does not ask the
/// table a second time for the row's own symbol.
#[test]
fn a_paths_symbol_is_the_same_for_the_queueing_call_and_every_later_one() {
    let queued = Sharded::<CanonicalPath, QueuedPath>::new();
    let count = AtomicUsize::new(0);
    let path = |text: &str| {
        ProgramPath::from_js_parts(text.into(), text.into())
            .unwrap()
            .canonical()
            .clone()
    };
    let a = path("/project/a.ts");
    let b = path("/project/b.ts");
    // Queued first: the entry's symbol, and this call is the queueing one.
    let (a_symbol, a_new) = queued_path_symbol(&queued, &count, &a, true);
    assert!(a_symbol.is_some() && a_new);
    // A lookup after the queueing: the same symbol, nothing queued, no entry made.
    assert_eq!(
        queued_path_symbol(&queued, &count, &a, false),
        (a_symbol, false)
    );
    assert_eq!(
        queued_path_symbol(&queued, &count, &a, true),
        (a_symbol, false)
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);
    // Looked up first, queued later: the lookup makes the entry and its
    // symbol, the later call is the one that queues.
    let (b_symbol, b_new) = queued_path_symbol(&queued, &count, &b, false);
    assert!(b_symbol.is_some() && !b_new);
    assert_ne!(b_symbol, a_symbol);
    assert_eq!(
        queued_path_symbol(&queued, &count, &b, true),
        (b_symbol, true)
    );
    assert_eq!(
        queued_path_symbol(&queued, &count, &b, false),
        (b_symbol, false)
    );
    assert_eq!(count.load(Ordering::Relaxed), 2);
}
