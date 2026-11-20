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
