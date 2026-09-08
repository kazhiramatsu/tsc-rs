# H2.8a A6-14: declaration references in import publication

Base `d0ba24c65bc4aec438d1ce90121188f577f1f342` is the completed A6-13 checkpoint. Production paths are crates/emitter/src/builtins.rs (ImportReExportPlan and
the CommonJS import publication producer) and crates/emitter/src/printer.rs
(the Identifier text-source consumer added as A6-14-3 below). No checker,
factory, metadata storage, System, shared import access helper or ordinary
identifier substitution change. The87 complete-command before and18 resolver projection witnesses are
frozen before activation.

Current create_import_re_export_statement constructs an import binding access
without resolving the source declaration name. TS appendExportsOfDeclaration
uses factory.getDeclarationName, then ordinary expression substitution decides
whether that name denotes an export, an import, or a local value. Native18-case
queries prove the existing resolver already makes those decisions correctly:
named/default/namespace conflicts are local values; an exported import-equals
has a SourceFile export container. Its existing Value exclusion and type-only
alias behavior must remain unchanged.

A6-14-1 replaces the cached syntactic ImportBinding in each publication plan with
its declaration and local-name node identities. Retain import-plan admission,
exportEquals suppression, export-specifier order/deduplication and exported-name
syntax. Remove import-equals's manual ExportObject value rewrite: its existing
resolver SourceFile projection provides that decision instead.

A6-14-2 constructs the declaration name using the existing factory: clone the
ordinary identifier, retain its text range and inherited flags, add NoSourceMap
and NoComments; a generated name uses getGeneratedNameForNode(declaration).
Preserve the declaration's identity across repeated exports. The new publication
value consumer respects NoSubstitution/LocalName and non-substitutable generated
identities. It queries export container first, then the existing import binding
projection. A SourceFile export becomes an access whose cloned property retains
the declaration-name metadata. Named/default imports become the existing typed
import property access. Namespace/import-equals references with no property
remain the declaration name. A conflicting local value also remains that name.
Generated-name allocation/storage and module-info key representation are unchanged;
this step does not claim a new general generated IdentifierNameMap implementation.

The replacement access receives only setTextRange(name), exactly like TS. It
must NOT call set_original_and_range on a flagged declaration clone: native
setOriginalNode merges metadata and would incorrectly copy NoSourceMap to the
whole substituted expression. Keep declaration-name, exported string-literal
and replacement-access ownership separate; do not clear flags after construction,
rewrite source strings or change the ordinary substitution convenience helper.
The preexisting create_import_binding_access retains imported propertyName||name
and its syntax kind. External re-export getters keep their separate path.

E-PROTOCOL and E-RESOLVER-BASE are premise-unchanged: the existing borrowed
checker supplies the typed projections and abort propagation. E-NAMES-BASE and
E-METADATA-BASE are modified-requalify at this declaration-name consumer, with
factory-owned names, sparse metadata and session arenas unchanged. E-PRINTER-BASE
is modified-requalify for A6-14-3: no printer bypass or checker dependency is introduced.
No new host, sink, cancellation or disposal edge is added. An additional sink
fault witness is not applicable; existing resolver errors propagate through ?.

The87 commands cross CJS/AMD/UMD with JS/TS, named/quoted/default/namespace
imports and conflicts, import-equals local/export-object publication, exportEquals
suppression, combined default+namespace, external quoted re-export, Unicode and
escaped aliases, and multiple commented exports. All compare full diagnostics,
ordered bytes/paths/callback metadata, declarations, maps, result, status and
exit. The two native before jobs are33 exact each and54 first-map failures each,
taking59.76s and57.03s,exit101. Every failure changes only main.js mappings; the
lib.js map and other map fields are identical. Later before fields are unqualified.
Positives execute four times across jobs; failures twice in independent jobs.

The58 AMD/UMD commands retain TS5107 option diagnostics and are output/name/map
controls, not semantic-diagnostic branch proofs. CJS29 commands and separate18
resolver cases qualify active semantic lookup. The resolver-only fixture uses
fresh library-free TS programs and equivalent native bound snapshots. Both query
projections match twice in all18 native cases (one unit,0.01s,exit0). TS symbol and
emit flag fields are explanatory context; the native unit compares only the
export-container and import-declaration projections. Both TS observers repeat
fresh observations twice and their full checks pass. Do not normalize diagnostics
or rewrite any previously frozen options.

After require all87 complete commands and18 resolver projections, all24 A6-12
alias-conflict commands, preceding200 export-name commands and56 declaration-name
controls. The target is359/367 complete commands exact twice, retaining only
eight H2.9 parse-recovery refusals. Require every before positive, both earlier
JS getter residues and all54 new first-map failures to become exact; this is a
target until measured. Also run483 emitter units/451 existing emitter contracts
and25 checker statement/chain/specifier units. Freeze any intermediate after
before a further production edit. Failed shared-helper commands execute once per
job; duplicate panic renderings are not repetitions. The added standalone printer controls qualify A6-14-3; System/token
comment phase ownership is unchanged.

Whole769 replay, other A owners, B–E and hosted acceptance remain open. The
schedule's focused edit workflow applies; historical full CI and certificate
walk are omitted and not claimed. Poll all jobs to real exit before mutation.

Pinned complete TS6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- getName: 24788–24799, SHA256 `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33`.

- getDeclarationName: 24809–24811, SHA256 `2774ac8674f5e2cbedb331ad2c3c64fa2474c0df15cd30c16f824775fdb87714`.

- appendExportsOfImportDeclaration: 111651–111683, SHA256 `65a02d44f2100e9793cad4f3fef13ea84611f16ce6c0a660e91eabc665ff8de0`.

