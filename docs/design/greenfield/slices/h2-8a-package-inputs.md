# H2.8a A5: local package output paths resolve to inputs

Status: focused implementation verified, 2026-09-08.
The original and amended machine readiness checks passed before their runtime edits. Base checkpoint
`4b0f4d75b5f79edcf93baca34c44b250a0d68710`. The full H2.8 objective remains open.

The original H2.7c case `nodeAllowJsPackageSelfName2.ts#default` now passes the
rootDir admission boundary but fails its unchanged complete tuple. Its source
`/test/foo.js` imports `js-self-name-import/foo.js`; `/package.json` maps the
types condition to `/types/src/foo.d.ts`. With rootDir `/`, declarationDir
`/types` and config `/tsconfig.json`, TS resolves that output path back to
`/src/foo.js`. Rust instead loads the preexisting declaration output, then
correctly rejects overwriting that input with TS5055. The fix belongs in
resolution, before graph loading. Collision preflight and the expected tuple
remain authoritative. No fixture/path-specific dispatch is permitted.

The failed regression is retained in `target/h2-8a-a4-h2-7c-regressions.log`:
14 passing tests and one failing test, whose sole residual is this original
input. The historical 31 exact corpus cases and other H2.7c focused/API tests
pass. A1–A4 retain 140 focused and 23 original D/E output cases, twice.

## Owners and concrete implementation

All spans refer to pinned TypeScript 6.0.3 `_tsc.js`. The readiness manifest
records exact hashes, original Rust implementation bytes and frozen witnesses.

| Step | Upstream owner | Native representation and action | Validation |
| --- | --- | --- | --- |
| A5-1 | `nodeModuleNameResolverWorker`, 40943–41075 | A private stack of owned request states retains requesting directory, reporter disposition and `DiagnosticList`. The public module fact entry returns diagnostics with either success or miss through `HostModuleResolution`. | Raw resolution results, two repetitions, reuse after a fallible host call |
| A5-1 | `getTemporaryModuleResolutionState`, 41260–41276 | Temporary package probes retain no request state/reporter. Both diagnostic-only retry entries push explicitly suppressed request states. | Existing alternate-result contracts and new nested request controls |
| A5-2 | `getLoadModuleFromTargetExportOrImport`, 41659–41883 | Both ordinary selected-target and iterative bare-import string-target branches try an input link before `probe_export_target`. Bare child requests use their own package directory and discard their diagnostics; array fallback restores the caller. | exports/imports, bare alias, bare self-name, outer fallback |
| A5-2 | `tryLoadInputFileForPath`, 41808–41881 | Host-owning resolution checks config lookup, truthy output options, node_modules exclusion and config-source scope; derives ordered common-directory guesses, reports ambiguity, then probes original input extensions. A first existing candidate still uses the existing package-field loader/package-ID owner. | explicit/config/default/ambiguous roots, misses, scope and directory order |
| A5-2 | `getOutputDirectoriesForBaseDirectory`, 41869–41880 | Declaration directory precedes distinct outDir. Actual config source selects host cwd as base; otherwise each common-directory guess is the base. Preserve raw option spelling until upstream normalization. | relative bases, equal/empty options, component boundaries, case policy |
| A5-2 | `getCommonSourceDirectory`, 116460–116475 | Reuse `program::output_directories` and the existing path normalizer. Retain config-file path separately from optional config-source presence in the resolver. No parse/checker state is retained. | config path without source, parsed config and explicit root |
| A5-2 | `getPossibleOriginalInputExtensionForExtension`, 16592–16594 | Apply the exact original-extension order, including `.tsx` before `.ts`, and module extension families. UTF-16 slicing follows the candidate directory length. | TS/JS/JSX/TSX/MTS/MJS/CTS/CJS and JSON target controls |
| A5-2 | `extensionIsOk`, 41431–41433 | Filter original extensions through the request's existing effective extension pass before `file_exists`; declaration-only and config masks retain their separate semantics. | noDtsResolution, JS eligibility and type-reference controls |
| A5-3 | `addResolutionDiagnostics`, 123055–123062 | Loader binding transfers owned diagnostics to `ModuleResolution` on success and miss. Both compiler diagnostic aggregators include module rows in the Program-construction stream. The semantic provider admits the qualified fileless TS2209/2210 shape; other unsupported records still fail closed. | raw duplicate diagnostics, command deduplication, noEmitOnError, existing fabricated TS9701 control |
| A5-4 | `resolveTypeReferenceDirective`, 40060–40250 | Keep the existing entry/result unchanged. Primary custom roots use the legacy directory loader, without exports; secondary exports targets are inside node_modules and excluded before this worker can report. | Four complete primary/secondary type-reference controls |

