# M0: workspace, codegen, harness, oracle — steps

Parent design: greenfield.md §2 (layout), §3 (codegen), §6
(diagnostics), §7 (harness/oracle/goldens/tiers/invariants), §8
(ledger). Everything in M0 is infrastructure: no TypeScript semantics
are implemented, but everything later measures against what M0 builds,
so its schemas are contracts — change them only by doc edit first.

Gate: the oracle side of the goldens exists for the full corpus, and
every xtask runs green against an engine that returns zero
diagnostics.

## Stage 0.1: workspace scaffold [M]

Create the `tsrs2/` cargo workspace with the greenfield §2 crates:

```toml
# Cargo.toml
[workspace]
members = ["crates/syntax", "crates/binder", "crates/types",
           "crates/checker", "crates/diags", "crates/harness",
           "crates/oracle", "crates/conformance", "crates/fuzz",
           "crates/xtask"]
resolver = "2"
```

Dependency direction (enforce in each Cargo.toml; a violation is a
stop condition): `syntax ← binder ← checker`, `types ← checker`,
everything ← `diags`. `harness`/`conformance`/`fuzz` depend only on
the checker's public `check_program` API; nothing depends on `oracle`
outside `#[cfg(test)]` / xtask.

Add the determinism lint from day 1: workspace-level clippy config
denying `std::collections::HashMap` iteration in `types`, `binder`,
`checker` (use IndexMap or sort-before-iterate; core-interfaces §2 —
member-table order is observable).

Commit: `m0 0.1: workspace scaffold`.

## Stage 0.2: vendor the pin [M]

Copy the pinned TypeScript 6.0.3 npm package's `lib/` into
`vendor/typescript-6.0.3/lib/`: `_tsc.js` (the implementation being
ported), `typescript.js` (used by oracle tooling such as the scanner
dump), `typescript.d.ts` (public interface reference),
`diagnosticMessages.generated.json` or the repo's
`diagnosticMessages.json`, and all 108 `lib.*.d.ts` files (the layered
standard libraries — the design uses them VERBATIM; there is no
curated single-file lib, greenfield §7.1).

Record `sha256sum vendor/typescript-6.0.3/lib/_tsc.js` in
`vendor/PIN.md` together with the npm version string from
`package.json` ("6.0.3"). Every ledger hash (§8) is computed against
this tree.

Also vendor the conformance corpus the same way the parent repo does
(`ts-tests/tests/cases/conformance/**`, ~5,900 fixtures).

Commit: `m0 0.2: vendor tsc 6.0.3 + corpus`.

## Stage 0.3: xtask codegen — enums [M]

Two extraction modes, both from `vendor/typescript-6.0.3/lib/_tsc.js`.
NEVER hand-write a value (see the README's TypeFlags renumbering
warning — memory of 5.x values is wrong for 6.0).

(a) Runtime IIFE tables. These enums exist as
`var <Name> = /* @__PURE__ */ ((<Name>N) => { ... })` blocks whose
member lines match the regex
`^\s*<Var>\[<Var>\["(\w+)"\] = (-?\d+)\] = "(\w+)";`.
Verified table locations at the 6.0.3 pin (re-grep on re-vendor):

| Enum | _tsc.js line |
|---|---|
| `SyntaxKind` | 2919 |
| `NodeFlags` | 3318 |
| `ModifierFlags` | 3363 |
| `RelationComparisonResult` | 3404 |
| `FlowFlags` | 3429 |
| `SymbolFlags` | 3461 (includes the `*Excludes` merge masks — the binder consumes these) |
| `TypeFlags` | 3558 |
| `ObjectFlags` | 3633 |
| `SignatureFlags` | 3680 |
| `DiagnosticCategory` | 3694 |
| `ModuleKind` | 3714 |
| `TypeFacts` | 46297 |
| `CheckMode` | 46386 |

