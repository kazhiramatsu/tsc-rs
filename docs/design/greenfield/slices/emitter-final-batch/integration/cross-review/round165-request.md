# Round165: runtime input registration preflight blocker

Read-only review only; no edits or Cargo/Node/test/generator runs. Canonical source is frozen in /Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep at 4dfc0fb4d3b3ca2364957c85d9ad84bd0a677dc2 while r224 finishes36historical replays+transpile+workspaceclippy. r215 preparation had an unrelated artifact-filter failure, fixed in external r224. Retirement remains unchanged.

Our actual `scripts/walk-static-checks.py` r228 found an H2.5g runtime input closure error BEFORE the canonical walk. Its other two curated-anchor checks reported no static anchor blockers. Read-only r226 found23stale policy/fuzz source hashes only, to refresh after final source.

I matched h2-5g-profile.mjs changedRuntimeInputPaths() exactly in a read-only census:
/tmp/emitter-runtime-input-census-r229.json.
86parent inputs+237NEW_RUNTIME_INPUTS =323. Missing599 files:61production,535test paths,3other. Of599,486 are byte unchanged from trusted main3b1f5fe;113 changed on this train. One stale listed path crates/emitter/tests/unit/tagged_template/tests.rs was deleted in d364a056a, absent in current and trusted main; no pretend replacement.

Prepared, NOT APPLIED:
/tmp/emitter-runtime-input-proposal-r229/{crates/oracle/h2-5g-profile.mjs,.github/ci/contracts/h2-5g-profile.schema.json,proposal.json}
/tmp/prepare-emitter-runtime-inputs-r229.py.
Proposal preserves all86parent inputs, all236surviving priorNEW entries, all existing NON_RUNTIME_SHADOW_INPUTS unchanged; explicitly appends all599missing current paths (conservative hash inclusion), drops solely the deleted test-file identity, and pins exact runtime count323->921 in generator and schema minItems/maxItems. No corpus disposition, case counts, runtime behaviour, semantic pin masks, historical observations, or admission policy changes. Comment explicitly says hashes are input identities, not test-execution proof.

Please independently review actual closure arithmetic/diff and deletion provenance. Is conservative inclusion of535test paths correct under the existing changed-crates fail-closed rule, or does it create an actual downstream contract violation? I prefer including hashes over inventing new exclusions. Identify any downstream fixed count/source hash/registration obligations not handled by canonical walk/schema repin. Is the narrow2file proposal sufficient? Would deleting the stale listed path violate any immutable lineage beyond the deliberately current runtime-input list (parent86 fully preserved)? Propose minimal correction if needed. Do not broaden architecture/runtime or treat old parent profile observations as current.

After source test success, apply reviewed2files before source pin refresh and the ONE official canonical walk. Post-walk immutable inventoryv36 and finalV unsplitCI remain as agreed164. No merge/qualification yet.
