# H2.7d/e original candidate inputs

Status: candidate preparation, 2026-09-07, based on `fd95c196`. This packet
prepares observations while the separate bundle and declaration-map workers
investigate their Rust producers. It activates no runtime owner and makes no
Rust, hosted acceptance, or slice-closure claim.

`crates/oracle/h2-7de-candidates.mjs` joins the global owner dispositions with
the frozen H2.6c, H2.7b and H2.7c qualifications by full case ID and verifies
the source path and identity. It reconstructs the original compiler and
conformance units, matrix settings, roots, config and symlinks against pinned
source/expansion records. Project inputs use all 233 pinned project backing
files, including configs and assets, through one shared mount. Both transpile
rows retain their actual API and units; they are not converted to Program
requests.

| Candidate set | Unique case IDs |
| --- | ---: |
| H2.7d/e union | 325 |
| H2.7d required | 315 |
| H2.7e required | 13 |
| Both d and e | 3 |
| Compiler / conformance / project / transpile | 118 / 31 / 174 / 2 |
| Whole-Program inputs / retained transpile API controls | 323 / 2 |
| Overlap with H2.6c / H2.7b / H2.7c | 177 / 0 / 2 |
| Newly admitted Rust cases | 0 |

The 177 H2.6c overlaps have the historical `admitted-for-execution`
disposition, including saved bundle/map TypeScript observations. Their empty
`required_slices` does not remove them from this join or prove that the later
bundle/declaration-map runtime is complete. The H2.7c overlaps are the two
`declarationDir3.json` module variants. These overlaps remain inside the 325
union; no prior-band count is added to it.

Effective input inspection adds five later intersections that the global
static dispositions did not name: both configurations of
`declarationEmitPrefersPathKindBasedOnBundling2.ts` and the AMD/System
`outFilerootDirModuleNames` cases have `rootDir` (H2.8a), while
`sourceMapWithNonCaseSensitiveFileNames.ts` explicitly requests a
case-insensitive host (H2.8b). Source syntax inspection adds one further
H2.9 intersection: `jsFileCompilationTypeAssertions.ts` reaches `/src/a.js`
with parse diagnostics TS17008 and TS1005. The complete observation verifies
that every source receiving a syntax/depth boundary is actually reached by
the Program. No source exceeds the existing transform depth limit of 256.
The original options and source bytes are retained.

| Remaining owners | Cases |
| --- | ---: |
| H2.7d | 280 |
| H2.7d + H2.7e | 3 |
| H2.7d + H2.8a | 23 |
| H2.7d + H2.8b | 5 |
| H2.7d + H2.9 | 4 |
| H2.7e | 8 |
| H2.7e + H2.8c | 2 |

The three compound rows are `declarationMapsOutFile.ts`,
`declarationMapsOutFile2.ts` and `declarationMapsWithSourceMap.ts`. Their
bundle/source-order behavior passes from d to e before declaration-map
admission; neither worker may count them as independently closed. General
directory/host/no-emit/transpile behavior retains its later owner. Project
references and prepend/build runtime remain BLD1 controls, and targeted
ordinary Program APIs remain H2.8d; this one-shot census does not admit those
surfaces.

Project preparation follows the pinned
[projectsRunner.ts option construction](https://github.com/microsoft/TypeScript/blob/050880ce59e30b356b686bd3144efe24f875ebc8/src/testRunner/projectsRunner.ts#L448).
In 36 new candidate variants, `resolveMapRoot` or `resolveSourceRoot` resolves
the descriptor value against `/.src`. The older H2.6c/H2.7b observer kept the
raw descriptor value. The new inputs record the full descriptor, resolved
effective options and a distinct input identity; historical artifacts remain
unchanged and their observations are not transferred to this input route.
No local filesystem path is serialized.

Project compiler options, including the resolved paths, are built before
config parsing and passed as its existing-options argument. The eight
config-driven project variants have only `outFile` or `outFile` plus
`allowJs` in their configs; those fields do not conflict with descriptor or
runner defaults. All 36 path-resolution variants use explicit inputs without
a config. Correcting the preparer's construction order consequently leaves
all 325 input records and all 323 complete emit tuples byte-identical.

The compiler baseline remains TypeScript 6.0.3 at
`050880ce59e30b356b686bd3144efe24f875ebc8`. The pinned native reference rejects
outFile and retains the legacy skip dispositions documented in the
[TypeScript 7 workflow](../typescript-7-workflow.md). This candidate packet
does not retire 6.0.3 cases or transition the accepted version.

Reproduce the input preparation with:

```sh
node crates/oracle/h2-7de-candidates.mjs --check
taskpolicy -b nice -n 15 node crates/oracle/h2-7de-observations.mjs --check
```

The source/unit byte checks, config-root agreement, parent joins and complete
input artifact reproduction pass locally. `h2-7de-observations.v1.json`
records 323 whole-Program emit tuples, each repeated twice on a fresh Program
with one serial worker: 646 TypeScript runs, 639 callback writes and five
emitSkipped cases. All pairs match. The tuple retains complete reported and
emit diagnostics with related information, callback bytes and materialized
BOM bytes, write order, callback source files and data keys/diagnostics/map
URL position/build-info data, raw source-map JSON and input-source arrays,
emitted-file/source-map absence versus empty arrays, status and exit.
Input-source order and the names of the pinned libraries are also retained.
Callback text is checked before encoding to prevent local workspace paths
from being hidden in base64 fields.

The build-info callback in `incrementalOut.ts` remains a later-owner
reference. Both transpile API controls remain unexecuted here. This captures
the one-shot emit tuple; host resolution traces and the project runner's
separate declaration recheck are not claimed. Focused worker controls have
separate denominators and are not added to these original case IDs. These
TypeScript observations do not certify any Rust implementation.
