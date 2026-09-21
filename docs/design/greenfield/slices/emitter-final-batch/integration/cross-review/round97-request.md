Read-only review; no edits, Cargo, artifacts, or remote actions. Current Recovery-next c1e699ab5 is running native contracts; all investigation should use isolated /Users/hiramatsu/dev/tsc-rs-emitter-final-gate-prep (same committed HEAD). Do not touch Census67, whose process is paused by the controller.

Final walk preflight found inherited maintenance gaps: 8 unclassified Rust pin holders, missing H2.8a candidates/observations schema contracts + registry entries, and separately stale policy/fuzz references. Codex will not blindly repin frozen observations or claim source-hash updates qualify behavior. Focus this round on a minimal safe design for the first two classes; policy/fuzz mint waits final source freeze.

Read scripts/pin-audit.py, scripts/walk-preflight.py, .github/ci/qualification.mjs and these 8 files:
crates/xtask/tests/unit/h2_2c_acceptance/de_legacy_collector.rs
crates/xtask/src/h2_7de_acceptance.rs
crates/xtask/src/h2_6c_refusal_migrations.rs
crates/xtask/src/h2_6c_de_promotions.rs
crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs
crates/compiler/tests/integration/h2_7e_original_corpus_shared.rs
crates/compiler/tests/h2_7e_declaration_maps.rs
crates/compiler/tests/h2_7e_original_corpus.rs

These mostly pin immutable D/E sources (census956514..., inputsf2e078..., observationsef68d9...), not mutable current-stage artifacts. Original shared also pins library tree/default wrapper. de_legacy_collector uses workspace.join(path) + hash, constants, and historical qualification/manifests. h2_7de_acceptance uses named path constants. Plain AUDITED classification may silently allow --fix to rewrite immutable pins, while EXEMPT hides them from early detection (runtime does enforce). Consider a separate frozen/read-only audit classification with explicit extraction for the fixed syntactic forms versus narrow reasoned EXEMPT consistent with existing h2_2c_acceptance. Recommend least-impact concrete approach that does not weaken validation or add fake audited coverage.

For schemas: h2-8a-candidates.mjs emits both candidates809 and candidate-inputs809; h2-8a-observations.mjs emits809 completeTS6.0.3 observations twice. Both are already in chain-walk ORDER but missing .github/ci/contracts schemas and ARTIFACT_SCHEMA_CONTRACTS registry. Inspect their existing artifact structures and producer invariants; propose exact required surface and meaningful negative tests. New schemas should enforce current input/reference-only status and zero runtime admission; do not modify artifacts, counts, source_commit, or treat them as Rust qualification. Candidate-inputs also deserves a registry/schema though not a separate ORDER rung. Identify whether there is any reason these are intentionally omitted, and any downstream policy consequences of adding registry entries. Report concrete high/medium issues and small implementation recommendation.
