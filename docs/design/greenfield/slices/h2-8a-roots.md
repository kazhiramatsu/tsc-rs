# H2.8a A2: Program output directories and root diagnostics

Kind: runtime; status: implementation in progress, 2026-09-08. This continues the
[H2.8 train](h2-8.md), preserving the full H2.8a–e objective. A1's output-directory
and Unicode changes are the current worktree prerequisite. A2 activates rootDir
and config-based directory semantics, without activating targeted ordinary emit,
noCheck, composite/build-info, custom transforms or additional CLI modes.

The readiness manifest pins the exact worktree authority files before edits,
vendored spans, the original fixture projection, supplemental observations and
the implementation/test map. Run `python3 scripts/check-output-root-readiness.py`
before production edits. A2 is ready only with zero unresolved and
undispositioned rows, fresh hashes and every row connected to its step and test.
The original H2.8a-A1 fixture and its 34 source/TS tuples stay unchanged.

## Upstream and Rust semantic map

| Step | Pinned tsc owner | Rust ownership and exact action | Witness / failure behavior |
| --- | --- | --- | --- |
| A2-1 | `getCommonSourceDirectory` 116460–116475; `computeCommonSourceDirectoryOfFilenames` 121909–121939 | New `program::output_directories` owns pure `common_source_directory` and `inferred_common_source_directory`. Public re-exports allow compiler and loader to consume the same algorithm. RootDir truthiness precedes config directory, then component-wise source inference. Reuse program's platform-neutral lexical normalization and host's protected Unicode case fold. Paths remain owned `PathBuf`; all inputs are already validated Unicode Program paths/option strings. | The original 27 root/config windows and four case-fold windows. No eligible sources infer currentDirectory; differing roots infer an empty common directory. |
| A2-2 | `sourceFileMayBeEmitted` 16617–16634; `getSourceFilePathInNewDirWorker` 16638–16643 | The same module exposes `source_file_may_be_emitted_for_options` and `source_file_path_in_new_directory`, borrowed option/path inputs with owned outputs. Base Program eligibility and declarations are checked before noEmitForJsFiles. JSON requires outFile or a nonempty outDir; with root/config, skip it if its planned copy equals its source. Prefix selection uses canonical paths; callback output directory text retains dot segments. | Four JS/JSON cases, ignored declaration and empty-source controls, original A1 paths and declaration reference/map sequences. No phantom JSON root diagnostic or plan unit. |
| A2-3 | `getCommonSourceDirectory2` 123142–123157; root option validation 124639–124657; Program option gates 124907–124943 | After `StagedGraph::finish` propagates non-external reachability, select eligible ordinary sources using A2-2. Compute root/config diagnostics while the real `StagedSource::inclusion_reasons` and `ProgramConfigFile` are still available. Add a distinct `CompleteGraph::option_diagnostics` vector, transferred into `PreparationDiagnostics::options`; located root diagnostics must never be reclassified as semantic diagnostics. | Root requests reach their real result; noEmitOnError consumes TS6059/5011 before transforms/writes. Other mode/profile guards remain. |
| A2-4 | `createDiagnosticExplainingFile` 125851–125932; related information 125939–125987 | New loader diagnostic worker consumes ordered existing inclusion reasons, chooses the first import/reference location, omits the reason chain for a sole located reason, and retains all other reasons/related information. Reuse existing reason-message and config-location helpers; factor only the root-related-information block shared with casing diagnostics. Additional references use their real parent/span and TS1399/1400/1401. No reason is fabricated from filenames. | Five import/reference cases plus config-root TS1410 locations; related diagnostic absence is distinct from an empty list. Existing casing tests must remain unchanged. |
| A2-5 | `createDiagnosticForOption` 125368–125386; migration check 124919–124943 | Program directory diagnostics create TS5011 for non-noEmit/non-composite configs lacking a truthy rootDir when output options request a different inferred layout. Locate on outFile, otherwise outDir and declarationDir, using retained option key spans; preserve the migration child message and config-relative directory spelling. TS common-root failure selects outDir. | Original configured true/false noEmitOnError cases; exact positions/messages and unchanged output paths. Config parser does not guess the future loaded source set. |
| A2-6 | Program selection and path owners above; `toFileNameLowerCase` 874–876 | `compiler::common_emit_source_directory` delegates to A2-1; delete its raw prefix inference. Emitter `plan` delegates source eligibility and relocation to A2-2; `EmitHost::canonical_output_path` delegates to the shared canonical path worker. Remove the rootDir guard in `execute`, keep every other guard. Canonical collision keys must preserve protected Unicode letters. | All 40 root witnesses, A1's 42 controls, emitter contracts, H2.7c regressions and existing output-directory map/reference tests. |