- appendExportsOfDeclaration: 111743–111762, SHA256 `b1e3c0856abab75bf412486742c157c5a6b6a7a41fd293c039ba0936fa70cad2`.

- createExportStatement: 111791–111804, SHA256 `d533ed2215809bab955e6b206a545e71c1a8490754d2bb396fec41aca15000cc`.

- createExportExpression: 111805–111851, SHA256 `75fd880a658644ec017e38813933a1710d9f1ec7929387c8755990e3d6c9fbf8`.

- substituteExpressionIdentifier: 111946–111989, SHA256 `972830b79228dc51aaec4b3b13ebd2a12795701304627fef3bdc5ba8b7ab3a96`.

- getReferencedImportDeclaration: 87900–87918, SHA256 `340da277e95c697ff52f213006bacc361d4c71bf18eff9f1c28dc29531b79624`.

- getGeneratedNameForNode: 21652–21666, SHA256 `7aeec7c8966a869665e0b8f01a41cd52e75bc957006170fd446ea819be9a6ea0`.

- getReferencedExportContainer: 87870–87899, SHA256 `64fe010400264ccd927bf7b73511da7c480cf87f60b40ed5c8f43c8090aea8eb`.


A6-14-3 — StringLiteral Identifier text-source dependency

The frozen first complete after is353/367 exact twice in339.89s,exit101:
81/87 import publication,24/24 alias conflicts,160/160 ordinary export names,
32/40 map controls,48/48 export specifiers and8/8 default re-exports. Six escaped
aliases still fail at their first map (once each); eight H2.9 refusals remain.
The new reference maps are repaired but emitted export-name strings lose raw
escape spelling. This producer already carries the correct text-source node;
the printer ignores its Identifier kind. Preserve the frozen first-after record
before activating this consumer dependency.

getLiteralTextOfNode delegates Identifier spelling to getTextOfNode2, then
uses double quotes and escapeString/escapeNonAsciiString according to the
literal's NoAsciiEscaping flag. Ordinary source spelling is usable only with a
parent, a positioned range and the same source-file identity as the current
literal. Otherwise use the text-source identifier's text. Generated identities
use their existing finalized printable text and never claim a source token.
Do not follow an original link to treat an unpositioned clone as parsed syntax.
Use the existing typed SourceRange and quote routine. Existing source StringLiteral
text stays in its separate verbatim-token branch. This step does not extend
private/numeric/JSX text-source kinds or generated-name allocation.

E-STRINGS is modified-requalify for lexical versus cooked provenance.
E-PRINTER-BASE is modified-requalify at this worker branch; the printer retains
its existing hooks, immutable structural plan and lack of checker dependencies.
E-METADATA-BASE and E-NAMES-BASE still use the existing identity and allocation
contracts. The runtime allowance now includes printer.rs for this branch only.

Fresh72 standalone printer cases cross8 identifier spellings,4 origins
(parsed,clone,synthetic,foreign source),2 NoAsciiEscaping values plus8 source
string controls. Before64 are exact twice and8 parsed escape cases fail twice;
the standalone runner executes and compares both repetitions independently.
The TS observer also repeats each fresh output twice and its full check passes.
This provides before proof for source identity, range/parent guards, Unicode,
underscores, quote choice, line continuations and lone-surrogate token reuse.
All64 before positives retain their own origins; no later-field inference
is made from compiler first-map failures. Require all72 after plus the367
complete commands and existing451 printer/transform contracts.

The producer-only emitter after has481 passing units and2 failures caused by
LegacyScriptJsxResolver's absent declaration-name projections; all451 contracts
pass. That intermediate is frozen. A test-only ModulePublicationResolver now
supplies only import-declaration local-name projections and exported import-equals
SourceFile containers, retaining ordinary reference answers and output assertions.
The production checker remains unchanged and its18 fresh projections are required
again after the final implementation.

- createStringLiteralFromNode: 21535–21543, SHA256 `a2fa6c4e9dd96af89655a0a7d44368bcdd05ad599a3ef7e898a2a64e3e5fe9ee`.

- getTextOfNode2: 120442–120465, SHA256 `bfe06a8f5079928e8ff23d6e34aad8110eb12b343b26a300861290242c5403e4`.

- getLiteralTextOfNode: 120467–120479, SHA256 `43989b908107b6f48eae6547835a82a24937f43ba2ddd8673c48b83018d8201e`.


Final measured validation:all87 fresh complete commands match twice, including
all33 before positives and all54 first-map failures. All24 alias-conflict commands
also match twice, repairing both preceding JS getter residues through their
later declaration/map/result fields. Ordinary export names160, map controls32/40,
export specifiers48 and default re-exports8 retain their exact membership. This
is359/367 complete commands exact twice in354.04s; the eight retained
H2.9 parse-recovery refusals execute once each and are the only exit101 cause.

All483 emitter units,451 existing contracts and72 fresh standalone printer
observations pass (each printer observation twice). All26 checker units pass:
25 adjacent statement/chain/specifier units plus the18-case resolver projection
unit, which compares both projections twice. The emitter and checker invocations
exit0. Test provider corrections preserve the old output assertions; production
checker, factory, metadata storage and ordinary substitution helpers are unchanged.

The frozen final record is ratchets/h2-8a-import-publication-reference-after.v1.json;
target/h2-8a-import-publication-reference-after-location.txt locates the outside
source/log archive. The first compiler after and producer-only emitter after
remain separately frozen. Whole769 replay, other A causes, B–E and hosted
acceptance remain open. No complete global corpus count is inferred here.
