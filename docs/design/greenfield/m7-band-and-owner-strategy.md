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
| 8.1c | declaration/function/accessor/heritage grammar: parameter and type-argument lists, computed names, accessor declarations, heritage clauses | 8.1c.1 parameter-list TS1014 (6) + TS1015 (4); 8.1c.2 source-file TS1046 (7); re-mine after each producer | selected declaration shapes and their suppression order match; owning micro-canaries green |
| 8.1d | statement/expression/target grammar: break/continue and labels, await/yield/for-await, `this`/`super` ordering, meta-properties, regex rescan and flag gates | 17009 (58), 17011 (11), 1501 (19), 1309 (16) | target/statement rows close with scanner and semantic pass provenance preserved |
| 8.1e | module/import/export and format: tri-state implied format, Node sync-import rows, import/export-type usage, export-assignment decisive extension | 1479 (143), 1340 (72), 1471 (68), 1361 (33), 1362 (31), 1203 (4) | A10 → B16 → A11 dependency order holds; module-format canary and selected exact rows close |
| 8.1f | strict/private/JSDoc/ES-target grammar: private-name placement, strict-mode-only checks, JSDoc type syntax that belongs to semantic grammar | 18016 (31), 18010 (24), 17019/17020 (12), 18028 (2) | no JSDoc-semantics exclusion is confused with an in-scope grammar row; private/target canary green |
| 8.1g | owner-mined checker-grammar residue and family close | re-snapshot after 8.1a-f | supported FN=0, all four A5 canaries green, FP=0, exact T1/T2/T3 losses=0 |

The table fixes content identity, not permission to combine all rows in
one PR. Split a sub-slice further when the exact-row D2 survey finds
more than one producer owner. The A5 `checker-grammar` family closes
only at 8.1g; earlier sub-slices close their frozen target queues.

### 5.1 Regex owner split: 8.1d.3p then 8.1d.3v

The regex queue crosses a representation boundary and is therefore two
separate producer-owned slices:

1. **8.1d.3p — scanner/parser prerequisite.** Thread the effective
   `ScriptTarget` through parser and scanner, generate the ES5 and ESNext
   identifier tables, make every identifier decision target-aware, port
   `reScanInvalidIdentifier`, store `SourceFile.language_version`, and
   materialize `RegularExpressionLiteral.isUnterminated`. This closes M1
   review debt item 1 but claims no regex diagnostic parity.
2. **8.1d.3v — validator producer.** Port the complete
   `reScanSlashToken(reportErrors=true)` validator closure and publish it
   through `checkGrammarRegularExpressionLiteral`, including UTF-16
   positions, Unicode-property data, target gates, and tsc's
   primary/related/same-start suppression.

The exact TypeScript owners are:

- checker entry `checkRegularExpressionLiteral`,
  `d2:d4bfabf885ae6a20b8a8ccc55181fa1872cce2cc2798117d75c15750f07aa520`,
  `_tsc.js:73931-73938`;
- scanner producer `reScanSlashToken`,
  `d2:98b428bbf97e88486c3282ee5c4c025822cf7e23c433c628421df49f2355953b`,
  `_tsc.js:9893-9996`, followed by its validator closure through line
  10844;
- target gate `checkRegularExpressionFlagAvailability`,
  `d2:5247dc69f8f6c6f5b2df9dea65489091d4df24630eda94801ff11ac0375eeea8`,
  `_tsc.js:10839-10844`.

The post-8.1d.2b queue contains 21 fixtures / 40 matrix cases and 87 oracle
diagnostics. Its fresh post-8.1d.3p immutable snapshot confirmed 31 exact
matches and supported FN 56:
TS1125 x17, TS1198 x4, TS1199 x3, TS1499 x1, TS1501 x19, and TS1508 x12.
All are semantic `checker-grammar` rows. The full-corpus side of that same
snapshot also retained `parser579071.ts` TS1005 as an initially overlooked
false negative from the same validator producer; it is reviewed as a
same-owner target-external closure gain, not a second slice.

### 5.2 Module-format owner sequence: A10 then B16 then A11

The fresh post-regex survey keeps the design dependency order
**A10 → B16 → A11**. The 8.1e code-level cluster is not one producer:
TS1471/1479/1541/1542 are emitted by `resolveExternalModule`, while
TS1203 is emitted by `checkExportAssignment`; TS1340 and TS1361/1362
belong to two further import-type usage workers.

A10 is the accepted-set-neutral prerequisite. Its direct owners are:

- `getImpliedNodeFormatForFileWorker`
  (`d2:e6f65ad86b4e675208b7b4ff66493081e4c4833bc69cb73e194933283d4bbc60`,
  `_tsc.js:122500-122513`);
- `getImpliedNodeFormatForEmitWorker`
  (`d2:58ff154ca300f5354a06782c109a8a6198133c233192a14d72b11b7890cd2dc8`,
  `_tsc.js:125496-125509`).

