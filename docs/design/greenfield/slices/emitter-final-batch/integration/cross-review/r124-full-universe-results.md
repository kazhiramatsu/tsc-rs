# Full EF7 and PLAN-BASE comparison at the r120 source

Source `b451489e4a18abbff42651d8eb814537f4c5f800` and the immutable universe binary remained unchanged throughout both full runs. All **217 EF7 rows and 1,798 PLAN-BASE rows compare exact twice**, with no new differences or failure captures. The original reference membership is unchanged; `r124-full-universe-results.json` records every executed ID, fixture content hash, binary hash and log hash.

Both Rust tests return **101**, solely because their stale-KNOWN assertions correctly detect the old one-row and 35-row refusal registrations. This is not a green test-suite claim. The corpus impact proof remains pending, so these registrations have not yet been retired. The three independent comparator/shard guards pass, with no ignored tests.

The raw receipts and compressed logs are under `../records/local/`. EF7 took 425.67 seconds and PLAN-BASE took 2972.034 seconds. These are demoted functional measurements concurrent with the census, not performance qualification. The archived driver preserves both raw failing exits; the archiver verifies exact fixture ID coverage and the sole stale-guard failure before recording this result.
