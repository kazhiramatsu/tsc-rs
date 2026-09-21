# r27: missing-await recovery admission design

Read-only actual Claude review, no edits, Cargo or subagents. Use Fable; if a
rate limit is reached we will switch to Opus. Review bounded production changes
and report concrete findings in about 45 lines.

The data-only stage is now green: 179 syntax library tests, the original 36
inputs' reachable missing nodes and full syntax diagnostics against two TS
parses, and the existing recovery-provenance contract. See
codex-r25-native-findings.md and recovery-native-facts-r25b.json. In particular
asyncFunction10 has THREE reporting attempts (one retained, two suppressed),
and await(f()) reparses whereas await f() is already an AwaitExpression.

The proposed next-stage files are under /tmp/emitter-recovery-stage-b, relative
to this worktree. They are not applied yet. Compare four files to current:
crates/syntax/src/recovery.rs, crates/syntax/src/lib.rs,
crates/emitter/src/builtins.rs, crates/xtask/src/utf16_literal_recovery_census.rs.

Proposal: preserve old literal-only admission; admit missing AwaitExpression
operands only when every structural event has a retained diagnostic and a
unique recorded missing Identifier FULL-start, every reachable zero-width
Identifier matches such an event and has AwaitExpression parent, and there are
no token skips or reparses. Ignore unreachable speculative nodes and JSDoc.
This should initially cover 24 of 36 known rows, but is a structural predicate,
not a case allowlist. A new census mode compares old and new predicates over
all acceptance inputs to discover the full newly admitted population; legacy
census output remains unchanged. We will compare complete TS/native commands
twice for all newly admitted inputs and add nearby positive/negative controls
before retiring any knowns. Reparse and other missing-node shapes stay refused.

Please inspect whether the predicate over-admits, misses relevant provenance,
or could reject clean inputs previously admitted. Check emitter transform and
printer handling of a zero-width await operand (ES5, ES2015, ES2017+, async
generator, nested/body/parameter/default positions and source maps). Identify
where the census might miss inputs or project different parse options. Suggest
the smallest evidence-backed corrections and the highest-value adjacent cases.
Do not call 24 rows exact until complete native command comparisons succeed.