(b) Const-inlined enums. `Ternary`, `CheckFlags`, `ElementFlags`,
`InferencePriority`, `IntersectionState`, `UnionReduction`,
`ContextFlags`, `TokenFlags`, `InternalSymbolName`, `TypeMapKind`,
`AccessFlags`, `RecursionFlags`, `ExpandingFlags`, `InferenceFlags`,
`ParsingContext`, `ScriptTarget`, `CharacterCodes` have NO runtime
table — esbuild inlined them as literals annotated with block
comments: `entry & 1 /* Succeeded */`, `priority & 128 /* ReturnType */`.
Extract them by mining every `(-?\d+) /* <Member> */` annotation in
`_tsc.js`, grouping by member name, and REQUIRING consistency: a
member name annotated with two different literals is an extraction
error (two enums share the member name) — disambiguate by hand into
the generator's config, listing which function ranges belong to which
enum. Generate a verifier that re-scans all annotations against the
generated tables so re-vendors catch drift.

Output: `crates/types/src/flags.rs` + `crates/syntax/src/kind.rs`
(committed, marked `@generated`, each constant doc-commented with the
tsc name).

Acceptance pins (unit tests in the generated crates, values verified
against the 6.0.3 source — do not edit them to make them pass):

```rust
assert_eq!(SyntaxKind::Identifier as u16, 80);
assert_eq!(TypeFlags::STRING_LITERAL.bits(), 1024);   // 6.0 renumbering!
assert_eq!(FlowFlags::TRUE_CONDITION.bits(), 32);
```

Commit: `m0 0.3: enum codegen from vendored tsc`.

## Stage 0.4: xtask codegen — diagnostic messages [M]

Generate `crates/diags/src/gen.rs` from the vendored
`diagnosticMessages.json`: one `pub static <Name>: DiagnosticMessage`
per entry carrying code, category, text template, and the
`reportsUnnecessary`/`reportsDeprecated`/`elidedInCompatabilityPyramid`
bits (greenfield §6). Name transformation: replace every non-alnum run
with `_`, trim edge underscores; prefix a leading digit with `_`.
Also generate `ALL_BY_CODE: &[(u32, &DiagnosticMessage)]`.

`Diagnostic`/`MessageChain`/`RelatedInfo` structs per
core-interfaces §7, including the final `compareDiagnostics` sort
(file, start, length, code, message text — `_tsc.js` near 17842) and
adjacent-duplicate dedup.

Commit: `m0 0.4: diagnostic message codegen + Diagnostic model`.

## Stage 0.5: harness — fixture expansion to program.json [M]

`crates/harness` owns ALL directive parsing; node never parses
directives and Rust never re-parses them downstream (greenfield §7.1).

- Parse the leading comment block for `// @name: value` directives.
  Strip one leading BOM BEFORE directive parsing (the parent repo's
  BOM-poisoning incident is the reason this sentence exists).
- Multi-file fixtures split on `// @filename:`.
- MATRIX options (`target`, `module`, `lib`, and any comma-separated
  value on a matrix-capable option): expand to one program.json per
  matrix point with a `matrixKey`. No option is "unsupported" — a
  directive the harness does not know is a hard error listing the
  fixture, so gaps surface immediately instead of as silent skips.
- Lib resolution: map `target`/`lib` to the exact lib file layering
  using the vendored tsc's own tables — port `libEntries`/`libMap`
  (`_tsc.js` 36426/36542) and `getDefaultLibFileName` (11255). The
  files come from the vendored `lib/` (stage 0.2).
- Emit the program.json schema VERBATIM from greenfield §7.1
  (`schema: 1`; files as base64; options normalized to tsc option
  names, strict-family NOT pre-expanded — expansion is engine
  behavior, not harness behavior).

CLI: `cargo xtask expand <fixture.ts> --out-dir <dir>`.

Verification: expand 20 hand-picked fixtures covering BOM, CRLF,
multi-file, and a `target: es5,es6` matrix; snapshot the JSON outputs
as unit tests.

Commit: `m0 0.5: fixture expansion + program.json`.

