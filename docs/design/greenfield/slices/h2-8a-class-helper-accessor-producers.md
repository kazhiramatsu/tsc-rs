# H2.8a A6-29: class helper requests and accessor producers

The three selected producer corrections are qualified on the H2.8 train.
The [frozen complete comparison](../../../../ratchets/h2-8a-class-helper-accessor-producers-after.v1.json) has 1020/1140 commands
exact twice: all 928 preceding positives are preserved, and all 92 selected
failures (76 fresh and 16 original A28) are repaired. The actual command exits
101 because 120 explicitly outside cases remain; it is not an all-green suite.
There are 2160 primary executions and no
supplemental captures. Exact cases complete twice; a first failed comparison
ends that case after one primary execution, as recorded before launch.
The four ES2015 getter-comment cases retain exactly their old getter mapping
rows, while their setter rows now match TypeScript. All other map fields and
the declaration maps in those cases match. Their first comparison still fails
at source maps; no later JavaScript comparison or complete failed tuple is
claimed. The other outside first vectors and all eight typed boundaries remain
unchanged.

The [adjacent emitter regression record](../../../../ratchets/h2-8a-class-helper-accessor-producers-regressions.v1.json) passes all 494
units and 451 contracts with identical test IDs and no assertion changes.
Original A28 reaches 412/492 exact twice, leaving 80 originally outside cases.
This result does not update the old global 769-case count. H2.8a and H2.8b–e
remain open; hosted acceptance is required before landing.

The following packet retains its original ready scope, observations and
prospective requirements; the records above carry the actual after outcome.

The complete repeated before is frozen at `9cc63a83d134816bd7785c435a171ef10a5dcd64`.
Both native jobs have 32/144 exact complete commands and the same 112 first
failure vectors, with no typed or unrepresented failure. The selected three
producer corrections own 76 new complete cases and 16 existing A28 cases.
H2.8a, the global matrix and H2.8b–e remain open.

## Authority and scope

Follow the current emitter architecture and post-H1 schedule. The
[before record](../../../../ratchets/h2-8a-class-helper-accessor-producers-before.v1.json)
retains both actual exits (101), the first archive, 352 primary executions and
176 separately counted supplemental captures. The immutable source audit pins
69 whole TypeScript 6.0.3 functions and three whole helper declarations.
The [readiness record](../../../../ratchets/h2-8a-class-helper-accessor-producers-readiness.v1.json)
maps every source owner, witness, Rust value and architecture premise to a step.
The trusted train base is `10748f6ee19ec083ce5748224930c5dcfbbd86df`.

Only these production files may change:

- `crates/emitter/src/builtins/class_fields/downlevel.rs`: helper request timing
  and generated setter modifier construction.
- `crates/emitter/src/factory.rs`: existing `NodeFactory::modifier_flags`
  becomes `pub(crate)`; its projection body and modifier creation stay intact.
- `crates/emitter/src/builtins/es2015.rs`: replace flags on the actual accessor
  receiver clone at `transform_accessors_to_expression`.

The third file was selected after the repeated before and before production.
The explicit scope amendment proves its bytes were already pinned before the
first TS observation, first native job and repeat. The original two-path
proposal and all old bytes remain retained; the amendment does not rewrite
the initial selection. Two preparer/runner errors before the native repeat
were nonlaunch failures and remain recorded separately from native execution.

## Mechanical steps

**A6-29a — request the class-name helper at named evaluation.**
TypeScript `classFields.visitVariableDeclaration` calls named evaluation before
visiting its children. `createClassNamedEvaluationHelperBlock` requests
setFunctionName before the private environment, heritage and member transforms.
At the existing `needs_named_evaluation` predicate, native lowering requests
the existing helper early. The existing later AST call creation stays in place;
no identifier or generated name is allocated by the new request. The context's
existing per-unit deduplication retains the first request. TypeScript's helper
attachment and stable priority ordering, and the native private-in ordering
rule, stay unchanged. The three audited helpers have no priority/dependencies.

**A6-29b — give the setter fresh modifiers.**
TypeScript `transformAutoAccessor` passes visited modifiers to the getter and
`createModifiersFromModifierFlags(modifiersToFlags(modifiers))` to the setter.
After creating the getter, native lowering projects its modifier flags through
the existing factory method and creates fresh synthetic setter modifiers.
Reuse the canonical factory modifier order. Keep the getter's source tokens,
accessor original/map/comment metadata, and synthetic node positions unchanged.
The existing projection omits Decorator, which the creator also does not emit;
this does not claim generic modifier projection parity or change its body.

