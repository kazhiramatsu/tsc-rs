# Foundation status-line repair and membership reference refresh

The hosted foundations job at candidate `5b28906cf5544c63bb55f4c97e40f714672fd722`
failed after all native syntax tests passed. With `--nocapture`, test diagnostics
split libtest's `test NAME ... ok` lines; the exact-name verifier therefore
refused the output. The original full eight-target syntax command reproduced
native exit 0 and the same verifier refusal locally.

The command now uses `--show-output`, preserving both whole status lines and
successful-test diagnostics. The verifier is unchanged. The real fixed syntax
batch passes all 14 tests and the original runner, including its TypeScript
observers, passes all 17 foundation targets / 49 Darwin tests. Both preserved
hosted and native interleaved logs still refuse; an injected fake success line
also refuses. Python planner controls pass 88 tests, including the full frozen
catalog count assertion. This is bounded test membership, not whole-compiler
compatibility coverage.

Broader Node checks found a separate stale reference in the non-authoritative
FCI H2.5g membership plan. The last plan update at
`5a0275bc74777cc5a34b2c61c56b4173b29ed6d2` bound the old qualification digest.
The old qualified bytes were recovered from that commit and their raw digest
verified against the old plan. Both original shadow reports, using old inputs
and a candidate with only the current source digest substituted, are identical
apart from that digest. All 9,027 case IDs, ordering, dispositions and four shard
projections remain unchanged (8,511 admitted / 6 H2.8a / 510 H2.9 deferred).

Only `case_id_source_sha256` is refreshed. The five legacy membership/shard
hashes and all shard ranges remain unchanged; their opaque legacy projection
is not equated with the separately computed source-order digest. The checker
and its tests are unchanged. The old source refuses under the new plan. All
94 tests across six Node files now pass; the original repeat-check passes.
This shadow is not an acceptance test or a new native execution claim.

Actual read-only Claude Opus reviews 233 and 234 cover both bounded changes.
The integrator separately executed the historical equality and native checks;
those executions were not performed by Claude. Current policy source pins,
27 artifact contracts and the FCI readiness chain (45 envelopes / 42 ready)
pass. No Rust, ratchet, contract or fixture changes were made. All 784 Rust
files still match the committed r597 walk certificate, all 920 current runtime
inputs are fresh, and the original 90 untracked files retain bytes and mtimes.
The certificate remains valid for its original scope; final CI is separate.

The old r599 local CI was deliberately stopped at exit -15, qualified false,
after four phases. Its raw log and receipts remain in `stopped-local-ci/` and
are never counted as final CI. The new candidate still requires a complete
unsplit local gate and hosted checks before merging. The new v39 inventory is
written last from the repair commit; v38 and historical source-state records
remain unchanged.

The unsuccessful initial hosted-log request, actual successful API retrieval,
original failing Node run and both successful repairs are retained in this
archive. Raw logs and the repair diff are gzip-compressed without changing
their contents; the manifest records both stored and raw hashes.
