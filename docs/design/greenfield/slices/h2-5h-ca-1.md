# H2.5h / CA-1 — corpus-adoption evidence: the `h2-5h-qualification` ES5-band TypeScript observation sweep

Status: design-gate packet for the first H2.5h corpus-adoption
packet. This packet edits no compiled production file: its entire
write surface is one new oracle generator, one new contract schema,
one new ratchet artifact, the registry rows that bind them, and the
handoff/index/envelope records. The corpus conformance ratchet is
byte-identical acceptance (T0 = 100.0000%, 49024/49024, FP = 0,
unchanged), and the full local gate's Rust phases see zero crate
byte change.

## 1. Identity, purpose, and boundary

- **Slice ID / kind:** `h2-5h-ca-1`, kind `evidence` (oracle
  observation machinery; corpus-inert by construction).
- **Purpose:** freeze the TypeScript 6.0.3 oracle observations for
  the H2.5h corpus-adoption band — the dependency-closed ts-tests
  rows whose only remaining required slice is `H2.5h` — as
  `ratchets/h2-5h-qualification.v1.json`, produced by the new
  generator `crates/oracle/h2-5h-qualification.mjs` under the new
  strict contract
  `.github/ci/contracts/h2-5h-qualification.schema.json`, registered
  in the artifact-contract table. The artifact is the sole
  execution/diagnostic expectation store for every later CA packet
  (the Rust seam census, the seam implementations, and the final
  `run_h2_5h` acceptance wiring consume it; none of them re-derive
  expectations).
- **Non-goals:** no Rust production or test edit of any kind; no
  `cargo xtask` command, local-gate phase, or hosted-acceptance
  change; no seam implementation; no project-suite observation (the
  82 project rows are recorded as a typed deferral block owned by
  CA-3); no ratchet update outside the new artifact and the
  walk-managed pin rebinds.
- **Prerequisites:** the complete H2.5h-b ladder (B-1..B-5) merged —
  the joint `[transformES2015, transformGenerators]` runtime is live
  at `languageVersion < ES2015` with admission floor ES5
  (B-5 merge `2d6835796075ed97028c8aaff979ac7bacbcf2a4`, final head
  `d7fafd23c759150d08d75e48850a02f8e34ee88c`).
- **Trusted base:** `2d6835796075ed97028c8aaff979ac7bacbcf2a4`
  (current `main`, equal to the B-5 merge).
- **Activation state:** before — no H2.5h qualification artifact
  exists; the H2.5h corpus band is unobserved; the owner-inventory
  rows `transform-es2015`/`transform-generators` carry corpus
  disposition `deferred-h2`. After — the band's TypeScript
  observations are frozen and contract-registered; every runtime
  admission fact is UNCHANGED (this packet flips no owner
  disposition, no transition row, and no gate wiring).
- **Next owner:** CA-2 (Rust seam census + seam closures), per the
  ladder in §2.
- **Authority artifact hashes (verified at authoring, all at the
  trusted base):**
  - `ratchets/h2-candidate-dispositions.v1.json`
    `3ebf2e4480b1eaa66db266437ed7d690d4a0970a6dbcb5903d8b97f51bb323b1`
  - `ratchets/h2-owner-inventory.v1.json`
    `6d70a762d5ceda8b1664c2e643bbb64c8f53f501642cfbfba330251961b3dfd4`
  - `crates/oracle/h2-5g-qualification.mjs` (ported source)
    `390e55fdb8e4d0b931a3d1934945fbe0a1dcb7e9ad09dccffa38cb35c1933676`
  - `.github/ci/contracts/h2-5g-qualification.schema.json`
    (contract precedent)
    `e2748aff7f0497c82197bba3a59fc22bc175ab1f463ab7ecafa83b401d7e2175`
  - `ratchets/h2-5g-qualification.v1.json` (predecessor artifact,
    live pin-carrier)
    `547e0b13d7c4b75a07edf61934de9ae1dbd1cd8b4431866c93941fffb19a1a83`
  - `vendor/typescript-6.0.3/project-profile-classification.v1.json`
    `b89589c1372a2c2bb4d8415f8f5b3168605fd11cb43d5b9b55828d834f54342a`
  - `vendor/typescript-6.0.3/lib/_tsc.js`
    `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`
  - handoff `docs/design/greenfield/slices/h2-5h-a.md`
    `89a2eb1376f52210783bfae21b6ce3d1662686a0039bb94564bc8f6dd1ce0f63`

