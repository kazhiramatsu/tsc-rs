# EF8 — API-owned known rows: boundary evidence and counterexample search

Read-only evidence for [README §3 EF8](../README.md#ef8--出力経路の仕上げとa-close候補):
"通常のfresh commandの反復と、同じProgram/TransformationResultの公開API再利用は別".
Sources cited are at start SHA `c35e00ccb`. No test was executed.

## 1. Disposed print — typed known 2

| Field | Value |
| --- | --- |
| Row ids | `decorator-binding-direct/lifecycle/dispose/numbered#direct#after_dispose`, `decorator-binding-direct/lifecycle/dispose/file-level#direct#after_dispose` |
| Fixture | `crates/emitter/tests/fixtures/decorator-binding-direct.json`, `route: direct-generated-name-controls`, 146 cases (groups synthetic 48 / global 22 / lifecycle 76); the two rows are `group: lifecycle, op: dispose, kind: numbered|file-level` |
| Test | `crates/emitter/tests/decorator_binding_contract.rs` `KNOWN_DIVERGENCES` (lines 62–71); the assertion at lines 1016–1027 requires `Err(PrinterError::Transform(TransformError::InvalidLifecycle {..}))` on the second print |
| Recorded reason | "tsc keeps the `emitNode` of synthetic nodes after `TransformationResult.dispose()`, so a fresh printer prints them again; Rust refuses printing a disposed transformation with a typed `InvalidLifecycle` (session model, as C01's lifetime rows record)" |
| Hosted | witness job `decorator-binding` (unfiltered emitter target `decorator_binding_contract`, inventory v23) |

Why only the public API reaches it: the row's sequence is `transform_nodes(...)`
→ `printer.print(&mut result, …)` → `result.dispose()` → `printer.print(...)`
again on the *same* `TransformationResult`. The ordinary emit entry
(`E-ENTRY`: `ProgramSession::{run,emit}` → `emit_files_with_activity`) owns its
transformation for the duration of one command and never exposes the result to
a caller; the architecture rows `E-DECL-SESSION` / `E-DECL-FORCE` describe every
declaration entry as a consuming session call as well. The compiler-side
lifetime rows are C01's `literal-update-pipeline.json` (`literal_update_pipeline_contract`,
hosted), which record the same InvalidLifecycle refusal only under the
`#direct` route. Every complete-command control repeats with a fresh Program
(`repetitions: 2`, "Each repeats on a fresh Program and empty in-memory sink",
[h2-7de-bundle-sinks.md](../../h2-7de-bundle-sinks.md)); no fixture in the
scanned set carries a second print request on a reused result.

Counterexample search (source reading, time-boxed): an ordinary command would
need a second `print` on a disposed result. The only re-print of one
transformation inside ordinary emit is the JS + map pair (one print with a
recorder) and the declaration pair, both produced before the first sink call
(`E-OUTPUT-SCRIPT`); none of them disposes between prints. **No counterexample
found**; the row stays API-owned (the C01/API owner). Adjacent API-owned
lifetime rows that must not be counted as ordinary-emit failures either:
`decorator_binding_contract.rs`'s "failure carry" note (tsc's printer keeps
`generatedNames`/`tempFlags` after a hook throws and continues on the *same
printer*; Rust finalizes names per print) — a printer-reuse API behaviour with
no `#[test]` row of its own in the scanned fixtures.

## 2. Custom-transformer shared-node SUPER — known 4