**A6-29c — replace ES5 accessor receiver flags.**
TypeScript `getName`/`getInternalName` can supply NoSourceMap and local/internal
name flags. `transformAccessorsToExpression` then uses `setEmitFlags`, replacing
them with NoComments | NoTrailingSourceMap. Native currently adds these flags
and retains NoLeadingSourceMap. Use existing `EmitMetadata::set_flags` on this
receiver only, keeping its clone, text range and first-accessor-name map range.
Do not preserve unsupported local/internal flags with a bespoke bit clear.
The shared CommonJS controls must verify module qualification after replacement.
Other add/set sites and the separate escaped-name raw-range owner are unchanged.

**A6-29d — complete comparison and adjacent regressions.**
Run all 1140 distinct commands twice: the prior 996 plus the new 144. Require
at least 1020 exact, comprising all 896 old positives, 32 new before positives,
76 new owned repairs and the 16 existing repairs already in the 996 inventory.
Require all 494 emitter units and 451 contracts with the same test IDs, no
additions, replacements or assertion weakening. Freeze the first after result
before any correction or scope amendment, even if a gate fails.

## Complete-case dispositions

The 144 fresh cases span 12 class shapes, ES5/ES2015/ES2022, CommonJS/ESNext,
and both class-field modes. Strict ordinary libraries, CRLF, outDir, source
and declaration maps, all diagnostics, ordered write callback metadata and
result/exit fields are compared. The upstream observer and independent check
each execute every case twice. TS5107 occurs in 48 cases and TS4094 in 48;
exits are 0:64, 1:48 and 2:32. Declaration blocking gives 48 cases two writes
and 96 cases four writes. No output is assumed present when TypeScript omits it.

The selected cause memberships overlap: helper request order 48, ES5 receiver
flags 24 and setter modifiers 24. Their complete-case union is 76. Four ES2015
static-modifier-comment cases also have the setter map defect but remain
outside because their getter loses `/* static */`. Their first vector may
advance from the known map to that exact known JavaScript comment difference;
review that transition against frozen supplemental bytes. No other changed
outside vector is automatically accepted.

The other 32 outside cases use the separate ES2022 implementation in
`class_fields.rs`: retained anonymous accessor receivers, static-field ranges
and accessor ranges. TypeScript's unassigned `_a` receiver is observed behavior,
not an oracle error to repair here. All 32 new positive controls remain required.
Expected remaining failures are 84 old outside plus 36 new outside, 120 total;
the original A28 inventory would reach 412/492. These are prospective counts.
Full failed tuples are not claimed from first vectors or supplemental captures.

## Rust ownership and architecture

The machine record maps typed requests, helper-reference identity, visited and
fresh modifier arrays, source flags, cloned receiver flags, original/map/comment
identities and per-unit lifetime. Parsed syntax is immutable. Context requests
are disposed with the source transform; synthetic arrays and sparse metadata
belong to the detached arena and are discarded with the emit. No global cache,
printer text rewrite, diagnostic exception or target admission is introduced.

Required rows: E-PRINTER-BASE, E-POSITIONS, E-COMMENTS-G, E-COMMENT-SCOPE-H,
E-RESOLVER-BASE, E-ARENA, E-METADATA-BASE, E-CONTEXT, E-ENTRY, E-PROTOCOL,
E-ORDER-G, E-ORDER-H, E-MAPS, E-NAMES-BASE, E-NAMES-CLASS-G, E-NAMES-H,
E-HELPERS-BASE, E-HELPERS-PROVENANCE-G and E-METADATA-G. The factory visibility,
helper request producer and receiver metadata are requalified by this slice;
unchanged invariants retain only their explicit dated qualification. E-MAPS's
old dormant wording supplies no inherited map-parity claim. The current source
and complete map observations establish the bounded map behavior here.

Run `python3 scripts/check-class-helper-accessor-producers-readiness.py` before
editing. Preserve all five preceding readiness authorities and archive explicit
hash refreshes for the 407th compiler registration, packet/index and dependent
records. After qualification, update this packet and the affected architecture
evidence with actual counts. The user-authorized workflow uses focused complete
observations and adjacent product regressions, then hosted acceptance before
landing. Historical certificate walks and full developer CI are omitted and
are not claimed as passing. No H2.8 close or global acceptance count is implied.
