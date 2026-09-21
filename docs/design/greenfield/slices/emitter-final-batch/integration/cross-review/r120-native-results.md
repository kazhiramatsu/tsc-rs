# r120 immutable native results

Source: Recovery HEAD `b451489e4a18abbff42651d8eb814537f4c5f800`, tracked clean throughout. Compiler contracts binary SHA-256 `9d220a04991f6bb3167f48aed3f458ccde564781cb89731536674e6d9e4fd641`. The full pipeline exited0.

| Scope | Result |
| --- | --- |
| New declaration/comment/token controls | 110 complete commands exact twice; 6 Rust tests passed |
| Emitter crate | 1,015 tests passed across all23 test binaries; zero failed, ignored or filtered |
| Recovery and metadata boundaries | 1,930 complete commands exact twice; 10 Rust tests passed |
| Original System/recovery groups | 4,088 complete commands exact twice; 11 Rust tests passed; 472 unrelated contract tests filtered |

The new110 controls include all three r116 failures, now exact. The entire3,860-record boundary capture archive is byte-identical to r106. These are separate comparison memberships, not an additive claim about a unique compiler corpus.

The three complete observation archives contain220,3,860 and8,176 records respectively, exactly two per selected case. Every actual tuple equals its frozen TypeScript tuple and every error is null. The original4,088 archive is2,615,237 bytes, SHA-256 `7f563be630852a7e8dedb0e6c4439dfc1ad577e76a5c74aaaeed50a22c3848aa`. `*-capture-summary.json`, zero-entry `*-differences.json` and empty `*-text-diffs.txt` preserve the comparison index. `r120-emitter-all-tests-summary.json` verifies every copied emitter executable and receipt; `r120-regression-retention.json` links the prior boundary and failed-control evidence.

Execution receipts and lossless logs are under `records/local/*-r120.*`. The native compiler build took111.009s, emitter build515.437s, new controls44.874s, boundaries921.546s and originals4,536.738s. These are demoted functional-run observations that overlap a separately frozen census; **none qualify performance**. All source/HEAD bytes and immutable executable hashes stayed fixed. Historical scripts are archived under `records/*r120*.py`; their fixed process IDs, exclusive outputs and source identities make them execution records, not a general rerun interface.

The original4088 pipeline is complete. Recovery stays frozen because r124 PLAN-BASE still reads its source/fixtures. Corpus selection/replay, KNOWN retirement, final chain walk, full local CI, hosted validation and merge remain pending. This result does not qualify those unfinished scopes.
