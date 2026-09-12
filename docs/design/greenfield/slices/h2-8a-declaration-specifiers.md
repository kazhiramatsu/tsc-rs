# H2.8a-DECL-SPEC1: declaration module-specifier option provenance

Runtime residual repair, begun 2026-09-12 from main
`0aaf808bef9b0f5f64737821c63c273056dee2c4` (PR #515 including CFG #514).
Branch `work/h2-8a-declaration-specifiers`, independent worktree
`/Users/hiramatsu/dev/tsc-rs-declaration-specifiers`. Codex owns this packet.
Claude owns the ES5 wrapped-class static-this/super residual in a different worktree.

The original H2.8a output matrix contains
`typescript-6.0.3/compiler/declarationEmitPathMappingMonorepo2.ts#default`.
Its frozen TypeScript declaration names `@ts-bug/core/SvgIcon`. The historical
A6-37 checkpoint records unequal declaration bytes; it is not a fresh native
baseline. This packet first replays that unchanged command and independent
ordinary-source witnesses on the current base.

## Scope and design

Program conversion already owns `paths` in insertion order, their declaring
config directory, normalized `rootDirs`, and the current config file path.
They live in immutable `ProgramOptions`, alongside the separate `CompilerOptions`
bag. `get_specifier_for_module_symbol` currently constructs
`SpecifierCompilerOptions::new(state.options)` with empty paths, roots, and config
identity. The existing inverse-path and project-relative workers therefore do
not receive the complete effective options seen by TypeScript.

The change must pass the existing immutable Program options through the
authoritative checker provider, then project only the four module-specifier
inputs into its existing local options value. The provider borrows the same
`PreparedProgram` as module resolution; no process cache, new mutable state,
parallel shadow map, or fresh config parse is introduced.

| Upstream input / transition | Rust producer, owner and consumer | Lifetime / ordering |
| --- | --- | --- |
| compilerOptions.paths | config converter or public ProgramOptions builder → ProgramOptions::paths → ModulePathMapping entries | Program-owned; preserve entry and substitution order exactly |
| compilerOptions.pathsBasePath | ProgramOptions::with_config_paths / config conversion → paths_base_path | Shares ownership with effective paths; child replacement clears/replaces origin |
| compilerOptions.rootDirs | config conversion / ProgramOptions::with_root_dirs → normalized ProgramPath display paths | Preserve configured order; existing inverse-root algorithm consumes it |
| compilerOptions.configFilePath | current ConfigRootPlan / public ProgramOptions builder → config_file_path | Current project config, distinct from the config declaring inherited paths |
| checker host ownership | PreparedModuleProvider → AuthoritativeModuleProvider → CheckerState's existing borrowed provider | One consuming ProgramSession; ObservedProvider must forward the same metadata |
| getSpecifierForModuleSymbol options | project program inputs after a cache miss, then apply existing bundle baseUrl override | Existing symbol cache remains keyed by containing file and resolution mode; options are immutable for the checker lifetime |
| absent program option metadata | legacy/test providers retain the default absent getter | Their caller cannot supply a ProgramOptions bag; preserve current behavior |

Planned production edits are limited to:

1. `crates/checker/src/lib.rs`: add an object-safe default getter returning
   `Option<&tsc_program::ProgramOptions>` to `AuthoritativeModuleProvider`.
   This is an immutable input projection, with no lookup or fault conversion.
2. `crates/compiler/src/lib.rs`: return the prepared Program's options from
   `PreparedModuleProvider`; forward it through the diagnostic-observing wrapper.
3. `crates/checker/src/node_builder/specifier.rs`: reassemble the split option
   inputs for the existing `getSpecifierForModuleSymbol` worker. Preserve missing
   baseUrl, original paths strings, config provenance, cache ordering, bundle
   override, resolution-mode selection, and all host capabilities.

Class fields, ES2015, standard decorators, generated bindings, common metadata,
printer, shared test registration, accepted-state manifests and old exporters
are outside the edit set. A new standalone compiler integration target avoids
shared `contracts.rs` edits. If an exercised algorithm needs a correction beyond
transport, record its failure and amend this packet before that production edit.

## Upstream and current boundaries

All semantics come from vendored TypeScript 6.0.3. The readiness manifest pins
the exact source bytes, body spans, current Rust symbols, predecessor artifacts,
and input/observer hashes. Its source map includes:

- `getSpecifierForModuleSymbol` (`_tsc.js:53060-53109`): file/ambient/no-host
  exits, mode and cache lookup, complete option bag, bundle override, first specifier.
- `getPathsBasePath` (`16595-16599`): paths absence; baseUrl first, then declaring
  paths base, then current-directory fallback.
- `getLocalModuleSpecifier` (`45579-45639`): rootDirs-relative result; inverse
  paths; project-directory and package-scope selection.
- `tryGetModuleNameFromPaths` (`45817-45849`): ordered mappings/substitutions,
  normalized exact/wildcard matching, endings and validation.
- `tryGetModuleNameFromRootDirs` (`46049-46063`): relative candidates through
  configured roots and existing ending rules.

`E-PROTOCOL`, `E-RESOLVER-BASE` and `E-RESOLVER-IDENTITY-G` remain unchanged:
the live resolver borrows parsed identities and produces owned output facts.
Their architecture validation ref is `0653e10d`; this packet rechecks their
current symbols and complete-command composition on the new base rather than
inheriting that old date as fresh qualification. The declaration session,
activity, output-path, and tracker concern rows marked active-unqualified in
the architecture are research inputs; their current behavior is checked by the
focused whole-command regressions and final acceptance. This packet qualifies
only the newly supplied declaration module-specifier inputs, not those entire rows.

Transform scheduling, flags, original/current/synthetic node identities, source
ranges, lexical receivers, generated names, token/comment cursors and sink
failure order are unchanged. Only the existing inferred ImportType's module
specifier choice may change. This creates no new host callback or write-fault
edge. No new sink fault-injection requirement is inferred from an input projection.

## Witnesses and acceptance

The 24 initial witnesses are frozen in
`crates/compiler/tests/fixtures/h2-8a-declaration-specifiers{,-inputs}.json`.
Each TypeScript command runs twice through `emitFilesAndReportErrorsAndGetExitStatus`.
They cover exact/wildcard/absolute/dot-segment paths, substitution and map order,
baseUrl absence, inherited/replaced/empty paths, project/package boundaries,
rootDirs, maps, declaration-only output, noEmitOnError, and direct Program options.
Three initial project-scope controls also report TS5090 because their paths
targets omit `./`; the diagnostic and resulting complete command remain frozen.
The noEmitOnError control reports TS2554 and writes nothing. These are not
zero-diagnostic observations or successful emission counts.

Six separately frozen composition witnesses add valid counterparts of the three
project/package controls plus AMD outFile with relative paths, absolute paths,
and no paths. Their rootDir differs from baseUrl, so the bundle override is
observable. All six TypeScript commands have zero diagnostics and exceptions,
and each runs twice. They are in `h2-8a-declaration-specifiers-composition{,-inputs}.json`,
with the independent `observe-h2-8a-declaration-specifiers-composition.mjs` observer.
The initial observations and observer remain byte-identical.

Reproduce upstream observations without overwriting existing evidence:

```sh
taskpolicy -b nice -n 15 node scripts/observe-h2-8a-declaration-specifiers.mjs declaration-specifiers --check
```

The standalone `h2_8a_declaration_specifiers` target compares the unchanged
original command and all focused commands, with each failed repetition caught
inside the two-run loop. Native captures store ordered bytes, metadata, Program
facts, outcome/diagnostic debug representations and the frozen expected tuple;
they are not a claim that a new shared JSON comparator was implemented. The
existing complete-command comparator is reused without modification.

Use a dedicated target, CARGO_BUILD_JOBS=2, CARGO_INCREMENTAL=0,
CARGO_PROFILE_TEST_DEBUG=0, empty RUSTC_WRAPPER, and taskpolicy -b / nice -n 15.
Run one heavy local command at a time across both worktrees. The primary command is:

```sh
cargo test --offline -p tsc-rs-compiler --test h2_8a_declaration_specifiers declaration_specifier -- --test-threads=1 --nocapture
```

After repair, require exact complete tuples twice, all adjacent checker
specifier/chain/statement units, existing config commands, formatting and scoped
clippy. Final hosted `cargo xtask acceptance` must preserve the accepted diagnostic
set (49,024/49,024, FP=0, FN=0) and inspect every band delta. No walk/chain-walk or
full developer CI is substituted for this current workflow. Original 769 and
class 1,228 totals remain historical until separately remeasured; focused gains
are never added to those totals.

## Current readiness

Before production changes, the initial 24 native cases execute twice: 7 exact,
17 unequal; the unchanged original monorepo command also fails twice. The six
composition commands execute twice: 4 exact, 2 unequal (valid separate package
scope and absolute bundle paths). Both failing runs exit 101 and retain complete
captures and archived test binaries. Repeated captures are identical after
removing only the repetition label. Production inputs equal the trusted main
base throughout; the second test binary includes only the additional test group.
Receipts are `ratchets/h2-8a-declaration-specifiers-before.v1.json` and
`ratchets/h2-8a-declaration-specifiers-composition-before.v1.json`.

The first witness commit is `f7fd77d4c`. The readiness manifest maps the seven
semantic rows to the three implementation steps and includes the independently
observed bundle override. The current local gap is missing input transport to
existing inverse-path and project-relative algorithms; the source review and
before comparisons require no algorithm-body correction. There are no unresolved
design items. Run `python3 scripts/check-h2-8a-declaration-specifiers.py --before-production`
before step 1, and the same check without that flag after production changes.
The before-only check additionally rejects any production drift from this base.
