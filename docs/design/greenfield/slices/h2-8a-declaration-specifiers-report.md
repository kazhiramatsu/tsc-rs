# H2.8a-DECL-SPEC1 implementation report

The inferred declaration module specifier now receives the effective Program's
ordered paths, paths origin, rootDirs, and config identity. For the unchanged
`declarationEmitPathMappingMonorepo2.ts#default` command, the declaration now uses
TypeScript's `import("@ts-bug/core/SvgIcon").SomeInterface` and the complete command
matches twice. This closes the witnessed option-transport gap only.

## Implementation and frozen authority

Base: `0aaf808bef9b0f5f64737821c63c273056dee2c4`.
Production: `da69d7e4a0cdba5789bf8636195c984c2572d2ca`.
The change touches exactly three production files: checker provider API,
checker NodeBuilder specifier input projection, and compiler provider forwarding.
The existing inverse-path, rootDirs, package-scope, ending, cache and bundle
selection workers remain unchanged. The bundle baseUrl override still follows
the effective option projection.

The [design](h2-8a-declaration-specifiers.md) and readiness manifest record the
upstream bodies and ownership before implementation. Initial witness commit
`f7fd77d4c` and composition/design commit `7ca3f5252` precede the production change.
The original corpus input, both new input/oracle groups, both observers and the
shared whole-command comparators are unchanged by the repair. Emitter transforms,
printer, metadata, and shared compiler `contracts.rs` registration are untouched.
Cargo discovers the new standalone `h2_8a_declaration_specifiers` integration target.

## Measured before and after

| Group | Cases | Before exact | After exact | Repetitions |
| --- | ---: | ---: | ---: | ---: |
| declaration-specifiers | 24 | 7 | 24 | 2 |
| declaration-specifiers-composition | 6 | 4 | 6 | 2 |
| original monorepo command | 1 | 0 | 1 | 2 |

Both before commands really exited 101. Their failed repetitions were retained;
they are not inferred counts. The combined after command ran all three tests,
passed 3/3 and really exited 0 on the production commit. Source input hashes
remained unchanged across the run. Its executable SHA-256 is
`8979ad768796faab9a979d35b0d42a0d6e57bc65affdbde9a1b2fc6a0d77e778`.

All 60 focused native captures are retained. Repeated captures are identical
after removing only the repetition label. All 11 previously exact cases retain
identical captures across the repair. The 19 repaired focused cases change only
their writes and, where applicable, source-map output in the emit result. Their
Program facts, diagnostic capture, status writes and command exit code remain
unchanged; JavaScript writes retain identical bytes and metadata.
Per repetition, 19 declaration writes and two declaration-map writes change.
All non-declaration writes, including the JavaScript map, remain identical.

Whole-command equality uses the existing shared comparator, including ordered
diagnostics, writes, bytes, BOM, source association, metadata, maps, status and
exit. Native diagnostic and outcome debug captures supplement that comparison;
this packet does not introduce or claim a new serialized diagnostic comparator.

The 30 focused cases deliberately include diagnostic controls: 26 cases return
command exit 0, three TS5090 controls return 2, and the noEmitOnError TS2554
control returns 1 with no writes. These expected command exits also match twice.
The test process exits 0; the packet does not claim every fixture is diagnostic-free.

Receipts:

- `ratchets/h2-8a-declaration-specifiers-before.v1.json`
- `ratchets/h2-8a-declaration-specifiers-composition-before.v1.json`
- `ratchets/h2-8a-declaration-specifiers-after.v1.json`

The local run directories under `target/declaration-specifier-runs/` retain
prelaunch source hashes, actual logs, captures, archived executables and source
archives. Receipt hashes identify those artifacts independently of their local paths.

## Regression qualification

Local regression receipt:
`ratchets/h2-8a-declaration-specifiers-regressions.v1.json`.

| Check | Result |
| --- | --- |
| NodeBuilder specifier, chain and statement units | 28/28 pass; actual exit 0 |
| Existing CFG commands and diagnostic-routing commands | 16 cases exact twice; 2/2 tests pass; actual exit 0 |
| Formatting, whitespace, readiness, observer syntax | All pass; actual exit 0 |
| Scoped checker/compiler libraries and new integration-target clippy | Pass with the recorded existing-lint exemption below; actual exit 0 |

All completed candidate runs used the same source inputs as the focused after
measurement. The checker and CFG executables are archived and hash-identified.
The checker unit command also selected the compiler package to retain the same
dependency feature union; its compiler library test filter selected zero tests,
which is not counted as additional coverage. An earlier checker-only preparation
build was stopped before any tests ran; its real signal exit is retained separately.

Strict `-D warnings` clippy exits 101 on both the candidate and the trusted base
production sources because of `clippy::large_enum_variant` on the unchanged
`crates/checker/src/engine.rs:1038` `ExcessPropertyOutcome`. The diagnostic bodies
are byte-identical. The baseline comparison retained identical test inputs,
temporarily projected the three production files to their base bytes, then
restored the candidate bytes exactly; branch history was not rewritten.
The qualified command uses `-D warnings -A clippy::large_enum_variant` with
`--no-deps` for both libraries and the new integration target. No source-level
lint exemption or unrelated enum-layout change was introduced. Strict clippy
without that CLI exemption is not claimed to pass.

Hosted acceptance is separate from these local results. Its result must be read
from the pull request's `gates` job before landing; this report does not infer
hosted counts from focused tests.

The hosted workflow's fixed `cargo xtask acceptance` entrypoint remains unchanged.
The focused integration target is a local owner regression and is also discovered
by ordinary Cargo workspace test enumeration; it is not added to the hosted
acceptance band membership by this packet.

## Boundaries

This packet establishes the 31 witnessed complete commands, including the one
unchanged original matrix row. It does not remeasure the original 769-row output
matrix or 1,228 class rows, increase their historical totals, close every
NodeBuilder module-specifier branch, or claim H2.8 completion. The separate
ES5 static-this/super implementation owns emitter changes in its own worktree.
