# H2.8a A6-6: declaration class dependency order

Kind: runtime at checkpoint 7ebdee933; all before observations are
recorded. The readiness check must pass before the production edit.
A6-2's original ClassExtendsVisibility ES5/ES2015 commands retain private import
Bar after namespace Strings where TypeScript puts Bar before it. This packet
owns only class serializer evaluation order and completion of its pending
statement-tracker effects before restoring class scope.

TypeScript serializeAsClass constructs heritage clauses before property,
static, constructor and index builders. Its statement tracker synchronously
finds accessible symbol-chain roots and includes private dependencies. Native
serialize_as_class constructs heritage after those builders, and the Rust
tracker's pending vector is drained only after serialize_symbol completes.
serializeSymbolWorker can emit a class then its merged namespace within that
single call; the namespace's direct include_private_symbol overtakes the earlier
heritage dependency. The late drain also observes the later/restored scope.

A6-6-1 moves the existing heritage-clause construction, without changing its
algorithms or output-node order, immediately after static-base determination
and the existing heritage approximate-length update, before class properties.
A6-6-2 calls the existing include_tracked_private_symbols at class completion,
before restoring old_enclosing and before returning to serializeSymbolWorker's
module branch. Native class serialization contains no direct private-inclusion
call, so its ordered pending requests can finish in that class scope before
later direct inclusion. The drain reuses existing symbol-chain lookup, exported
identity normalization, type-parameter visibility and source-scope checks; it
retains the existing first/last deferred stack selection and deduplication.
No statements are sorted or moved after serialization.

Allowed runtime path is crates/checker/src/node_builder/statements.rs:
serialize_as_class's existing heritage block and its completion call only.
NodeBuilderTracker storage/dispatch, include_tracked_private_symbols,
include_private_symbol, names, resolver interfaces, factories and printer are
unchanged. Existing serialize_symbol's context restore remains the fallible
outer boundary if an added drain fails. Other callback interleavings, class
implementation/JSDoc sanitation and Class|Property special alias dispatch remain
separate owners and are not certified by this bounded class-order repair.

E-PROTOCOL, E-RESOLVER-BASE and E-METADATA-BASE are premise-unchanged but rechecked:
checker and syntax identities remain borrowed in one emit request; the pending
symbols remain owned within its statement tracker; generated nodes stay in the
same arena. The class-completion consumer shortens the pending effect window
without extending a borrow or introducing a new public callback/state carrier.

The draft observer records 24 commands: eight JS shapes and four TS shapes,
each at ES5 and ES2015. It includes imported/local bases, merged namespaces,
reversed export assignment order, repeated bases, member presence,
classes without bases/merges, two derived classes, and nested TS scopes. Every
TypeScript observation contains all writes, callback metadata, diagnostics,
emit-result presence, status and exit, twice. The original require-alias band
of 22 commands and existing node-builder statement/chain controls are adjacent
regressions. Inputs and expected results must not be edited after comparison.

Pinned bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- serializeSymbolWorker: 53992–54179, SHA256 `dc9bf6e639d95e72cefd3e99de392150843321b4b1f785340d52751bbe4bbc38`.
- serializeAsClass: 54600–54686, SHA256 `ccb4c10cb58d4154e83303d911464ee3af0f91f4b9cc33d3d43e98266e113bff`.
- symbolTableToDeclarationStatements.trackSymbol: 53748–53774, SHA256 `ee1e2dc70eccbb279a3a341025a67d433df1dd5e2536ebf4208652289694d5e0`.
- visitSymbolTable: 53938–53975, SHA256 `6c24132a883411f0dd4cce6dfc03ad19ff6f3cdfeaede3e283f075c90e325235`.
- includePrivateSymbol: 54180–54186, SHA256 `47e3cfac44d1da24d0576ff68a44b9145c0b4d6d090ec814e2fb751ef8a09906`.
- serializeBaseType: 55339–55363, SHA256 `124e40de6a7801ccc545cc6c27da54f0c402d203e9a299886d38ec409c72d9d2`.

The first complete native before records all 24 commands at 7ebdee933: 16
exact twice and eight failing complete comparisons in 24.72 seconds (exit 101).
All eight TypeScript controls are exact. The eight differences are solely
private base declaration order for base-namespace, reverse-namespace,
member-after-base and private-local-base at both targets. The immutable record
is ratchets/h2-8a-class-order-before.v1.json, with source/log copies outside the
checkout as located by target/h2-8a-class-order-before-location.txt.

Inspection of the complete TS writes shows that the inline JSDoc comment in
the initial member controls is not attached: both compilers emit field:any.
Those immutable controls remain useful field/unused-dependency controls but
do not establish typed-member evaluation order. A separate four-command
observer puts the JSDoc comment on its own line. It confirms field:Member and
requires Base before Member, with and without the merged Labels namespace at
ES5/ES2015. These additional input/observation rows are separate and must have
a native before result before the class order edit. No initial input or
expected row is corrected after seeing the native result.

All four additional typed-member commands fail before the edit (exit 101,
2.08 seconds, target/h2-8a-class-member-before.log). Their complete writes
confirm two ordered effects: native emits Member before Base, and in merged
cases emits Labels before both; TypeScript emits Base, then Member, then Labels.
Comments, field types and all other displayed declaration text agree. The
full comparisons remain authoritative and will expose any additional fields
after this first mismatch is repaired. These witnesses directly support moving
heritage evaluation and draining class tracker effects before the merged scope.
The original 22-case require-alias band is also being replayed at 7ebdee933 to
preserve a complete current before for both ClassExtendsVisibility originals
and twenty adjacent commands.

The current original before finishes in 65.01 seconds (exit 101): 19 commands
are exact twice; ClassExtendsVisibility ES5/ES2015 and ReexportedCjsAlias ES2015
fail twice. The latter is a newly identified A6-5 regression: main.d.ts loses
the second destructured require alias import. Its ES5 sibling is exact. That
regression remains owned by the alias serializer and is not claimed to be fixed
by class evaluation order. The entire original 22-case test remains mandatory,
including this failed adjacent control. All six full failure tuples, sources
and log are outside the checkout, located by
target/h2-8a-class-original-before-location.txt; the manifest binds
ratchets/h2-8a-class-original-before.v1.json.

The member before is recorded in ratchets/h2-8a-class-member-before.v1.json,
with immutable external copies at the location recorded by
target/h2-8a-class-member-before-location.txt. The class readiness binds six
upstream owner bodies, two ordered steps, three architecture rows, all 28
fresh commands (12 failed / 16 exact before), both original class commands and
all twenty adjacent original dispositions. There are zero unresolved or
undispositioned rows within the class evaluation/completion boundary. The
alias regression, final full769 replay and broader H2.8 closure remain open.

The class-order after run confirms all 28 class witnesses and all 40 existing
local/ambient alias controls exact twice (four tests, 89.30 seconds). Both
ClassExtendsVisibility originals now match completely, and the original require
band is 21/22 exact twice. The additional ExportForms pair and compound
namespace original also remain exact. The original binary takes 100.03 seconds
and exits 101 solely for the already recorded ReexportedCjsAlias ES2015
regression; no new failure appears. The combined log is
target/h2-8a-class-order-after.log. Adjacent checker unit replay and the alias
regression repair remain pending; no complete H2.8a qualification is claimed.

The subsequent checker replay passes all 17 statement/chain unit tests in
0.03 seconds (target/h2-8a-class-order-adjacent.log, exit 0). The class-owned
comparisons and adjacent unit checks are green. The separate alias regression
still makes the full original 22-case test red and must be repaired before
whole-band qualification or landing.