The local gaps are: missing config common directory, missing TS5011/6059,
partial empty-option/JSON eligibility, partial case-insensitive component
inference and incorrect generic Unicode folding in the emitter. Existing
loader inclusion reasons/config spans and normalized-root handling are shared
prerequisites, read and hash-pinned afresh. Existing generated names, transforms,
printer contexts, comment cursors and source-map recorder order are unchanged.
Changing those owners requires an amendment first.

Allowed production paths are `crates/program/src/{output_directories,lib,loader}.rs`,
`crates/compiler/src/lib.rs`, and `crates/emitter/src/{plan,host,execute}.rs`.
The integrator is the sole writer. Test/observer and packet/manifest changes
are evidence work. No production fixture lookup, case/path dispatch, output
text substitution, expectation normalization or unknown-success fallback is
permitted. No module needs to retain checker borrows or parse trees beyond
existing session lifetimes.

Architecture impacts: `E-PLAN-SCRIPT`, `E-DECL-PATH` and the canonical portion
of `E-OUTPUT-SCRIPT` are modified-requalify. Remaining `E-PLAN-FUTURE` and
`E-OUTPUT-FUTURE` API/build products remain future-owned-fail-closed.
Current architecture rows are research inputs; no stale qualified ref grants
new behavior. Program loading retains source/diagnostic ownership and the
read-only host / write-only sink dependency direction. No new host capability
or filesystem query is introduced.

## Observations and acceptance

`scripts/observe-output-roots.mjs` asserts the original 27-case projection
SHA256 `3a7b08b0e6e373f9071cbb1a962ab0cd78d6b7d4c78fb386c5b8ec21585d728b`
and adds 13 separately named supplemental cases. Every complete observation is
repeated on a fresh pinned 6.0.3 Program. Both base and supplement preserve
callback/materialized bytes, BOM, order, source associations, metadata presence,
diagnostic streams and related information, emitSkipped, optional result lists,
status output and exit code. The compiler's existing exact comparator now also
distinguishes absent callback diagnostic metadata from an empty diagnostic list.

The root validation prefix intentionally is not a directory-component test:
both `/project/sr` and `/project/sr/` normalize to `/project/sr` and accept
`/project/src/a.ts`. Relocation uses the trailing-separator common directory
separately and retains that source's absolute output path. JS outside root
produces both overwrite and root diagnostics; JSON whose copy would equal its
source is excluded and produces neither. Case-insensitive common inference
must distinguish sibling components `src` and `src2` and preserve U+0130.

```sh
taskpolicy -b nice -n 15 node scripts/observe-output-roots.mjs --check
python3 scripts/check-output-root-readiness.py
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts h2_8a_output_roots -- --nocapture --test-threads=1
```

The baseline is `target/h2-8a-roots-expanded-before.log`. All 40 cases must
execute exactly after implementation. The original 27-case log separately
records 25 rootDir refusals and two missing TS5011 diagnostics. New errors in
the broader earlier bands are investigated on unchanged input/expectations;
no pass is inherited from an old refusal. SourceMap/declarationMap, config
extends and additional filesystem/host intersections still require the later
H2.8a matrix inventory and close. A2 passing alone cannot close H2.8a.

Resource ceiling: two Cargo build jobs, one oracle process and one Program test
thread. Runtime editing follows the ready gate and before observation. Final
local validation uses the focused root suite, adjacent A1/H2.7c/map controls,
emitter tests, changed program loader/casing tests and all-target Clippy; the
whole H2.8a train later runs existing hosted acceptance before landing. The
historical certificate walk/full developer CI remain outside the current
lightweight workflow and are not claimed.

## Boundary amendment and first replay

The first A2 runtime replay matches 39 of 40 root observations and all 42 A1
controls. `reason/reference/true` exposes the shared inclusion-message helper
adding quotes to a reference argument whose diagnostic template already owns
the quotes. A2-4 also corrects `source_inclusion_reason_message`'s path/type
reference arguments, using the stored raw specifier. For imports, replace the
private `SourceInclusionReason::Import::specifier` with `reference_text`, copied
from the prepared parent text at the already retained UTF-16 request span in
`process_module_requests`. Diagnostic formatting consumes that original literal,
including double quotes and escapes, without synthesizing a quote style.
Use the prepared snapshot's existing `PositionIndex::utf16_to_byte` for both
boundaries and copy that UTF-8 slice; do not rescan the source prefix for each
import. Both boundaries are validated module-request token spans. Pin the
unchanged `prepared.rs` and diagnostics `text.rs` inputs for this native seam.
The reason remains owned by the staged graph; no extra long-lived syntax borrow
or public field is introduced. Pin `fileIncludeReasonToDiagnostics` at
`_tsc.js:129300–129342` before this edit. Existing casing observers stay exact.