## 2. The corpus-adoption ladder (ratified here)

The B-5 packet's next-owner record names H2.5h corpus adoption and
its concerns. This packet decomposes that work into ordered packets
and is itself the first; the decomposition follows the
evidence-first rule the H2.5h-a campaign already exercised (freeze
the oracle expectation before any production edit is scoped against
it):

> **CA-1** | evidence | the `h2-5h-qualification` observation sweep
> (this packet): generator + contract + registry + minted artifact;
> compiler/conformance rows observed ×2; project rows typed-deferred
> to CA-3 | corpus-inert; T0 byte-identical
>
> **CA-2** | runtime | the Rust seam census over the CA-1 artifact
> (reproducible probe, uncommitted driver) and the seam closures it
> sizes: the remaining unported `checkExternalEmitHelpers` sites of
> the ES5 band, the tslib collision-alias and CommonJS import-equals
> lanes, the exported/namespace/decorated `promoteToIIFE` lanes, the
> es2018 tagged-template `LiftRestriction` consumer, and the es2021
> `ObjectAssign`/`__assign` ES5 fork — split into CA-2a/CA-2b at its
> own design gate if the census demands | corpus ratchet
> non-regression; census-named cases flip mismatch → exact
>
> **CA-3** | evidence+runtime | the project-suite observation
> harness (first project-suite reach in the H2 qualification
> ladder; the vendored
> `project-profile-classification.v1.json` rows are the plan
> inputs) and the 82 deferred rows' observation + execution | the
> CA-1 `project_deferral` block retires
>
> **CA-4** | evidence | acceptance wiring: `run_h2_5h` with exact
> baked totals, the `fn acceptance` append with the
> hosted-acceptance canonical-body and
> `qualification-policy.v2.json` pin updates under the h2-5h-a
> conditional clause, the local-gate `h2-5h-oracle` phase, the
> H2.5g→historical / H2.5h-live profile transition, the
> `h2-transition` row flip, and the handoff close; the es2018
> ObjectRestSpread re-base decision is recorded here or spun into
> its own packet | hosted + local gates green with the new runner

Later packets pass their own design gates; nothing in this packet
authorizes a production Rust edit.

## 3. Required-reference table