## Stage 0.6: oracle driver + process pool [M]

`oracle/driver.mjs` (node, runs the vendored `typescript.js`):

- stdin/stdout JSONL: one request line `{id, programJsonPath}` → one
  response line `{id, diagnostics}` per greenfield §7.2's
  diagnostics.json schema.
- Build an in-memory `CompilerHost` from program.json (files +
  resolved libs), `ts.createProgram`, then collect per file:
  `getSyntacticDiagnostics` + `getSemanticDiagnostics` +
  `getSuggestionDiagnostics`. **Never call `emit()`** — the emit-free
  contract is what deletes the suggestion-band emit-marking artifact
  class (greenfield §6), and the checker will implement the
  emit-marking rules itself in M7.
- Serialize chains and relatedInformation fully (T3 fidelity), with
  UTF-16 offsets as tsc reports them.

Rust `crates/oracle`: a typed client + a persistent pool of N driver
processes (default N = min(4, cores/2); the pool exists so nothing
ever spawns per-fixture process storms). API:
`oracle_diags(program_json: &Path) -> Vec<OracleDiag>`.

Verification: run the pool over 100 fixtures twice; identical output
both runs; kill -9 one worker mid-run and confirm the pool respawns
and completes.

Commit: `m0 0.6: oracle driver + process pool`.

## Stage 0.7: goldens + T0 classifier + ratchet [M]

- Golden format per greenfield §7.3
  (`goldens/<fixture-relpath>.json.zst`, both sides at T3 fidelity +
  T4 CLI hashes; the tsrs2 side is EMPTY at M0).
- `cargo xtask oracle-refresh` fills the oracle side for the full
  corpus (this is the M0 gate's long pole; budget one overnight run,
  then it is incremental).
- T0 comparator: set equality of (file, code, line, col) per
  greenfield §7.4; `cargo xtask conformance` prints per-tier rates
  and writes the mismatch JSON the mining loop consumes (same shape
  as the parent repo's FCC snapshot: top one-sided codes + per-fixture
  mismatch lists).
- `ratchet.toml` per §7.4; CI target `cargo xtask ci` = build + test +
  conformance + ratchet check.
- Classifier gate per §7.5: NEW one-sided diagnostics vs golden
  hard-fail. There is NO ignore list (full libs make LIBCODES-style
  exemptions unnecessary — every code counts).

Commit: `m0 0.7: goldens + T0 classifier + ratchet`.

## Stage 0.8: invariant runner + ledger tool [M]

- `cargo xtask invariants --suite all` skeleton with the five suites
  from greenfield §7.6 (prefix-determinism, idempotence,
  jobs-independence, encodings, matrix-independence) running against
  the empty engine (all trivially green now; they activate for real
  at M1/M5).
- `cargo xtask ledger check` / `ledger coverage` per §8: parse
  `/// tsc-port:` doc comments, verify `tsc-hash` against the vendored
  slice, list unported pub fns in hot modules.

Final M0 gate:

```sh
cargo xtask ci                       # green end to end
cargo xtask conformance | tail -3    # T0 = 0.00% (engine empty), corpus fully classified
ls goldens | wc -l                   # full corpus present
```

Commit: `m0 0.8: invariants + ledger tooling`.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| A flag constant disagrees with a hand-checked _tsc.js use site | Extraction grouped two same-named members from different enums | Add the function-range disambiguation to the codegen config; the verifier must catch this class |
| Oracle diagnostics differ between runs | Driver kept state across requests (module caches, host reuse) | One fresh Program per request; the PROCESS persists, the program does not |
| Matrix fixture produces one program.json | The comma split ran only for known-matrix options | Matrix capability is per option, declared in one table; unknown directives must hard-error |
| Golden refresh differs from spot oracle runs | Offset encoding drift (UTF-16 vs bytes) | Offsets are tsc's UTF-16 code units end to end in oracle-side artifacts; engine-side conversion happens at comparison time (core-interfaces §7) |