A5-5 is the end-to-end graph correction: replay the unchanged original H2.7c
case and all original 32 exact tuples through the existing corpus comparator.
This row adds no independent upstream algorithm; it joins A5-1–A5-4 with the
unchanged input/output collision and command owners.

Allowed production files are `crates/program/src/module_resolution.rs`,
`crates/program/src/loader.rs`, and `crates/compiler/src/lib.rs`. Test helpers,
registration, the fresh observer/fixture, and readiness documents are evidence
surfaces. No emitter, parser, checker algorithm, sink or type-reference API
change is in this packet. A newly exposed owner gap requires a concrete packet
amendment before its implementation.

## Request lifecycle and diagnostics

The resolver-private request stack is separate from the existing active cycle
stack. A public module call pushes one owned state, runs the existing resolver,
and moves its diagnostics into the result; errors restore the prior depth. The
iterative `ImportsTargetState::Bare` evaluator pushes/pops child request states
beside the existing cycle pushes/pops. Its existing error cleanup restores both
depths. This preserves stack safety for long package-map chains. The child
result deliberately does not append diagnostics to its parent, matching the
actual upstream caller at 41673–41704. Suppressed retry entries preserve their
own requesting directory and return no primary diagnostic rows.

Raw resolution diagnostics are not deduplicated. The fresh genuine-miss witness
has two identical TS2209 entries in its resolution result, while the ordinary
command reports the diagnostic once. Existing Program getter sorting and
deduplication owns this distinction. Unknown resolution records remain typed
failures; the existing TS9701 negative contract must continue to pass.

Type-reference probing was audited separately. The fresh primary custom-root
controls have no exports lookup; fresh secondary controls resolve the package
export but retain zero ambiguity diagnostics because the target lies inside
node_modules. Its declaration-only extension mask also excludes all original
input extensions. Adding an otherwise unreachable host-diagnostic carrier to
that API is unnecessary for this owner.

## Evidence and architecture dispositions

`scripts/observe-package-output-inputs.mjs` produces 46 complete observations
twice on fresh hermetic Programs. Inputs include declaration and JavaScript
output directories, extension priority, raw/parsed config identity, missing
inputs and outputs, case policy, bare rewrites, JSON and type-reference controls.
Each observation retains loaded-file order, exact module and type-reference
results and their raw diagnostics, complete callbacks/bytes/metadata, command
reporting, emit-result presence and exit status. The original H2.7c corpus
remains frozen separately. No oracle output is inferred from Rust.

The readiness check ran before runtime edits with ten upstream owners and five
steps, then thirteen owners and six steps for A5-6. It verifies five architecture
dispositions, the frozen fixtures and observers, baseline production bytes,
and zero unresolved/undispositioned rows.
The architecture dispositions are `E-PROTOCOL` modified-requalify;
`E-RESOLVER-BASE`, `E-PLAN-SCRIPT`, `E-DECL-PATH`, and `E-OUTPUT-SCRIPT`
premise-unchanged but rechecked. The graph changes before the borrowing checker
bridge and output planner, with no new parse provenance or transform lifecycle.
Full H2.8a profile/corpus qualification remains the final A close.

A2 dependency amendment: its original whole-file pin on `module_resolution.rs`
proved the reused lexical path helpers before A5. A5 changes the resolution
owner in that file, so A2 will pin its unchanged pure helper tail (beginning at
`fn combine_paths_spelling`) against the exact original 4b0f4d75 bytes. The tail
must remain byte-identical; the original whole-file digest remains as historical
provenance. Do not silently refresh the whole-file hash to accept a path change.
Both the A2 50-window comparison and its updated readiness check remain required.

Development follows the authorized lightweight workflow: targeted full tuple
comparisons and adjacent regressions, then existing hosted acceptance before
landing. Historical certificate walks and full developer CI are not claimed.

## A5-6 amendment: module-format details in root diagnostics

