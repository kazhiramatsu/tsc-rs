# OPS-COVER-4A / 4B: syntax, symbol, option and host/Program contracts

2026-09-17. Integrator: Codex. Baseline: main `eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7`
(PR #550); the first branch commit only retains T1's completed hosted evidence.
This batch connects two bounded parts of OPS-COVER-4 to ordinary PR CI.
It does not change compiler production code, expected observations, admission or STAGE.

## Problem and boundary

Sixteen existing standalone Cargo targets had no direct PR execution entry.
Changing one of their files selected every existing acceptance/witness group,
but still did not execute that changed target. This batch adds target-level
ownership and a separate `foundations` witness job, sharing the build within
each of the five crates. Shared inputs continue to select their existing
consumers, including the full fallback where ownership was not bounded.

| Slice | Existing targets | Observations |
| --- | --- | --- |
| OPS-COVER-4A / syntax | `entity_names`, `new_meta_property_name`, `owned_literal_values`, `recovery_provenance`, `scanner_escape_diagnostics`, `template_escape_flags`, `template_flags` | Identifier/entity predicates, literal UTF-16 values, parser recovery and scanner diagnostics, template escape flags |
| OPS-COVER-4A / binder/types | `owned_symbol_names`, `compiler_option_number_contract` | Symbol-key identity and diagnostics, arbitrary export/module names, JavaScript number identity and option comparisons |
| OPS-COVER-4B / host | `compiler_host_contract`, `filesystem_host_contract` | Missing vs empty, path/case/realpath identity, Unicode/UTF-16 boundaries, listing order, I/O errors, symlinks and read-only capabilities |
| OPS-COVER-4B / Program | `h2_7d_bundle_source_facts`, `host_platform_smoke_contract`, `utf16_config_paths`, `utf16_module_paths`, `utf16_raw_source_boundary` | Module facts/input order, equivalent native/memory hosts, config/package/path caches, explicit raw-source decode boundary |

These are API/value/fault contracts. Their test-function counts are not complete
compiler command counts. The bundle fixture's 55 rows overlap existing emitter
evidence; the new target compares Program facts, not a new set of 55 emit passes.
The six raw UTF-16 source rows include four explicit Rust decode limitations;
passing the guard does not mean those four inputs gained TypeScript parity.
Windows-only test declarations remain unexecuted by the Linux job and macOS local
run. Full Windows/platform qualification remains REL1.0b.

The test bodies and frozen fixtures are unchanged. In particular the noEmit
refusal in `load_emitting_program` is checked at that narrow API boundary;
registration does not replace it with the compiler's broader command behavior.

## Authority and reproducibility

The nine existing observers independently check their frozen TypeScript 6.0.3
fixtures before Rust runs. The compiler pin is
`569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39`
for `vendor/typescript-6.0.3/lib/typescript.js`. Each observer keeps its existing
repetition/hash checks. `observe-bundle-plan.mjs` additionally checks the
H2.7d/e candidate inputs and requires the pinned Node version.

Syntax fixture memberships are entity129, meta-property10, literal27,
recovery20, scanner23 and template43. Binder retains seven generated-table
inputs; raw-source retains six and bundle-plan55. These are separate
observations with different fields and overlaps, so they are not summed as a
product compatibility numerator. The other tests have Rust-native invariants
and fault controls; no upstream-oracle claim is added to them.

`baseline/source.json` hashes the measured production, test and fixture inputs;
`baseline/receipt.json` retains exact argv/env, toolchain, exits, timing,
compressed logs and executable hashes. The same recording tool captures the
combined runner in `final-verified/`. Both successful measurements check that their inputs stayed
unchanged while executing. No fresh oracle output is accepted as a replacement
for a frozen expectation.

## Entry and ownership contract

`scripts/foundation_witnesses.py` lists all sixteen target owners and their
observer/fixture inputs. `scripts/witness.py` exposes each as a normal suite.
`--list` lists test functions for the local platform, and `--dry-run` lists
Cargo and oracle commands without executing either. Each small target runs
unfiltered with `--all`; `--case` is refused. The local runner and hosted job
use the same implementation.

The runner enumerates the registered sources' plain `#[test]` functions and
explicit Linux/Unix/Windows cfg conditions, then checks each Cargo target's
name, executed test names and complete libtest summary. A missing target,
zero tests, ignored/filtered result, duplicate or substituted test name,
unsupported cfg/platform, observer failure or native failure fails closed.
This is a bounded recognizer for these source files, not a general Rust parser;
adding macros or unfamiliar test attributes requires an explicit runner review.

Target/fixture/observer changes select their owners. The shared literal input,
bundle-plan fixture/observer, scalar-path helper, runtime code, manifests,
vendor and unknown inputs preserve full coverage. Registering the new Program
consumer does not narrow the old bundle input selection. Foundation runtime
dispatch bytes are added to the qualification policy's execution hash set.
The inventory hashes the same new module and reports all sixteen entries.

The new job uses pinned Node, two Cargo workers and the existing 60-minute
hard limit. It leaves the approximately 40-minute controls job unchanged.
Tests from one crate share one Cargo invocation; crates execute serially.
No new parallel local builds or one-PR-per-target workflow is introduced.

## Local validation

The unchanged baseline passes all nine observer checks and all sixteen native
targets: syntax12, binder2, types3, host22 and Program8, **47 tests on macOS**.
Linux adds the invalid-directory-entry contract, so its expected total is48.
The single newly configured group is validated through `.github/ci/replay.py
witnesses`, using the same sixteen suites as hosted.

```sh
python3 scripts/witness.py syntax-entity-names --all --dry-run
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py host-memory --all
python3 docs/design/greenfield/slices/witness-coverage/foundations/run-local.py final --label NEW_RECORD_DIRECTORY
python3 -m unittest discover -s .github/ci -p test_replay.py
node .github/ci/qualification.mjs check-policy
python3 docs/design/greenfield/slices/witness-coverage/inventory.py --check
```

The first composed run in `final/` stopped after syntax tests passed: the output
validator expected a short Cargo target name instead of `tests/<target>.rs`.
That failed receipt is retained. The corrected runner in `final-verified/`
passes all sixteen targets /47 tests in77.135s (observers48.176s, Cargo28.756s).
The planner/runner regression suite passes71 tests, including the real Cargo
announcement format; policy validation and its two focused tests also pass.

The output directory must be new. The baseline command in `run-local.py` is
retained for replay and does not rewrite prior evidence. Planner/runner tests
exercise isolated target selection, shared dependency preservation, platform
membership, dry-run inactivity, output tampering and both failure channels.

## Integration and remaining scope

The departing batch also retains T1's completed hosted receipts and the
five-child Claude POST-T1 handoff. Claude's emitter changes are outside this
branch. Production and the compiler accepted state remain byte-identical to
the baseline.

Final head `447920e6c73f9a81d0aae8c2729a395426c9ff3b` passed all ten
replay jobs, both planners and both aggregate gates in PR #551:14 successful
checks. The policy/runner changes selected every acceptance and witness group.
PR #551 merged at `45d6f68485556860f08c3949833276660d7a92eb`.
[The merge receipt](hosted/merge.v1.json) verifies two parents and a Git tree
identical to the tested candidate.

OPS-COVER-4 is not closed wholesale: checker, harness, fuzz and the broad
Program contract target remain, as do unregistered lib/bin harnesses and
OPS-COVER-3's original-wrapper/filter accounting. The next inventory names
those remaining entries explicitly.

The [post-emitter coverage backlog](remaining-seven.md) records the seven remaining
standalone targets and separates static declaration counts from executed tests.


## Hosted validation: PR #551

[The receipt](hosted/receipt.v1.json) retains both workflow runs, all14 job logs,
SHA-256 hashes, candidate/tree identity and measured time. Every replay and
aggregate check succeeded at the exact candidate head.

| Replay job | Whole-job time | Result |
| --- | ---: | --- |
| [acceptance (late)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344808/job/105065004138) | 17m02s | success |
| [acceptance (early)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344808/job/105065004169) | 12m00s | success |
| [acceptance (wide)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344808/job/105065004218) | 27m50s | success |
| [witnesses (retained)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022827) | 8m49s | success |
| [witnesses (declaration-maps)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022837) | 6m25s | success |
| [witnesses (primary)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022858) | 10m33s | success |
| [witnesses (decorator-binding-pipeline)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022869) | 17m08s | success |
| [witnesses (foundations)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022875) | 1m28s | success |
| [witnesses (printer)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022891) | 3m01s | success |
| [witnesses (controls)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35178344800/job/105065022923) | 29m08s | success |

Foundations runs16 targets /48 Linux tests, including the Linux-only filesystem
contract. Its observers take25.348s and Cargo build/replay43.765s; the whole
job takes1m28s. Local macOS remains47 tests. The existing controls job retains
21 compiler-direct selections /70 tests, and the binding pipeline retains
763 exact /four known across767 complete commands. Known rows remain separate
from exact compatibility results.

The ten replay jobs total **133m24s**, excluding
plans, aggregate gates and main push. The longest job is
**29m08s**, below the45-minute split-review
threshold and60-minute hard limit. These are whole-job observations on different
hosted workers; they are not an isolated performance-improvement claim.
