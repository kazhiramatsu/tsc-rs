# H2.8a A6-8: parsed and updated class transform flags

Kind: runtime at checkpoint f8ead8db9ce1f9f429d8c3bb5db36dea00cc2540.
All before observations are complete; readiness precedes production edits.
Only crates/emitter/src/builtins.rs is an allowed production path.

A6-8-1 adds the property-owned ContainsTypeScriptClassSyntax bit from an
actual computed name or a static modifier with an initializer. Existing
NodeData and modifier arrays provide these facts. After child aggregation,
a class declaration promotes that bit to ContainsTypeScript; a declare
modifier instead yields exactly ContainsTypeScript. The class local flags
must not retain the parsed NodeFlags::AMBIENT shortcut: upstream class
creation tests modifiers, unlike upstream property creation's different
ambient predicate. Class expressions retain the class-syntax bit without
promoting it to the TypeScript visitor gate.

A6-8-2 applies the same completion to flags_after_update. Recompute the
class-syntax bit only for PropertyDeclaration, ClassDeclaration and
ClassExpression, preserving the existing mask for other kinds. This avoids
stale bits when static members/initializers/modifiers are removed and admits
new bits when they are added. Parameter properties are a distinct producer;
this change neither recomputes their unrelated masks nor claims their flags.

A6-8-3 validates the actual TypeScript visitor gate and the existing class
wrapper/metadata consumers through complete commands. Twelve ES5 fresh
failures place static initialization outside the class IIFE. Two nested
ES2015 failures insert a newline before the static assignment. All fourteen
remain mandatory; if flags alone leave a metadata difference, pin its actual
owner before any amendment. No class-wrapper algorithm, class-fields pass,
ES2015 pass, factory constructor or printer edit is authorized by this packet.

E-METADATA-BASE, E-ORDER-H and E-PROTOCOL are premise-unchanged and rechecked.
Flags remain in TransformArena's existing sparse tables; updates use existing
syntax identities and child propagation. Pass registration, hook ordering,
borrowed resolver lifetime and public host/sink interfaces are unchanged.
E-SYNTAX-FACTS is not applicable: its planned scanner-token storage is not
used or closed by deriving class facts from existing parsed syntax. Missing
node references continue to return typed TransformError values.

The fresh observer records 50 complete commands twice in TypeScript 6.0.3:
12 shapes crossed with JS/TS and ES5/ES2015, plus two ambient TS controls.
The native before at this checkpoint is 36 exact twice and 14 failed
(67.72 seconds, exit 101). Exact comparisons include all writes, callback
metadata, diagnostics, result presence, status and exit. Failed fresh cases
stop at their first whole-comparison mismatch, so no unexamined field is
claimed exact. Separate parse observations cover 96 class/property nodes:
74 differ in the owned bits (ContainsTypeScript and class-syntax), with
ambient class declarations comparing the complete flag word. Nine factory
class/property update controls have six differing and three exact rows.
Those two unit tests exit 101 (0.03 seconds). A separate immutable artifact
adds six JS/TS class-expression updates: add/remove static members and replace
with computed members. Four differ and two match; one test exits 101
(0.01 seconds). Both observer scripts execute each TS observation twice.
These are flag-specific auxiliary controls, not additional complete commands.

The original projection contains ten commands, all compared twice: five
class/static-property source files at ES5 and ES2015. Before: three exact and
seven failed (46.94 seconds, exit 101). Inspection of all seven complete
failure tuples finds five ES5 JavaScript placement differences. DefaultsErr
also has an independent declaration export-name difference in both targets;
ExportAssignedClassInstance2 also has an independent declaration alias-shape
difference in both targets. Diagnostics, result, write metadata other than
text/byte lengths, source order, status and exit match in these failures.
Those four original commands remain failed H2.8a work until their checker
owners are repaired; they are not exclusions from the final 769 comparison.
The remaining six must match after the class-flag repair. The full ten-row
comparator stays unconditional and will still fail on independent residues.

