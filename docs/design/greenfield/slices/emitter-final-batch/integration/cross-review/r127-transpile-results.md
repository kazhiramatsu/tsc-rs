# Separate transpile and noCheck boundary check

At Recovery `d0edc1a74a6249318b19aa449ab2735983a8df46` (Rust bytes identical to b451), all nine transpile route tests pass, with zero ignored/filtered tests. All 291 fixture IDs execute twice: JS has 146 exact and seven strictly unchanged known refusals; declaration has 87 exact; ordinary noCheck Program has 50 exact and one strictly unchanged known refusal. There are no new divergences or stale KNOWN entries. The comparison/manifest rejection tests, source-checking counters, repeated-call separation and API-fact checks also pass.

The eight refusals comprise seven parse-recovery boundaries and one target100/JSON source-kind boundary. They are different observations from EF7's now-exact 36 rows and are not emit successes. The actual payloads and precise causes remain unchanged in the live fixture. The seven parse cases are under a separate bounded review in Opus122; no new admission or retirement is authorized by this result alone.

`r127-transpile-results.json` pins the reference fixtures, immutable binary, all receipt hashes and full per-case reports. Each report stores one actual payload after the test verifies an independent second run matches. The existing source-map parse-refusal control separately passes (one test,482 filtered), using the immutable r120 contracts binary at the same Rust bytes. Other decorator/bundle/post-T1 native KNOWN tables are already empty by source inspection; that inspection is not a new runtime qualification.

The census was paused only for the 4.664-second Cargo build and resumed with its executable hash/inode unchanged. Test timing is demoted functional evidence, not performance qualification. Root remains frozen for the pending census/replay pipeline; this report is prepared separately.