| Reference | Role | State before → after |
| --- | --- | --- |
| `h2-candidate-dispositions.v1.json` `cases[].required_slices` | selection authority: the band = rows containing `H2.5h` whose every other required slice is closed through `H2.5g` | unchanged (read-only input; pin-carrying, observation-projection-excluded per §5.4) |
| `h2-owner-inventory.v1.json` rows `transform-es2015`, `transform-generators` (both `owner_slice: "H2.5h"`, disposition `deferred-h2`) | owner-closure rows recorded in the artifact as `disposition_before_h2_5h` | unchanged (the flip is CA-4's) |
| vendored classifications/expansions (compiler + conformance) + `compiler-config-plans` | fixture identity + recorded compiler config plans | unchanged, byte-compared (oracle inputs are never pin-rebound) |
| `vendor/typescript-6.0.3/project-profile-classification.v1.json` | project-row identity for the typed deferral block | unchanged; NEW ninth input, observation-projection-excluded |
| `crates/oracle/vfs-directory-overlay.mjs` | hermetic host directory semantics | unchanged, byte-compared |
| `crates/oracle/h2-5g-qualification.mjs` | ported source (structure, guards, receipt, shards) | unchanged (the 5h generator is a sibling, not an edit) |
| the six h2-5h-a chain artifacts + handoff doc | walk cascade authorities (doc edit → witness re-pin → owner-graph → gap-matrix → dispositions) | re-minted pin-lines only, by the chain walk |
| `.github/ci/qualification.mjs` `ARTIFACT_SCHEMA_CONTRACTS` + schema-boundary list | registry binding | + one row, + one list entry |
| `.github/ci/qualification.test.mjs` "artifact-to-schema mapping" deep-equal table | registry-table snapshot (runs in the gate's oracle phase) | + the matching `[schema, artifact]` pair |

## 4. Pinned upstream map

The oracle surface this generator drives is the vendored TypeScript
6.0.3 bundle (`lib/typescript.js`) and implementation
(`lib/_tsc.js`,
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`),
exactly as the 5g generator drives them; no new upstream API is
introduced. The observation protocol is unchanged:
`ts.createProgram` over the hermetic VFS, then
`ts.emitFilesAndReportErrorsAndGetExitStatus` with the write/report
capture hooks, twice per case, with
`run_fingerprint_sha256`-compared determinism
(5g generator lines 840-874). The harness option surface
(`OPTION_LINE_PATTERN`, `HARNESS_ONLY_OPTIONS`, `optionValue`,
`effectiveCompilerOptions` with
CRLF/`noErrorTruncation`/`skipDefaultLibCheck`), fixture unit
splitting (`makeUnits`), virtual-config plan replay
(`parseConfigContext` against the recorded compiler config plans),
explicit root selection (`explicitRootSelection` incl. the
last-write-wins VFS canonicalization), the hermetic host
(`createProgramCase` + `createHermeticDirectoryOverlay`), the
source analysis (`featureRoots`, `outputSlice`, parse-diagnostic /
AST-depth / import-attributes / advanced-comment markers), and the
write/diagnostic serialization are ported line-for-line from the 5g
generator (sha pinned in §1); the semantic 5h deltas are enumerated
in §5.1-§5.6 and the mechanical constant/label deltas in §5.7.
The band definition matches the transition authority: the H2.5h
target rung is `absent`/`es5`/`es3`
(`crates/oracle/h2-transition.mjs:443`), consistent with the live
admission floor ("ES3 computes as unset per
`_computedOptions.target`", `crates/emitter/src/builtins.rs:152`).

## 5. Generator design (the exact 5h deltas)

### 5.1 Selection contract

From the pinned global dispositions:

- global rows: every case whose `required_slices` contains
  `H2.5h` — authoring-time projection **2,012** (asserted at mint);
- candidates: those whose every other required slice is in the
  closed set `{H2.1a..H2.5g}` — authoring-time projection **932**.
  (Port detail: the generator's `CLOSED_SLICES` literal gains
  `H2.5h` itself — 5g's already contained `H2.5g` — and the
  selection-layer `closedBefore` set is derived by filtering the
  band slice out, exactly the 5g structure; the analysis-layer
  filter therefore treats the band slice as closed, which is
  correct because no analysis marker can produce `H2.5h`);
- suite partition of the candidates — authoring-time projection
  compiler **231** / conformance **619** / project **82**; the
  compiler+conformance ids must each resolve through exactly one of
  the two suite classifications (`moduleCandidates` coverage
  asserted equal to the non-project candidate count), and the
  project ids must each resolve in the vendored project
  classification;
- the projections above are selection-layer facts derived from the
  pinned artifact at authoring; the generator asserts the exact
  integers at mint and freezes them as schema summary consts, which
  from then on are the review surface.

### 5.2 Band guards

- target guard: effective `options.target` must be `undefined`,
  `ts.ScriptTarget.ES3`, or `ts.ScriptTarget.ES5`;
  `targetStateName` renders `absent` / `ES3(0)` / `ES5(1)` and
  fails loudly on anything else;
- module guard: the same 15-state list as 5g (absent through
  `Preserve(200)`);
- expected target/module distributions: seeded from the pinned
  dispositions projection (ES5(1) 836 / absent 9 / ES3(0) 5; the
  ten-state module rollup) as `EXPECTED_TARGET_STATES` /
  `EXPECTED_MODULE_STATES` consts, asserted against the measured
  effective options at every build (both failure messages print the
  measured canonicals — §7 step 3's bootstrap rule).

### 5.3 Analysis, admission, and deferral

`analyzeCase` is ported unchanged: per reached source file the
feature roots, output-extension owner, parse diagnostics, AST depth
(cap 256), import attributes, and advanced-comment placement mark
required slices; `remainingSlices` = those not in the closed set
(which now includes `H2.5g`); disposition
`admitted-for-execution` when empty, else `deferred-to-slices`
retaining the first later owner. The admission-contract sentence is
re-worded for the band: all selected rows are dependency-closed
through H2.5g at the option/owner-inventory layer; a selected
compiler/conformance case is admitted only when every emit-eligible
reached source computes to the ES5 floor, has no parse diagnostics,
has AST depth ≤ 256, and requires no later source/output owner; the
joint `transformES2015`+`transformGenerators` pass runs after the
already-closed transformESNext/class-field/ES2021/ES2020/ES2019/
ES2018/ES2017/ES2016 pipeline and before the module transformer;
diagnostics and writes are exact; deferred cases fail before the
first Rust sink write. Expected within-band deferral owners are a
subset of `{H2.8a, H2.9}` (every feature/output owner below them is
closed); the mint measures the actual `first_deferred_slices`
rollup and the schema freezes it.

### 5.4 Project deferral block (typed, CA-3-owned)

The 82 project candidates are NOT observed. The artifact records a
top-level `project_deferral` object: `owner: "h2-5h-ca-3"`, the
case count, and per-row `{ id, descriptor_path, source: { path,
bytes, sha256, git_blob_sha1 } }`. The `source` identity comes from
the global dispositions row; `descriptor_path` comes from the
matching `project-profile-classification.v1.json` row (dispositions
project rows carry NO descriptor field), and the generator asserts
the join both ways: the classification row must exist for the id,
and its `descriptor_path` must equal the dispositions row's
`source.path` (measured true for all 82 at authoring). The block's
reachability guard is structural: `cases[]` contains only
compiler/conformance rows, so no Rust consumer can execute a
project row from this artifact; the count participates in the
932 = observed + deferred partition assert. The three inputs that
serve only selection/deferral identity — `owner_inventory` and
`global_candidate_dispositions` (both already
projection-excluded in 5g) and the NEW `project_classification`
(the key mirrors its `compiler_classification`/
`conformance_classification` siblings) — are excluded from the
observation-relevant input projection (`observationInputs`), so a
pin-only rebind of any of them never invalidates stored
observations or the check receipt; their observation-relevant
content is enforced through the per-case identity guards and the
count/partition asserts, exactly the 5g rationale (5g generator
lines 937-953).

### 5.5 Receipt, shards, reuse

Ported from 5g with names re-keyed: receipt path
`target/h2-5h/check-receipt.v1.json`, kind
`h2-5h-qualification-check-receipt`, shard env
`TSRS_H2_5H_CHECK_SHARDS` (default 4, max 8), internal shard mode,
`--write` serial with per-case observation reuse, `--check` receipt
attempt before any observation, full-re-observation fallback, and
receipt minting only by a green full re-observation check. The
receipt global key includes the typescript record, the
observation-projected inputs, the execution contract, the owner
closure, and the vendored lib-directory inventory record; the
cases-observation roll is the sorted case-fingerprint hash. The 5g
legacy migration modes (`--upgrade-observation-layout`,
`--rebind-contract`) and the two legacy layout generator-sha consts
are NOT ported.

### 5.6 Artifact identity

- `schema: 1`, `status: "qualified-typescript-oracle"`,
  `phase: "H2.5h-es5-target"`;
- `origin`: `h2_5h_b_b5_merge`
  (`2d6835796075ed97028c8aaff979ac7bacbcf2a4`), `h2_5h_b_b5_head`
  (`d7fafd23c759150d08d75e48850a02f8e34ee88c`), and the recorded
  H2.5g closure lineage `h2_5g_validation`
  (`0653e10d84351c33ebd34d9442198ffff754722b`) / `h2_5g_merge`
  (`507a96ac51af39fe0285760cdbf3244422fc40bd`) as immutable commit
  consts. The live `h2-5g-profile`/`h2-5g-qualification` artifacts
  are deliberately NOT const-pinned: they re-mint on every chain
  walk while H2.5g remains the gate's live oracle phase, and a
  frozen byte pin here would tear on the first walk (their freeze
  to historical lineage is CA-4's transition);
- `selection_contract` with the §5.1 integers and the candidate
  definition sentence;
- `owner_closure`: the two owner rows with
  `disposition_before_h2_5h`;
- `execution_contract`: as 5g (`typescript_repetitions: 2`,
  `rust_repetitions: 2`, `normalization: "none"`,
  `deferred_boundary: "typed failure before first sink write"`)
  plus the §5.3 admission sentence;
- `cases[]`: the observed compiler/conformance rows, 5g record
  shape (input files base64 + hashes, per-file analysis, the single
  stored observation + both run fingerprints, `rust_expectation`);
- `project_deferral`: §5.4;
- `summary`: the 5g rollups plus `project_deferred_cases`, with
  the partition asserts extended to observed + project = candidates.

### 5.7 Mechanical constant/label deltas (the complete list)

Every 5g-specific numeric or name constant re-keys for the band;
none of these is a semantic change and each is asserted by the
mint or the check:

- band consts: `GLOBAL_H2_5H_ROWS = 2_012`,
  `GLOBAL_CANDIDATES = 932`, `OBSERVED_CANDIDATE_CASES = 850`,
  `PROJECT_DEFERRED_CASES = 82` replace the inline
  `11_910`/`9_027` guards;
- `observationTarget()` and the shard arithmetic
  (`Math.floor((OBSERVED_CANDIDATE_CASES - 1 - index) / count) + 1`),
  `shardOrdinal`, `adoption.size`, `reusedObservations`, and
  `cases.length` guards re-key `9_027`/`9_026` → 850/849;
- `typescript_runs === 2 × 850 = 1_700` replaces `18_054`;
  `deterministic_typescript_cases` → 850;
- inline suite asserts `4_712`/`4_315` → `231`/`619`;
- the per-case `selection_origin` label →
  `"global-h2-5h-candidate"`; the receipt kind →
  `"h2-5h-qualification-check-receipt"`; the progress label,
  receipt hit/miss, stale-artifact, fresh-summary, preflight, and
  usage strings re-key `H2.5g`/`h2-5g` → `H2.5h`/`h2-5h`;
- the summary gains `observed_candidates` and
  `project_deferred_cases`; `summary.candidates` becomes the
  GLOBAL 932 (the observed count lives in `observed_candidates`),
  and the selection contract re-keys
  `candidate_denominator` → `observed_candidate_denominator` plus
  the new `project_deferred_candidates`;
- the 5g-only legacy arms (`--upgrade-observation-layout`,
  `--rebind-contract`) and their two layout-generation sha consts
  are not ported; the 5f-profile origin consts are replaced by the
  four §5.6 commit consts.

## 6. Gap delta

No `h2-5h-a-gap-matrix` row changes: the matrix tracks the
ES2015/Generators implementation capabilities, all of which B-5
closed (13 exists / 0 partial / 0 missing), and this packet adds
evidence machinery, not capability. The corpus-adoption state
itself is tracked by the owner-inventory dispositions and the
transition rows, which this packet reads but does not flip.

## 7. Implementation plan (dependency order)

1. **Contract schema**
   `.github/ci/contracts/h2-5h-qualification.schema.json`: draft
   2020-12, `additionalProperties: false` throughout, `$id` ending
   `/h2-5h-qualification.schema.json`, structure cloned from the 5g
   contract with the §5.6 identity (phase const, origin commit
   consts, `project_deferral`, summary fields + the new
   `observed_candidates`/`project_deferred_cases`). Only the
   registry's supported JSON-schema keyword subset is legal
   (`prefixItems` is NOT in it): `owner_closure` enforces
   min/max 2 + `uniqueItems` with the two-key owner enum, and the
   exact `[transform-es2015, transform-generators]` order stays
   generator-enforced (`OWNER_KEYS` order) under the artifact
   fingerprint. Selection-layer consts (2,012 / 932 / 850 / 82 /
   231 / 619 / 1,700 / the target- and module-state tables projected
   from the pinned dispositions) are seeded at authoring and
   cross-asserted by the generator's own tables; observation-derived
   summary values (virtual-config/symlink/admitted/deferred/no-emit
   counts, write/diagnostic totals, the disposition and
   first-deferred rollups) carry NO const until step 4 freezes the
   measured values.
2. **Generator** `crates/oracle/h2-5h-qualification.mjs`: the §5
   port. Mode surface: `--preflight`, `--write`, `--check`,
   internal shard mode; unknown modes fail with the usage line.
3. **Mint** (demoted, serial): `taskpolicy -b nice -n 15 node
   crates/oracle/h2-5h-qualification.mjs --write` — fresh
   observation of every observed-band case ×2. The
   `EXPECTED_TARGET_STATES`/`EXPECTED_MODULE_STATES` tables are
   seeded from the dispositions projection BEFORE the first mint
   (the distribution assert runs in every mode, `--preflight`
   included, so empty tables cannot bootstrap); both assert
   failure messages print the measured canonical rollups so a
   projection miss is self-correcting in one iteration. The
   `--preflight` mode is the cheap pre-mint validation of the whole
   selection layer (counts, project cross-check, fixture identity,
   effective distributions) without observation.
4. **Freeze consts + reuse proof**: copy the measured
   observation-derived summary values (virtual-config/symlink/
   admitted/deferred/no-emit counts, write/diagnostic totals, the
   disposition and first-deferred rollups) into the schema consts;
   re-run `--write` — it must adopt every stored observation
   (`reused_observations` = observed-band count) and the artifact
   diff must be confined to the `generator`/`contract` path-hash
   records and the top-level fingerprint (the artifact embeds both
   hashes, so byte-identity across a generator or schema edit is
   impossible by design); then `--check` (full re-observation path,
   mints the receipt), then `--check` again (receipt hit, seconds).
5. **Registry**: add the `H2.5h qualification` row to
   `ARTIFACT_SCHEMA_CONTRACTS` and `h2-5h-qualification` to the
   schema-boundary list in `.github/ci/qualification.mjs`, and the
   matching `[schema, artifact]` pair to the deep-equal
   "artifact-to-schema mapping" table in
   `.github/ci/qualification.test.mjs`;
   `node .github/ci/qualification.mjs check` and
   `node --test .github/ci/qualification.test.mjs` green.
6. **§8 amendments + chain walk + envelope** (see §8).

Allowed files (the envelope's `allowedPaths`): exactly
`crates/oracle/h2-5h-qualification.mjs`,
`.github/ci/contracts/h2-5h-qualification.schema.json`,
`ratchets/h2-5h-qualification.v1.json`,
`.github/ci/qualification.mjs`,
`.github/ci/qualification.test.mjs`,
`docs/design/greenfield/slices/h2-5h-a.md`,
`docs/design/greenfield/slices/h2-5h-ca-1.md`,
`docs/design/greenfield/slices/README.md`,
`ratchets/fci-readiness/h2-5h-ca-1.v1.json`,
`ratchets/fci-readiness/h2-5h-a.v1.json`,
`ratchets/fci-packet-bootstrap.v1.json`, plus the walk-managed
re-pin carriers (the six h2-5h-a chain artifacts). The
`h2-5g-qualification`/`h2-5g-profile` artifacts are NOT in the
write surface: none of their inputs changes in this walk (no crate
Rust bytes, and neither pins the handoff doc), so both must stay
byte-stable — the walk checks them read-only. Forbidden prefixes: every
compiled crate (`crates/emitter`, `crates/checker`,
`crates/compiler`, `crates/harness`, `crates/binder`,
`crates/syntax`, `crates/types`, `crates/diags`,
`crates/conformance`, `crates/xtask`) and `.github/workflows`.

## 8. Evidence, ratchet, and documentation amendments

1. `docs/design/greenfield/slices/h2-5h-a.md`: packet-ladder item 4
   — the §2 CA ladder with CA-1 marked LANDED at the implementation
   sha; the packet-checker list is unchanged (the new artifact's
   full `--check` joins the once-per-slice packet-checker
   obligations of the CA packets, not the historical h2-5h-a list).
2. `docs/design/greenfield/slices/README.md`: one index row for
   CA-1 (family "H2.5h / CA-1").
3. Envelope `ratchets/fci-readiness/h2-5h-ca-1.v1.json`: status
   `ready`, trustedBase = the branch base, predecessors =
   `[{h2-5h-b-b-5, receiptSha256
   433d7ff02e94ba79de11e1d6b75372086c7e236fc693c6113db296c11e6fae12}]`,
   §7 allowedPaths/forbiddenPrefixes, proof commands = §9;
   bootstrap `allowedPacketIds += h2-5h-ca-1`; the h2-5h-a envelope
   re-pins for the handoff amendment.
4. Chain walk (mjs/doc-only cascade; no crate Rust byte changes, so
   the h1 ladder must verify green WITHOUT re-minting): mint
   h2-5h-qualification; then the handoff-doc cascade in
   dependency order — `h2-5h-a-comment-scope-witnesses` (pins the
   doc) → `h2-5h-a-owner-graph` (pins the comment-scope artifact)
   → `h2-5h-a-gap-matrix` (pins the owner graph) →
   `h2-5h-a-dispositions` (pins both) →
   `h2-5h-a-es2015-generators-witnesses` LAST (it pins BOTH the
   doc and the owner-graph artifact, so re-minting it before the
   owner graph strands a stale pin — the B-5 walk precedent).
   `h2-5h-a-foundation` pins no doc and none of its inputs change
   here: it verifies green untouched (check-first; a re-mint would
   be byte-identical adoption). Verify battery over the untouched
   ladder (`--check` only); `.github/ci/qualification.mjs check` +
   `node --test .github/ci/qualification.test.mjs`; slice-readiness
   for h2-5h-ca-1 AND the whole predecessor chain; pin-sweep.

## 9. Acceptance

All of the following at one head:

```text
node --check crates/oracle/h2-5h-qualification.mjs
node crates/oracle/h2-5h-qualification.mjs --check   # receipt hit after step 4
node crates/oracle/h2-5h-a-foundation.mjs --check
node crates/oracle/h2-5h-a-comment-scope-witnesses.mjs --check
node crates/oracle/h2-5h-a-owner-graph.mjs --check
node crates/oracle/h2-5h-a-gap-matrix.mjs --check
node crates/oracle/h2-5h-a-dispositions.mjs --check
node crates/oracle/h2-5h-a-es2015-generators-witnesses.mjs --check
node .github/ci/qualification.mjs check
node --test .github/ci/qualification.test.mjs
node .github/ci/slice-readiness.mjs --check h2-5h-ca-1
cargo xtask ci --baseline <trusted-base>             # full local gate, all phases
```

plus, recorded in the train log: the step-3 reuse proof
(second `--write` byte-identical, `reused_observations` = the
observed-band count), the step-4 receipt-hit `--check`, and the
mint totals (candidates / observed / admitted / deferred /
project-deferred, TypeScript runs = 2 × observed,
`deterministic_typescript_cases` = observed). The corpus
conformance ratchet must be BYTE-IDENTICAL at the trusted base
(T0 = 100.0000%, 49024/49024, FP = 0, every band) — this packet
can only add evidence, never change compiler behavior. Slice
completion = all of the above green at the final candidate head
plus the hosted `gates` check green on the PR.

## 10. Traceability

| Deliverable | Evidence |
| --- | --- |
| band selection (2,012 → 932 → 850 observed + 82 deferred) | artifact `selection_contract` + summary consts + the §5.1 mint asserts |
| observation determinism | per-case double fingerprints + `deterministic_typescript_cases` == observed |
| observation content integrity | per-case `case_fingerprint_sha256`, whole-artifact fingerprint, the check receipt roll |
| project deferral | `project_deferral` block, its classification cross-check, the partition assert |
| owner closure | `owner_closure` rows w/ `disposition_before_h2_5h` |
| registry binding | `ARTIFACT_SCHEMA_CONTRACTS` row + schema-boundary entry + `qualification.mjs check` |
| ladder ratification | §2 + h2-5h-a.md item 4 + the README row |

Heavy-command ceilings: the mint is a single serial node process
(observation reuse on re-run); demote with
`taskpolicy -b nice -n 15`; sharded `--check` obeys
`TSRS_H2_5H_CHECK_SHARDS` (set 2 when the user is present, never
run the full `--check` standalone at default shards during a
running gate). Single write owner: this train is the only writer of
every §7 path.

## 11. Prohibitions

No hand-authored observation bytes; no case-ID or path-specific
branches; no normalization of TypeScript output; no generic
fallback converting an unknown option/target/module state into
success (every unknown state fails the mint); no production Rust
edit; no gate/hosted wiring change; no owner-inventory,
transition-row, or gap-matrix disposition flip; no edit to the 5g
generator, contract, or receipt; no summary const invented rather
than measured.

## 12. Unresolved items

None. An independent design-gate review (2026-08-23, fresh-context
agent, every citation/number/cascade recomputed) returned
READY-WITH-FIXES: 1 blocker (the `qualification.test.mjs` deep-equal
table missing from the write surface — independently discovered by
the implementer in parallel) + 6 fixes (walk order for the
es2015-generators witness re-mint, the step-3/4 bootstrap and
reuse-proof sequencing, `descriptor_path` provenance + join assert,
the `owner_inventory` projection exclusion, the mechanical-delta
enumeration §5.7, the six chain `--check`s in the proof battery) +
5 notes (foundation is not doc-pinned; closed-set wording; input
key naming reconciled to `project_classification`; a cosmetic line
range; the vacuous 5g-rebind hedge dropped). All folded into this
document before the design-gate commit.

Decisions closed at authoring: the ladder decomposition (§2);
project rows typed-deferred to CA-3 rather than observed here
(§5.4); the live-5g-artifact non-pinning with commit-const lineage
instead (§5.6); the legacy 5g migration modes not ported (§5.5);
summary consts frozen from the mint rather than pre-declared (§5.1,
§7 step 4). Numeric projections in §5.1 are asserted, not assumed:
a mismatch at mint stops the train for re-review of the selection
layer.

## 13. Citation status

Verified at authoring against the trusted base: the 5g generator
line anchors cited in §4/§5 (selection 1578-1597, observation
840-874, projection rationale 937-953, receipt 983-1152, admission
sentence 1558-1560, summary/mint tail 1679-2138); the transition
band rule `h2-transition.mjs:443`; the admission-floor comment
`builtins.rs:152`; the owner-inventory rows
`transform-es2015`/`transform-generators` (owner slice `H2.5h`,
disposition `deferred-h2`); the B-5 next-owner record
(`h2-5h-b-b-5.md:114-119`); the hosted-acceptance conditional
clause (`h2-5h-a.md:302-314`, CA-4's obligation); every commit sha
resolved by `git rev-parse`; every artifact sha computed by
`shasum -a 256`.
