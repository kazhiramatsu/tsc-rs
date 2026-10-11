# Vendored native TypeScript test inputs

Each directory is one profile: upstream files copied byte for byte from
`microsoft/TypeScript` at one commit, at their upstream paths under `upstream/`,
with a `manifest.json` that pins the commit, each set's Git tree id (or blob
inventory for a filtered set), file and byte counts, and the sorted names of all
compiler and conformance reference baselines (`baseline-names.txt`).

| Profile | Commit | Contents |
| --- | --- | --- |
| `7.1.0-dev-aa814927` | `aa8149273b23401f0a79a5f0384c42de51888693` (main, 2026-10-09; no 7.1 tag yet) | `tsc/testdata/tests/cases/{compiler,conformance,transpile}`, `tsc/testdata/tests/lib`, `tsc/internal/bundled/libs`, `tsc/internal/diagnostics/diagnosticMessages.json` and the localized messages, the option declarations (`tsc/internal/tsoptions/declarations_generated.go` and its neighbours, `tsc/internal/core/{compileroptions,options_generated}.go`), the kind and API encoder tables, the reference baselines of `tsc/testdata/baselines/reference/{compiler,conformance}` (`*.errors.txt`, `*.js`, `*.js.map`, `*.sourcemap.txt`, `*.types`, `*.symbols`, `*.trace.json`) and of the transpile, tsoptions, tsconfig parsing, tsc, tsc -b, watch, API and astnav suites, and `tsctests-scenarios.json` (the tsc and tsc -b scenarios recorded by `scripts/tsctests_scenarios.py`) |

Produce or verify a profile with:

```sh
python3 scripts/vendor_typescript_native.py --commit <full sha> --profile <name>
python3 scripts/vendor_typescript_native.py --profile <name> --check
```

The harness contract `native_vendored_inputs_match_the_manifest` recomputes every
blob id and inventory digest. The files are never edited by hand; a new upstream
commit becomes a new profile. The design is the
[TypeScript 7.1 conformance packet](../../docs/design/greenfield/slices/conformance-ts71/README.md).
