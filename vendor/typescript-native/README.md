# Vendored native TypeScript test inputs

Each directory is one profile: upstream files copied byte for byte from
`microsoft/TypeScript` at one commit, at their upstream paths under `upstream/`,
with a `manifest.json` that pins the commit, each set's Git tree id (or blob
inventory for a filtered set), file and byte counts, and the sorted names of all
compiler and conformance reference baselines (`baseline-names.txt`).

| Profile | Commit | Contents |
| --- | --- | --- |
| `7.1.0-dev-19dadef8` | `19dadef8888ba5b27d8b9f622480745cf623e020` (main, 2026-09-29; no 7.1 tag yet) | `tsc/testdata/tests/cases/{compiler,conformance}`, `tsc/testdata/tests/lib`, `tsc/internal/bundled/libs`, and the `*.errors.txt` files of `tsc/testdata/baselines/reference/{compiler,conformance}` |

Produce or verify a profile with:

```sh
python3 scripts/vendor_typescript_native.py --commit <full sha> --profile <name>
python3 scripts/vendor_typescript_native.py --profile <name> --check
```

The harness contract `native_vendored_inputs_match_the_manifest` recomputes every
blob id and inventory digest. The files are never edited by hand; a new upstream
commit becomes a new profile. The design is the
[TypeScript 7.1 conformance packet](../../docs/design/greenfield/slices/conformance-ts71/README.md).
