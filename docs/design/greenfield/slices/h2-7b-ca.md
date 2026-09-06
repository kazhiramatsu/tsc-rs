# H2.7b completion and transition

Status: local close update, 2026-09-07. Runtime closure was merged in PR #509
at `23d5275cddf5c8f4e871672f86b3e766f17bcf5e`; this update adopts its evidence.
The user authorized continuing the lightweight workflow recorded in
[h2-7b-w5.md](h2-7b-w5.md). No additional hosted call is introduced.

The frozen TypeScript 6.0.3 qualification band contains 1,593 candidates:
1,557 admitted cases (893 compiler, 468 conformance, 196 project), all exact
on both Rust repetitions; 36 deferred (26 first owned by H2.7c, 10 by H2.9).
The declaration band's divergence manifest is absent, the repository's
representation of zero rows. No empty manifest is created.

PR #509's final hosted acceptance passed (39m54s for acceptance, 40m06s for
the job); the diagnostic corpus matched 49,024/49,024, FP=0, FN=0. The W5
close record owns the detailed validation lineage. No runtime code changes
are part of this state transition, and those suites are not rerun here.

The `h2-5g-profile` remains the live profile despite its historical name.
Its `admitted_profile` and older adoption counts retain their original bands.
The close adds `h2_7b_*` adoption counts 1,593 / 1,557 / 1,557 / 0 / 36,
adds H2.7b to active runtime slices, advances the recorded completed count
25 → 26 and inactive count 11 → 10, and sets both next-slice fields to H2.7c
(`declaration-diagnostics-and-options`). Following the packet's additive
accounting, runtime admissions are 9,196 + 1,557 = 10,753 and executed
candidates are 9,715 + 1,557 = 11,272. These are the existing profile's
adoption counters, not a new unique-corpus census. STAGE does not change.

The H2.5h-a live parent assertions, schema and parent mirrors move together.
The H2.7a close's `transition_landing` moves; its entire `runtime_contract`
remains byte-equivalent historical content. The four H2.7a foundation
artifacts and the H2.7b qualification observations remain unchanged.
Nine W5 runtime/test input paths omitted by the pilot's certificate step
are registered in the live profile (245 → 254 input identities).

Validation for this update: profile generation/check, foundation generation
(the six controls are reobserved twice), H2.7a close generation/check, and
JSON schema validation for all three updated artifacts. The historical
certificate chain and full developer CI remain omitted. Updating these
three records does not claim that every downstream historical pin is fresh.

Handoff: H2.7c owns stripInternal, isolatedDeclarations, declarationDir and
declaration diagnostic/forced-output semantics. H2.7d owns bundles, H2.7e
declaration maps, H2.8a the general output matrix, H2.8c transpile/noCheck,
and H2.8d ordinary targeted/API emit axes. The existing 6.0.3 band is not
silently widened or converted into a TypeScript 7 completion requirement;
7-series case/option dispositions are recorded in the next packet.
