# H2.5h-a — ES2015/Generators architecture and design packet

Readiness: **blocked**

This is the concrete handoff for the first post-H2.5g design-gated work. It is
not an implementation specification yet and authorizes no production edit.
Its readiness and active-slice status are owned by the
[slice-packet index](README.md) and the
[post-H1 schedule](../post-h1-completion-slices.md), not by this draft in
isolation.

The block is intentional and mechanically fail-closed: the required final
H2.5g profile and architecture freeze at an immutable final validation ref,
recorded merge-ref lineage, versioned slice-readiness
manifest/schema/checker, complete owner/local-gap inventories, Rust mapping,
and frozen witnesses do not yet coexist.

For the future packet, vendored TypeScript 6.0.3 owns semantics. The current
code and tests plus freshly revalidated rows in
[the current emitter architecture](../emitter-architecture.md) own Rust facts.
Earlier H1, residual, inventory, and emitter-design documents provide rationale
or history only; they are not implementation instructions.

## Prerequisite transition

The post-H2.5g roadmap-review branch performs these steps in order:

1. verify that every qualified H2.5g row in
   [the current emitter architecture](../emitter-architecture.md) names the
   immutable implementation/evidence commit as its final validation ref, then
   record the actual merge ref separately as delivery lineage and prove that
   it contains that validation ref with every profile-bound runtime/evidence
   input byte-identical; never substitute the merge ref for the validated
   commit;
2. generate the complete pinned `transformES2015`, `transformGenerators`,
   resolver, syntax-fact, factory, helper, printer, comment, and module
   composition owner graph;
3. generate the current Rust local-gap matrix without editing production code;
4. decide owner SCC boundaries and amend H2.5h-a/H2.5h-b suffixes before
   assigning implementation files;
5. add the shared versioned slice-readiness schema and checker, then replace
   this blocked handoff with the complete packet required by
   [the mandatory design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate);
6. freeze oracle-produced positive, adjacent-negative, composition, and fault
   witnesses; and
7. run the packet checker. Production work begins only when it reports fresh
   hashes, zero undispositioned rows, zero unresolved rows, full
   architecture/upstream/local-gap/step/test coverage, and legal lifecycle
   transitions.

If the ready packet adds an H2.5h ts-tests runner to hosted acceptance, it must
also specify the reviewed change to the canonical `acceptance` body and the raw
source SHA-256 pins in `.github/ci/qualification-policy.v2.json`. It must prove
that no owner-control runner became reachable there; owner controls stay in
the complete local gate. Updating a pin without the corresponding reviewed
source/entry transition is not evidence.

The readiness checker command and expected counts/hashes are deliberately not
invented here: they are outputs of the versioned post-merge research/design
slice. Their present absence is itself the blocking condition. An agent that
encounters this page before that transition stops after read-only inventory;
it does not create ES2015 or Generator runtime code.

## Mandatory architecture inputs

The ready manifest must enumerate every row in the current architecture map
exactly once, including entry/protocol/plan/output and dormant future-product
rows. Each row receives one applicability disposition:
`premise-unchanged`, `modified-requalify`, `activate`,
`future-owned-fail-closed`, or `proven-unreachable`. A wildcard in prose does
not satisfy this requirement, and an omitted supposedly unrelated row is an
undispositioned row.

At minimum, the owner graph must treat the following current and planned
sub-rows as reachable inputs unless the pinned research supplies a
`proven-unreachable` proof:

- `E-ARENA`, `E-RESOLVER-IDENTITY-G`, `E-CONTEXT`, and `E-SYNTAX-FACTS`;
- `E-METADATA-BASE`, `E-METADATA-G`, `E-METADATA-G-CLASS`, and
  `E-JSX-FACTORY-G`;
- `E-CAPTURE-BASE`, `E-CAPTURE-CLASS-G`, `E-CLASS-PENDING-G`,
  `E-DECORATOR-INITIALIZERS-G`, `E-DECORATOR-CLASS-INITIALIZERS-G`, and
  `E-DECORATOR-PARAMETER-PROPERTY-G`; the packet must state how every
  ES2015/Generators class or wrapper consumer preserves all three independent
  ordered lanes (class-definition pending effects, member `addInitializer`
  queues, and the exactly-once class-decorator finalizer), their
  consumer-specific placement and receiver selection, the synthesized
  `PropertyDeclaration -> Parameter` provenance chain, and constructor-local
  parameter-property materialization in every target/field-mode route;
- `E-ORDER-G` and `E-ORDER-H`;
- `E-RESOLVER-BASE`, `E-RESOLVER-CAPTURE-BASE`, and
  `E-RESOLVER-CAPTURE-H`;
- `E-CHECKER-FACTS-BASE` and `E-CHECKER-FACTS-H`;
- `E-NAMES-BASE`, `E-NAMES-CLASS-G`, and `E-NAMES-H`;
- `E-HELPERS-BASE`, `E-HELPERS-PROVENANCE-G`, and `E-HELPERS-H`;
- `E-PRINTER-BASE` and `E-PRINTER-G`;
- `E-COMMENTS-G` and `E-COMMENTS-H`; and
- `E-POSITIONS` and `E-STRINGS`.

The complete packet must also disposition:

- `EA-GAP-FLAGS`, `EA-GAP-CAPTURE`, `EA-GAP-COMPOSITION`, and
  `EA-GAP-MAPS-DECLS`; the last remains `future-owned-fail-closed` under
  H2.6/H2.7 unless the pinned graph proves that H2.5h must requalify a shared
  provenance fact;
- the exact ES2015 -> Generators -> module pass order and every hook/finalizer
  composition edge; and
- every inherited deferral, revalidated with its guard, earliest owner, typed
  failure boundary, and adjacent-negative control.

No `TBD`, implied repository search, remembered tsc behavior, data-model
choice, unnamed file edit, or hand-authored expected output is permitted in
the ready packet.
