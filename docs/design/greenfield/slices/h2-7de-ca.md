# H2.7d/e completion and transition

Status: **complete**, adopted 2026-09-07. Runtime delivery is
[PR #511](https://github.com/kazhiramatsu/tsc-rs/pull/511), merged at
2026-09-07 13:34:12 UTC as `ad84a7e6e46bd7c5ccd9ac1eb78580dab18151a2`.
[Hosted acceptance 34123778139](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34123778139) passed on
`7db1e4d595d5f5ca25c569a80190ecdfddd8a8c9`: acceptance 45m14s, job 45m28s
(job 101747561233, completed 2026-09-07 13:32:48 UTC).

The complete log preserves all 29 historical conformance/H1/H2 summaries
outside the intentionally updated H2.6c band. H2.6c passes 643 candidates:
481 exact, 158 known, four deferred and 17 current refusal migrations, twice.
The final D/E comparison confirms the exact 283 D and eight E-only original
ID sets, each twice, against the frozen 291-case joint qualification.

The first and second failed runs and their bounded corrections remain dated
history below. H2.7d and H2.7e close in dependency order under one final runtime.
The user-authorized lightweight workflow uses this existing hosted acceptance
before runtime landing and local metadata regeneration afterward. Historical
certificate walks and full developer CI were omitted.

## Scope and observable contract

H2.7d implements JavaScript/declaration bundles, `outFile`, source order,
module naming, declaration references, collisions and partial-output failures.
H2.7e implements declaration maps for ordinary, getter and forced declaration
paths, including Bundle recording. Compatibility remains pinned to TypeScript
6.0.3 (`050880ce59e30b356b686bd3144efe24f875ebc8`); reviewed native TypeScript
7 source informs ownership without replacing TS6 observations.

Comparisons retain complete callback/materialized bytes, BOM, callback order,
source associations and metadata, diagnostics and reporting, `emitSkipped`,
emitted-file lists, raw map observations, and absent-versus-empty distinctions.
Map JSON and recorder observations are compared alongside the emitted text.

## Corpus accounting

| View | Candidates | Exact eligible coverage | Later |
| --- | ---: | ---: | ---: |
| D | 315 | 283 | 32 |
| E | 13 | 11 | 2 |
| D/E intersection | 3 | 3 | 0 |
| Shared union | 325 | 291 | 34 |

The [shared qualification](h2-7de-qualification-draft.md) joins unchanged
original inputs and TS tuples. D covers 280 exclusive cases plus 3 shared cases;
E covers 8 exclusive cases plus those same 3. Formal admission deltas are
D 280 then E 11, totaling 291 once within this joint band. Final D 283 / E 11 coverage
must not be summed to 294. These are band admissions, not globally new case IDs.
The old H2.6c overlap 160 is already included; the other 131 are outside H2.6c
only and are not proved globally new.

The 34 later rows remain H2.8a 23, H2.8b 5, H2.8c 2 and H2.9 4. The two transpile
inputs retain their original API route. General output/config/host axes,
ordinary targeted emit, caller-supplied custom transforms, prepend/project
references and builder runtime retain their H2.8, API1 and BLD1 boundaries.

## Implementation and corrections

- Bundle transform phases retain source order, shared helpers and one printer
  writer/recorder. Source-specific activity inspects every actual Bundle member
  without constructing extra transformer lists. Declaration wrappers, references,
  per-source diagnostics and bundle-wide blocking share the existing resolver.
- [Module naming and generated bindings](h2-7d-system-generated-names.md) preserve
  AMD/System dependency identities, source-local parsed-name collisions and
  shared Bundle generated names. System wrapper/helper/setter names advance
  across sources; computed-field temporary bindings retain their loop scope.
  Mixed JSON keeps its AMD wrapper or System expression and source association.
- [JavaScript declaration producers](h2-7d-javascript-declarations.md) preserve
  owned JSDoc import types, effective readonly literal/unique-symbol types, and
  the source location of collapsed private methods. No output-specific rewrite
  or case-ID dispatch substitutes for the checker/NodeBuilder correction.
- [Map recording](h2-7d-map-recording.md) and the [Bundle artifact worker](h2-7e-m7.md)
  distinguish callback sourceFiles from recorder source order and preserve
  map-before-text callbacks. Parameter-property flags, System assignment source
  identity and synthetic constant-substitution ranges match their TS producers.
- The old-route comparison exposed two additional lifetime/projection defects.
  `b08df5afa` preserves the original `emitDeclarationOnly` bit on current H2.6c's
  map floor; earlier MapFamily and H2.7b DeclarationFamily remain unchanged.
  The two CommonJS Bundle inputs therefore change their prepared options
  honestly; original files/settings/TS expectations do not change. `16cbb6d50`
  restricts ordinary JS-to-declaration parsed metadata handoff to Bundle roots:
  TS disposes SourceFile annotations but retains Bundle child annotations.
  This removes three nonbundle decorated declaration-map regressions. Fresh
  getter/forced calls receive no ordinary snapshot.

## Recorded local verification

| Evidence | Result |
| --- | --- |
| [Original D corpus](h2-7d-original-corpus.md), including shared 3 | 283 complete production tuples twice; final naming-correction rerun 304.80s |
| [Original E-only corpus](h2-7e-original-corpus.md) | 8 complete tuples twice and 16 CLI runs; final naming-correction rerun 18.94s |
| Bundle internal comparisons | 130 complete comparisons in 86.31s, before the two hosted corrections |
| Ordinary/getter/forced Program paths | 4 Bundle Program tests; latest full E API rerun after SourceFile buffering: 532 calls and 130 snapshots in 59.37s |
| [Bundle sinks](h2-7de-bundle-sinks.md) and request canaries | 10 sink cases twice plus 4 stateful callback throws twice; buffering-correction rerun 12.81s |
| Old H2.6c routes, third collector | 160 exact + 17 typed refusals, each twice; 106.04s |
| Full local H2.6c acceptance | 481 exact / 158 known / 4 deferred; 17 refusal migrations, twice; B activity 552 |
| Emitter regressions | 480 unit tests pass before hosted corrections; all 451 contracts pass again after the final correction (1.63s) |
| Adjacent compiler contracts | 21 pass in 427.96s, including declaration transformer replay, all 16 H2.7c contracts and dormant H2.7a controls |
| Populated legacy registries and historical controls | 19 pass in 194.39s, including missing/duplicate/unknown IDs and changed result rejection |
| Qualification and metadata | 41 policy controls, 28 source pins, four schemas and profile/foundation/close freshness pass |
| Static checks | All-target Clippy passes for six changed crates; emitter/compiler/xtask pass again after the final correction (60.74s), with warnings denied; formatting and diff checks pass |

The old collector (`target/h2-6c-de-legacy177-third.json`) reports no unstable,
setup-failure or comparison-failure IDs. First-repetition successful activity is
B 259 / C 0 / D 155 / E 6. Its 17 typed refusals are 16 outDir inputs and
1 case-insensitive host input; error-path activity is unavailable from the consuming API and is
never inferred zero. This is a measured rerun after the projection/lifetime
fixes, not reuse of the earlier 155-exact/5-mismatch result.

Full local H2.6c acceptance now passes all 643 candidates: 481 exact / 158 known
/ 4 deferred, with 639 executed rows, 17 current refusal migrations and two
repetitions. The log is `target/h2-7de-h2-6c-acceptance-shrink-test-profile.log`.
B activity is 552: the prior 293 declaration members plus the measured 259
members from the old D/E overlap. The manifest shrinks from 318 known to 158;
exact cases rise from 321 to 481. Original input/TS expectations and the four
source-deferred rows remain unchanged. The earlier 188/451 profile snapshot is
historical; current transition fields match the measured 481/158 manifest.
The accepted hosted run independently confirms these H2.6c totals, the 17
refusal migrations, and all final D/E original comparisons.

## Ordinary source failure boundary correction

The first candidate `b706142daa562fab2f1f145767c620f1623974b5` failed
[hosted run 34116180082](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34116180082)
after 5m33s. Compilation, conformance and H1 passed; H2.1a then rejected two
writes from the source-deferred `bigintArbirtraryIdentifier.ts` input. Later
bands, including D/E, were not reached.

The Bundle callback change had also dispatched ordinary SourceFile outputs
before all Program sources had transformed successfully. In this six-source
input, two valid sources reached the sink before a later unsupported source
failed. Ordinary SourceFile outputs now retain their previous whole-Program
staging boundary; Bundle callbacks still run before declaration transformation
and at the end of the Bundle. Forced declaration dispatch is unchanged.
Historical ordinary source-deferred failures remain typed and occur before
the first sink write.
No input, expected tuple, promotion or deferred denominator was changed.

All 295 H2.1a candidates pass again (246 exact, 49 historical source-deferred,
704 exact diagnostics and 256 writes, twice; 233.31s). The current unpromoted
multi-source controls are all covered by that existing test. The Bundle sink
controls pass in 12.81s, E's 532 calls/130 snapshots in 59.37s, and original
E-only8/CLI16 in 18.42s. These are local correction checks; the corrected
candidate still requires its own passing hosted run. H2.1e also passes all six
candidates (4.59s); all 451 emitter contracts pass (1.28s), and emitter/compiler/
xtask all-target Clippy with warnings denied passes (59.19s). Four schemas,
profile/foundation/close freshness, formatting and diff checks pass.

## Printed-parent naming correction

The second [hosted run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34117768773)
on `a8fededca48f6e002ecac3d380d1cfbe5e62c096` failed after 34m44s (acceptance
step 34m31s). All bands through H2.5g passed, including the restored H2.1a
deferred-source boundary. H2.5h then reported a new JavaScript-write divergence
in the original `operationsAvailableOnPromisedType.ts#target%3Des5` case.
H2.6/H2.7 were not reached by this run.

The focused original-input replay found three `e_1` → `e_2` substitutions
inside the same 6,097-byte output. The shared numbered-name finalizer eagerly
allocated a parent that already had a later naming moment, then allocated it
again. The [bounded correction](h2-7d-system-generated-names.md#printed-parent-allocation-correction-2026-09-07)
limits eager allocation to parents absent from the printed numbered-binding
inventory. Printed parents retain their existing naming moments; unprinted
System setter parents retain their generated identity and global/bundle
uniqueness checks. Original input bytes, TypeScript expectations and divergence
manifests remain unchanged.

The focused frozen command comparison passes both fresh repetitions (7.23s,
after 4m39s compilation). Full H2.5h passes 932 candidates: 838 exact, 50 known
divergences and 44 deferred, with two repetitions and one local worker. No
manifest entry was added or changed to accommodate the regression.

All 283 original D cases pass twice (304.80s), as do the eight E-only original
cases and 16 real CLI invocations (18.94s). The three module/name tests pass
their 20 module, 12 System and six underscore-alias inputs twice (60.92s);
these retain their JavaScript-only facet boundary. All 451 emitter contracts
pass (1.63s after compilation), including the existing for-await naming
controls. Emitter/compiler/xtask all-target Clippy passes with warnings denied
(60.74s). The current profile's 323 input identities, foundation and close
artifacts are regenerated and fresh; four metadata schemas, formatting and
the original H2.7a runtime-contract equality check pass. The corrected final
candidate still requires its own hosted acceptance result before adoption.

## Adoption and transition

The D/E closure activates both slices in dependency order and adds 291 band
admissions once: summary 11,075 admissions / 11,594 executed, 29 completed
runtime slices and 7 inactive slices. Both next fields become H2.8a
(`full-output-matrix`); H2.8a is not activated by this close. Final D coverage
283 / E coverage 11 is distinct from the ordered deltas 280 / 11. Old H2.6c
overlap 160 is not an additional increment, and current H2.6c transition counts
remain 481 exact / 158 known / 4 deferred.

The profile generator/schema and live parent mirrors in the H2.5h foundation
and H2.7a close move together with the shared qualification/input bindings.
The original H2.5g admission fields and H2.7a dormant runtime evidence remain
historical. Current profile, foundation and close generation and freshness checks pass.
All four metadata schemas pass, the original H2.5g admitted profile is unchanged,
and the dormant H2.7a runtime contract remains byte-equivalent as parsed JSON.
The D/E joint qualification binds its artifact, generator, schema and nine
direct inputs; its 291-case admission increment is counted once. These checks
were performed after this metadata transition on 2026-09-07.

The README, current schedule and separate D/E slice-index rows adopt this
completed record together. STAGE, the pinned
TypeScript version and H2.8/H2.9/API1/BLD1 owner boundaries remain unchanged.
