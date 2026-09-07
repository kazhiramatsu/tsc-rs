# H2.7d module alias underscore observations

This separate TS6.0.3 packet contains six complete ordinary Program.emit
observations, each repeated on two fresh Programs. It changes neither the
original module-identity 24-case fixture nor the System generated-name 12-case
fixture; both original hashes are pinned before and after observation.
No Rust, profile, CI or runtime admission change is included.

`scripts/observe-module-alias-underscores.mjs` produces
`crates/emitter/tests/fixtures/module-alias-underscores.json`. Each control has
three sources: a dependency exporting value, a bridge importing and re-exporting
that dependency, and a main source importing both and re-exporting the bridge.
Both bridge and main use the same dependency, making cross-source alias state
observable. AMD retains duplicate dependency entries for the separate import
and re-export declarations; System groups equal dependency names into one setter.

| Case | Bridge alias / factory parameters | Main alias / factory parameters |
| --- | --- | --- |
| amd/bundle/dep_ | dep_1, dep_2 | dep_3, bridge_1, bridge_2 |
| amd/bundle/dep__ | dep__1, dep__2 | dep__3, bridge_1, bridge_2 |
| amd/standalone/dep__ | dep__1, dep__2 | dep__1, bridge_1, bridge_2 |
| system/bundle/dep_ | local dep_1; setter dep_1_1 | local dep_2; setter dep_2_1; bridge_1_1 |
| system/bundle/dep__ | local dep__1; setter dep__1_1 | local dep__2; setter dep__2_1; bridge_1_1 |
| system/standalone/dep__ | local dep__1; setter dep__1_1 | local dep__1; setter dep__1_1; bridge_1_1 |

AMD's require/exports parameters are retained in the full observation and
omitted only from this table. System exports/context advance through 1/2/3 in
the bundles and restart at 1 for each standalone output. Its exportStar and
exportedNames helpers likewise share bundle state and reset per output file.

The original AMD/System relative-case options are copied without dropping
declaration, strict, listEmittedFiles, target or resolution settings. The two
standalone controls explicitly replace outFile with outDir. Each uses the same
sources and root order as its corresponding double-underscore bundle case.
All 19 standard libraries remain mounted at /lib with their original bytes;
the observation records their ordered hashes. The TS5101 outFile diagnostic and
both TS5107 module/resolution diagnostics remain in bundle observations; the
standalone cases retain both TS5107 diagnostics. All other diagnostic streams
are empty, emitSkipped is false and sourceMaps is absent.

The fixture preserves complete JavaScript and declaration callback/materialized
bytes, BOM policy, callback/source ordering, callback metadata, pre-emit and
separate final Program diagnostics, emittedFiles/sourceMaps presence, common
directory and actual module/source identities. Unexpected exceptions fail the
observer. `module_registrations` is a lexical inspection aid decoded from the
complete emitted JS, not an inferred naming algorithm or a generated-ID trace.
CLI status, targeted/forced APIs and the possible later phantom-parent event
composition remain outside this packet.

Pinned `_tsc.js` references and span hashes accompany the inputs:
makeIdentifierFromModuleName at 13703, getLocalNameForExternalImport at 27696,
AMD collectAsynchronousDependencies at 110442, System setters at 112403,
printer bundle/file reset at 117058/117072 and 117117, generated-name caching
and uniqueness at 120624/120638, makeUniqueName at 120741, and module-derived
name generation at 120813. makeUniqueName preserves an existing trailing
underscore; it appends a separator only when the base lacks one. Consequently
dep_ becomes dep_1, while dep__ becomes dep__1. Removing every trailing
underscore from a stored base cannot preserve both observations.

Validation: --write and --check each execute 12 fresh Programs serially at
background priority; all six complete tuples reproduce twice. The two original
fixture hashes, dependency/compiler/Node identities, input and diagnostic
assertions, syntax and whitespace checks pass. No Rust build or comparison is
claimed by this oracle-only packet.

```sh
taskpolicy -b nice -n 15 node scripts/observe-module-alias-underscores.mjs --check
```
