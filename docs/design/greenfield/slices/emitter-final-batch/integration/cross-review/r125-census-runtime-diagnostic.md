# Census runtime diagnostic; no source or qualification change

The original frozen Census process56011 remains the authority. A one-second read-only macOS sample found all144 sampled stacks in SHA-256 software compression, reached from `recovery_parse_snapshot::input`. It does not establish the fraction of the whole run spent hashing. Actual Claude Opus121 reviewed the profile-only alternative and recommended keeping the original measurement, requiring broader evidence before attempting a second census.

An isolated scratch crate used pinned `sha2 0.10.9`, Rust1.93.0, no added features, two private target directories, and no changes to the repository. Its two builds differed only in sha2's dev opt-level0 versus3; the compiler log confirms debug assertions stay enabled. Four hashes of the218,972-byte vendored `lib.es5.d.ts` took0.0613/0.0633 seconds at0 and0.00538/0.00379 seconds at3. All four batches produced the same digest as Python hashlib. The scratch dependency graphs match one another, but their libc is0.2.189 versus the census's0.2.186; this is not exact census-graph qualification.

The small benchmark excludes JSON serialization, parser work, source loading and the census's remaining-input distribution. It runs demoted alongside other functional jobs. Its numbers cannot predict a completion time or qualify performance. The first cold scratch build also took131 seconds, so a cheap full census rebuild was not assumed.

Decision: preserve the active census and its binary/source/output identities; do not start a replacement census or change Cargo.toml, dependency features, hash algorithms, captured inputs or schemas. The possible dependency-profile improvement is a later tooling investigation. Any later alternative must have separate provenance and complete output equivalence. The current parser replay tools already have their own reviewed optimized build configuration.

Evidence: `round121-opus-response.md`, `records/census-r78-sha256-sample-r125.txt.gz`, and `records/sha256-profile-probe-r125.json.gz` (complete scratch source, lockfile, verbose build logs and measurements). These diagnostic artifacts are not compiler compatibility evidence.