The file format is tri-state: decisive MTS/MJS and CTS/CJS extensions
win; ordinary TS/JS extensions consult package scope only for
Node16-through-NodeNext resolution or a `node_modules` path; every
other case remains undefined. The emit worker separately preserves
decisive extensions and explicit package `"type"` evidence, so an
explicit `"commonjs"` scope is not collapsed with an absent `type`.
The oracle pin uses `module=esnext`, `moduleResolution=bundler`, and a
root package `"type":"module"`: a plain `.ts` export-assignment target
keeps an undefined format and remains default-importable when synthetic
defaults are allowed, while the adjacent `.mts` target reports TS1192.

The A10 immutable target is the complete next-owner fixture set: 24
fixtures / 83 matrix cases / 1,222 oracle diagnostics, initially
600 exact matches, zero false positives, and 550 supported false
negatives. This deliberately broad target makes A10 prove no movement
before B16 changes the same module-format surface.

B16 then owns the full `resolveExternalModule` mode-mismatch queue:
TS1471 x68, TS1479 x143, TS1541 x1, and TS1542 x1 (213 exact
identities). The two type-only rows are part of the same conditional
branch and may not be omitted from the frozen queue. Its direct owner is
`resolveExternalModule`
(`d2:58affa056a8868001624fd3f98caed569d985fe09acd96da15ae894ab49e97a4`,
`_tsc.js:49473-49663`); the exact direct prerequisites are
`createModeMismatchDetails`
(`d2:dcf9f742c5c48599c686a369e4ca8dbc5396c12acf295c32aa8bad4a125f0b3d`)
and `hasResolutionModeOverride`
(`d2:de5a2190d2bb781f0d83d5c5128bbf8b27d19fd63ef4e41b011062f5f1313d3d`).
Package `exports`, `imports`, and self-name targets are resolved only as a
format-evidence projection when the ordinary resolver verdict remains
`Suppressed`; their symbols and members are not published into general
checking. This boundary is required for producer ownership and preserves
the all-corpus FP=0 invariant.

A11 then owns `checkExportAssignment`
(`d2:fa6db14850191332391b180605df6635041f4d72ad223c01a4900e4413c64e5f`,
`_tsc.js:86391-86501`). It uses the decisive emit format for TS1203,
publishes that grammar row in checked JavaScript, and ports the live
verbatim/isolated type-only branches without absorbing the adjacent
import/export usage producers.

TS1340 then stays with `getTypeFromImportTypeNode`
(`d2:6be6bf7180e57fbc1816b64ac99cd952c2800cfc585f40763dff9fee6dadc508`,
`_tsc.js:62821-62880`). When ordinary package resolution remains
`Suppressed`, only a bare recovered ImportType in type meaning may read
the B16 package target module's own flags. That projection exists only
to decide TS1340; it does not expose the package symbol, exports, or
members to general checking.

TS1361/TS1362 stay with the lazy callback inside
`onSuccessfullyResolvedSymbol`
(`d2:7727a05897150ebcee38a45f37636edcf5fcf3863bc5672843cca9ae0f4bc5c9`,
`_tsc.js:48157-48204`). The producer reads the raw alias flags before
following the target, applies the complete non-JSDoc
`isValidTypeOnlyAliasUseSite` predicate, and attaches TS1376/TS1377
related information from the exact import/export-type declaration.
The checked-JavaScript export-namespace row is published through the
exact non-JSDoc diagnostic marker rather than the global JavaScript
allowlist.

Accepted progress on 2026-07-26:

- 8.1a landed 505 exact matches through the modifier/decorator owner
  cluster, with no loss or false positive;
- the 8.1b entry survey split object-literal grammar from the broader
  declaration queue, then closed its frozen 116-row queue across
  T1/T2/T3. The family moved to 2,329/3,013 with supported FN 684;
- the 8.1c owner survey then separated ordinary signature parameter
  grammar from declaration-file source grammar. 8.1c.1 closed its
  frozen TS1014 x6 + TS1015 x4 queue at all three live tiers, moving
  the family to 2,339/3,013 with supported FN 674;
- 8.1c.2 closed the frozen declaration-file TS1046 x7 queue through
  `checkGrammarSourceFile` at all three live tiers, with no target-external
  gain or loss. The family moved to 2,346/3,013 with supported FN 667;
- 8.1d.1 closed the frozen `this`/`super` ordering queue,
  TS17009 x58 + TS17011 x11, through `checkThisBeforeSuper` and the
  all-reaching-path `isPostSuperFlowNode` walk. All three live tiers
  gained the same 69 identities with no loss or false positive. The
  family moved to 2,415/3,013 with supported FN 598 and its constructor
  default-value canary passed;
- the 8.1d.2 exact-row survey corrected the initial code-level grouping:
  TS1309 x16 is split evenly between `checkAwaitGrammar` and
  `checkGrammarForInOrForOfStatement`. 8.1d.2a closed the first
  producer's TS1309 x8 Node CommonJS queue, including checked-JavaScript
  publication. T0/T1/T2/T3 each gained the same eight identities with
  no loss or false positive, moving the family to 2,423/3,013 with
  supported FN 590;
