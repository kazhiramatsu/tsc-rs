# PGO release pipeline (0b.4)

[release.yml](../../../../../.github/workflows/release.yml) runs on `v*` tags and
`workflow_dispatch`. A tag publishes a release; dispatch creates prerelease
`pgo-<run-id>` at the selected commit. Linux (`ubuntu-24.04`) and macOS
(`macos-15`, native runner architecture recorded in `BUILD.json`) each build
three stages serially: **normal**, **instrumented**, **PGO**. The distributed
variants are normal and PGO. Both use the checked-in release profile (currently
fat LTO, one codegen unit); only PGO adds `-Cprofile-use`. There is no native CPU
flag. Object trees are discarded between stages to fit hosted disks.

The workflow and the on-demand PR experiment [pgo.yml](../../../../../.github/workflows/pgo.yml)
share [.github/ci/pgo.py](../../../../../.github/ci/pgo.py),
[scripts/pgo_build.py](../../../../../scripts/pgo_build.py), and
[the measurement script](../../../../../.github/ci/pgo_measure.py).
The experiment keeps its full conformance/suites + hono/TypeScript recipe and
its existing opt-in gate. Release training is a separate fixed recipe: hono
`8dcd52b92beada28962e818bdca8a137a6beb656`, two checked-in 128-case
[conformance batches](../../../../../.github/pgo/) from vendor profile
`7.1.0-dev-aa814927`, and zod `2bf7b06` (the benchmark pin in
[performance.md](../../../../performance.md)). Zod's abbreviated pin is resolved
unambiguously from a full checkout; the full commit is recorded. Release zod
checks production sources, excluding tests/benchmarks, with a standalone frozen
noEmit config; this is not the entire developer workspace.

The corpus job freezes sources, configs and the integrity-checked Node declaration
dependencies from the pinned TypeScript lockfile. Its manifest hashes every
input and each batch list. All downloads precede training; training only runs
checked-out executables against frozen local files, with one worker/checker and
no install/fetch commands. The manifest is verified before and after training.
Each pass must produce nonempty `train-*.profraw`; build-script profiles are
removed before training and excluded from merging. `LLVM_PROFILE_FILE` makes the
CLI take the C exit chain: ordinary `_exit` would lose the profile. Rustc's own
`llvm-profdata` merges it. Missing batches, crashes and missing inputs fail the run.

Each `tsc-rs-<rust-host>-{normal,pgo}.tar.gz` contains an executable, `BUILD.json`
and `SHA256SUMS`. Each BUILD manifest is also a separate asset; the release has
aggregate `SHA256SUMS`. Verify those hashes before using a download.
BUILD records commit, rustc/toolchain/LLVM, effective release settings, lock/vendor
hashes, corpus identity, exact batch identities, binary hash and the merged
profile hash (null for normal). Profiles/logs and raw measurements are run artifacts.
On Linux the same runner measures both binaries on hono and zod: one warmup,
three interleaved native rounds under GNU time, then two identical Cachegrind Ir
counts per binary. Diagnostics and exit status must match. The job summary and
`linux-MEASUREMENTS.md` contain the table. Cachegrind follows the experiment;
GNU time supplies wall/RSS, not instructions. Hosted Ir is separate from local
hardware instruction counts and cannot be compared numerically to this Mac.

For tsrs comparisons, pin its commit/npm version and record binary hashes,
platform, corpus/config, thread settings and output equality. Compare release
**normal vs `tsrs-sym-nopgo`**, and separately release **PGO vs the npm PGO binary**.
Use matching native architectures; a translated binary is not a valid partner.
For comparisons across tsc-rs revisions keep corpus, toolchain and recipe fixed,
training a fresh profile for each revision. Never mix normal and PGO in a ratio.
**PGO is reported separately and never counted as an architectural gain.**

Integrator acceptance remains: dispatch once, verify green jobs and published
assets, then download the native binaries and measure VS Code at #913+ in an
owned quiet window (`serial.py`, `TSRS_WORKERS=1 TSRS_CHECKERS=1`, warmup 2,
9 alternating rounds; compare diagnostics first). Record commit/run URL, target,
binary/corpus/profile hashes, both instruction medians, delta and peak footprint
in [design §8](README.md#8-見込みと停止規則第-2-版レビューの算術に差し替えb1-で確定)'s
PGO sensitivity row. The historical −20.8% is the 11-02 base, not a #913 result.
Local lane work runs no cargo or network and cannot establish hosted acceptance.
