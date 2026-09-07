# H2.7e declaration-map API and sink references

Status: repeated TypeScript reference packet, 2026-09-07. No Rust runtime
admission. `scripts/observe-declaration-map-apis.mjs` produces
`crates/compiler/tests/fixtures/declaration-map-apis.json` from the pinned
TypeScript 6.0.3 compiler, source commit
`050880ce59e30b356b686bd3144efe24f875ebc8`.

The packet contains 54 small source/config/call sequences: 11 getter groups,
27 forced-emit groups and 16 sink groups. Each repeats on another fresh
Program, preserving cache state within its sequence. Per repetition there
are 190 feature calls: 77 declaration getters, 92 forced emits and 21
ordinary commands. One ordinary `noEmit` command retains H2.9 ownership.
Two additional ordinary targeted declaration API sequences retain H2.8d
ownership and have their own denominator. The existing H2.7c getter 19 and
forced 37 fixtures are unchanged.

The sources cover typed TS pairs, an anonymous exported class with private
members, JSDoc-typed JavaScript, JSON, empty and declaration-only programs,
and Unicode/BOM paths. The four declaration/declarationMap Boolean states
are tested where their branches differ. Calls cover whole/first/last targets,
cached getters, getters before and after force, noEmit/noEmitOnError/isolated
declarations, inline-map allocation and sourceRoot. Program option, syntactic,
global and semantic diagnostics are captured after each sequence, preserving
the initial cache state of the API under test.

Every call records complete diagnostics or EmitResult, raw map JSON and its
input filenames, absent versus empty arrays, callback order and exact UTF-8
bytes, BOM materialization, source files, callback data before/after mutation,
exceptions and partial writes. Ordinary commands additionally retain reported
diagnostics, status writes and exit. Actual materialized write indices are
separate from attempted callback bytes. A read-only virtual `/lib` mounts only
the pinned standard libraries; no workspace path or physical output is used.

| Branch | TypeScript 6.0.3 result |
| --- | --- |
| Normal declaration map | Callback order is map then declaration; emittedFiles order is declaration then map. |
| Forced JSON, declaration/map enabled | Only `data.d.json.ts` is written; the list also contains `data.d.json.ts.map`, while sourceMaps is `[]`. |
| Forced JSON, declaration disabled/map enabled | Declaration is written without a map callback/list entry; sourceMaps is absent. |
| Forced TS/JS, declaration disabled/map enabled | `Error: Debug Failure.`; no EmitResult. A JSON-first whole request retains its JSON write before the later TS failure. |
| Forced noEmit | Declaration/map writes continue with emitSkipped=true. |
| Ordinary noEmit command after getter | No writes, emittedFiles/sourceMaps both `[]`, emitSkipped=false. The private-member diagnostic remains reported, with exit 2. This command stays H2.9. |
| noEmitOnError early return | emitSkipped=true, emittedFiles=`[]`, sourceMaps absent. |
| Direct callback `onError` | TS5033; remaining callbacks continue and failed paths remain listed. |
| Direct callback `throw` | Exception propagates; callback attempts and already materialized writes remain recorded, with no EmitResult. |
| Compiler-host System `throw` | The real TS write wrapper retries, then reports TS5033 through onError and continues. |
| Declaration SkippedUnchanged | Callback mutates `data.skippedDtsWrite=true`; its declaration list entry is omitted and the map entry remains. |
| Map SkippedUnchanged | The map callback has no data/skip-return protocol; omitting materialization retains its map list entry. |

The skipped-write cases inject the emitter callback state used by the upstream
builder adapter; they do not execute or admit builder runtime. The two-source
sink fixtures independently fail/skip the first map or declaration and retain
the second source's behavior. Combined onError controls preserve diagnostic
ordering. Fifteen disabled-declaration Debug Failure calls and four direct
callback exceptions are retained per repetition; none is a Rust success
expectation.

At native reference commit `1f70213d4922b434345f639b441681e470c7cfc1`, a
[source comparison](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/compiler/emitter.go#L350)
shows map-before-text emitted-file accumulation, exclusion of failed writes
from that list, and the text output path passed to the map-write diagnostic.
The [forced declaration gates](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/compiler/emitter.go#L243)
also leave force outside the noEmit/declaration-error emitSkipped branches.
These are static native-source findings; this packet runs no native API test
and does not replace the 6.0.3 observations or transition the accepted version.

Reproduce the complete observations and byte-for-byte fixture check with:

```sh
taskpolicy -b nice -n 15 node scripts/observe-declaration-map-apis.mjs --check
```

Both fresh-Program repetitions, fixture reproduction, callback/BOM byte
shape checks and dependency identities pass locally. Rust tests and hosted
acceptance belong to the subsequent runtime integration packet.
