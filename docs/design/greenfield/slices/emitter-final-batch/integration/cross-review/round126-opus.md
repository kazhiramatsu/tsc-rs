Feature parity is enough, and the regression test catches the observed failure without weakening any assertion. This is from reading the uncommitted diff, `crates/xtask/src/recovery_parse_snapshot.rs` and the selector, plus a short read-only extraction from the census snapshot; I made no edits or builds.

## Does `preserve_order` parity cover both digests?

**Where the representation matters.**
- `input()` computes `input_id = sha256(to_vec(json!([file_name, text_sha256, options_json])))` (`recovery_parse_snapshot.rs:39-42`).
- `options_json` is a `json!({...})` object. With `preserve_order` it serializes in written order (`script_target`, `language_variant`, …); without it, the default sorted map changes the bytes. That explains `0000b94d…` becoming `e9f1c740…`.
- The graph digest does the same: node descriptors are `json!({"node":…,"pos":…})` objects hashed through `to_vec` into `ast_shape_sha256`/`ast_context_sha256`.
- In both places, byte order depends only on how serde_json represents maps. There is no float formatting involved: the data is integers, strings and booleans. So matching the census's `preserve_order` setting is the only representation fact that needs to agree.

**Census side.** The census's xtask gets `preserve_order` through feature unification:
- `crates/program/Cargo.toml:15`
- `crates/harness/Cargo.toml:17`
- `crates/checker/Cargo.toml:17`

**Equality checks aren't affected.** `replay`'s `assert_eq!(self::input(...), *input)` and `assert_eq!(&options_json(...), o)` compare `Value`s. With `preserve_order`, `Map` is an IndexMap whose equality ignores order, so those checks keep the same meaning. Only the hashed bytes change.

**End-to-end check already exists.** `select-recovery-parse-corpus.py:99` asserts the current replay's `core` and `profiles` equal the census-recorded values for every input. The `core` value contains the order-sensitive graph hashes, so any remaining graph-digest disagreement fails there. It can't pass silently.

## Other standalone settings that could invalidate the comparison

| Setting | Status |
|---|---|
| Crate versions | The candidate `Cargo.lock` is copied (`:97-98`) and the build runs `--offline`, which pins serde_json and indexmap. Cargo will still rewrite the lock to add the root package. |
| Toolchain | `cwd=source_tree`, so `rust-toolchain.toml` applies. |
| Profile (`opt-level=3`, `debug=0`, dev debug assertions and overflow checks on) | Doesn't affect serialization. |
| `current-recovery-profiles` feature | Only affects the profile/recovery fields that the selector also compares. |
| Digest source | Byte-identical, checked by the `include_bytes!` hash assert at replay start. I confirmed the working-tree file is still `3ed02323…`. |

**Two cheap evidence steps worth adding:**
1. Record the resolved `serde_json`, `indexmap`, `base64` and `sha2` versions from `build/Cargo.lock` after the build, and assert serde_json and indexmap equal the census lock's versions.
2. Once, in read-only form, record the census xtask's serde_json feature set (`cargo tree -e features -i serde_json`) to show it is `std` plus `preserve_order` only. Integer-only data makes `arbitrary_precision` or `float_roundtrip` irrelevant, but the record removes any doubt.

## The regression test

- **It catches the observed failure.**
  - The input is the verbatim first census input, not something this binary produced and read back.
  - `snapshot::replay` recomputes `input(file_name, text, options)` and asserts equality with the recorded object, including `input_id`.
  - Under a sorted map the `input_id` becomes `e9f1…`, so the assertion fails in `cargo test` (`:109`), which runs with the same manifest and features as the replay binary.
- **Nothing is weakened.** The test adds a check and changes none of the existing ones.
- **Scope.** It directly covers ordering in the `options` object. Graph-descriptor ordering is covered by selector `:99` against the census's recorded `core`.
- **Keep any `core` check out of this unit test.** A per-input `core` equality check would be valid only for the current tree, because projection and merge-base trees may legitimately differ on `core` (selector `:107-110`). The unit test runs for all trees, so adding it there would break them.

Your round 125 dispositions and next steps are consistent with my findings.
