# H2.7d original corpus comparison draft

This standalone compiler test joins the unchanged candidate census, original
inputs and complete TS 6.0.3 observations. It has been compiled and checked against the artifact shapes and source hashes.
Execution through the newly connected bundle pipeline remains pending.
No candidate is admitted by adding this test. All D-only failures remain failures;
there is no allowlist, diagnostic replacement or input reduction.

| Frozen owner set | Original IDs | Comparison in this packet | H2.6c overlap |
| --- | ---: | --- | ---: |
| D only | 280 | Ordinary production command tuple, two fresh Programs per ID | 154 |
| D + E | 3 | Original inputs/oracles retained; bundle maps remain a separate integration boundary | 1 |
| D + H2.8a | 23 | Reference: rootDir 4, outDir 19; emitter option/profile entrance | 16 |
| D + H2.8b | 5 | Reference: importHelpers 2, incremental 1, composite 1, insensitive host 1 | 1 |
| D + H2.9 | 4 | noEmit loader refusal 3, each twice; malformed JS source reference 1 | 0 |
| E only | 8 | Existing independent E original comparator | 5 |
| E + H2.8c | 2 | Transpile references, five original units each; no whole-Program oracle | 0 |

The union remains 325: D 315 + E 13 - compound 3. The all-owner historical overlap
is 177. If all 280 D-only comparisons pass, 154 repeat historical H2.6c IDs and 126
are outside that historical set. These are distinct input IDs, not additional
runtime activity/admission counts. Actual local exact counts are printed after
both repetitions; a successful reference check never increments that count.

## Input and observation identity

The test pins the complete files below, joins each original case ID and source
identity, and verifies each D original source byte length/SHA256 under the pinned
`ts-tests/tests/cases` tree. The input file SHA fixes JSON member order and every
virtual file byte; the matching per-row JS `JSON.stringify` identity is joined
between the frozen census and oracle. Rust's sorted JSON serializer is not used
to invent a replacement input hash.

| Artifact | SHA256 |
| --- | --- |
| `ratchets/h2-7de-candidates.v1.json` | `1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d` |
| `ratchets/h2-7de-candidate-inputs.v1.json` | `f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a` |
| `ratchets/h2-7de-observations.v1.json` | `1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2` |

D-only includes 156 project inputs using the complete shared 233-file mount and
the original `lib.es5.d.ts` default override, 9 configs and one file-symlink input.
Two compiler originals also retain drive-rooted `A:/`, `B:/` or `c:/` paths on
the POSIX test host; filesystem-native absolute-path checks do not replace TS paths.
The mount/write/config/alias precedence mirrors the frozen observer. Config
syntax/location provenance remains attached while the original effective options
and roots drive `load_emitting_program`. Config discovery does not replace roots.
Resolved project mapRoot/sourceRoot values remain the newer census input contract;
historical H2.6c observations are not reused as expectations for those paths.

All 41 effective option keys occurring across the 315 D rows are handled explicitly.
TS's serialized `lib.*.d.ts` names are checked against the existing Rust catalog's
logical keys. The two `traceResolution:true` rows retain the observer's no-op trace
reporting contract; this host reporting flag has no Rust CompilerOptions field.
All 107 pinned standard library files are mounted under `/lib` and collectively
SHA256 checked (`basename + NUL + complete bytes + NUL`, sorted basenames). Loaded
library order and loaded source text are obtained from the real PreparedProgram.

## Production comparison and remaining entrances

`ProgramSession::emit_command_for_harness` uses the real `emit_for_cli` and
`EmitCommandOutcome` reporting producer. It was added in root's `f1d39e7a` and
corrected by `1d1ae2bd` (`d621cb63` / `a47f64e8` in integration). The comparison
does not initialize a getter session, inject a common directory/source order,
provide a mock resolver, concatenate per-source output, or infer status/exit.

Every D-only repetition compares source and standard-library order, every callback
path/order/byte/BOM/sourceFiles/metadata field, reported diagnostics, emitSkipped,
emit diagnostics, emittedFiles, sourceMaps (including canonical JSON bytes), status
writes and exit code. Absent and empty values remain distinct. Failures are
aggregated per original ID and repetition, with JSON field or decoded output byte
positions; unexpected runtime errors report partial callback paths and hashes.
The next fresh Program still runs after a failed repetition. This is the complete
frozen Program/command tuple, not a new filesystem/CLI subprocess claim. Relocating
these originals into generated configs would change absolute paths, config
diagnostic locations or project lib defaults, so such CLI controls require their
own explicitly separate evidence.

The 35 later-owner originals remain references. noEmit is tested at
`load_emitting_program`'s `ValidateOptions` entrance. rootDir/outDir and
incremental/composite have ordinary emitter profile gates; malformed JS reaches
the `ParseDiagnosticsDeferred`/H2.9 source boundary. Bundle maps require the D/E
connection. ImportHelpers and case-insensitive host have no independent ordinary
refusal in the pre-connection tree: root must review their bundle guards when the
blanket outFile refusal is removed. This draft neither converts those three rows
to successes nor claims their future typed refusal has been measured.

The primary 280 includes JS source maps (152 sourceMap rows and four inline map
rows), output collisions, invalid option combinations and TS diagnostics such as
6082, 6504, 8013 and 8037. No diagnostic is suppressed to make a visitor/printing
facet look like a complete Program success. Getter/forced/cache and sink-failure
integration, CLI subprocesses, canary/global profile and formal adoption remain
separately owned. Existing E tests and all TS fixtures are unchanged.
