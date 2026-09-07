# H2.7d/e completion and transition — draft

Status (2026-09-07): **local comparisons pass; hosted acceptance and adoption
are pending**. This record prepares the separate H2.7d and H2.7e closures in
dependency order. It records no passing D-only intermediate runtime and no
D/E hosted run. Root integration, PR delivery and final adoption remain open.

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
| [Original D corpus](h2-7d-original-corpus.md), including shared 3 | 283 complete production tuples, twice |
| [Original E-only corpus](h2-7e-original-corpus.md) | 8 complete tuples twice and 16 CLI runs; final rerun 33.28s |
| Bundle internal comparisons | 130 complete comparisons; final rerun 86.31s |
| Ordinary/getter/forced Program paths | 4 Bundle Program tests; final E API rerun: 532 calls and 130 final diagnostic snapshots pass in 59.86s |
| [Bundle sinks](h2-7de-bundle-sinks.md) and request canaries | 10 sink cases twice plus 4 stateful callback throws twice; final rerun 16.07s |
| Old H2.6c routes, third collector | 160 exact + 17 typed refusals, each twice; 106.04s |
| Full local H2.6c acceptance | 481 exact / 158 known / 4 deferred; 17 refusal migrations, twice; B activity 552 |
| Emitter regressions | 480 unit tests and 451 contracts pass |
| Adjacent compiler contracts | 21 pass in 427.96s, including declaration transformer replay, all 16 H2.7c contracts and dormant H2.7a controls |
| Populated legacy registries and historical controls | 19 pass in 194.39s, including missing/duplicate/unknown IDs and changed result rejection |
| Qualification and metadata | 41 policy controls, 28 source pins, four schemas and profile/foundation/close freshness pass |

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
This is local acceptance evidence; hosted acceptance remains pending.

## Pending adoption

Complete the existing single-job hosted acceptance with the registered joint
band and measured legacy promotions. Record the actual candidate, passing run
URL and delivery identity here before marking either slice complete.

The prepared metadata transition adds 291 band admissions: summary 11,075
admissions/11,594 executed, completed runtime slices 29, inactive 7, next and
next-runtime H2.8a (`full-output-matrix`). Original H2.5g admission fields and
H2.7a dormant evidence remain historical. The future D/E close patch preserves
the current H2.6c transition counts of 481/158. Synchronize the D/E adoption
state in the live profile/schema, slice index and README only with actual adoption.
STAGE is unchanged.
