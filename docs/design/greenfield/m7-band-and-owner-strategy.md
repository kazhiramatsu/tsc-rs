# M7 band and owner reconnaissance

Status: active M7 entry supplement. Adopted after the phase-9 2XXX
close on 2026-07-26. This document owns the investigation that happens
before an M7 semantic slice. [m7-tail-steps.md](m7-tail-steps.md) still
owns semantic stage order and acceptance.

## 1. What carries forward from the 2XXX sweep

The 2XXX sweep succeeded because implementation did not start from a
code-ranked tail alone. It first:

1. fixed the compared universe and exact scope;
2. measured the whole band;
3. traced exact rows to tsc emitter/dependency owners;
4. identified the current Rust pipeline boundary;
5. proved the smallest positive and negative shapes against the oracle;
6. landed one producer-owned slice with immutable before/after evidence;
7. re-snapshotted and repeated until the supported residue was zero.

M7 keeps that loop. It does **not** assume the remaining work is simple
because 2XXX is closed.

No remaining family combines as much type construction, instantiation,
relations, inference, and reporting depth as 2XXX. The residual risks
are different rather than absent:

- checker grammar is broad and order-sensitive across syntax, checker,
  module-format, and target gates;
- unused diagnostics have very high volume and depend on reference
  marking across binder/checker/module paths;
- suggestions cross pass/category boundaries and activate T1;
- program/options work changes aggregation, skip-checking, file-less
  diagnostics, and the formatter used by T4.

The expected semantic depth is lower than the 2XXX core, but the
cross-pipeline and measurement risk remains high enough to require
reconnaissance before implementation.

## 2. The M7 meaning of “band”

Outside 2XXX, a raw numeric range is not an implementation owner.
The same code may appear in semantic and suggestion passes, and one
numeric range can contain grammar, flow, inference, program, and JSDoc
producers. [non-2xxx-first-order.md](non-2xxx-first-order.md) and
`diag-families.json` therefore define the M7 **virtual bands**:

```text
A5 family = exact set of (code, pass) rows + owner + canaries
```

Examples are `checker-grammar`, `unused`, `suggestion-band`,
`flow-derived-suggestions`, and `program-resolution-options`.
`cargo xtask families report` is the live numerator/denominator for
these bands.

This term does not create another A2 scope pin. The only early A2 band
pin is `2xxx`, because only that band has its own fixed A1 accepted-set
view. M7 families remain under the All view and reach the A2 global
freeze at M7 close. To prevent post-result scope adjudication, a family
entry survey decides every currently visible scope question before its
first semantic edit. A later proposed scope addition for an active
family stops the slice and lands as a separately reviewed adjudication;
it never rides the implementation that benefits from it.

## 3. Mandatory family entry survey

Before the first implementation branch for each M7 virtual band:

1. Run full conformance and `cargo xtask families report`; record the
   family total, supported FN, canaries, T1/T2/T3 shadow identities,
   current scope hash, and A1 accepted-state hash.
2. Enumerate the exact family FN occurrences, including every matrix
   point. Do not reconstruct them from aggregate code counts.
3. For representative and top-mass rows, run
   `cargo xtask port-plan --diagnostic-json <exact-identity.json>`.
   Record the D2 declaration identity, shortest emitter path, SCC,
   ledger joins, escape rows, and the first unported Rust boundary.
4. Read the selected tsc declarations and their direct prerequisites.
   A code with multiple emitters is partitioned by the emitter reached
   by the exact row, not assigned by name or code alone.
5. Oracle-probe the smallest positive shape and at least one adjacent
   negative shape. Expected spans, category, chain, and related
   information come from the oracle.
6. Freeze the slice queue in the branch evidence: one producer owner,
   target fixtures, expected exact gains, expected escape/disposition
   removals, and any exact upper-tier blocker.
7. Before editing, create a `slice-evidence snapshot` outside the
   worktree for the selected fixtures with `--band all`. Afterward,
   verify it against the trusted base. Scope/universe drift, FP, or any
   tier loss stops the slice.

