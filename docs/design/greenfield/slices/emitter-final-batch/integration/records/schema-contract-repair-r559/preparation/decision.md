# r543 failure and bounded schema repair

The fourth actual sanctioned walk, `20260920-232502-12419`, exited 1 after
10741.332 seconds. Round 2 passed all 75 producers without reminting, all
16 owner-control scripts and 61 registered tests passed, but the final
`node .github/ci/qualification.mjs check` rejected a cyclic local schema ref.
No certificate or overall success is attributed to r543.

An independent per-contract audit found 25 of 27 accepted and two failures.
The H2.7c diagnostic schema recursively referenced itself, outside the existing
validator subset. The H2.5h-a dispositions schema still required the historical
45-row roster although the generator and architecture already require 58.
All original 45 rows remain identical; 13 previously approved rows were appended.
Independent FCI validation before repair passed 45 envelopes / 42 ready.

Actual Opus rounds 221 and 222 reviewed the alternatives and applied diff.
We retain the generic cyclic-ref refusal and do not change the TS serializer,
Rust compiler, output comparison, corpus, owner dispositions, or admissions.
H2.7c now validates one terminal related-information layer using its own scalar
forms, following the existing H2.8a shape. This covers all current 373 root and
378 related diagnostics. It is a bounded qualification contract, not a claim
that future TypeScript diagnostics can never be deeper: deeper children,
including a child with an empty related-information array, fail closed.
H2.5h-a synchronizes exactly seven literals to 58 rows (indices 0..57) and the
approved distribution 18/19/10/11, keeping all four zero counts and closed enums.

Two official writers refresh the schema envelope digests. Five new registered
controls validate both real artifacts and current schema digests; preserve
nullable and empty-string scalar fields; reject missing/unknown/bad child data
and unqualified recursion; reject 45,57,59 rows and index58; enforce exact
summary counts and the runtime non-admission boundary. Existing generic cycle
negative tests remain. The full CLI, not a duplicate 130MB unit-test pass,
validates all27 contracts and FCI before another sanctioned walk.

The previous walk's H2.5g receipt was minted by a full new-key comparison of
9027 cases, each observed twice; round2 reused it with observed=0. The retained
progress snapshot contains verbatim receipt/outcome/events/summary and the
round2 log. The driver overwrites per-rung logs per round, so no first-round
verbose shard log is reconstructed or claimed. The failure remains a failure.

There is no implemented --tail/resume mode in chain-walk.sh. After focused
validation, the failure evidence and bounded repair are committed before a new
ordinary sanctioned walk. Only its actual green invocation can mint the next
certificate. Final exact-head unsplit CI, hosted checks and merge remain required.

Validation completed at source HEAD f46dc801560cd8ccaf8ccce8dcec6135615f0244:
r554 official dispositions writer exit0 (0.832s); r555 official H27c writer
exit0 (212.466s),42 corpus /244 focused, each twice; registered r556 tests
66/66 PASS (63.405s); exact full CLI r557 exit0 (18.780s), all27 contracts,
45 FCI envelopes and42 ready. Final generated comparison r558 covers14
artifacts and59 changed leaves:52 SHA256 leaves and the7 already reviewed
current-source line references, with28 payload surfaces unchanged. H27c
qualification changed only its contract and overall fingerprint hashes.
The H25g new-key receipt remains byte-identical, SHA256
c454d7a51dfeaa31fc8ea68bca810fd84e06308bd858f206dc85a0760065dfbb.
