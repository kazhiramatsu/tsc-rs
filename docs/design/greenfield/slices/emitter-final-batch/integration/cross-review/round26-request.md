# r26: newly exposed acceptance failures

Read-only actual Claude review, no edits/Cargo/subagents. Fable unless rate limit.
Ordinary candidate 3c1dfa51c is frozen in the integration worktree; parser data-only
work in audit is separate and not in this CI. We made a CI-repair worktree at
/Users/hiramatsu/dev/tsc-rs-emitter-final-ci-repair from that candidate.

CI now proves conformance T0/T1/T2/T3 all 49,024/49,024, FP/FN0; H2.5h 888 exact,
44 deferred, no known; H2.6a 175 exact/2 deferred and H2.6b6 exact. New stops:
1. h2-1a: privateNameInInExpression.ts#target%3Desnext now emits, but historical
source-deferred expectation still requires refusal. Frozen original case has
fingerprint ed4b13ea9f6d38a3c9e4bd51cb798eeb002b222e29f52e7eff2b79421930d0fe,
required_slices=[H2.8a], two TS runs. Existing CURRENT_EXACT_SOURCE_PROMOTIONS and
execute_observed(CurrentExactSourcePromotion) seem the right way to migrate it,
after complete output/diagnostic/result/exit comparison twice, preserving the
historical artifact. Please review the cause of old refusal/current admission and
whether other still-unpromoted source-deferred H2.1a rows need the same check.
2. h2-6c: its vector manifest loader explicitly rejects empty cases. We retired
all cases but kept an empty file; remove the live file, preserving the archived
retired rows, and update any current guard that unnecessarily requires its
existence. Check consumers of ratchets/h2-6c-known-divergences.v1.json.

Root is independently investigating controls' bundle-declarations oracle: retained
payload pin changed b51176... -> bd63c9... after dependency provenance refresh.
Do not investigate that third item unless it affects (1)/(2).

Please report concrete findings, about 40 lines. We will not weaken old refusal
or exact comparators, or change historical TS observations to make the gate pass.
