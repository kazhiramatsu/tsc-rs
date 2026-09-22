Both bounded candidates are technically sound to test in an isolated tree once the current proof finishes. Nothing below claims either is safe; the risks that remain are consumer behaviours that only measurement can settle, not scheduling. I also correct the census-scope claim from round 122. Node probes of the vendored 6.0.3 are in `/tmp/r80/dd.mjs`; I made no edits or builds.

## 1. Missing `TypeReference` in a `VariableDeclaration` type slot

**The shape is a sound candidate.** Today, `supports_missing_nodes` already pairs each zero-width `Identifier` 1:1 with a recorded parser missing-node event, keyed by position (the `missing_positions` map) and diagnostic index. The only rejection is the parent-kind check (`recovery.rs:~300-318`).

**Proposed conjunct.** Keep all existing event, index and position checks, and add this parent case: the parent is a `TypeReference` that
- has zero width;
- has no type arguments;
- names this identifier;
- occupies the `type` field of a `VariableDeclaration`;
- has a `VariableDeclarationList` parent.

**Two missing types in one list** (`let a: , b: = 2`) should be a **positive**. The invariant is that each zero-width identifier owns exactly one recorded event at its own position, and each lies in a distinct declaration's type slot. The existing position map already rejects a shared or duplicate owner, so no count rule is needed. TS emits `export let a, b = 2;` and `export declare let a: , b: ;`, with two separate 1110 diagnostics.

**Measured TS reference** (ES2020 ESM, `declaration`/`declarationMap`/`sourceMap`, checked program):

| Source | JS | d.ts | Diagnostics |
|---|---|---|---|
| `export let x: = 1;` | `export let x = 1;` | `export declare let x: ;` | only 1110 |
| `let x: = 1; export {};` | `let x = 1;` | `export {};` | only 1110 |
| `export let y: ;` | `export let y;` | `export declare let y: ;` | only 1110 |
| `export let x: /*c*/ = 1;` | `export let x = 1;` (comment **dropped**) | `export declare let x: ;` | 1110 at **20**, after the comment |
| `export let a: , b: = 2;` | `export let a, b = 2;` | `export declare let a: , b: ;` | 1110 ×2 |

The d.ts maps contain segments for the empty type, including a backward-column delta such as `AAAD`. The checked program adds no semantic diagnostic for the empty name.

**Downstream behaviour the experiment must decide:**
1. **Parser facts.** Native must produce the same zero-width `TypeReference` and `Identifier` at the post-trivia position (20 in the comment case), with one paired event.
2. **JS erasure.** Native attaches the erased zero-width type as `type_node` metadata on the name.
   - The r104/r113/r116 type-comment phases must do nothing on the empty range. I expect `has_nonempty_extent` to prevent them, but that's unmeasured.
   - The `=` anchor sits at the type end (20), so `/*c*/` must be dropped exactly as TS drops it.
3. **d.ts.**
   - The declaration transformer must reuse the explicit empty `TypeReference`. No inferred-type serialization should happen, because an annotation is present.
   - Any entity-name visibility or accessibility query on `""` must neither refuse nor add a diagnostic.
   - The printer must emit `x: ;`.
4. **declarationMap.** The Before/After segments for the zero-width type, including the backward column, must match. This is the highest-risk point.
5. **Checker.**
   - A checked program must report only 1110: no 2304 for `""`, no panic, and the type resolves to the error type.
   - noCheck and transpile go through the path with no checker resolver.
6. **Comments and maps.** JS source-map neighbours that include comments.

**Controls.**
- Positives: the rows above, both exported and not, initialized and not, run through ordinary and transpile routes, checked and noCheck, with JS, d.ts and both maps.
- Negatives, which must still refuse: type-argument, union, nested and qualified slots, `Parameter`, `PropertyDeclaration`, and expression-position missing identifiers.

## 2. Missing comma between two retained declarations

From the present facts a narrow and checkable invariant exists. The report must be all of the following:
- a `Parser` event with no missing node, a retained diagnostic index and `length > 0`;
- a current-token report (`is_current_token_report`: its start is the trivia-skipped full start);
- a report with **no** `TokenSkipped` action anywhere at or after its full start within the list;
- located in exactly one reachable `VariableDeclarationList` under a `VariableStatement`, with consecutive declarations D_i and D_{i+1} such that:
  - `D_i.end == event.full_start == D_{i+1}.pos`, so only trivia separates them and no token is outside the tree;
  - the name's trivia-skipped start equals `event.start`;
  - `event.start + event.length == name.end`, so the report covers exactly the next element's name token;
- a diagnostic whose code is ',' expected (1005), used as an extra conjunct on the retained diagnostic rather than as sufficient proof on its own.

**How this rules out the wrong cause:**
- A lost token would leave a `TokenSkipped` action or text that isn't covered.
- A lost owner would leave the reported token outside D_{i+1}'s name.
- A missing name would carry a missing-node event instead.

Keep the first version to `VariableStatement` lists only. `parseDelimitedList` also serves argument, parameter, array and object lists, and those need their own evidence.

**Consumer risk to measure.**
- TS JS output:
  - ESM: `export const a, number = "missing colon";` — `const` without an initializer is kept;
  - CommonJS: `exports.number = exports.a = void 0; exports.number = "missing colon";` — no assignment for `a`.
- TS d.ts: `export declare const a: any, number = "missing colon";`, and transpileDeclaration adds 9010.
- The native d.ts serialization of an uninitialized `const` (`a: any`) and the extra 9010 diagnostic are the unverified parts.

## 3. Census coverage (corrected)

My round 122 statement was too strong. The correct scope is: the dedicated h2-8c route IDs are absent from the census roster, but exact parse-input coverage — the (file name, text, `ParseOptions`) triple — can't be settled until the completed snapshot is inspected. The roster doesn't record per-file content hashes, and my regex search only covered one syntactic spelling.

Emoji/scanner recovery and the JSON source-kind question remain outside this candidate.
