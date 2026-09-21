# Emitter integration delivery — 2026-09-21

[PR #561](https://github.com/kazhiramatsu/tsc-rs/pull/561) is merged. The final
integration candidate is `f712e97728b81a1d223af70c6cfd456277c6f626`; the actual merge is
`8e63b77a676311fe660a32a3ce18227e8c0f2ad5` at 2026-09-21T11:20:20Z. Git ancestry and exact tree equality were
verified after merge. The merge preserves the candidate's implementation and tests.

All 26 checks selected for that candidate succeeded:
[acceptance run 35590113913](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35590113913)
and [witness run 35590113993](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35590113993),
including both planners, early/wide/late, all selected witness groups, `gates`
and `witness-gates`. The final raw run/job records, full log ZIPs and post-merge
PR response are preserved locally under `/tmp/emitter-final-policy-correction-r621/hosted/`.
The links above identify the hosted source records independently of that local archive.

| Observed population | Final candidate result |
| --- | --- |
| EF7 universe | 217 exact, 0 known, 0 failed; each exact comparison repeated twice |
| EF7 PLAN-BASE | Four disjoint shards: 450 + 450 + 449 + 449 = 1,798 exact; 0 known, 0 failed; twice each |
| H2.5g acceptance | 9,027 candidates; 8,511 exact, H2.8a 6 deferred, H2.9 510 deferred; 2 repetitions |
| Additional emitter rows | 21 exact, 0 known, 0 failed |
| External helper imports | 551 exact, 0 known, 0 failed |
| Empty-block comments | 72 exact, 0 failed |

The original submitted 68 KNOWN rows were retired after complete comparisons;
[the residual audit](residual-audit.md) and [integration history](README.md) retain
the causes, actual repairs, adjacent controls and prior failures. Deferred
populations and memberships are not added to the exact case count.

The compiler ES5 expectation and bounded H0 current-declaration validator repairs
retain their [focused results](records/native-validator-repair-r612/resolution.md).
[Inventory v40](../../witness-coverage/inventory.v40.json) records registration,
not test execution: 77 standalone targets, 53 unfiltered / 19 filtered / 5 indirect,
16 lib/bin harnesses with one direct command, and 14 targets with shared acceptance sources.

## Scope of completion

This closes the emitter integration and the selected hosted verification under
the current focused-local/hosted policy. It does not issue a new historical-profile
qualification. All 18 delegated [architecture entries](architecture-validation.md)
remain `active-unqualified`: the repaired CLI unit-test bytes differ from frozen
H2.5a–H2.5g profile inputs. Their promotion criteria are unchanged.

The earlier full local CI r603 failed; r617 was intentionally stopped when the
retired chain requirement was corrected. Neither is a passing result or a pending
routine merge prerequisite. [The correction record](records/verification-policy-r621/resolution.md)
preserves the interruption and restoration facts. No further full local CI or
hash-chain regeneration was performed to complete this merge.

Two known boundaries remain in the separate transpile population: composite
recovery around an invalid Unicode identifier, and ScriptTarget.JSON. The original
census's 110 load failures remain unqualified; 108 separately compared projects
do not replace them. Build/watch, general public API re-emit and TypeScript 7
compatibility are separate product tracks. CST/AST separation remains a future
refactoring topic. The user's next requested work is performance comparison
preparation, followed by measurement and evidence-led tuning.