- 8.1d.2b then closed the second producer's TS1309 x8 `for await`
  Node CommonJS queue through `checkGrammarForInOrForOfStatement`.
  T0/T1/T2/T3 again gained the same eight identities, with no
  target-external gain, loss, or false positive. The family moved to
  2,431/3,013 with supported FN 582, completing the frozen TS1309
  queue across both exact emitters;
- 8.1d.3p closed M1 review debt item 1 before activating the regex
  validator. ES5/ESNext identifier tables, effective-target threading,
  the guarded ESNext invalid-identifier recovery retry,
  `SourceFile.language_version`, and regex unterminated state are now
  represented. This prerequisite is accepted-set neutral: all-corpus
  T0/T1/T2/T3, the 21-fixture regex queue, supported views, and oracle
  universes are unchanged, with zero false positives.
- 8.1d.3v ported the complete `reScanSlashToken(reportErrors=true)`
  validator closure into a dedicated UTF-16 syntax module. Scanner
  codegen now derives the ordered Unicode-property maps and sets from
  vendored `_tsc.js`; the checker publishes exact target gates and
  primary/related/same-start groupings, including checked JavaScript.
  The frozen queue moved 31/87 to 87/87. Full all-corpus T1/T2/T3 each
  gained 57 identities with no loss or false positive: the frozen 56
  plus the reviewed `parser579071.ts` TS1005 same-owner closure. The
  family is now 2,488/3,013 with supported FN 525 and canaries 2/4.
- 8.1e A10 ported both implied-format workers and upgraded the
  in-memory package host input from a boolean to the required
  module/CommonJS/other/missing distinction. All existing consumers now
  preserve undefined format explicitly; `checkExportAssignment` keeps
  its pre-A11 decision boundary. The 24-fixture target remained
  600/1,222 and the full corpus remained 32,207/49,024, with zero
  false positives, no T1/T2/T3 gain or loss, and unchanged oracle
  universes. This closes the accepted-set-neutral prerequisite for
  B16 without claiming diagnostic parity;
- 8.1e B16 closed the complete `resolveExternalModule` mode-mismatch
  queue: TS1471 x68, TS1479 x143, TS1541 x1, and TS1542 x1. The
  24-fixture target moved from 600/1,222 to 813/1,222 with FP=0 and
  supported FN 550→337. Full-corpus T1/T2/T3 each gained exactly the
  same 213 identities with no loss or false positive; all-corpus T0 is
  32,420/49,024 and supported T0 is 32,419/48,477. The implementation
  keeps package target lookup diagnostic-only, so no downstream
  type/member diagnostic moved. The checker-grammar family is now
  2,701/3,013 with supported FN 312 and canaries 2/4;
- 8.1e A11 closed the exact `checkExportAssignment` queue: TS1203 x4
  plus TS1282/1283/1284/1285/1289 x1 each. Its four-fixture target
  moved from 16/41 to 25/41 with FP=0 and supported FN 25→16.
  Full-corpus T1/T2/T3 each gained exactly nine identities with no
  target-external movement or loss; all-corpus T0 is 32,429/49,024 and
  supported T0 is 32,428/48,477. The checker-grammar family is now
  2,710/3,013 with supported FN 303 and canaries 2/4;
- 8.1e TS1340 closed all 72 supported identities owned by
  `getTypeFromImportTypeNode` across the two Node import-type
  attribute fixtures and four module modes. The target moved from
  560/664 to 632/664, with its supported view at 632/632. Full-corpus
  T1/T2/T3 each gained exactly 72 identities with no loss or
  target-external movement; all-corpus T0 is 32,501/49,024 and
  supported T0 is 32,500/48,477. The checker-grammar family is now
  2,782/3,013 with supported FN 231 and canaries 3/4;
- 8.1e TS1361/TS1362 closed the frozen 64-row type-only alias value-use
  queue: TS1361 x33 and TS1362 x31 across 29 fixtures. The target moved
  from 28/150 to 92/150, with every targeted type-only-alias FN closed
  and FP=0. Full-corpus T1/T2/T3 each gained exactly 64 identities with
  no loss or target-external movement; all-corpus T0 is 32,565/49,024
  and supported T0 is 32,564/48,477. The checker-grammar family is now
  2,846/3,013 with supported FN 167 and canaries 3/4;
- 8.1f.1 closed all 31 TS18016 private-name placement identities owned
  by `checkGrammarObjectLiteralExpression`. Its seven-fixture target
  moved from 256/353 to 287/353. Full-corpus T1/T2/T3 each gained 31
  identities with no loss or target-external movement; all-corpus T0
  is 32,596/49,024 and supported T0 is 32,595/48,477. The
  checker-grammar family is now 2,877/3,013 with supported FN 136, and
  all four canaries are green. The next owners are the separately
  frozen checked-JavaScript method and accessor TS18028 producers.

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
