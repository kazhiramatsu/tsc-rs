# design: LSP and incremental parsing — the door we keep open

Status: OUT of the 2XXX-first goal (phases 0-9 are batch-only), but a
planned consumer — an LSP server will eventually sit on this engine.
This doc fixes (a) the facts of tsc's incremental architecture, (b)
the design rules the batch build must follow NOW so the LSP track can
be added WITHOUT re-architecting, and (c) the future track itself.
tsc anchors verified at the 6.0.3 pin.

## 1. How tsc actually does incrementality (verified)

Three layers, and — the load-bearing fact — the CHECKER is not one
of them:

1. **Incremental reparse** — `IncrementalParser.updateSourceFile`
   (`_tsc.js` 35817): given the old SourceFile + a text change range,
   it does NOT patch the tree in place. It builds a **syntax cursor**
   over the old tree (`createSyntaxCursor` 36116), shifts positions
   of untouched regions, marks change-intersecting nodes, and runs
   the NORMAL parser — which consults the cursor: `parseSourceFile`
   takes `syntaxCursor` as a PARAMETER (29014), and the list-parsing
   machinery asks `canReuseNode` (30239) per element, splicing
   reusable old nodes instead of re-parsing them. Incrementality is
   a parser INPUT, not a parser rewrite.
2. **Program-level reuse** — `tryReuseStructureFromOldProgram`
   (123292): watch/builder reuses whole unchanged SourceFiles; a
   changed file is reparsed (via layer 1 in LS contexts, or fully).
3. **The checker is DISPOSABLE.** tsc creates a fresh TypeChecker
   per Program version; every edit produces a new Program and the LS
   re-asks it. There is NO incremental type checking. Symbols, types,
   links, caches — all rebuilt per version, made affordable by lazy
   checking (only queried files/nodes resolve).

Consequence: our one-write links discipline, the resolution stack,
and the fresh-per-program checker are ALREADY the shape tsc's own LS
runs on. The LSP door is a front-end concern only.

## 2. Rules effective NOW (batch phases 0-9)

Cheap commitments that keep §3 additive; each is either already true
or a one-line adjustment:

1. **Reserve the cursor parameter.** The parse entry is
   `parse_source_file(file_name, text, opts, cursor:
   Option<&SyntaxCursor>) -> SourceFile` from day 1; batch always
   passes `None`, and `SyntaxCursor` is an empty placeholder type
   until the L-track. (Amends impl-parser.md's entry signature.)
2. **No hidden parse state.** All parser/scanner state lives in the
   `Parser`/`Scanner` structs (already the impl-parser design) — a
   cursor-driven reparse constructs them fresh per call.
3. **Positions are pos/end only**; parents and error aggregation are
   a separate `for_each_child` pass (impl-nodes §2). Node reuse =
   position shift + re-stamp pass; nothing else encodes location.
4. **NodeIds are per-parse, never persisted** (greenfield §4.1
   already forbids cross-run id stability). The checker may not
   cache anything keyed by NodeId beyond one Program's lifetime —
   automatic today because the Checker itself is per-Program; keep
   it true (no global memo tables keyed by node/symbol ids).
5. **UTF-16 maps and line maps per file** (impl-nodes §3) — LSP
   positions are UTF-16 line/character; the conversion
   infrastructure is shared with the diagnostic boundary.
6. **Idempotence/determinism invariants** (m0 stage 0.8) double as
   LSP correctness: "same program text ⇒ byte-identical diagnostics"
   is exactly what makes didChange→publishDiagnostics stable.

Explicitly NOT reserved: in-place tree mutation, generation-tagged
arenas, checker-state invalidation. tsc needs none of them; neither
do we.

## 3. The future L-track (post phase 9, or parallel after M7)

- **L1 — incremental reparse**: port `IncrementalParser`
  (updateSourceFile 35817: change-range normalization, position
  shifting with `aggressiveChecks` assertions) + `createSyntaxCursor`
  (36116) + the `canReuseNode` consultation (30239) inside
  `parse_list`/`parse_delimited_list` (impl-parser's list engine —
  the hook point already exists in its loop head). Arena strategy:
  reused old nodes are COPIED into the new parse's arena with
  shifted positions (copy-on-reuse), preserving rule 4; the copy is
  O(reused nodes) but allocation-cheap, and tsc's own reuse is
  node-object reuse only because JS has a GC — copying is the
  arena-model equivalent. Acceptance: a differential harness that
  applies random edit scripts to corpus fixtures and asserts
  incremental-parse tree == fresh-parse tree (tsc's own
  aggressiveChecks pattern), plus diagnostics byte-equality.
- **L2 — program/SourceFile reuse**: cache parsed SourceFiles keyed
  by `(text hash, script_kind, language_version, language_variant)`;
  rebuild Program cheaply per version (`tryReuseStructureFromOldProgram`
  semantics: unchanged files reused wholesale, module-resolution
  re-validated). The lib bind/check snapshot (greenfield §9) already
  covers the expensive half.
- **L3 — the LSP server**: a new `crates/lsp` consumer. Fresh
  Checker per Program version (tsc's model, §1.3); lazy per-file
  diagnostics (the driver already checks files independently);
  queries (hover/definition/completion) reuse the checker's symbol/
  type machinery — display text quality rides on the T2/T3 work the
  conformance ratchet drives anyway. Cancellation: the only NEW
  cross-cutting hook — tsc threads a throttled cancellation token
  through checkSourceElement/resolveCall-scale loops; reserve it as
  a no-op `check_cancelled(&self)` on the Checker called from the
  driver loop (add when L3 starts; listed here so the driver's shape
  anticipates it).

## 4. Relationship to the batch goal

Nothing in the L-track changes observable batch behavior, and no
batch phase may take a dependency on L-track machinery. The
protection is the same ratchet that protects everything else: L1's
differential harness feeds the same invariant suite, and a
regression in batch conformance while landing L-work is a stop
condition like any other.