The first repaired 46-case replay matches 44 complete tuples (45.03 seconds).
All loaded-file and raw resolution facts match, including the repeated miss
and the four type-reference controls. Two relative-output cases still differ
only in TS6059's explanation: the ECMAScript package-format child is missing.
Their original expectations are unchanged in `package-output-inputs.json`.

`observe-output-root-format.mjs` adds 50 fresh complete commands, twice each:
32 combinations of `.ts`, `.js`, `.mts`, `.cts` and eight package `type` states;
16 initial script/CommonJS forms under Legacy/Force module detection and four
package states; two located imports. The states include empty-string, false
and object values, an absent field, and a missing package file. These witnesses
retain callback output as well as full root diagnostic chains and related data.

The amended upstream owners are `explainIfFileIsRedirectAndImpliedFormat`
(129225–129275), `getImpliedNodeFormatForFileWorker` (122500–122513), and
`getImpliedNodeFormatForEmitWorker` (125496–125509). The existing pinned
`createDiagnosticExplainingFile` owner appends these siblings after the inclusion
reason chain. Fixed module extensions have no TS package-scope/lookup fields;
retaining a generic native package scope must not create a false note for them.
For standard extensions, existing `PreparedSourceFile` module-detection and
implied-for-emit facts select the note. Missing package scope produces the
CommonJS not-found explanation only when the implied-format lookup occurred.

A5-6 adds `crates/program/src/prepared.rs` to allowed production files for one
owned package fact: the JavaScript truthiness of the parsed `type` value.
`PackageMetadata::module_type` alone cannot distinguish an empty string from a
nonempty unrecognized string, or an absent field from a truthy object. The
existing package-JSON producer computes the fact once with its existing
truthiness helper; metadata construction/compatibility retains it. Root
reporting reads the observed package map and never reparses JSON or reads the
host. A2's dependency on this metadata file is explicitly requalified by these
new observations and its original 50 root cases; its previous file digest is
retained in the amendment provenance.

This is the ordinary command's initial options-diagnostic state. It uses actual
parsed module-detection facts and does not infer CommonJS binding with text
search or create a checker early. Under explicit Legacy detection, requesting
semantic diagnostics before options can change TS's lazy explanation cache;
that per-getter lifecycle remains H2.8d's separately required API work. Project
reference redirects remain on their existing BLD1 boundary.

The amended readiness gate must cover thirteen upstream rows, six steps, and
both unchanged 46-case and new 50-case fixtures before A5-6 runtime editing.
The new native `package_input_request_diagnostics_restore_after_host_error_and_nested_rewrites`
contract independently exercises a failing input probe, resolver/cache reuse,
and discarded child diagnostics; its actual test must execute, not merely
compile under an unmatched filter.

The initial 50-case replay had 26 missing module-format details and 16 incorrect
derived artifact-kind labels in the new observer. The observer now classifies
`.mjs`/`.cjs` using the existing comparator schema. A fresh TS replay changed
only those 16 labels; every actual callback, byte sequence, diagnostic and
result field remained equal to the saved first observation. The corrected
before comparison remains required before runtime editing.

## Local implementation result

The corrected root-format before replay has exactly 26 divergent cases and
24 exact controls (31.36 seconds). A5-6 then passes all 96 complete cases twice
through production Rust: 46 package-input cases and 50 root-format cases
(`target/h2-8a-package-inputs-format-after.log`, two tests, 102.88 seconds).
No expected TS diagnostic, output byte or resolution fact was changed to match
Rust. The preceding 44/46 result and corrected 26-case before failure remain
separate evidence.

The adjacent replay passes nine tests in 179.33 seconds
(`target/h2-8a-package-inputs-adjacent.log`): all 140 A1–A4 focused windows, all
32 original H2.7c tuples, and the existing unsupported-resolution TS9701 control.
The unchanged `nodeAllowJsPackageSelfName2.ts#default` now resolves to the source
and writes both declarations without diagnostics. The native failing-host and
nested-rewrite lifecycle contract also executes and passes one test
(`target/h2-8a-input-request-lifecycle.log`); an earlier zero-test filter was
not validation. A2's unchanged path-helper tail and bounded metadata amendment
pass its readiness check.

These are local checkpoints. Full output inventory/profile, remaining map and
collision axes, final Program/emitter regressions and Clippy, the actual D/E
runner and final historical acceptance replay remain required before H2.8a
closes. H2.8b–e and hosted acceptance remain open.
