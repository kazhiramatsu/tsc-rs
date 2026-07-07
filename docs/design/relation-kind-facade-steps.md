# RelationKind facade steps

Companion to `type-checking-2xxx-execution-plan.md` section B
(No-Behavior Scaffolds) and `type-checking-2xxx-roadmap.md` Phase 4.
The target model is `checker-key-functions.md` section 1.5. Every stage
in this workstream must be byte-identical: the full gate must show
`NEW FALSE POSITIVES: 0`, `NEW FALSE NEGATIVES: 0`, and zero
added/removed diagnostics.

This workstream is order-independent of the candidate boundary
(`candidate-boundary-steps.md`); either may land first.

## Files this workstream may touch

- `src/checker/relations.rs`
- `src/checker/mod.rs` (the `RelationState` struct only)
- `src/checker/infer.rs` (the `cast_comparable` save/restore only)

Needing any other file is a stop condition (see EXECUTION-GUIDE.md).

## Current shape (verified anchors)

- `RelationState` (`src/checker/mod.rs`, near line 494) holds
  `relation_cache`, `relation_stack`, `relation_depth_overflow`,
  `keep_head_for_missing`, `erase_generic_sigs`, `comparable_cache`.
- `is_assignable_to` (`src/checker/relations.rs`, near line 126) picks
  its cache by reading `self.rel.erase_generic_sigs` and inserts only
  for top-level queries (`relation_stack.len() == 1`).
- `cast_comparable` (`src/checker/infer.rs`, near line 687) is the only
  writer of `erase_generic_sigs`: it saves, sets `true`, runs
  `comparable_dir` both directions, restores.
- `check_assignable` (`src/checker/relations.rs`, near line 105) is the
  reporting entry; it delegates to `is_assignable_to` first.
- `relation_cache` / `comparable_cache` have exactly ONE reader and
  ONE writer each, both inside `is_assignable_to` (the get near
  line 131, the top-level insert near line 156). No other code touches
  them; Stage 3's blast radius is exactly that function plus comments.
- `RelationState` is constructed only through `#[derive(Default)]`
  (`RelationState::default()` in `Checker::new`, `mod.rs` near
  line 799). No literal construction site exists.

### Stage 1 edit list: every `erase_generic_sigs` site

`grep -rn erase_generic_sigs src/` returns exactly the sites below
(re-run it first; if new sites have appeared since this doc was
written, convert them by the same read/write rules and note them in
the commit body):

| Site (near line) | Kind | Edit |
|---|---|---|
| `mod.rs:504` `pub erase_generic_sigs: bool` + its doc comment | field decl | replace with the `kind` field (snippet below) |
| `relations.rs:130` `let comparable = self.rel.erase_generic_sigs;` | read | `let comparable = self.rel.erase_generic_sigs();` |
| `relations.rs:1316`, `relations.rs:1437` `if self.rel.erase_generic_sigs {` | read | `if self.rel.erase_generic_sigs() {` |
| `relations.rs:2036` `... = if self.rel.erase_generic_sigs {` | read | method call, same conversion |
| `relations.rs:2418` `let erase = force_erase \|\| self.rel.erase_generic_sigs;` | read | method call, same conversion |
| `infer.rs:693-696` save / set `true` / restore | the only write | kind save/restore (snippet below) |
| `infer.rs:691` comment "gated on erase_generic_sigs" | comment | reword to "gated on `RelationKind::Comparable`" |

Field replacement in `mod.rs` (doc comment rewritten for the new
field; use the full path or add a `use`):

```rust
/// Active relation kind for the current query (tsc: which relation
/// map a query consults). `Comparable` subsumes the old
/// `erase_generic_sigs` flag: single-signature pairs relate with
/// their own type parameters erased to `any` (signaturesRelatedTo
/// `eraseGenerics = relation === comparableRelation`). Set for the
/// duration of a `cast_comparable` query via save/restore in
/// `infer.rs`; results go to the kind's own cache.
pub kind: crate::checker::relations::RelationKind,
```

Write conversion in `cast_comparable` (`infer.rs`; needs
`use crate::checker::relations::RelationKind;` or the full path):

```rust
// before                                   // after
let saved = self.rel.erase_generic_sigs;    let saved = self.rel.kind;
self.rel.erase_generic_sigs = true;         self.rel.kind = RelationKind::Comparable;
/* comparable_dir both directions */        /* unchanged */
self.rel.erase_generic_sigs = saved;        self.rel.kind = saved;
```

After converting, all three greps must return nothing:

```sh
grep -rn "\.erase_generic_sigs;" src/
grep -rn "erase_generic_sigs =" src/
grep -rn "erase_generic_sigs: bool" src/
```

## Stage 0: Baseline [P]

```sh
git status                      # must be clean; note HEAD
cargo build --release
cargo test --release            # expect: first suite 98 passed (or current baseline)
./verify.sh golden-save         # golden must correspond to HEAD
```

## Stage 1: `RelationKind` state replaces the bool flag [M]

