The eight pin holders need a third, verify-only class rather than plain AUDITED or plain EXEMPT, and the two missing H2.8a schemas were an omission, not a deliberate exemption. Both fixes can be made without weakening any check. Read-only: I read `scripts/pin-audit.py`, `scripts/walk-preflight.py`, `.github/ci/qualification.mjs`, the eight files, the three H2.8a artifacts and the H2.7d/e schema. I ran `pin-audit.py` in its read-only mode (it exits 2 with the 8 unclassified files). Nothing was edited.

## High

**H1. Plain AUDITED would both mis-repair and under-cover these files.**
- **`--fix` rewrites them silently.** It replaces any stale adjacent `(path, hex)` pin with the disk hash. For these D/E pins that would reconnect tests to changed D/E evidence without reviewing their expectations.
- **The pins aren't immutable either.** The three D/E artifacts (`956514…`, `f2e078…`, `ef68d9…`) match their pins today, but they were last re-minted in `c370e0241` (2026-09-18). They're pins that must be re-reviewed after each re-mint, not frozen records.
- **Its extractor misses two of the forms here**, so AUDITED would claim coverage it doesn't have:
  - the `PAIR` regex needs `"path",\s*"hex"`;
  - `de_legacy_collector.rs:238-249` uses `pinned(&workspace.join("ratchets/…"),\n "hex")`, where a `)` sits between path and hash, so it yields zero pairs;
  - `h2_7de_acceptance.rs:164-176` uses `(CENSUS, "hex")` with a named constant, also zero pairs.
- **Plain EXEMPT** hides a changed D/E artifact until a test or runner reaches it.

## Medium

- **M1. Pins that no path-based audit can check.** Some hashes aren't tied to a workspace path:
  - `INVENTORY_SHA` (a target-dir file) and `history_sha`;
  - input fingerprints such as `7849…` (h2_7e_declaration_maps.rs:1036);
  - the library-tree digest `9bb5…` and the `lib.d.ts` hash `0e64…`, whose paths aren't adjacent (h2_7d_original_corpus_shared.rs:108/117);
  - every case/input fingerprint in `h2_6c_de_promotions.rs`.

  These stay enforced when the tests or runners execute. Record them as intentionally unaudited per file, with no invented coverage.
- **M2. Adding registry entries has one real consequence:** `walk-preflight.py:registry_surface` requires a `<rung>.schema.json` for every ORDER rung after `ORDER_START`, and `validateArtifactSchemaContracts()` then validates each registered artifact.
  - `qualification-policy.v2.json` doesn't pin `qualification.mjs` or the contracts (I checked).
  - Run `scripts/pin-index.py --check` after adding the files.
  - The schemas must **not** embed `{path, sha256}` constants, or they join the schema-const pin family that `walk-preflight` re-verifies on every mint. The H2.7d/e qualification schema already has no 64-hex constants; follow that.

## Recommendation 1: `scripts/pin-audit.py`

Add a verify-only class `FROZEN_VERIFY`, mapping each file to its expected extracted-pair count:

```python
FROZEN_VERIFY = {  # D/E re-mint-reviewed pins; never auto-repaired
  "crates/xtask/src/h2_6c_refusal_migrations.rs": 3,
  "crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs": 3,
  "crates/compiler/tests/integration/h2_7e_original_corpus_shared.rs": 3,
  "crates/compiler/tests/h2_7e_declaration_maps.rs": 2,
  "crates/compiler/tests/h2_7e_original_corpus.rs": 2,
  "crates/xtask/tests/unit/h2_2c_acceptance/de_legacy_collector.rs": 3,
  "crates/xtask/src/h2_7de_acceptance.rs": 3,
}
```

- **Extraction:** the existing `PAIR` regex, plus two fixed syntactic forms:
  - `JOIN = r'\.join\("((?:ratchets|vendor|goldens|crates/oracle|\.github)/[^"\n]+)"\)\s*,\s*\n?\s*"([0-9a-f]{64})"'`
  - a named constant: resolve `const NAME: &str = "path";` in the same file, then match `\(\s*NAME\s*,\s*"([0-9a-f]{64})"\s*\)`.
