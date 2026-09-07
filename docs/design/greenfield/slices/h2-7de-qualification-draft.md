# H2.7d/e TypeScript qualification join draft

This packet qualifies an identity-preserving join of the existing TypeScript
reference corpus. It does not certify a Rust comparison, activate a runtime
profile, register hosted work, or close either slice.

| Membership | Candidates | Eligible full Program | Later |
| --- | ---: | ---: | ---: |
| D band | 315 | 283 | 32 |
| E band | 13 | 11 | 2 |
| D/E intersection | 3 | 3 | 0 |
| Union | 325 | 291 | 34 |
| D only | 312 | 280 | 32 |
| E only | 10 | 8 | 2 |

The 291 eligible rows are D-only 280, E-only 8, and D/E 3. Eligibility follows
the frozen effective-owner census; it does not depend on Rust output or on a
test passing. The latest partial Rust results must therefore never be read by
this generator or used to change its membership.

Of the 34 later rows, 32 already have complete whole-Program TypeScript
observations: H2.8a 23, H2.8b 5, and H2.9 4. The remaining two H2.8c rows
are transpile API references with preserved original units/options and no
whole-Program observation. Both stay in the union denominator. All 323
existing observations remain available, including the 32 later references.

## One shared artifact

The proposed files are:

- `crates/oracle/h2-7de-qualification.mjs`
- `ratchets/h2-7de-qualification.v1.json`
- `.github/ci/contracts/h2-7de-qualification.schema.json`

One artifact is smaller than separate D and E artifacts because both use the
same three frozen source artifacts and share three cases. The existing C
consumer in `crates/xtask/src/h2_7c_acceptance.rs` validates a named artifact's
kind, status, fingerprint, and direct input identities. The C profile check
in `crates/oracle/h2-5g-profile.mjs` reads explicit summary denominators; it
does not require one artifact for each runtime slice. A common D/E artifact
can follow those checks while exposing both bands as views of one union.
This draft does not change either consumer or the admission policy.

Each case stores original case ID, suite/source identity, unchanged required
owners, D/E membership, remaining owners, and input route. Its candidate,
input, and observation references contain exact array indices and SHA256
hashes of the complete original JSON rows. The observation reference also
hashes the complete TypeScript tuple. These are references into three
immutable artifacts, pinned by full-file SHA256 in the generator and in the
qualification's `inputs` list. The input file's full hash covers the shared
233-file project mount as well as all 325 input rows.

The generator checks one-to-one case-ID coverage and preserved order, source
bytes/SHA256/git-blob identity, candidate/input/observation agreement, two
recorded observations, full tuple/write/map shapes, callback and materialized
byte lengths/BOM, and all summary denominators. There is no option floor,
configuration rewrite, source filter, tuple normalization, or inherited
parent success. The original three files remain byte-identical.

## Execution and consumer contract

`node crates/oracle/h2-7de-qualification.mjs --write` creates the join;
`--check` reconstructs it and checks exact artifact freshness. Both validate
direct observation provenance. They do not run TypeScript again or walk all
historical certificates. The frozen observation producer already records
323 full Programs compared twice, 646 TypeScript runs. The new artifact's
execution contract states this distinction explicitly. It makes no claim
that a fresh TypeScript emit occurred during join validation.

A future consumer must validate the qualification fingerprint after removing
`qualification_fingerprint_sha256`, validate the generator/schema/direct-input
hashes, then resolve the case references by index and check their IDs and row
hashes. It selects `eligible-for-rust-comparison` rows and applies its real
production comparator to the unchanged prepared input and complete tuple.
It must not treat that disposition or `qualified-typescript-oracle` as a Rust
pass. D and E acceptance counts overlap by three; their sum is not the union.

Focused API, sink, and printer packets retain separate denominators and
observers. Registration, current runtime-input closure, request canaries,
hosted acceptance, and adoption remain root-owned follow-up work. No runtime,
policy, registration, or existing expectation is edited by this draft.

## Local checks

The generator's syntax check, write, and check pass. The repository's existing
`validateJsonSchemaSubset` accepts the schema and artifact. Four focused
negative schema checks reject a changed eligible denominator, a missing
eligible observation, a promoted transpile row, and an added Rust-success
field. No Cargo or full CI was run for this packet.
