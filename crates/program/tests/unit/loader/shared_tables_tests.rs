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
    DirectoryResolutions::append_to(table, &directory, &mut walk);
    let key = |specifier: &JsString, mode| (directory.clone(), specifier.clone(), mode);
    assert_eq!(walk.len(), 3);
    assert_eq!(walk.get(&key(&model, ResolutionMode::CommonJs)), Some(&2));
    assert_eq!(walk.get(&key(&model, ResolutionMode::EsNext)), Some(&4));
    assert_eq!(
        walk.get(&key(&event, ResolutionMode::Unspecified)),
        Some(&3)
    );
}