| Field | Value |
| --- | --- |
| Row ids | `decorator-super-direct/es2015/set/shared-node-used-twice`, `decorator-super-direct/es2015/define/shared-node-used-twice`, `decorator-super-direct/es2022/set/shared-node-used-twice`, `decorator-super-direct/es2022/define/shared-node-used-twice` |
| Fixture | `crates/emitter/tests/fixtures/decorator-super-direct.json`, `route: direct-transform-custom-before`, 32 cases (8 shapes × {es2015, es2022} × {set, define}); options `module: 200 (preserve)`, `outDir: /project/out`, `newLine: 1`, no maps |
| Observer | `scripts/observe-decorator-super-direct.mjs` injects the shape with a `customTransformers.before` transformer on the parsed source and records the emitted JavaScript of the complete emit |
| Test | `crates/emitter/tests/decorator_super_direct_contract.rs`; header lines 5–8: "No Program input produces these shapes (no transform ahead of transformESDecorators emits a CommaListExpression or shares an expression node)"; lines 385–390: the `shared-node-used-twice` shape is "a recorded divergence (no credit)": tsc lowers a node shared by two required-value contexts twice (`var _a, _b;`), the port memoizes the required-value lowering per node id and prints the first lowering twice; the port's text is pinned as exactly that rewrite |
| Hosted | witness job `direct` (filtered emitter target `decorator_super_direct_contract`), reported as "direct 28 exact / 4 known" in docs/witness-testing.md |

Why only a custom transformer reaches it: the shape needs one expression
*object* to be a child of two parents when `transformESDecorators` lowers
`super` property assignments. Parse trees never share nodes, and the port's
detached arena only appends synthetic nodes (`E-ARENA`: "Parsed trees remain
immutable; the detached arena appends synthetic nodes"). The transforms that run
ahead of ESDecorators at 6.0.3 are `transformTypeScript` and
`transformLegacyDecorators`; both build their synthesized references with fresh
factory nodes (`getLocalName`/`createAssignment`), and `visitEachChild` reuse
returns the original node into the *same* position only. The 28 non-divergent
rows of the same fixture (comma-list and the three other shared-node orders)
are exact, so the boundary is precisely "the same node used twice in two
required-value contexts".

Counterexample search: I found no first-party transform in the vendored
`_tsc.js` reading that reuses an expression node under two parents before
`transformESDecorators`; the fixture's own 4 `shared-node-discarded-then-used`
/ `used-then-discarded` / `update-…` shapes show tsc and the port agree
whenever the node is required-value in at most one context. **No ordinary
Program counterexample identified.** Residual risk to record in A-CLOSE §D
rather than close silently: this is a reading result, not an exhaustive trace
of every `transformTypeScript`/`transformLegacyDecorators` return path; the
implementation half of EF8 (integrator) should confirm with one probe per
transform that returns a visited expression into two positions (parameter
properties, enum/namespace lowering, legacy decorator `__decorate` argument
lists) and, if any reaches the SUPER lowering, move that row to README §6 C.

## 3. Adjacent API-owned rows (not among the 2 + 4, not ordinary-emit failures)

| Route | Rows | Where | Owner |
| --- | --- | --- | --- |
| Declaration-map stateful API (`calls[]`, `transport`, `sink_rules`) | 54 | `crates/compiler/tests/fixtures/declaration-map-apis.json` (`h2_7e_declaration_map_apis`, hosted `declaration-maps/declaration-map-apis`) | H2.7e API |
| Transpile routes | 287 inputs, 22 known-native + known-open | `crates/compiler/tests/fixtures/h2_8c_transpile/*` (`transpile_routes_contract`, hosted `transpile-routes`) | H2.8c |
| Printer direct targets (`printer_failure_contract`, `literal_update_contract`, …) | direct | emitter fixtures, hosted `printer` job | printer/API |
| Typed refusals in compiler controls (`emit_refused: true`) | 280 complete commands (147 hosted) | `output-directories` 9, `output-roots` 3, `output-root-format` 24, `package-output-inputs` 2, 6c/7b profiles | these are ordinary-command refusals and **do** count for ordinary emit; listed here only to separate them from the API rows |

## 4. What this file does not claim

No runtime result. The 2 + 4 rows remain frozen in their fixtures; retiring
either needs a fresh TypeScript observation and the API owner's design (session
model for dispose; memo policy for shared nodes), not an emitter-final commit.