Ten `edge_cases` extend the packet without changing either prior projection:
hidden config directories with both blocking settings, declarationDir alone
and with outDir, duplicate option keys, JSON prefix/equal-output controls and
protected sharp-s casing, double-quoted and escaped import literals. Their
complete TS observations repeat twice. The
original supplemental projection SHA256 is
`a20c0303a3f82602f6870f583e2a842b352e33288321003c54cc4636a39ac182`.
The added production test is
`output_root_option_and_json_edges_match_typescript_observations`; its first
comparison is `target/h2-8a-roots-edges-before.log`.

A2-5's config-relative helper follows `getRelativePathFromFile` and
`ensurePathIsNonModuleName`, pinned at `_tsc.js:5694–5733` and 5616–5618.
Hidden `.hidden` names require `./.hidden`; only actual relative paths retain
their prefix. A2-2's relocation worker receives the common-directory string
exactly as its caller supplies it: ordinary planning provides a trailing
separator, while JSON eligibility passes `getNormalizedAbsolutePath`'s result
without that separator. The shared worker must not insert a separator or
normalize that argument again. This distinction can change eligibility even
when `getOutputPathsFor` subsequently suppresses an equal-location JSON copy.
No path or diagnostic is repaired after production output.

A2-6 removes the obsolete private emitter `normalize_lexical_path` once both
callers delegate to the shared worker. The unchanged original H2.7c rootDir
corpus observation now executes instead of asserting its old refusal; its
acceptance reader reports 32 exact and 10 deferred while retaining the
historical artifact's 31/11 membership. All 50 root observations, earlier
controls and owner regressions remain required before this packet completes.

The corrected replay passes all 50 root observations and all 42 A1 controls
twice (five tests, 150.23s after compilation), recorded in
`target/h2-8a-roots-corrected.log`. The complete Program contract suite passes
467 tests with five previously ignored tests (5.18s after compilation), in
`target/h2-8a-program-regressions.log`. The 50-case oracle `--check` also passes.
The later combined A1/A2/A3 replay also passes all 116 cases, including the
retained-position-index refinement and comparator extraction (six tests,
212.17s after compilation; `target/h2-8a-filesystem-after.log`). H2.7c,
map/reference controls, final Program regressions, Clippy and the H2.8a global
close remain pending.

## Native directory representation amendment

The whole-emitter regression passes 480 unit tests and 447 contracts but finds
four existing `output_plan_contract` failures (the full log is
`target/h2-8a-emitter-a3-regressions.log`). Native `EmitHost` returns its common
directory as `&Path`, for which a trailing separator is optional. Its original
plan/declaration/case-fold/JavaScript-family controls use `/project/src` and
retain their original relocated expectations. The low-level TS string worker
cannot receive that directory verbatim: its prefix slicing expects the
separator supplied by TS's `getCommonSourceDirectory`.

A2-6's private `plan::source_file_path_in_new_dir` is the native adapter:
append a separator for a nonempty common directory only when absent, borrowing
the existing string otherwise, then call the shared worker. An empty common
directory stays empty. Document the optional separator on the existing
`EmitHost::common_source_directory` method. The shared worker and JSON
eligibility keep their exact input-string behavior; they must not receive this
native-directory adaptation. This is within the existing A2 allowed owners,
with the already pinned `getCommonSourceDirectory` and relocation spans.
No fixture or existing contract expectation changes. Recheck the four original
contracts, the complete emitter suite and the full A1/A2/A3 comparison.

## A5 dependency amendment

A5 changes the package-resolution owner in `module_resolution.rs`. A2's reused
lexical path helpers, from `fn combine_paths_spelling` through the file end,
remain byte-identical to checkpoint `4b0f4d75b5f79edcf93baca34c44b250a0d68710`.
The readiness manifest retains the original whole-file hash as provenance and
now verifies the unchanged helper tail against both that checkpoint and the
current source. This is the bounded dependency amendment described in the
[A5 packet](h2-8a-package-inputs.md), not a refreshed compatibility observation.
The original 50 complete root observations remain unchanged and must pass.

A5-6 also adds the package parser's owned `type` truthiness to
`PackageMetadata`. The new field distinguishes empty strings and absent values
from truthy unrecognized strings and objects when explaining TS6059. The
amendment retains the original `prepared.rs` digest, limits the changed region
to `PackageMetadata`, and byte-compares everything outside that region against
4b0f4d75. The current region is pinned separately. No existing A2 observation
changes; the original 50 root cases and the additional 50 format cases must
requalify this dependency.
