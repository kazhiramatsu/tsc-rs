# H2.8a A6-15: computed synthetic-default alias names

Base is the completed A6-14 checkpoint `6689382a56f3397573bf17120252dd71ddc5a87a`.
The complete769 checkpoint is frozen in
ratchets/h2-8a-global-after-a6-14.v1.json:706 exact and63 failed, all twice,
1145.77s,exit101.42 previously failing commands become exact and two previous
positives regress; all188 project positives remain exact. This is an open
checkpoint, not A closure. The two regressions are
jsDeclarationsReexportAliasesEsModuleInterop ES5/ES2015, whose complete tuples
differ only in usage.d.ts's `export=` alias spelling instead of `default`.

A6-15-1 corrects the single computed-option consumer in
crates/checker/src/node_builder/statements.rs::serialize_as_alias. TS
serializeAsAlias checks allowSyntheticDefaultImports computed when the checker
is constructed. Existing CompilerOptions::allow_synthetic_default_imports_effective
already matches the TS6 computed option: explicit false/true wins and absence
means true, independent of esModuleInterop. Replace the raw Some(true) test
with this existing method. No option default, diagnostic, target lookup, alias
recursion, ImportSpecifier propertyName, internal/private symbol queue, factory,
source metadata or printer algorithm changes. The existing immutable options
are borrowed through the live checker; the method returns a bool and introduces
no allocation, callback, query cache, new lifetime or error edge. Existing
serializeAsAlias ledger span/hash remains valid for this missing branch guard.

Pinned TS owners are serializeAsAlias, _computedOptions.allowSyntheticDefaultImports,
getAllowSyntheticDefaultImports binding, and checker allowSyntheticDefaultImports
capture. Dependency closure is the existing alias declaration and nonrecursive
target query, target's merged symbol and unescaped name, computed bool, then
getInternalSymbolName/includePrivateSymbol and the existing declaration-specific
serializer branches. Preserve their order. Private statement arena and borrowed
checker ownership stay unchanged. E-PROTOCOL, E-RESOLVER-BASE, E-METADATA-BASE,
E-PRINTER-BASE are premise-unchanged; E-STRINGS is modified-requalify at this
synthetic symbol-name consumer, not at escaping/printing.

84 fresh complete TS commands cross ES5/ES2015, allowSyntheticDefaultImports
unset/true/false, JS esModuleInterop unset/true/false, CJS named-default/direct-
default/named imports, ES named-default imports, and TS CJS counterparts. Every
case includes declaration and source maps, diagnostics/spans, ordered writes,
callback metadata, emit result, status and exit. All expectations are TS6
produced, run twice and byte checked again; no normalization or option rewrite.
ES5 and explicit-false deprecation output controls retain TS5107 and are counted
separately from active semantic controls. No new sink/host edge, so no additional
fault injection is applicable. Unconditional comparisons retain every case.
Two independent native before jobs are required because the fresh helper catches
outside its internal repetition loop; failures execute once per job and print
twice, while positives execute twice per job. Count real executions only.

After must run all84, both original regressions, the existing local-alias,
class-dependency-order, repeated-target-alias, export-specifier and CJS-default-
reexport controls, plus checker statement/chain/specifier units. Preserve every
before positive and inspect any retained failure. A later actual769 replay is
required; do not subtract this correction from the frozen63. Other A owners,
B–E and hosted acceptance remain open. Follow the schedule's user-authorized
lightweight edit loop; historical full CI/certificate walks are not claimed.
Poll every job to real exit before canonical mutation.

Pinned TS6.0.3 owners (vendor/typescript-6.0.3/lib/_tsc.js):

- serializeAsAlias: 54707–54946, SHA256 `60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277`.
- _computedOptions.allowSyntheticDefaultImports: 18087–18095, SHA256 `36cb4ff3681d4b7afa94e5cbf0c03061693ee24e457ac026ba2b6e5dba2efecf`.
- getAllowSyntheticDefaultImports binding: 18251–18251, SHA256 `e1cf1cdb386e7d66c983019311a16423ca59742684fafd8f954675666e9a6a9b`.
- checker allowSyntheticDefaultImports capture: 46467–46467, SHA256 `76ae1e78ff9c9870a24546c290c8379bcf53a901fd6c92841c80547753c027ad`.

Before:78 exact per job (four actual executions each),6 failed in each independent
job (two actual executions each), 87.19s and 87.3s,exit101.
All six fail at main.d.ts callback bytes with the same export= vs default import
name difference; later fields remain unqualified until after. These six cover
JS named-default, synthetic option unset, all three interop values and both
targets. No outside mismatch was found in this fresh matrix.20 commands have
active semantic diagnostics;64 commands are TS5107 output controls (94 option
diagnostics total). Two owned failures are active semantic controls and four
are deprecation output controls. Explicit true/false, direct-default, TS and
named/ES-default controls are positive. Frozen complete logs/source receipts are
in ratchets/h2-8a-synthetic-default-alias-before.v1.json and its outside archive. After target is84/84 exact twice plus all
adjacent controls; only measured results can qualify this target.

Final measured result: all84 fresh commands and172 preceding alias/class/name
controls are exact twice (256 complete commands,8 tests,213.09s,exit0).
Both original regressions are exact twice (12.27s,exit0). All25 adjacent
checker statement/chain/specifier units pass (0.02s,exit0). Every before
positive is retained and all six fresh declaration-name differences are fixed.
The result and outside source/log receipts are in
ratchets/h2-8a-synthetic-default-alias-after.v1.json. The original769 global
checkpoint remains the measured706/63 at its recorded A6-14 source. This focused
repair does not revise that immutable checkpoint or claim another whole replay.
Other A owners, four prior standalone parent NoNestedComments controls, B–E and
hosted acceptance remain open.
