Proposal B is sufficient, given six invariants. Two of those are stronger than your draft: a restricted source difference instead of just dropping identity equality, and a no-spread/monotonic check. Also, one existing native validator will reject a successor build as written, and it has to be re-pointed to the successor pin rather than relaxed. Read-only: I read the replay-tree scripts at `4eb742acb` and compared source trees; I ran nothing.

## B is sufficient: why the union covers every affected row

- **Parser trees:** only `crates/syntax/src/recovery.rs` and its unit tests differ between census `67df86615` and the Recovery-next candidate in `crates/syntax|types|diagnostics` (`git diff --stat`). Core digests should therefore be identical.
- **What production consumes:** it reads recovery only through `has_supported_emit_recovery()` (emitter builtins.rs:16262, `preflight_source`) and `events().len()`, which appears only in the refusal message. `has_supported_emit_recovery()` is the `context_recovery` profile key, so the replay captures it.
- **Why B covers everything:** a row's admission can only change if a captured unit's profile flags change. Adding every row that contains a changed input, whatever its role, is therefore a superset of the rows whose verdict could change, and no aggregator is needed.
- **Claims:** keep "selected", never "newly admitted". Full native and TS whole-command comparisons decide qualification.

## Invariants the selector must enforce in successor mode

1. **Identity:** the successor replay's `input_artifact_sha256` equals the snapshot SHA, `digest_code_sha256` is equal, and the input-ID set is identical. That gives zero omitted inputs.
2. **Core equality:** for every input, `successor.core == snapshot.core`. The existing `current == snapshot` exact check stays mandatory and unchanged.
3. **Complete profiles:** both sides have exactly the six boolean keys (`literal`, `missing_await`, `missing_declaration`, `parameter_gaps`, `statement_gaps`, `context_recovery`), none null.
4. **No spread, monotonic:** the nested edit only adds admission, and its bridge only runs when `allow_statement_gaps` is set.
   - Fail explicitly if any `literal`, `missing_await`, `missing_declaration` or `parameter_gaps` flag changes.
   - Fail explicitly on any true→false transition.
   - A future edit that isn't admit-only must then come with its own reasoning rather than being unioned silently.
5. **Source restriction:** the core digest excludes `parse_recovery`, so core equality alone doesn't prove recovery events and actions are unchanged. Prove it by source restriction instead:
   - require successor `source_files_sha256` to equal the current-67 build's for every path except an explicit allow-list (`crates/syntax/src/recovery.rs`, `crates/syntax/src/recovery/context.rs`, `crates/syntax/tests/**`);
   - require `Cargo.lock` and `rust-toolchain.toml` to be equal;
   - record the SHA of the `recovery.rs` diff.
   - The remaining limit: recovery.rs could in principle hold event-recording code. The reviewed diff touches only predicate functions. If you want this mechanical, emit a separate top-level `recovery_events` digest from `replay-recovery-parse.rs` (outside the digest module, so `digest_code_sha256` is unchanged), in both a current-67 replay and the successor replay, and assert them equal. That costs one extra replay, not a census run.
6. **Union and reasons:**
   - For each row, for each unit whose `input_id` changed: append `{"successor": <head>, "input_id", "path", "role", "before": {...}, "after": {...}}`.
   - Never remove or rewrite existing reasons, and never rewrite the five reports.
   - Newly selected rows pass through the existing fallback, `command_input` and safe-integer assertions, so a missing command input is an explicit error.
   - `load_failures` are carried through unchanged.
   - Add `summary.successor_changed_inputs` and `evidence.successor_{sha256, head, source_files_sha256, binary_sha256}`.
   - Leave `head`, `input_manifest` and the other top-level input authority at 67.

**Limit:** units are non-declaration, non-JSON sources. Declaration files aren't transformed, so they never reach `preflight_source`. JSON parsing can't produce paren or statement-gap `ListAbort` shapes. Record this as a stated boundary, not a proof.

## Replay wrapper: successor mode

Add `--baseline-kind successor`:
- `--with-recovery-profiles` is required;
- the parser tree must be clean: `git diff --quiet HEAD -- crates/syntax crates/types crates/diagnostics` and no untracked files there;
- keep the existing historical digest-schema byte checks (`nodes.rs`, `observable_fields.rs`, `for_each_child.rs`);
- replace `source_hashes == source_identity(source_tree)` with the allow-list difference from invariant 5, and record both identities;
- keep `Cargo.lock` pinned from the source tree, but assert it equals the successor's.

The projection and merge-base branches stay untouched and keep validating against the frozen census-67 script bytes.

## Validator that needs re-pointing, not weakening

`recovery_corpus_native.rs:473` requires `selection.syntax_tree_hash == git rev-parse HEAD:crates/syntax` in the *code* workspace. Once the consumer is built with the successor syntax, that comparison fails against the 67 value.
- **Required change:** when the selection carries successor evidence, compare the code workspace's syntax tree hash to `evidence.successor_syntax_tree_hash`, which the successor replay build records. Keep the 67 value as the input authority.
- **Digest check:** the `digest_code_sha256` check at :482 stays as it is. The replay tree keeps `recovery_parse_snapshot.rs`; Recovery-next deleted it, so a merged consumer must keep the replay tree's copy.
- **TS observer and comparator:** don't check the syntax tree, so they need nothing.

## Selector tests (bounded)

- **Regression:** without `--successor`, the output is byte-identical to the current selector's output on the existing fixture.
- **Rejections:**
  1. a successor core mismatch;
  2. a missing or null profile key;
  3. an input ID added or dropped;
  4. an `input_artifact_sha256` mismatch;
  5. a `parameter_gaps` change;
  6. a true→false transition;
  7. a `current != snapshot` mismatch, which must still reject.
- **Positive cases:**
  1. a `statement_gaps`/`context_recovery` change on a module-request-role unit selects its row and carries the before/after reason;
  2. a row already selected for another reason gets the extra reason appended;
  3. `load_failures` are unchanged.

## Pause resolution

Acceptable as described:
- the f4 captures use the immutable copied binary;
- the next builds use only the syntax, emitter, compiler and checker packages;
- the SHA, inode and text maps are saved;
- the text inode is checked before `SIGCONT`, and nothing is ever copied onto the running path.

The census binary is only at risk if something builds xtask into `/Users/hiramatsu/dev/tsc-rs-emitter-final/target`, so a `cargo test -p tsc-rs-xtask` in that target directory must wait until the census finishes. The recorded pause intervals cover both the wrapper's `seconds` and the census's per-universe `elapsed` fields.
