# H2.5h-a / E-COMMENT-SCOPE-H — global comment scope study

Document role: **frozen tsc scope graph for the mandatory comment-scope
sub-packet** (step 1 of the six-step plan in
[the H2.5h-a handoff](h2-5h-a.md#first-mandatory-design-packet-global-comment-scope)).
This records the complete pinned-tsc ownership graph of the three scoped
comment values and the exact current-Rust delta. It authorizes no
production edit; the witness freeze and the implementation packets
follow it.

Authority: vendored TypeScript 6.0.3 (`vendor/typescript-6.0.3/lib/_tsc.js`)
owns semantics; freshly qualified rows in
[the emitter architecture](../emitter-architecture.md) own Rust facts.

## 1. The complete tsc scope graph

The triple lives as three printer-closure variables and appears at exactly
four site clusters. There are no other occurrences in the pinned bundle
(`grep -n 'containerPos\|containerEnd\|declarationListContainerEnd'`
returns only the lines cited below).

### 1.1 State

`_tsc.js:116957-116959` — `createPrinter` scope:

```js
var containerPos = -1;
var containerEnd = -1;
var declarationListContainerEnd = -1;
```

Initial value `-1` doubles as the "no container yet" sentinel read by both
guards in §1.4.

### 1.2 Writers (one set site, one restore site)

**Set — `emitLeadingCommentsOfNode(node, emitFlags, pos, end)`**
(`_tsc.js:121007-121032`): after optionally emitting the node's leading
comments, and only when `(pos > 0 || end > 0) && pos !== end`:

- `containerPos = pos` when `!skipLeadingComments || (pos >= 0 &&
  (emitFlags & NoLeadingComments))` — i.e. the container claim is made
  even when the leading emission itself was suppressed *by flag*, but not
  when it was skipped for `pos < 0` or `JsxText`;
- `containerEnd = end` under the mirrored trailing condition, and
  additionally `declarationListContainerEnd = end` **iff
  `node.kind === VariableDeclarationList`** — the only writer the third
  value has;
- `skipLeadingComments = pos < 0 || (emitFlags & NoLeadingComments) ||
  node.kind === JsxText`, and `skipTrailingComments` mirrors it over
  `end`/`NoTrailingComments`.

**Restore — `emitTrailingCommentsOfNode(node, emitFlags, pos, end,
savedContainerPos, savedContainerEnd, savedDeclarationListContainerEnd)`**
(`_tsc.js:121033-121052`): under the same `(pos > 0 || end > 0) &&
pos !== end` gate, the triple is restored **from the caller-saved values
first**, and only then are the node's trailing comments emitted
(`emitTrailingComments(end)`, skipped for `NotEmittedStatement`). A
node's trailing trivia is therefore emitted under its **parent's**
container scope, not its own — this ordering is load-bearing for the
`x /*TAIL*/` relocation counterexample in the handoff.

Synthetic comments bypass the triple entirely: synthetic leading
comments are emitted inside `emitLeadingCommentsOfNode` after the claim,
synthetic trailing comments inside `emitTrailingCommentsOfNode`
**before** the restore, and neither consults the guards in §1.4.

### 1.3 Save/restore carriers (the only two)

- **Recursive pipeline** — `pipelineEmitWithComments(hint, node)`
  (`_tsc.js:120978-120986`): saves the triple into locals, runs
  `emitCommentsBeforeNode(node)` (which performs the §1.2 set via
  `emitLeadingCommentsOfNode` over `getCommentRange(node)` and toggles
  `commentsDisabled` for `NoNestedComments`), runs the child pipeline
  phase, then `emitCommentsAfterNode(node, saved…)` (which un-toggles
  `NoNestedComments`, performs the §1.2 restore-then-trailing, and
  repeats the trailing phase for `getTypeNode(node)`'s range when
  present).
- **Iterative binary trampoline** — `createEmitBinaryExpression`'s
  `onEnter`/`onExit` (`_tsc.js:118390-118468`): replicates exactly the
  same save/emitCommentsBeforeNode/…/emitCommentsAfterNode(saved…) pair
  across the explicit `state.*Stack[state.stackIndex]` arrays, gated per
  level by `shouldEmitComments(node)`. This is the printer's counterpart
  of the checker's binary trampoline: any Rust design must give the
  iterative route the same scope topology as the recursive one.

No other function saves, sets, or restores any of the three values.

### 1.4 Guarded readers (the only two)

- `forEachLeadingCommentToEmit(pos, cb)` (`_tsc.js:121219-121233`):
  emits leading source trivia only when
  `containerPos === -1 || pos !== containerPos` — a child starting at
  the active container's start must not re-claim the same prefix. When
  `hasDetachedComments(pos)`, the walk reroutes past the recorded
  detached range (`forEachLeadingCommentWithoutDetachedComments`); the
  detached-comments state (`detachedCommentsInfo`) is adjacent machinery
  with its own lifecycle and never touches the triple.
- `forEachTrailingCommentToEmit(end, cb)` (`_tsc.js:121234-121238`):
  emits trailing source trivia only when `containerEnd === -1 ||
  (end !== containerEnd && end !== declarationListContainerEnd)` — the
  declaration-list special case exists so `var x = 1, y = 2;` emits the
  list's trailing trivia once at the list boundary rather than at the
  last declarator sharing the same `end`.

`emitLeadingCommentsOfPosition`/`emitTrailingCommentsOfPosition` (token
and list positions, e.g. the binary operator route at
`_tsc.js:118434-118441`) reach the same two readers and therefore the
same guards; they introduce no additional scope state.

## 2. Current Rust delta

The current printer deliberately projects the `containerPos` guard onto
explicit local parent/child boundaries instead of threading a scope
value — `token_owned_comment_phase_prefix`
(`crates/emitter/src/printer.rs`, tsc-port headers citing
`emitLeadingCommentsOfNode` and `forEachLeadingCommentToEmit`) and the
delimited-list phase around
`emit_leading_comments_for_delimited_list_start_in_parent` own the two
qualified projections. `crates/emitter/src/comment_cursor.rs` provides
the monotone resume cursor (`CommentCursor`/`CommentResume`) but no
container scope.

Consequences the sub-packet must close:

- the local projections cover the parent/child and list-start cases they
  were qualified for (H2.5g scope), but there is no single threaded
  value, so each new consumer re-derives ownership — exactly the
  divergence class the handoff's counterexample exercises once
  synthetic wrappers relocate statements;
- `containerEnd`/`declarationListContainerEnd` have **no Rust
  counterpart at all** today: trailing-trivia dedupe currently relies on
  the resume cursor's monotonicity, which cannot express "the enclosing
  container already owns this end position";
- the iterative/recursive dual topology (§1.3) must be one Rust
  mechanism, not two.

The mandated Rust shape (immutable `CommentEmissionScope` threaded
through an explicit `EmitContext`, no `Default` nested scope, root-only
construction, contextless APIs deleted) is fixed by the handoff; this
study confirms the tsc graph it must reproduce and adds one
representation requirement: the restore-before-trailing ordering in
§1.2 must be expressed structurally (the child's emission returns to the
parent scope *before* the child's trailing phase runs), so the Rust
context for a node's trailing phase is the **parent** scope value, not
the child's.

## 3. Witness set to freeze (step 1 remainder)

Oracle-captured bytes only (no transcription), each with remove-comments
and adjacent-negative controls, covering:

1. the handoff counterexample (`x /*TAIL*/` under a synthetic arrow
   wrapper) — leading claim + parent-scope trailing;
2. direct child sharing the container start (`containerPos` guard hit);
3. ordinary delimited list and multiline list starts (list phase vs
   ordinary phase split already qualified in H2.5g);
4. `VariableDeclarationList` trailing dedupe
   (`declarationListContainerEnd` hit: `var x = 1, y = 2; /*T*/`);
5. binary-expression nesting deep enough to drive the trampoline stack
   (iterative carrier parity);
6. `NoLeadingComments`/`NoTrailingComments`/`NoNestedComments` flag
   interactions with the claim conditions in §1.2;
7. synthetic leading/trailing comments alongside source trivia (bypass
   ordering in §1.2);
8. detached-comment reroute adjacency (`hasDetachedComments` path);
9. type-node trailing repetition (`emitCommentsAfterNode`'s second
   trailing phase);
10. zero-width and `pos === end` ranges (the outer gate), plus
    `NotEmittedStatement` trailing suppression.

The witness generator extends the H2.5h-a foundation's direct-control
mechanism (fresh-process pinned-tsc observation, two runs, fingerprints)
and is the next machine increment of this sub-packet.