- **Behaviour:**
  - a count mismatch is a hard error (exit 2), so an extractor miss can never pass as coverage;
  - a stale pin exits 1 with "D/E pin changed: re-review the owning rung; not auto-fixed";
  - `--fix` skips these files.
- **`h2_6c_de_promotions.rs`:** goes in **EXEMPT**. It holds only content fingerprints, on the `h2_1a_acceptance.rs` precedent.
- **Also exempt:** `de_legacy_collector`'s `af689…` pin uses `H2_6C_QUALIFICATION_RELATIVE_PATH`, a constant defined in another file, so it stays runtime-enforced. Its companion `INVENTORY_SHA` and `history_sha` do too. List those in the per-file notes.
- **Tests:**
  - each count is met at the current bytes;
  - a mutated hex exits 1;
  - `--fix` leaves `FROZEN_VERIFY` files byte-identical;
  - an extra unrecognised pin form makes the count mismatch.

## Recommendation 2: H2.8a schemas

- **Not intentional:** `h2-8a-candidates` and `h2-8a-observations` became ORDER rungs in `2793145a6` (2026-09-08) with no schemas and no documented exemption. The H2.7d/e census and observation artifacts have no schemas only because they aren't rungs, so there's no precedent for leaving these out.
- **Files:** add
  - `h2-8a-candidates.schema.json` and `h2-8a-observations.schema.json` (names must equal the rung names);
  - `h2-8a-candidate-inputs.schema.json`: the registry allows non-rung artifacts, and this one is the actual input authority.

**Required surface.** Use exact keys (`required` plus `additionalProperties: false`), hashes as `pattern: ^[0-9a-f]{64}$`, and no hash constants.

| Artifact | Pinned values |
|---|---|
| **Candidates** | `schema` = 1<br>`kind` = `"h2-8a-candidates"`<br>`status` = `"candidate-inputs-only"`<br>`typescript` = `"6.0.3"`<br>`source_commit`: 40-hex<br>`generator.path` = `crates/oracle/h2-8a-candidates.mjs`<br>`cases`: exactly 809; each item exact keys, `disposition` = `"candidate-only"`, `runtime_admitted` = false, `input_sha256` hex, `required_slices` non-empty and `^H2\.`<br>`summary`: `runtime_admitted` = 0, `unique_candidates` = 809, `whole_program_inputs` = 809, `transpile_controls` = 0<br>809 is already the producer's own assertion (`prepare()`). |
| **Candidate inputs** | `kind` = `"h2-8a-candidate-inputs"`<br>`typescript` = `"6.0.3"`<br>`shared_mounts.projects`: 233 items of `{path, text}`<br>`cases`: 809 with exact keys<br>`input.route` ∈ {`whole-program`, `transpile-api`} with a per-route `oneOf` for the required shape. The producer supports both routes; the zero-transpile fact is pinned by the candidates summary. |
| **Observations** | `status` = `"typescript-reference-only"`<br>`repetitions` = 2<br>`generator.path` constant<br>`cases`: 809, each with `disposition` = `"typescript-reference-only"`, `repetitions` = 2, and a `typescript_observation` object with its required keys<br>`summary`: `runtime_admitted` = 0, `deterministic_cases` = 809, `typescript_runs` = 1618, `transpile_controls_not_executed` = 0 |

Every keyword this needs is in the `JSON_SCHEMA_KEYWORDS` subset (`const`, `enum`, `pattern`, `min/maxItems`, `oneOf`, `additionalProperties`).

**Registry:** entries labelled "H2.8a candidates", "H2.8a candidate inputs" and "H2.8a observations".

**Negative tests** (mutating in-memory copies against the subset validator):
- a status of `"qualified"`;
- one case with `runtime_admitted: true`;
- `summary.runtime_admitted: 1`;
- 808 cases;
- an extra top-level key;
- disposition `"admitted"`;
- observation `repetitions: 1`;
- a wrong generator path;
- a missing or non-hex `input_sha256`;
- a transpile-route input missing its route-specific fields.

**Scope:** none of this qualifies Rust behaviour or changes artifacts, counts or `source_commit`. The policy and fuzz re-mints stay deferred to the final source freeze, as you planned.
