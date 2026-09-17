# OPS-COVER after the emitter milestone: parked coverage work

Static review at `447920e6c73f9a81d0aae8c2729a395426c9ff3b` (PR #551).
[The source-hashed inventory](remaining-seven.v1.json) records the seven
standalone targets that inventory v22 still marks without a direct PR entry.
No additional Rust execution, compatibility admission or repair is claimed here.

User priority,2026-09-17: complete the emitter and take that as the next
milestone. LSP work and general API development belong to a later, separate
phase. The proposals below are a parked coverage backlog, not the next
implementation batch or additional prerequisites for the emitter milestone.
Promote only a bounded contract check needed to validate an actual emitter
repair; do not expand into generic API cleanup while closing emitter work.

| Remaining target | Plain `#[test]` declarations in literal module closure | Integration concern |
| --- | ---: | --- |
| checker `authoritative_external_fact` | 1 | Checks the authoritative external-library fact against suggestion7016 in checked JavaScript; the spelling under node_modules must not override the provider |
| Program `contracts` | 486, including5 ignored |26 registered integration modules plus support; config/resolution/loader/path contracts, with five explicitly local Node oracle audits. Full-target execution needs an explicit ignored-test policy and measured budget |
| fuzz `contracts` | 47 | Includes a Node canonical-vector verifier and real producer/worker process replay; one executable-injection guard is Unix-only |
| harness `contracts` | 83 | Profile inventory, corpus/manifest joins, pin descriptors and module-suffix observations; follows literal `#[path]` modules, but macro includes need separate ownership |
| compiler `h2_5h_utf16_literal_rows` | 1 | Four fixed original IDs and two repeats per row; compare with the existing complete-original witness and H2.5h acceptance before adding duplicate work |
| compiler `h2_7d_original_corpus` | 1 | D283 shared comparator is already called by H2.7d/e acceptance. A missing standalone command is not283 untested cases |
| compiler `h2_8a_original_corpus` | 24 | Includes the23 directory references already in acceptance,769 output-matrix candidates and narrower projections. Shared helper reachability does not establish execution of all24 tests |

The counts are static attributes, not Cargo results: platform cfg, macro includes,
nested registration and ignored tests must be checked against the compiled
harness before a new execution contract is frozen. Do not sum these counts with
original corpus IDs or the foundation job's48 Linux tests.

## Candidate batches for the later coverage phase

1. **OPS-COVER-4C: authoritative checker boundary.** Register the one-test
   checker target with source ownership and explicit native membership. Preserve
   shared checker production fallback. Measure its incremental build/replay cost
   before choosing an existing group. This has no independent frozen oracle;
   describe the two Boolean-provider invariant checks at their actual scope.
2. **OPS-COVER-4D: Program contract modules.** Inventory the26 module registrations,
   shared scalar helpers, observer inputs and the five ignored local audits.
   Begin with prepared-program, path-identity and host-decode boundaries if the
   complete target cannot fit a measured job. Those modules contain28,8 and6
   plain test attributes respectively, not yet an execution receipt. Compare
   config modules against the compiler config/library witness's96 inputs so
   shared IDs retain their different API and command observations. A filtered
   first slice must leave the target classified as filtered rather than complete.
3. **OPS-COVER-4E: harness and fuzz tooling.** Combine these only after checking
   all referenced frozen artifacts and process prerequisites. Pin Node for the
   canonical verifier; retain producer exit/status, fake-divergence rejection
   and executable-injection guards. Link the existing pin descriptors instead
   of reminting historical evidence merely to register a test entry.

When this later coverage phase is resumed, implement at least two compatible
parts before its next hosted cycle. Any
baseline failure gets a cause and bounded owner repair before final-head replay;
never turn a failing test into an ignore or replace its expected observation to
make entry registration green. Keep the two-worker cap and45/60-minute review
and hard-limit budgets. All runtime and shared changes preserve their existing
full selection until narrower ownership is proven.

## Separate accounting work

OPS-COVER-3's three original-wrapper targets need an ID-and-comparison-field
join with existing acceptance and witnesses. `h2_7de_acceptance.rs` explicitly
calls `assert_original_corpus` and `assert_output_directory_references`; that
proves those call sites, not `assert_output_matrix_candidates` or arbitrary
projections in the same shared file. Retain existing admitted, known-divergence
and upstream-exception distinctions. Do not schedule a second complete D283
replay solely to reduce the static no-entry count.

The15 remaining lib/bin harnesses are a separate denominator. Program lib already
has a direct entry through resolution-cache; adding Program integration modules
does not close unrelated crate libraries. Claude's POST-T1 five-child emitter
handoff remains separate from all work above.
