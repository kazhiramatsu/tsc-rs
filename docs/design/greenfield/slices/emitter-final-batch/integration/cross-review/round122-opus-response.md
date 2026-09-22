None of the 7 is safely closable on this train. Each refuses at one parser-owned admission check, and the only fix in each case would be a new or widened recovery predicate in `crates/syntax/src/recovery.rs`. That is exactly what may not change until the census and replay proof finishes, and it would then need its own replay evidence. Keep all 7 as they are. The TS output in this answer comes from Node probes of the vendored 6.0.3 (`/tmp/r80/bb.mjs`, `cc.mjs`); the rest is source reading. I made no edits and ran no builds or Cargo.

## 1. Why each still refuses

All 7 stop at the same place. `preflight_source` (`emitter/src/builtins.rs:~16258`) returns `ParseDiagnosticsDeferred` whenever `has_supported_emit_recovery()` is false. That calls `supports_missing_nodes` (`syntax/src/recovery.rs:~186-420`) with the most permissive profile.

**(a) Missing colon — `export const a number = …` / `a string` (2 cases)**
- **TS parse:** one `',' expected` (1005) at the `number` token. `parseDelimitedList`'s `parseExpected(Comma)` reports it without consuming a token, and the loop parses `number = "…"` as a second declaration. There is no missing node and no skipped token.
- **Native:** the event has no missing node, so it goes through the report-only path (`recovery.rs:~250`), which needs one of three things:
  1. a `TokenSkipped` action at the event start — there is none;
  2. `report_has_retained_syntax_owner` — this only covers:
     - closing-paren runs;
     - an `ExpressionStatement` ending at the report's end (the report ends at 21, but the second declaration ends at 39);
     - `report_has_declaration_list_boundary`, which needs the list to end at the report's full start (the list ends at 39, the full start is 14);
  3. a context-recovery assertion — this doesn't apply.
- **What's missing:** a predicate for "a delimiter-only report inside a delimited list, where the next list element starts at the reported token and the previous element ends at the report's full start".

**(b) `let x: = 1; export {};` (report on / off / absent, and the noCheck Program — 4 cases)**
- **TS parse:** one `Type expected` (1110). The type is `TypeReference[6,6]` wrapping a missing `Identifier[6,6]`.
- **Native:** the event itself passes: it's a parser diagnostic with a missing `Identifier` and a retained diagnostic index. The reachable-tree walk (`recovery.rs:~300-318`) then rejects it. An empty identifier is only admitted when its parent is an `AwaitExpression`, a `TypeAssertionExpression` (in the statement-gap profile), or a context-recovery missing slot.
- **What's missing:** a missing identifier whose parent is a `TypeReference` in a type annotation. This is the one missing admission in these four cases.

**(c) The emoji identifier input (1 case)**
- It produces 9 diagnostics and 19 events, including scanner `Invalid character` (1127) and 1351.
- The recovered tree contains a binding pattern with a numeric-literal property, a missing `TypeReference` under `AsExpression`, and an `EmptyStatement`.
- Several event kinds fail: scanner-origin, non-literal diagnostics, and missing identifiers under disallowed parents. This is general malformed-input admission, not something a narrow gate can close.

## 2. Could any be safely repaired now? No

**(b) is the smallest future candidate, but not for this train.**
- **Gate:** admit an empty `Identifier` whose parent is a `TypeReference` with an empty range, and that `TypeReference` is the direct `type` child of a `VariableDeclaration` or `Parameter`.
- **Blocking invariant:** admission is a parser fact that every emit consumer reads. There is no route-aware split, and adding one would be a new phase interface.
  - For JS it looks safe: the annotation is erased, and TS prints `let x = 1;`.
  - For d.ts, TS prints `export declare let x: ;` when the declaration is exported, and `export {};` otherwise.
  - The native d.ts printer's handling of that missing identifier is unverified.
- **Replay impact:** the predicate change alters admission across the whole corpus. Every file whose only recovery is this shape would be newly admitted, so the successor proof (four replays plus the selector) would have to be redone with new emit comparisons.
- **Controls it would need:**
  - positives: JS and d.ts for `let x: = 1`, `export let x: = 1`, and `function f(p: ) {}`;
  - negatives:
    - a missing type inside a type argument or union (`let x: A< > = 1`);
    - a missing identifier in expression position;
    - two missing types in one file.

**(a):** the recovered tree is well-formed; TS emits `exports.number = …` in CommonJS and `export const a, number = …` in ESM. But closing it still means a new report-ownership predicate in `recovery.rs`. The d.ts side is also non-trivial: TS prints `export declare const a: any, number = "missing colon";` and adds diagnostic 9010. That puts it outside this train.

**(c):** not repairable narrowly.

## 3. Census coverage: none of these inputs is in the census

- The roster at `census-r78-claimed-id-inventory.json.gz` has 14,329 IDs, split into compiler 7,276, conformance 6,421 and project 632. It contains no transpile IDs.
- None of the four input text hashes appears in the roster: `3d29fca5…`, `b662f37e…`, `ae920ec9…`, `251c798d…`. Nor do the h2-8c IDs or the names.
- The roster records IDs, not per-file content hashes, so a content match can't be ruled in from it either.
- In `ts-tests/tests/cases`:
  - `declarationSingleFileHasErrors.ts` exists only under `transpile/`, which the census doesn't include;
  - a regex search of `compiler/` and `conformance/` found no `let|var|const name : =` shape;
  - the literal `export const a number` and `\u{1F600}` searches returned nothing.
- So these inputs are covered only by their dedicated h2-8c fixtures. Any future predicate change must add dedicated controls, as in section 2, rather than relying on the census.

Keep all 7 as known, exact to their old refusal output, with their specific missing predicates recorded:
- (a): delimiter-only report ownership in a list;
- (b): missing identifier in a type-annotation `TypeReference`;
- (c): general scanner recovery.

Record (b) as the first candidate for a later recovery train, with the gate and controls above.
