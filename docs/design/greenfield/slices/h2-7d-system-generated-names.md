# H2.7d System generated-name observations

This is a TypeScript 6.0.3 reference packet for shared bundle printing. It
adds `scripts/observe-system-generated-names.mjs` and the separate
`crates/emitter/tests/fixtures/system-generated-names.json`. It does not change
the original 24 module-identity inputs/observations, Rust, runtime admission,
global profiles or CI. The original fixture's SHA256 is pinned as
`965c6ab4b187ac8488b63e572544ea0924afd230af43077092ce57ed69bb91bb`.

The 12 controls use the complete options of the original `system/relative`
case: ES2015, System, Node10 resolution, declaration output, strict mode,
listEmittedFiles and the other original settings. They retain all standard
libraries under `/lib`, mounted with both the default filename and location.
The two standalone controls explicitly replace only outFile with outDir;
their source bytes and root order match their corresponding bundle controls.
Each case has explicit files, options, roots and current_directory, and an
observation with the same primary shape as `bundle-module-identities.json`.

| Controls | Count | Observed distinction |
| --- | ---: | --- |
| Two/three independent bundle modules | 2 | exports/context names advance `_1`, `_2`, `_3` across sources. |
| Three standalone modules | 1 | Each output starts at exports/context `_1`. |
| Parsed parameter collisions before/after/next suffixes | 3 | Earlier local parsed `_1` makes the first generated name `_2`, then the next source can use `_1`. The three-source case produces `_1`, `_3`, `_2`. Later parsed names do not retroactively rename earlier generated bindings. |
| Three-source star/import chain and before/after parsed helper collisions | 3 | exportStar/exportedNames and import/setter generated names share bundle state, while parsed-name checks remain source-specific. |
| Standalone star/import chain | 1 | Each file restarts exports/context, exportStar/exportedNames and dependency generated names. |
| Empty/global source between modules | 2 | Empty source does not reset names but adds a top-level strict prologue. Global exports_1/context_1 declarations are visible to the checker even before that source is printed, producing `_2`, `_3`. |

In the unmodified star chain, source b prints `exportStar_1`,
`exportedNames_1` and setter parameter `a_1_1`; source c prints helper suffix
2 and setter parameters `a_2_1`, `b_1_1`. With the parsed helper/setter names
in source a, those later generated names are unchanged. With the same parsed
names in c, its helpers become `exportStar_3` / `exportedNames_3`, and its
setter parameters become `a_2_2` / `b_2_1`. This separates current-source
identifier collisions, already-generated names and the nested generated
dependency parameter name. It cannot be reproduced by a monotonically
increasing per-bundle suffix or a union of every source's parsed identifiers.

Every observation records complete JS and declaration callback bytes, BOM
policy/materialized bytes, callback order and source association, callback
metadata, aggregate pre-emit and separate final Program diagnostics,
emitSkipped, emittedFiles and sourceMaps presence, source order, common
directory, module identities and loaded-library hashes. The bundle controls
retain TS5101 and both TS5107 option diagnostics; standalone controls retain
both TS5107 diagnostics. Other diagnostic streams are empty, emitSkipped is
false and sourceMaps is absent. No ignoreDeprecations option suppresses those
original diagnostics. Unexpected exceptions fail the observer; each recorded
exception field is null.

`system_registers` decodes the printed System.register parameters, dependency
arrays, setter parameters and helper declarations from the complete emitted
JavaScript. It is an inspection aid, not a synthetic generated-ID trace or a
replacement for byte comparison. No custom transformer or manually composed
JavaScript is used. The packet observes ordinary Program.emit after the same
pre-emit diagnostic/module-identity sequence as the original fixture; it makes
no CLI, forced API, native7 or Rust equivalence claim.

Fixed `_tsc.js` source references, with line-span hashes in the fixture:

- 112079–112092: System's per-source generated exports/context nodes.
- 112292–112319 and 112403–112410: exportedNames/exportStar and generated
  dependency setter parameters.
- 117058–117081: writeBundle resets after all sources; writeFile resets after
  its one source. Changing currentSourceFile does not reset generated names.
- 117117–117141: reset clears generated-name caches and the generated-name set.
- 120624–120667: cached generation and isUniqueName combine current-file/global
  uniqueness, reserved names and previously generated names.
- 120741–120779: makeUniqueName searches numeric suffixes from 1 on each request
  and records only the chosen generated name.

Validation: `--write` and `--check` each run 24 fresh Programs serially, two
complete observations per case, at background priority. Both runs reproduce
all 12 tuples exactly. Original fixture identity, observer/source dependencies,
Node/compiler pins, input/shape assertions, syntax and whitespace checks pass.
No Rust build or original-fixture regeneration is part of this packet.

```sh
taskpolicy -b nice -n 15 node scripts/observe-system-generated-names.mjs --check
```
# Rust candidate integration

The first complete module-identity comparison passed 13 of 20 ordinary
inputs. Seven System bundles restarted the second source's `exports` and
`context` names at `_1`, while the frozen output used `_2`.

The candidate now retains generated binding identities for System wrapper
parameters, import aliases, export helpers and setter parameters. A setter
keeps its relationship to the eventual name of its imported binding, including
export-only dependencies whose parent binding is not printed. Bundle printing
shares occupied generated names while retaining each source's own parsed
identifier collision table. Standalone printing retains its separate scope.

Existing built-in and shared-writer regression controls pass: 97 tests, no
ignored cases (0.14 seconds after compilation). The new compiler comparison
preserves the original 20 inputs and adds these 12 controls with the real
Program resolver. Its post-correction full-byte comparison is pending. Review
also identified the need to connect the actual checker's global-name oracle
for numbered candidates; that connection is still being prepared. This is
internal candidate work, not public outFile admission or H2.7d completion.