The family survey is investigation, not parity credit. A prerequisite
slice may be accepted-set neutral only under the
[prerequisite-only rule](definition-of-done.md#milestone-gates-vs-slice-fidelity).

## 4. Slice selection inside a family

Within a virtual band, choose slices by this order:

1. shared producer or missing model prerequisite;
2. tsc declaration/SCC and current Rust boundary;
3. exact diagnostic shapes unlocked by that owner;
4. diagnostic code only as a reporting label.

One slice has one producer owner. When several codes share that owner,
they may land together. When one code has several producers, it splits.
After each slice, re-run the family rollup; do not keep following the
old top-code ranking after its owner has changed.

When the remaining family queue becomes small and heterogeneous, switch
to [terminal-residue-protocol.md](terminal-residue-protocol.md).
Three probes hitting the same absent model or pipeline ceiling invoke
the stall playbook rather than three local patches.

## 5. Checker-grammar entry reconnaissance

Baseline: `main` at `b9ad04ac`, 2026-07-26. The live A5 rollup reports
`checker-grammar` at 1,710/3,013, supported FN 1,303, canaries 0/4.
Counts below are reconnaissance anchors, not a ratchet, and can overlap
where one code has multiple emitters.

The initial D2/code survey proves this is not one `checkGrammar*`
transcription:

- TS1479 143 and TS1471 68 route through `resolveExternalModule`
  (`d2:58affa056a8868001624fd3f98caed569d985fe09acd96da15ae894ab49e97a4`,
  `_tsc.js` 49473);
- TS1340 72 routes through `getTypeFromImportTypeNode`
  (`d2:6be6bf7180e57fbc1816b64ac99cd952c2800cfc585f40763dff9fee6dadc508`,
  62821);
- TS1361 33 and TS1362 31 route through the import/export-type usage
  worker
  (`d2:7727a05897150ebcee38a45f37636edcf5fcf3863bc5672843cca9ae9f4bc5c9`,
  48157);
- modifier/decorator rows route primarily through
  `checkGrammarModifiers`
  (`d2:984775a91d6ec0d2e27b820a9d34a31328ef5845e0fd5dd8a5e751f3040d2ca8`,
  89010), with TS1206 also emitted by decorator walkers;
- TS17009 58 and TS17011 11 route through `checkThisExpression` and
  `checkSuperExpression`
  (`d2:a59b5a3c825ad6c6c962303b3ee242e680907722a8df37fa1673f3abfe5ba83d`
  and
  `d2:7c10f3f025b12af336eecbca1ce37edf4735aef5a1f7df2a9b770a4a451ce0be`),
  not a shared grammar dispatcher;
- object-literal duplicate/accessor rows route through
  `checkGrammarObjectLiteralExpression`
  (`d2:05827e9a5c76ae100472c617286f76faf867600725482c2ec026a79d8e76309a`,
  89637);
- TS1501 routes through the regular-expression scanner availability
  worker
  (`d2:5247dc69f8f6c6f5b2df9dea65489091d4df24630eda94801ff11ac0375eeea8`,
  10839).

The 8.1 sub-slices are therefore:

| Slice | Producer cluster | Current anchor rows | Required exit |
|---|---|---|---|
| 8.1a | modifier/decorator ordering and placement: `checkGrammarModifiers`, obvious modifier/decorator reporters, async modifier | 1206 (129), 1029 (66), 1044 (32), 1275 (32), 1042 (30) | selected modifier/decorator exact rows close through live tiers; no follower diagnostic leaks past tsc's first-error ordering |
| 8.1b | object-literal grammar: `checkGrammarObjectLiteralExpression` plus object-member dispatch to `checkGrammarMethod` | 1117 (53), 1119 (36), 1118 (6), 1042/1162/1184/1255/1312 tail | frozen object-literal producer queue closes through T1/T2/T3 with no target-external gains |
| 8.1c | declaration/function/accessor/heritage grammar: parameter and type-argument lists, computed names, accessor declarations, heritage clauses | owner-mine after 8.1b | selected declaration shapes and their suppression order match; owning micro-canaries green |
| 8.1d | statement/expression/target grammar: break/continue and labels, await/yield/for-await, `this`/`super` ordering, meta-properties, regex rescan and flag gates | 17009 (58), 17011 (11), 1501 (19), 1309 (16) | target/statement rows close with scanner and semantic pass provenance preserved |
| 8.1e | module/import/export and format: tri-state implied format, Node sync-import rows, import/export-type usage, export-assignment decisive extension | 1479 (143), 1340 (72), 1471 (68), 1361 (33), 1362 (31), 1203 (4) | A10 → B16 → A11 dependency order holds; module-format canary and selected exact rows close |
| 8.1f | strict/private/JSDoc/ES-target grammar: private-name placement, strict-mode-only checks, JSDoc type syntax that belongs to semantic grammar | 18016 (31), 18010 (24), 17019/17020 (12), 18028 (2) | no JSDoc-semantics exclusion is confused with an in-scope grammar row; private/target canary green |
| 8.1g | owner-mined checker-grammar residue and family close | re-snapshot after 8.1a-f | supported FN=0, all four A5 canaries green, FP=0, exact T1/T2/T3 losses=0 |

The table fixes content identity, not permission to combine all rows in
one PR. Split a sub-slice further when the exact-row D2 survey finds
more than one producer owner. The A5 `checker-grammar` family closes
only at 8.1g; earlier sub-slices close their frozen target queues.

Accepted progress on 2026-07-26:

- 8.1a landed 505 exact matches through the modifier/decorator owner
  cluster, with no loss or false positive;
- the 8.1b entry survey split object-literal grammar from the broader
  declaration queue, then closed its frozen 116-row queue across
  T1/T2/T3. The family moved to 2,329/3,013 with supported FN 684.

## 6. M7 virtual-band order

Landing order remains:

1. `checker-grammar` — 8.1a-g above;
2. `suppression-surfaces` — audit/canary band, allowed to have no code
   rows;
3. `unused` semantic error surface — reference-marking prerequisite,
   then per-declaration workers;
4. `unused` suggestion surface, `suggestion-band`, and
   `flow-derived-suggestions` — separate producer queues under 8.4,
   followed by T1 activation;
5. `program-resolution-options` — skip-checking and global aggregation
   first, options/resolution rows next, deterministic formatter last.

Each virtual band receives its own entry survey immediately before its
first semantic slice so the owner map reflects the implementation state
at that point rather than the M7-start snapshot.
