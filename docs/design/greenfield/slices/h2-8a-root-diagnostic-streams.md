# H2.8a A6-3: root diagnostic streams

Kind: runtime; base 43e1107e8. The global before has two unequal original
pathMappingBasedModuleResolution_rootImport_*realRootFile commands, both twice:
Rust reports located TS6059 entries for imports in /root/a.ts in addition to
upstream's fileless TS5011/5055/5101. Loaded sources, writes, paths, blocked
outputs and emitSkipped already agree. The four sibling root-import originals
are already exact controls. No input option or oracle datum changes.

checkSourceFilesBelongToPath inserts lazy Program diagnostics. Their eventual
file association is created by createDiagnosticExplainingFile from inclusion
reasons. getOptionsDiagnostics selects only fileless/config-file diagnostics;
getProgramDiagnostics selects a source's rows and getSemanticDiagnosticsForFile
combines them with checker diagnostics. emitFilesAndReportErrors requests the
semantic stream only after empty option/global streams. Native ProgramDiagnostics
already models this command precedence and already separates source-owned
preparation.program rows from fileless rows. The loader currently puts every
output_directory_diagnostics result into PreparationDiagnostics.options, losing
that distinction before the consumer can apply it.

A6-3-1 changes only the private loader output_directory_diagnostics return to
carry two owned vectors, option diagnostics and root Program diagnostics.
Root checks push into the second vector; option TS5051/5011 and their existing
config locations remain in the first. A6-3-2 extends the existing
self.program_diagnostics with root rows in finish, retaining existing graph
order. Existing compiler aggregation then routes fileless root rows to options
and source-located rows to semantics. No diagnostic code/text matching, path
special case, source text scan or new command filtering is permitted. Preserve
root eligibility, common-directory calculation, message/related-info production,
canonical comparison, caches, emit blocking and write order.

Allowed production path is crates/program/src/loader.rs, the return shape and
collection of output_directory_diagnostics and its single finish call site.
Compiler command and diagnostic consumers are pinned shared prerequisites and
remain unchanged. A6-2's checker statement change is independent. H2.8d owns
broader API getter order, H2.8c owns noCheck entry changes, and H2.8b owns broader
host/diagnostic controls; this packet does not certify those products.

| State / transition | Native owner, consumer and lifetime | Gap / disposition |
| --- | --- | --- |
| Root diagnostic and inclusion locations | loader root_directory_diagnostic creates owned Diagnostic; staged source reasons remain borrowed | already-exact original text/location, unchanged |
| Root versus option collection | two private Vec<Diagnostic> values from output_directory_diagnostics to CompleteGraph | partial-or-stale single options vector; split at producer |
| Prepared lifetime | PreparationDiagnostics owns both vectors until ProgramSession consumption | shared-prerequisite, existing structure unchanged |
| Source/fileless routing | compiler emit_session_diagnostics and ordinary run partition preparation.program using prepared source identity | shared-prerequisite, existing typed source lookup |
| Command reporting | CliEmitSessionOutcome::into_reported selects semantic only after empty option/global diagnostics | shared-prerequisite, unchanged stage ordering |
| Output collision gating | ProgramDiagnostics::gate and preflight retain all applicable streams | shared-prerequisite, original writes and emitSkipped compared |

Pinned upstream owners in vendor/typescript-6.0.3/lib/_tsc.js:

- getOptionsDiagnostics: 124024–124029, SHA256 `7dd9bd43ca95058209d0fcc5948010fb8c5a94598db3426e65ef5fede7f1467a`.
- checkSourceFilesBelongToPath: 124639–124657, SHA256 `ce80660462a7406eafca61b870f95cc1d860753bbf6a7f7b4c97a857a0fec137`.
- emitFilesAndReportErrors: 129412–129467, SHA256 `9dc0128691c9a1bee5aeae85524cc8e2679b3905a4416a41095452e509951a8d`.
- getProgramDiagnostics: 123663–123673, SHA256 `9753e07619a4ecdb74e77cfdfcd4158359d009fb5e97da637d248bb88b054e47`.
- getSemanticDiagnosticsForFile: 123696–123701, SHA256 `4ce16a23f0eaec264998b7e21cb6635abf976ef4d6507b9b0d73d20fd2d36ecd`.
- createDiagnosticExplainingFile: 125851–125932, SHA256 `a52da4c2aafdb0c939e2bf00de5064eb03340c4858ad40378af65b5b6c9de41d`.

E-PROTOCOL and E-PLAN-SCRIPT are premise-unchanged but rechecked. Prepared
owned diagnostics remain separate from resolver state and output artifacts;
planning receives the same sources and options. Their current architecture
rows are pinned, including their historical validation and active lifecycle;
that lineage supplies no new pass for this repair. The manifest pins the
packet, architecture, original candidate/input/complete TS observation/before
artifacts, and baseline loader/compiler/prepared dependencies. It requires six
owners, two steps, two architecture rows, six complete original witnesses
(two failures, four positive controls), and zero unresolved/undispositioned
rows in the bounded collection fix before production edits.

The focused original_root_diagnostic_streams_match_complete_commands test
compares all six commands twice. Existing h2_8a_output_roots and
h2_8a_output_root_format compare the fifty root/config windows and fifty
extension/package-format windows; their expectations remain unchanged. Full769
and final Program/API regressions remain required. This packet preserves the
immutable before and does not close H2.8a or add accepted mismatch vectors.

The complete six-original projection passes twice in the combined
98.99-second A6-2/3 run (target/h2-8a-a6-2-3-after.log). That overall command
exits 101 only for four declaration-alias differences retained by A6-2.
The fifty existing root/config controls pass in three tests in 50.63 seconds
(target/h2-8a-root-streams-adjacent.log, exit 0). A first module-format filter
selected no test and supplies no evidence; the correctly selected fifty
root_diagnostic_module_format_notes controls then pass in
target/h2-8a-js-imports-after.log. That combined run exits 101 for one import
literal difference, with the module-format test itself passing. All 106
root/format commands compare their complete tuples twice.
