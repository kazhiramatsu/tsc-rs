# H2.7d original corpus comparison draft

## Current result and close addendum

The third integrated runtime comparison passed all 283 original D/D+E cases,
twice: 280 D-only plus the same three shared map cases. Complete source/library
order, callback bytes and metadata, diagnostics, maps, command reporting and
absent-versus-empty values remain the frozen expectations. The accepted runtime
candidate is `7db1e4d595d5f5ca25c569a80190ecdfddd8a8c9`; the final printed-parent
correction rerun also passes all 283 cases twice (304.80s). Its local regression
evidence is recorded in the [shared close](h2-7de-ca.md). Hosted run 34123778139 passes on the same candidate, delivered through PR #511
as `ad84a7e6e46bd7c5ccd9ac1eb78580dab18151a2` on 2026-09-07. D coverage 283 and E coverage 11 overlap by three; ordered
admission deltas are D 280 / E 11, joint 291. The 32 D intersections with later
owners retain their original inputs and owner boundaries.

The first-run 178/102, second-run 274/6 and third-result-pending statements below
are retained as packet history. They are superseded by the measured third result
above; no old failure was reclassified through an allowlist or changed expected
tuple. Separate old-route validation now measures 160 exact + 17 typed refusals,
twice, and full H2.6c acceptance is 481 exact / 158 known / 4 deferred. These
historical-route outcomes do not add a second 160 to the joint admission count.

## Initial comparison record (historical)

This standalone compiler test joins the unchanged candidate census, original
inputs and complete TS 6.0.3 observations. The first complete production run
compared all 280 D-only inputs twice: 178 exact and 102 failing cases (242.70s).
The second run incorporated option, JSON and input-projection fixes: 274 exact
and six failing D-only cases, each twice (243.06s). The third candidate fixes
those six declaration/computed-field cases and includes the original three D/E
intersections, for 283 complete comparisons per repetition. Its result is pending.
No candidate is admitted by adding this test. All D-only failures remain failures;
there is no allowlist, diagnostic replacement or input reduction.

| Frozen owner set | Original IDs | Comparison in this packet | H2.6c overlap |
| --- | ---: | --- | ---: |
| D only | 280 | Ordinary production command tuple, two fresh Programs per ID | 154 |
| D + E | 3 | Same original production command comparison, including complete map bytes and metadata | 1 |
| D + H2.8a | 23 | Reference: rootDir 4, outDir 19; emitter option/profile entrance | 16 |
| D + H2.8b | 5 | Reference: importHelpers 2, incremental 1, composite 1, insensitive host 1 | 1 |
| D + H2.9 | 4 | noEmit loader refusal 3, each twice; malformed JS source reference 1 | 0 |
| E only | 8 | Existing independent E original comparator | 5 |
| E + H2.8c | 2 | Transpile references, five original units each; no whole-Program oracle | 0 |

The union remains 325: D 315 + E 13 - compound 3. The all-owner historical overlap
is 177. The third candidate compares 283 D/D+E cases: 155 repeat historical
H2.6c IDs and 128 are outside that historical set. These are distinct input IDs, not additional
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
The mount/write/config/alias precedence mirrors the frozen observer. A test host
adapter trims trailing separators only for directory-existence queries, matching
the observer's canonical directory overlay while preserving MemoryCompilerHost's
exact file keys and directory enumeration. This handles drive roots (`A:/`)
and directory imports (`/a/src/`) without substituting source order. Config
path remains attached while the original effective options and roots drive
`load_emitting_program`. The frozen observer spreads parsed options, discarding
TypeScript's non-enumerable `configFile` AST while preserving `configFilePath`.
The comparator mirrors that distinction; config discovery does not replace roots.
Resolved project mapRoot/sourceRoot values remain the newer census input contract;
historical H2.6c observations are not reused as expectations for those paths.

All 41 effective option keys occurring across the 315 D rows are handled explicitly.
TS's serialized `lib.*.d.ts` names are checked against the existing Rust catalog's
logical keys. The two `traceResolution:true` rows retain the observer's no-op trace
reporting contract; this host reporting flag has no Rust CompilerOptions field.
The 107 named standard libraries retain their collective SHA256 check
(`basename + NUL + complete bytes + NUL`, sorted basenames). The independently
pinned `lib.d.ts` wrapper is also mounted under `/lib`, for 108 available files.
The frozen observer loads that wrapper in 21 D-only observations. Loaded
library order and loaded source text are obtained from the real PreparedProgram.

## Production comparison and remaining entrances

`ProgramSession::emit_command_for_harness` uses the real `emit_for_cli` and
`EmitCommandOutcome` reporting producer. It was added in root's `f1d39e7a` and
corrected by `1d1ae2bd` (`d621cb63` / `a47f64e8` in integration). The comparison
does not initialize a getter session, inject a common directory/source order,
provide a mock resolver, concatenate per-source output, or infer status/exit.

Every eligible D/D+E repetition compares source and standard-library order, every callback
path/order/byte/BOM/sourceFiles/metadata field, reported diagnostics, emitSkipped,
emit diagnostics, emittedFiles, sourceMaps (including canonical JSON bytes), status
writes and exit code. Absent and empty values remain distinct. Failures are
aggregated per original ID and repetition, with JSON field or decoded output byte
positions; unexpected runtime errors report partial callback paths and hashes.
Setting `TSC_RS_H2_7D_FAILURE_DIR` writes complete failed observations for diagnosis;
it never filters cases or changes comparisons. The next fresh Program still runs
after a failed repetition. This is the complete
frozen Program/command tuple, not a new filesystem/CLI subprocess claim. Relocating
these originals into generated configs would change absolute paths, config
diagnostic locations or project lib defaults, so such CLI controls require their
own explicitly separate evidence.

The 32 later-owner originals remain references. noEmit is tested at
`load_emitting_program`'s `ValidateOptions` entrance. rootDir/outDir and
incremental/composite have ordinary emitter profile gates; malformed JS reaches
the `ParseDiagnosticsDeferred`/H2.9 source boundary. Bundle maps require the D/E
connection and now compare as three full original observations. ImportHelpers and case-insensitive host have no independent ordinary
refusal in the pre-connection tree: root must review their bundle guards when the
blanket outFile refusal is removed. This draft neither converts those three rows
to successes nor claims their future typed refusal has been measured.

The primary 280 includes JS source maps (152 sourceMap rows and four inline map
rows), output collisions, invalid option combinations and TS diagnostics such as
6082, 6504, 8013 and 8037. No diagnostic is suppressed to make a visitor/printing
facet look like a complete Program success. Getter/forced/cache and sink-failure
integration, CLI subprocesses, canary/global profile and formal adoption remain
separately owned. Existing E tests and all TS fixtures are unchanged.

The runtime canary candidate also checks one D request for each nonempty outFile
and one E request for declarationMap=true at every validated ordinary call.
These assertions do not add fields to the frozen TypeScript tuples. Public
getters/forced emits, cache/empty/exception cases and the older-profile rejection
boundary have separate focused controls; formal adoption is still pending.
