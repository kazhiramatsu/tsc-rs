# SPEC-0b.4 — PGO release pipeline

Track: [ts71-arch-arena](../README.md), phase 0b ([SPEC-0b](SPEC-0b.md) §0b.4). Lane rules as [SPEC-arena](SPEC-arena.md); this
lane runs **no cargo at all** (the builds happen on the hosted runner); time box one working day. Patches under
`target/lane-pgo/patches/`.

## Inputs

`.github/workflows/pgo.yml` (DRAFT on-demand PGO + instruction counts: corpus freezing, instrumented build, training in serial
batches, profdata merge, optimized build, Cachegrind measurements), `scripts/pgo_build.py`, `.github/workflows/ci.yml`, the slice 8
record in `docs/design/greenfield/slices/ts71-perf-tsrs/README.md` (「slice 8：PGO」: −20.8% on VS Code on the 11-02 base; the
CLI's `_exit` does not flush the profile — see the section's notes), `docs/setup.md`.

## Deliverables

1. `.github/workflows/release.yml`: on a tag `v*` or `workflow_dispatch`, build the CLI three ways on the hosted runner —
   `normal` (today's release profile), `pgo` (instrumented → train → `-Cprofile-use`), and publish both as release assets with
   SHA-256 sums and a `BUILD.json` (toolchain, commit, LTO/codegen settings, training corpus identity, profile merge hash).
   Training corpus: hono (pinned as in pgo.yml), two fixed conformance batches, zod (pinned); no network at train time beyond the
   checkout. Reuse pgo.yml's steps; do not duplicate logic — factor shared steps into a composite action or a script.
2. The workflow also runs `serial.py`-equivalent instruction counts on the runner (GNU time / perf stat as pgo.yml does) for
   both binaries on hono and zod and writes them to the job summary (same-runner numbers; not comparable to this machine's).
3. A one-page `docs/design/greenfield/slices/ts71-arch-arena/pgo.md`: how the release is built, how to compare normal-vs-normal
   and PGO-vs-PGO against tsrs (`tsrs-sym-nopgo` and the npm binary), and the rule that PGO is reported separately and never
   counted as an architectural gain.
4. Validate the YAML (`actionlint` if available offline, else careful review) and keep `pgo.yml` as the experiment workflow.

## Acceptance (integrator)

One `workflow_dispatch` run green on the hosted runner; assets present; the VS Code normal vs PGO instruction counts for #913+
measured locally in a quiet window from the downloaded binaries and recorded in the design §8 (PGO sensitivity row).