In `src/checker/relations.rs`, near the top:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum RelationKind {
    Identity,
    Subtype,
    StrictSubtype,
    #[default]
    Assignable,
    Comparable,
}
```

In `RelationState` (`src/checker/mod.rs`), replace
`pub erase_generic_sigs: bool` with `pub kind: RelationKind` and keep a
compatibility accessor so reads stay mechanical:

```rust
impl RelationState {
    pub fn erase_generic_sigs(&self) -> bool {
        matches!(self.kind, crate::checker::relations::RelationKind::Comparable)
    }
}
```

Then convert every site in the "Stage 1 edit list" above:

- reads become `self.rel.erase_generic_sigs()` (method call);
- the save/restore in `cast_comparable` (`infer.rs`) becomes a
  save/restore of `self.rel.kind` — save whatever the current kind
  is, set `Comparable`, restore the SAVED value (do NOT hardcode the
  restore to `Assignable`);
- run the three "must return nothing" greps from the edit list before
  building.

No other logic changes. Verify:

```sh
cargo build --release 2>&1 | grep -E "^error" -A5   # expect: nothing
cargo test --release 2>&1 | grep "test result:"      # expect: all ok, same count
./verify.sh golden-check                             # expect: 0 NEW_FP / 0 NEW_FN, zero adds/removes
```

Commit: `relation-kind 1: RelationKind state replaces erase_generic_sigs`.

## Stage 2: `relate()` facade entry [M]

In `src/checker/relations.rs`, next to `is_assignable_to`:

```rust
/// Single relation entry point (tsc: one relation map per query).
/// `Assignable` and `Comparable` are real; `Identity`, `Subtype`, and
/// `StrictSubtype` are dark-launch placeholders that currently alias
/// the assignable behavior — no call site may rely on them yet.
/// Do NOT add boolean mode flags to `is_assignable_to`; add behavior
/// here, per kind (execution plan, hard constraint 3).
pub fn relate(&mut self, kind: RelationKind, src: TypeId, tgt: TypeId) -> bool {
    let saved = self.rel.kind;
    self.rel.kind = match kind {
        RelationKind::Assignable | RelationKind::Comparable => kind,
        // dark launch: unported kinds run as assignable
        _ => RelationKind::Assignable,
    };
    let r = self.is_assignable_to(src, tgt);
    self.rel.kind = saved;
    r
}
```

`is_assignable_to` itself keeps its current body and its current
meaning: "relate under the AMBIENT kind". Do not reroute its ~127
existing call sites
(`grep -rn "is_assignable_to(" src/ | grep -v "fn is_assignable_to" | wc -l`);
they inherit the ambient kind exactly as they inherit
`erase_generic_sigs` today. `cast_comparable` keeps its
bidirectional `areTypesComparable` role on top of the `Comparable`
kind.

Verify (same three commands as Stage 1; same expectations).

Commit: `relation-kind 2: relate() facade entry`.

## Stage 3: per-kind cache container [M]

In `RelationState`, replace the two cache fields with one container:

```rust
pub caches: [HashMap<(TypeId, TypeId), bool>; 5],
```

with accessors keyed by `kind as usize`:

```rust
pub fn cache(&self) -> &HashMap<(TypeId, TypeId), bool> {
    &self.caches[self.kind as usize]
}
pub fn cache_mut(&mut self) -> &mut HashMap<(TypeId, TypeId), bool> {
    &mut self.caches[self.kind as usize]
}
```

Update the get/insert sites inside `is_assignable_to`
(`relations.rs`, near lines 131-160). Per the "Current shape" notes,
these are the ONLY code references to either cache, so this stage
touches exactly two expressions plus comments:

```rust
// before (get, ~130-135; `comparable` local from Stage 1)
let comparable = self.rel.erase_generic_sigs();
let cached = if comparable {
    self.rel.comparable_cache.get(&(src, tgt))
} else {
    self.rel.relation_cache.get(&(src, tgt))
};
// after
let cached = self.rel.cache().get(&(src, tgt));

// before (insert, ~155-160)
if top_level {
    if comparable {
        self.rel.comparable_cache.insert((src, tgt), r);
    } else {
        self.rel.relation_cache.insert((src, tgt), r);
    }
}
// after
if top_level {
    self.rel.cache_mut().insert((src, tgt), r);
}
```

The `comparable` local has no remaining use afterwards — delete it.
Keep the top-level-only insert rule unchanged. Then
`grep -rn "relation_cache\|comparable_cache" src/` and update the
leftover doc comments (the `RelationState` struct docs in `mod.rs`);
the grep must end with zero code references to the old field names.
`#[derive(Default)]` on `RelationState` keeps working: arrays of
`Default` elements implement `Default`.

This is byte-identical because the ambient kind is always `Assignable`
or `Comparable`, which index exactly the two maps that existed before;
the other three slots stay empty.

Verify (same three commands). Additionally confirm byte-identity of
the corpus output:

```sh
diff -q /tmp/golden_now.txt /tmp/golden_diag.txt    # expect: no output
```

Commit: `relation-kind 3: per-kind relation caches`.

## Stage 4: Final gate and handoff

```sh
cargo fmt
cargo build --release
cargo test --release
./verify.sh golden-check      # expect: 0 / 0, zero adds/removes
./verify.sh golden-save
```

The facade is now real. Behavior work (actual `Subtype`,
`StrictSubtype`, `Identity` semantics, per-kind cache keys from tsc
`getRelationKey`) is NOT part of this workstream: it requires the
readiness checklist in `type-checking-2xxx-execution-plan.md`
("Before Relation Behavior") and a fresh mining ledger.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Golden movement after Stage 1 | A converted read inverted the flag (Comparable vs Assignable) | Re-grep `erase_generic_sigs`; each read must map `true` → `Comparable` |
| Golden movement after Stage 2 | `relate()` leaked its kind (missing restore) or `cast_comparable` double-sets | Save/restore `self.rel.kind` around the query; single writer |
| Movement after Stage 3 | Cache slot mix-up: comparable results in the assignable slot or vice versa | Check `kind as usize` indexing and that inserts stay top-level-only |
| Compile error on `Default` | Enum missing `#[default]` variant attribute | Add `#[default]` on `Assignable` |
| A fixture flips only under `--strict` | Kind not restored on an early-return path | Make the restore unconditional (save → run → restore, no `?`/early exits between) |