Immutable before records are ratchets/h2-8a-class-transform-before.v1.json,
h2-8a-class-flags-unit-before.v1.json, h2-8a-class-expression-unit-before.v1.json
and h2-8a-class-transform-original-before.v1.json. Snapshot locations and logs
are target/h2-8a-class-transform-before-location.txt,
target/h2-8a-class-flags-unit-before-location.txt,
target/h2-8a-class-expression-unit-before-location.txt and
target/h2-8a-class-transform-original-before-location.txt. Inputs and expected
results remain unchanged after these before runs. Required checks are all
50 complete fresh commands, all three auxiliary tests, the ten original
commands with the above explicit residual review, and adjacent builtins unit
tests. The global 769 replay and all remaining H2.8a–e work stay open.

Pinned TypeScript 6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- createPropertyDeclaration: 21890–21902, SHA256 `2f7df66cb0e988da54fd316bcba05db7e36dc1c9f8472f465001a4007f1dd4bb`.
- updatePropertyDeclaration: 21903–21905, SHA256 `993e39266d70e68575e3d46b2efaf63e3642f3cf00fecf2087d108c103260d68`.
- createClassDeclaration: 23339–23356, SHA256 `4116022701afc60714ac17403b6470636b06686462a2951b6a090fab0c87f1c9`.
- updateClassDeclaration: 23357–23359, SHA256 `e9a9bbea29832f1ee7b89c669fcbdabf91cf8994cb643fdf51f0953f531859af`.
- createClassExpression: 22927–22937, SHA256 `e9fff720f97ab0c52675bfb3eb7245496aca23e0ef31bfc8366d8f0d1fb8c648`.
- updateClassExpression: 22938–22940, SHA256 `b0a96dc25891132cbab5d16e0aab9ff6e8d83d92a2d24008e9ccd0bc918b2c3a`.
- propagateNameFlags/propagateChildFlags/aggregateChildrenFlags: 25101–25124, SHA256 `1b9a360a71ed509adb3bb36c556cd776667962a04c2925054756d63b9df79999`.
- getTransformFlagsSubtreeExclusions: 25125–25194, SHA256 `2d364dcf4298f054e648486f6e466f4b82d973fc80597df00ed06d9c612aa913`.
- visitorWorker: 94122–94127, SHA256 `3e070025ada6ea8db623872da91537a2ee373fdb66224c5ece5786511d972198`.
- getClassFacts: 94410–94426, SHA256 `96527ff84ba078ef8ea3d635bffc120f5ccf13c9441ff83640e65831989f4261`.
- hasTypeScriptClassSyntax: 94428–94430, SHA256 `2cc078fafd2c2ed3cbe7e1a48d8fea5fb75a6f220c96862ab9a99d800da0663b`.
- isClassLikeDeclarationWithTypeScriptSyntax: 94431–94433, SHA256 `f99b70774885874532bc1947c7b742b2f3257b1a97de87676502becfef31dab8`.
- visitClassDeclaration: 94434–94548, SHA256 `b4f4c7bb3c8f14a7776dd0ab5337e8c11b30104d7eb70b676c5dba79a9e1ae59`.
- visitClassExpression: 94549–94563, SHA256 `4dae4f7a40f66795c79f1667200c7b9d9d63898d21391eeda09415cd761b4605`.

The first flag-only after passed all 98 builtins unit tests but left four
nested-body layout failures and exposed a computed-key temporary collision.
The [class statement amendment](h2-8a-class-statement-layout.md) pins those
additional reached owners and preserves the complete first-after evidence.

The completed amendment after matches all 114 fresh complete commands twice
(two tests, 124.43 seconds, exit 0), including every initial layout
and computed-name failure. All 483 emitter unit tests pass (0.65
seconds), and all 451 emitter contracts pass (1.40 seconds).
The original ten-row comparator executes both repetitions and has six exact
commands; the four previously recorded declaration-text residues remain failed
whole commands (35.53 seconds, exit 101). An explicit complete-tuple
review confirms all ten JavaScript outputs now match; all other fields match
in the four failures, and their declaration differences are identical to the
immutable before. The unconditional original comparator is retained.
The bounded after record is ratchets/h2-8a-class-layout-after.v1.json, with
source/log/tuple copies at target/h2-8a-class-layout-after-location.txt.
Global 769, remaining output work and H2.8b–e remain unqualified.
