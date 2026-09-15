# Decorator super integration onto main

Integration base: `fbce8727f0f4a073f9149c190f2913db69952df2`.
Claude candidate snapshot: `ac6553f4f` in `draft/h2-8a-decorator-super`.
Landed in [PR #523](https://github.com/kazhiramatsu/tsc-rs/pull/523) as
`d671d8417d725ce54a4cfc42b6e7127c03347646` after acceptance and all witness jobs
passed. Subsequent focused selection and CI partitioning landed separately in
[PR #524](https://github.com/kazhiramatsu/tsc-rs/pull/524).

The archived after-18 receipt has SHA-256
`1cd9b70839fe032933cc5ddc2cd3f79c2f4685c9e1568879da56e650c245bfbc`.
Its production, fixture, script, test and eight patch hashes matched the supplied
candidate before integration. The receipt and archived patches remain unchanged;
they describe the candidate, not this integration commit.

Main already implements most of the candidate with newer generated-binding
identities and lossless `JsString` APIs. The integration retains those owners and
ports the remaining behavior: synthetic member order, private-in and logical
assignment source maps, escaped name provenance, parameter ranges, anonymous
computed property names, discarded-value memo boundaries and helper request order.
The compiler observer uses the existing scalar JSON adapter for these frozen
scalar fixtures; fixture bytes and their TypeScript expectations are unchanged.

## Focused local evidence

On the unmodified main production sources, followup3 was 26 exact / 22 failed
out of 48. After integration it is 48 exact / 0 failed, with two fresh programs
per case. The ES2015/set sample across the other collections initially exposed
54 failures. Rerunning only those failures repaired 40, then the remaining 14
passed after the helper, computed-name and alias corrections.

Synthetic direct controls: 28 exact, 4 recorded upstream divergences, 0 failed.
The four divergences are preserved as explicit controls, not reported as exact.
Focused emitter checks pass: 7 library tests and 68 decorator contracts. The
contracts caught a generated `default_N` binding incorrectly replacing the
runtime name `"default"`; the source-provenance adaptation now distinguishes
that synthetic name, and the unchanged contract passes in both module modes.
Formatting passes. Emitter Clippy has the same 16 findings (same diagnostic
kinds and files) as the integration base; dependency-inclusive Clippy also
stops on existing program findings. This is not an integrated lint-green claim.

## Hosted coverage

The `witnesses` workflow replays primary (670 observations plus two upstream
exceptions), controls (42 + 156 + 162 + 48, and direct controls), and retained
accessors (530) in independent jobs. Each compiler observation still executes
twice. The established `ci / gates` acceptance remains separate, so adding
witness coverage does not extend its existing near-hour serial chain.

The [hosted witnesses](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34962781338)
passed with primary 670 exact × 2 plus two upstream exceptions, controls 408 exact
× 2 plus direct 28 exact / 4 known, and retained 530 exact × 2.
[Acceptance](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34962781518)
also passed in 46m23s. Local work uses case selectors and focused emitter tests;
the current commands and follow-on measurements are in the
[witness guide](../../../witness-testing.md).
