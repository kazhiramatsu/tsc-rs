# H2.5h / CA-2a — corpus-adoption seam closures, the ES2015/wrapper cluster: promote lanes, comment ownership, generated names, static placement, the super fold, void-0 initializers

Status: design-gate packet for the third H2.5h corpus-adoption
packet, closing the CA-2 split's second half. The CA-2b-final
census leaves **185 failing rows = 63 promote-lane typed-seam rows
+ 122 write-diffs**, all owned by the six families below. The §5
recon (verified in-tree at the trusted base) shows five of the six
families are precise GAP FIXES on machinery B-4/B-5 already landed;
only the A+D promote lanes add new production lanes.

## 1. Identity, purpose, and boundary

- **Slice ID / kind:** `h2-5h-ca-2a`, kind `runtime`.
- **Purpose:** make the remaining 185 census rows execute
  byte-exact against the frozen CA-1 observations by closing:
  - **A — promoteToIIFE exported/namespace/decorated lanes** (63
    typed-seam rows): the four-condition fail-closed guard in
    `promote_class_declaration_to_iife` opens per upstream's
    `moveModifiers` branches;
  - **B — comment ownership** (~34 rows): (i) the synthesized-ctor
    body range stamping (class doc comment duplicated onto the
    inner ctor), (ii) dropped comments in ctor lanes, (iii) the
    detached file-header comments must precede the emitted helper
    block;
  - **C — generated names** (~19 rows): (i) the rest-loop `_i`
    per-printer-scope reservation, (ii) the assigned-name harvest
    for anonymous class/function expressions, (iii) hoist-numbering
    order in async/for-of composition;
  - **D — static-property placement** (~10 rows): statics inside
    the wrapper before `return C;` in the non-promoted and
    converted-loop compositions;
  - **G — the captured-this + super fold** (2 rows): the landed
    simplify passes' pattern-match gap under `_super`-collision
    renames;
  - **H — void-0 hoisted-temp initializers**: the synthesized
    block-scoped class temps must receive `= void 0` — the lane is
    landed; the es2015 predicate's parse-tree early-return skips
    synthesized declarations (§5.2).
- **Non-goals:** the project harness (CA-3); acceptance/gate
  wiring, activity/admission model (CA-4); any edit to the CA-1
  artifact's observations; any behavior change for targets ≥
  ES2015 (the corpus ratchet enforces byte-identity).
- **Prerequisites:** CA-1 (`a19bc3d7`, PR #468) and CA-2b
  (`602866a8`, PR #469) merged.
- **Trusted base:** `602866a8f420f235f0d1bd65c67a1ed971c89e9b`
  (current `main`).
- **Activation state:** before — 185 census rows diverge (63
  typed-seam + 122 write); after — the A-lane typed seams retire,
  the write families flip byte-exact (§9 records the measured
  final count; any residual is re-classified with a typed deferral
  + review), every existing band byte-identical, the CS-6 30-case
  and B-5 32-case witness gates green.
- **Next owner:** CA-3 (project harness), then CA-4 (wiring).
- **Authority artifacts:** the CA-1 artifact
  (`ratchets/h2-5h-qualification.v1.json`, in-train `--check`);
  `vendor/typescript-6.0.3/lib/_tsc.js`
  `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`;
  the CA-2b packet (census lineage); the verified pinned-upstream
  map and the preserved baseline (census log + per-family case-id
  lists + 122 byte-pair dumps), reproduced per §9.4.

## 2. Position in the ladder

The CA-2 split (ratified in CA-2b §2) names this packet as the
second half. Baseline (measured at the trusted-base bytes, full
850-row census): 806 admitted / 44 deferred(H2.9); 185 failing =
63 `A` + 122 write-diffs carrying `B/C/D/G/H` (families overlap
per case; the per-family sizes are sizing evidence, not a
partition). After CA-2a the corpus-adoption band's execution
surface is complete for compiler+conformance; CA-3 adds the 82
project rows; CA-4 wires `run_h2_5h`.

## 3. Required-reference table

| Reference | Role | State |
| --- | --- | --- |
| `ratchets/h2-5h-qualification.v1.json` | frozen expectation store | read-only (walk pin-rebinds only) |
| `crates/emitter/src/builtins.rs` `promote_class_declaration_to_iife` + `typescript_class_facts` (facts `96527ff8…`, visit `b4f4c7bb…`) | A surface — the four-condition guard (`decorated \|\| namespace_stack non-empty \|\| export \|\| default` → typed error) | edited (lanes open) |
| `crates/emitter/src/builtins/class_fields/downlevel.rs` (the ES5-band class-fields lane — `class_fields.rs:84-95` dispatches every target < ES2022 here; `class_temp_plan` :3331-3341, `prepend_loop_binding_declarations` :7219, the statics routing for wrapped classes) | D surface (+ H context) | edited |
| `crates/emitter/src/builtins/class_fields.rs` (wrapper/substitution + the ≥ES2022 `prepend_hoisted_declarations` lane — NOT the ES5 path) | reference | read-only |
| `crates/emitter/src/builtins/es2015.rs` (`add_constructor` `223c0cfd…` @8331 range-stamp verified upstream-exact; `transform_constructor_body` synthesized arm; `simplify_constructor` call @8613; passes @9098/9306/9593; `get_name` `9734f557…` @4908; `allocate_loop_variable_binding` @1141; `should_emit_explicit_initializer_for_let_declaration` @5382 verified exact) | B/C/G surfaces | edited |
| `crates/emitter/src/builtins/generated_bindings.rs` (`allocate_loop_variable` — the `_i` slot machinery, `makeTempVariableName` hash pinned) + `target_bindings.rs` (:454 finalize scope walk, :585-590 ReuseTempVariableScope arm) | C(i) surface — the finalize-time per-scope loop-family assignment | edited |
| the printer source-file entry (`crates/emitter/src/printer.rs`) | B(iii) surface — the detached-comments wrapping predicate | edited |
| CS-6 30-case + B-5 32-case witness gates; every B-train focused projection | frozen guards (byte-identical) | read-only |
| the census harness (scratch worktree, CA-2b §9.4 incl. the blocked-row contract) | verification instrument | scratch |

## 4. Pinned upstream map (spans = `_tsc.js`; every span read verbatim; empirical notes from the instrumented-compiler recon)

### 4.A+D The wrapper chain (three transformers)

- ts.ts `getClassFacts` **94410-94427** (facts bit 1 = static
  initialized props) and `visitClassDeclaration` **94434-94548**:
  `promoteToIIFE = languageVersion < ES2015 && (facts & 7)`
  @94436; the wrapper = `createImmediatelyInvokedArrowFunction`
  with `InternalEmitFlags.TypeScriptClassWrapper` @94474-94486;
  the wrapper variable list is `Let` @94505-94507 (es2015
  block-scoping later downlevels it); **the export lanes @94515-94546**:
  the wrapper varStatement is created with `/*modifiers*/ void 0`
  (94502-94506) — modifiers are ELIDED, never moved — and the
  export is re-expressed as a SEPARATE TRAILING statement
  (`createExportMemberAssignmentStatement` /
  `createExportDefault` / `createExternalModuleExport`; the es2015
  equivalent 105168-105172), lowered by the module transform to
  `exports.C = C;` / `exports.default = D;` after the wrapper
  (empirically verified for plain, `export`, `export default`:
  statics are INSIDE in all three; the emitted form is never
  `export var C = …`).
- classFields `visitClassDeclarationInNewClassLexicalEnvironment`
  **96971-97045**: statics lowered to `C.a = …;` statements
  injected INSIDE the wrapper body between the class declaration
  and its `return` @97004-97009
  (`addPropertyOrClassStaticBlockStatements` on
  `getInternalName`).
- es2015 `visitCallExpression` **107667-107669** →
  `visitTypeScriptClassWrapper` **107687-107783** (B-4 landed):
  splices the nested IIFEs into one — inner class-IIFE statements
  to its return, then the wrapper's remaining statements (statics,
  return); alias-assignment shape 107699-107725;
  `isVariableStatementOfTypeScriptClassWrapper` 106382-106384;
  the force-visit gate 104805. Wrapper order: [`__extends`?,
  ctor, members, pendingExpressions, static assignments in member
  order, `return`].

### 4.B Comment ownership

- (i) `addConstructor` **105263-105289**:
  `setTextRange(constructorFunction, constructor || node)` @105282
  — comment suppression is a transform+printer COOPERATION: the
  outer VariableStatement (`setTextRange(statement, node)`
  @105165) emits the class's comments and advances the GLOBAL
  `containerPos` (printer **121012-121022**, where the @121020
  disjunct `!skipLeadingComments || pos >= 0 && NoLeadingComments`
  means even a NoComments node with pos >= 0 STILL advances it);
  `forEachLeadingCommentToEmit` **121219-121233** then skips when
  `pos === containerPos`, so the synthesized ctor (ranged to the
  class per @105282) never re-emits the class doc. Stamping facts
  (independent-review-verified): `createDefaultConstructorBody`
  **105293-105309** ranges the statements array to `node.members`
  @105300-105301 AND ranges the Block to `node` @105307 with
  NoComments @105308 — the ctor-body Block is NOT pos=-1 (that
  description belongs to `transformClassBody`'s CLASS-BODY block
  @105238-105247, a different block). OUR synthesized lane
  (es2015.rs:8407-8409) is already upstream-identical on the
  Block; the actual Rust deltas are (a) the MISSING NodeArray
  ranges (our `create_block` es2015.rs:1777-1799 never ranges the
  array; upstream ranges it at 105300-105301, 105373-105381, and
  105239-105243) and (b) the PRINTER side: upstream's containerPos
  is global and survives from the VariableStatement across the
  wrapper chain, while our printer's one-edge local projection
  (printer.rs:9795-9890) does not thread the VariableStatement's
  claim through classFunction(pos=-1) → body → list to the ctor —
  so the duplicate prints from the parent-side phase despite the
  upstream-exact stamping.
- (iii) `emitSourceFile` **119710-119719**:
  `shouldEmitDetachedComment = statements empty || statements[0]
  not a prologue || nodeIsSynthesized(statements[0])` →
  `emitBodyWithDetachedComments(node, statements,
  emitSourceFileWorker)` (**121075+**). With the synthetic
  `"use strict"` at [0], the file's detached header comments print
  between the prologue (file level, 117077) and the worker
  (`emitHelpers` 119757 → statements): order = prologue → detached
  header comments → helpers → statements (oracle-matched).

### 4.C Generated names

- (i) es2015 `addRestParameterIfNeeded` **105818-105917** (temp =
  `factory.createLoopVariable()` @105831, GeneratedIdentifierFlags
  Loop) → printer `makeName` case Loop **120950-120959** →
  `makeTempVariableName` **120703-120740**: `_i` = TempFlags
  0x10000000, tried first per name-generation scope
  (`pushNameGenerationScope` **120480-120492** resets per
  function); the sequential fallback skips ordinals 8/13 (never
  `_i`/`_n`); **EmitFlags.ReuseTempVariableScope (1048576, the
  es2015 class-IIFE function @105203 and async-generator bodies)
  does NOT reset** — counters continue across the wrapper.
- (ii) `getNameOfDeclaration`/`getAssignedName` **11562-11580**:
  for anonymous function/arrow/CLASS expressions the name falls
  through to FOUR assigned-name sources — a PropertyAssignment OR
  BindingElement parent's `name`; a BinaryExpression-RHS
  position's LHS identifier OR access-expression name (via
  `getElementOrPropertyAccessArgumentExpressionOrName`, covering
  `A.b = class {}`); a VariableDeclaration identifier name —
  cloned UNCONDITIONALLY (no shadow analysis); factory `getName`
  **24788-24802** (clone stamped NoComments|NoSourceMap;
  InternalName exempts module-export substitution); the
  non-contextual-keyword swap at es2015 **105223-105224**;
  fallback = printer `generateNameForClassExpression`
  **120847-120862** (`class_1`).

### 4.G The captured-this + super fold

`transformConstructorBody` **105343-105389** builds the naive form
(prologue `var _this = this;` + the visited
`_this = _super.call(this, …) || this;` from
`visitCallExpressionWithPotentialCapturedThisAssignment`
**107784-107830**, the `|| this` production @107815-107821), then
`simplifyConstructor` **105625-105636** folds: pass 1
`simplifyConstructorInlineSuperInThisCaptureVariable`
**105429-105485** (find `var _this = this;` followed — possibly
across uninitialized var statements — by the transformed super
call; rewrite the declaration's initializer), pass 2 inline-super
return, pass 3 elide the unused capture. Synthesized-super lane:
`createDefaultSuperCallOrThis` **105656-105671**,
`hasSynthesizedDefaultSuperCall` **108074-108100**,
`complicateConstructorInjectSuperPresenceCheck` **105580-105624**.
Prologue order: defaults → rest → new.target capture → this
capture (**105360-105367**), all CustomPrologue.

### 4.H The void-0 initializer chain

`shouldEmitExplicitInitializerForLetDeclaration` **106448-106454**
(resolver NodeCheckFlags CapturedBlockScopedBinding=16384 /
BlockScopedBindingInLoop=32768 + hierarchy facts) +
`visitVariableDeclarationInLetDeclarationList` **106455-106472**
(`createVoidZero` on initializer-less converted lets). The class
temp reaches it via classFields `createClassTempVar`
**97056-97068** (flag read @97054): `requiresBlockScopedVar =
resolver.hasNodeCheckFlag(node, BlockScopedBindingInLoop)` on the
class expression → `addBlockScopedVariable` temps materialized by
the transformation context's `endBlockScope` **116219-116238** as
a `Let` list, which es2015 then converts. Plain
`hoistVariableDeclaration` temps stay bare `var _a;`.

## 5. Rust design (per-family fix loci, recon-verified at the trusted base)

1. **C(i)** — the `_i` machinery is LANDED
   (`generated_bindings.rs allocate_loop_variable`: the per-scope
   `loop_temp_taken` slot with the exact free-check semantics) but
   es2015 allocates in ONE visit-time scope (it never `.enter()`s
   per function, unlike es2017/es2018/es2021), so the second
   rest-param function falls to sequential `_a` and the
   planned-authoritative finalize keeps that spelling. Fix in the
   FINALIZE walk (`target_bindings.rs` :454 scope walk — our
   printer-scope equivalent; :573 already implements the
   ReuseTempVariableScope arm): assign the loop family per printer
   scope — first loop temp per scope takes `_i` when free, others
   fall to the temp sequence — mirroring the B-3 finalize-write
   precedent (label literals) and B-4's `renumber_state_bindings`.
   **C(iii) STOP-CONDITION FIRED (2026-08-23 night, the step-1
   census at e14bcbf4):** the draft claimed the finalize assignment
   owns the async/for-of hoist-numbering order; the measured
   residual (operationsAvailableOnPromisedType: upstream hoists
   `_i, c_1, …` in the generator group and `…, c_2, c_2_1` in the
   second group; ours swaps c_1/c_2) shows the divergence is the
   NUMBERED family's allocation order across the
   async(es2017)+for-of(es2015) composition — a different
   mechanism from the loop slot. C(iii) is re-scoped as its own
   bounded investigation inside step 1's family (diagnose which
   pass allocates each `c` binding and mirror upstream's
   numbering order); its rows stay step-1 witnesses and the
   packet's measured-final-count acceptance (§9.3) carries them.
2. **H** — the block-scope route is LANDED AND WORKING
   (review-verified): `class_temp_plan` reads
   BLOCK_SCOPED_BINDING_IN_LOOP
   (`class_fields/downlevel.rs:3331-3341`, mirroring 97054/97062),
   and `prepend_loop_binding_declarations` materializes a
   `NodeFlags::LET` list (downlevel.rs:7219) — the witness dump
   shows `var _a;` at exactly upstream's position, missing ONLY
   `= void 0`. The real blocker is
   `should_emit_explicit_initializer_for_let_declaration`'s
   parse-tree guard (es2015.rs:5386-5388):
   `parse_tree_resolver_node(node)? else return Ok(false)` — the
   SYNTHESIZED Let declarations have no parse-tree reference, so
   the predicate never runs. Upstream (`hasNodeCheckFlag`
   @88560-88564) returns false PER FLAG for synthesized nodes but
   still evaluates the full predicate, which yields void-0 in
   non-top-level, non-for-header contexts. Fix = handle the
   no-reference case per upstream: flags treated false,
   colliding-name false, the full predicate still evaluated. The
   fix lives in es2015.rs (allowed); `class_fields/downlevel.rs`
   joins the allowed surface for the D-family statics routing.
3. **G** — the fold passes are LANDED and pass 1's conditions are
   ALREADY upstream-exact (call site es2015.rs:8613; definitions
   @9098/9306/9593 — review-verified against 105429-105485). The
   real locus: under eager E-NAMES allocation the synthetic super
   parameter is spelled `_super_1` AT CREATION when a source
   parameter named `_super` collides, so `is_synthetic_super`
   (es2015.rs:8802-8815, matching text == "_super") never fires —
   upstream folds at transform time while idText is still
   "_super" and renames only at print. Fix = binding-identity /
   planned-base matching in the `is_synthetic_super`-family
   predicates (not the pass conditions), dump-driven unit repro
   first (collisionSuperAndParameter).
4. **C(ii)** — our `get_name` (es2015.rs:4908, hash-pinned to
   getName) reads only the node's own `name` field; add the
   assigned-name fallback arm (parent inspection:
   VariableDeclaration name / PropertyAssignment name /
   assignment LHS identifier) for
   ClassExpression/FunctionExpression/ArrowFunction, with the
   clone/flag semantics of §4.C(ii). The keyword swap exists at
   the caller.
5. **B(i)/(ii)** — `add_constructor`'s function-range stamp AND
   the synthesized ctor-body Block stamping are upstream-exact
   already (@8331, 8407-8409). Two real fixes per §4.B(i): (a)
   add the missing NodeArray ranges where upstream ranges them
   (the ctor-body statements array → `node.members` in both
   synthesized 105300-105301 and explicit 105373-105381 lanes,
   and the class-body array 105239-105243) — extend
   `create_block`'s callers or range explicitly at the three
   sites; (b) the printer-side dedup threading: propagate the
   VariableStatement's claimed comment container through the
   class-wrapper chain (classFunction pos=-1 →
   parenthesized/partially-emitted → body block → statement list)
   so the ctor's leading-comment scan sees `pos == containerPos`
   — within OUR architecture this is a claim-propagation arm of
   the CS-era `EmitContext`/`CommentEmissionScope` threading for
   the wrapper chain (the local one-edge projection at
   printer.rs:9795-9890 is the gap), mirroring upstream's
   global-containerPos semantics for exactly this path. The
   dropped-comment rows (~6) are measured per-dump during
   implementation and expected in the same threading family; a
   different mechanism stops the train for a packet amendment.
6. **B(iii)** — the printer's source-file entry wraps with the
   detached-comments pass under `emitSourceFile`'s predicate
   (§4.B(iii)); the CS-era comment machinery already owns detached
   comments — the fix is the wrapping predicate/position at the
   source-file level.
7. **A+D** — open the four guarded lanes in
   `promote_class_declaration_to_iife` per `moveModifiers`
   (94517-94545): exported/default lanes move the modifiers onto
   the wrapper statement with the export binding emitted after the
   wrapper (composition with the module transformers is
   census-covered across CJS/AMD variants); the namespace lane
   binds through the namespace machinery; the decorated lane
   composes with the legacy-decorator lowering. Verify/fix the
   class-fields statics routing for wrapped classes (statics must
   land INSIDE the wrapper between the class and the return,
   97004-09) — the D rows witness statics landing outside in
   non-promoted lanes today. The es2015 flatten is B-4-landed;
   the 32-case witness gate freezes its plain-lane bytes.

Implementation order = steps 1..7 above (smallest/independent
first; each step census-verified on its family case list before
the next; fmt/clippy batched before the first walk).

## 6. Gap delta

The `h2-5h-a-gap-matrix` capability rows stay 13/0/0 (fidelity
repairs + lane openings inside existing capabilities). The A-lane
typed-seam error retires when the lanes open — the
`pass-registration-boundary`-style anchors naming the seam message
re-anchor on the opened lanes (walk-managed re-mints).

Per-site local-gap classification:

| Site | Classification | Step |
| --- | --- | --- |
| finalize loop-family assignment (target_bindings) | partial-or-stale | 1 |
| class_fields hoisted-alias block-scope lane | partial-or-stale | 2 |
| simplify pass-1 match under renames | partial-or-stale | 3 |
| get_name assigned-name arm | missing | 4 |
| synthesized-ctor body range stamping | partial-or-stale | 5 |
| printer source-file detached-comments wrap | partial-or-stale | 6 |
| promote lanes (export/default/namespace/decorated) | missing (typed fail-closed today) | 7 |
| class_fields statics routing for wrapped classes | partial-or-stale | 7 |

## 7. Implementation plan and file surface

Steps as §5; per-step focused evidence:

1. C(i): unit projections for the multi-function rest fixtures
   (fresh-oracle bytes; collisionArguments* dumps as sources);
   finalize-walk unit contracts for the scope reset + the
   ReuseTempVariableScope continuation.
2. H: the classExpressionWithStaticProperties3 dump shape as a
   focused projection (ES5 polarity) + the ≥ES2015 inertness
   projection.
3. G: unit repro from the collisionSuperAndParameter dump; both
   fold polarities (folds when matched; stays split only where
   upstream splits).
4. C(ii): projections for `var y = class {}` /
   `{ k: class {} }` / `y = class {}` (oracle bytes) + the
   `class_1` fallback control.
5. B(i): the accessorAccidentalCallDiagnostic dump as the
   duplication witness; the explicit-ctor control keeps its own
   doc comment.
6. B(iii): the asyncFunctionTempVariableScoping/awaitUsing dumps
   (comment-before-helpers) + a no-comment control.
7. A+D: per-lane projections (export/default/namespace/decorated
   × statics placement) from the promote-family dumps + the
   census A-list sweep to zero.

Allowed files: `crates/emitter/src/builtins.rs`,
`crates/emitter/src/builtins/{es2015.rs,class_fields/downlevel.rs,generated_bindings.rs,target_bindings.rs}`
(`class_fields.rs` itself only if the wrapper dispatch demands),
`crates/emitter/src/printer.rs`, their focused test files
(`crates/emitter/tests/unit/{builtins,es2015,generated_bindings,target_bindings_tests.rs,…}`,
`crates/emitter/tests/integration/active_transform_contract.rs`),
plus the §8 evidence/doc surfaces and walk-managed carriers.
Forbidden: `crates/checker` (no checker change in this packet),
`crates/harness`, `crates/compiler` (production), `crates/xtask`,
`.github/workflows`, the module-transformer lanes CA-2b landed.

## 8. Evidence, ratchet, and documentation amendments

1. `h2-5h-a.md` item-4 CA-2a LANDED marker + the census final
   count; 2. README row; 3. envelope `h2-5h-ca-2a` (ready;
   predecessors = [h2-5h-ca-2b receipt]) + bootstrap; 4. chain
   walk = the CA-2b crate-byte cascade (h1 ladder → transition →
   1a → wave → 3c/3d → 5g pair → 5h adoption rebind → 5h-a chain
   with es2015-witnesses LAST → l0-source-options if an owner sha
   moved → battery + registry + readiness + pin-sweep); closure
   grows only if new test FILES are added (NEW_RUNTIME_INPUTS +
   identity + schema min/max, the CA-2b precedent). The 32-case
   witness gate runs in the suites — if a B-family fix changes a
   witness byte, STOP: the witness bytes are frozen oracle truth;
   re-examine the fix (the gate is the guard, never re-mint to
   fit).

### §8-A. Implementation-time amendments (2026-08-24, recorded at the stop points)

1. **C(iii) design correction — naming-moment order, not flat tree
   order.** The first fix attempt (flat print-tree ordinals in the
   finalize walk) broke the frozen loop-state oracle
   (`projects_loop_capture_labeled_break`: outer `state_1`, inner
   `state_2`). Verbatim-reading `generateNames`
   (`_tsc.js:120515-120596`) settled the real discipline: the
   pre-pass walks STATEMENT structure only — it never descends
   into function expressions, and it inlines only
   `ReuseTempVariableScope` FUNCTION DECLARATIONS
   (120568-120574) — so a nested function body's generated names
   resolve when its own emission pass runs. Ordinals therefore
   sort by (naming-moment path, in-scope sequence): parents
   before children, tree order within one scope. Landed as the
   two-phase source-numbered assignment in
   `target_bindings.rs` (`EnterNamingMoment`/`ExitNamingMoment`
   moment boundaries for reuse-flagged function EXPRESSIONS,
   which keep the parent uniqueness scope but delay the moment;
   phase 2 pre-assigns the numbered family in sorted order; the
   per-event numbered arm is now unreachable).
2. **B(i) landed shape — part (b) REVERTED at the hosted gate.**
   Part (a) as designed: the statements NodeArray ranges at the
   three block constructions (`transform_class_body`,
   `create_default_constructor_body`, `transform_constructor_body`;
   new factory `set_node_array_text_range`, the `update_node_array`
   range idiom). Part (b) — the ambient-claim consultation in the
   multi-line Block statement loop — FIXED the wrapper dup and
   passed every local suite, but the hosted 5g acceptance
   falsified the model: `_tsc.js`'s `containerPos` is LINEAR
   printer state (the last node emitted with a source position,
   121012-121022), not an ancestor-scoped claim. h2-5g case 4119
   (`systemModule7.ts#default`) is the counter-example: a
   preceding RANGED SIBLING subtree re-claims `containerPos`
   before the statement, so upstream prints the module's leading
   comment where the ancestor-claim consultation suppressed it
   (a nearest-ranged-ancestor refinement was tried and also
   falsified — the re-claimer is a sibling, not an ancestor).
   The consultation is REVERTED (the 5g band is frozen oracle
   truth and gates hosted acceptance); the wrapper dup-comment
   family is the named residual **h2-5h-ca-2a-r5**, pending a
   faithful linear `containerPos` model in the printer (a
   deliberate CS-era architecture change, its own design gate).
3. **B(iii) landed shape.** The printer's detached-comment
   order machinery (prologue → detached header → helpers →
   statements) and the ownership predicate were already exact;
   the defect was upstream of both: es2015
   `visit_source_file` rebuilt the SourceFile statements array
   WITHOUT the `updateSourceFile` `setTextRange(...,
   node.statements)`, so the predicate's Original-range gate
   failed. Landed as the array range copy in
   `visit_source_file`. Oracle probes pinned the blank-line
   split rule both ways (detached header before helpers WITH a
   blank line; attached comment stays with its statement
   without one).
4. **A/D landed shape.** The four-condition guard is OPEN —
   including the decorated lanes: upstream promotes
   unconditionally at `languageVersion < ES2015` (94436/94448)
   and both decorator transformers plus the `__metadata`
   machinery exist in-tree, so the seam refusal (not a missing
   owner) was the only blocker. `moveModifiers` landed as
   modifier elision inside the promote
   (`elide_moved_class_modifiers`) + the external-module lane
   split in the new `visit_top_level_class_declaration`
   (SourceFile dispatch arm): named export → wrapper statement +
   `export { X }`; default → wrapper + `export default X`;
   namespace-exported classes ride the EXISTING
   `visit_namespace_exported_declaration` splice. The census is
   the judge for the decorated subset; rows that still diverge
   re-classify per §9.3's typed-deferral clause.

5. **Census-driven residual fixes beyond the six §5 families** (the
   census surfaced these at the packet's write-diff sweep; each fixed
   in-train with an oracle-pinned witness): the printer NUL escape
   (`getReplacement`, `_tsc.js:16301-16310` — `\0`, or `\x00` before
   a digit; 12 rows), the AMD/CJS dynamic-import ES5 forks
   (`createImportCallExpressionAMD/CommonJS`,
   `_tsc.js:111009-111167` — function expressions below ES2015 plus
   the `"".concat` sync-eval coercion; 9 rows), the derived
   generated-binding flavor (`getGeneratedNameForNode` over a
   generated identifier: for-await result/catch temps re-derive from
   the base binding's FINAL spelling; metadata
   `generated_binding_derived_from` + the phase-1 upgrade-merge for
   Generators-rebuilt identifiers), and the multi-line list separator
   fallback (`siblingNodePositionsAreComparable`: non-comparable
   sibling positions in a `MultiLine` list take one line, never the
   same-line space).
6. **Census final (the merge head, post-revert): 185 → 79**
   (0 blocked; all write diffs; the pre-revert bytes measured 58 —
   the §8-A.2 revert returns the 21 wrapper dup-comment rows, now
   the named residual h2-5h-ca-2a-r5), measured on the §9.4 harness with all four scratch
   components restored (the frozen `patch_census.py` now carries the
   census command, the DUMP hook, the census-only activity-bookkeeping
   skip, and the CA-2b blocked-row compare — a worktree
   `git checkout -- crates/` wiped them mid-train and each was
   re-derived from the session transcript). Typed residuals per §9.3,
   named for the follow-up owner:
   - **h2-5h-ca-2a-r1 — statics placement + static this/super alias
     plumbing for wrapped classes (~20 rows)**:
     typeOfThisInStaticMembers ×7, thisAndSuperInStaticMembers ×2,
     superInStaticMembers1, staticPropertyNameConflicts ×2,
     classStaticBlock ×2, classInConvertedLoopES5,
     complexClassRelationships, derivedClassWithPrivate* ×2,
     classAbstractAccessor, accessorsOverrideProperty7, autoAccessor5,
     derivedClassSuperStatementPosition. Mechanism (recon'd, dumps in
     the session scratchpad): the class-fields statics-inside routing
     for `TypeScriptClassWrapper` classes (`_tsc.js:97004-97009`) and
     the `_a = CC` class-alias assignment placement/substitution.
   - **h2-5h-ca-2a-r2 — lone-surrogate cooked-text fidelity (4
     rows)**: unicodeExtendedEscapesInStrings/Templates 10-11 —
     `\u{D800}` cooks to U+FFFD through the `String` pipeline; the
     fix needs WTF-16-faithful cooked text (a data-model change, out
     of packet scope by the §12 stop rule).
   - **h2-5h-ca-2a-r3 — ES6/ESNext module-kind at ES5 target (4
     rows)**: es6modulekindWithES5Target ×2,
     esnextmodulekindWithES5Target ×2.
   - **h2-5h-ca-2a-r5 — wrapper dup-comment dedup (21 rows, the
     §8-A.2 revert)**: the class-IIFE wrapper re-emits a class's
     leading comment inside the wrapper
     (accessorAccidentalCallDiagnostic family); blocked on the
     linear `containerPos` printer model.
   - **h2-5h-ca-2a-r4 — singles (~10 rows)**: decoratedBlockScopedClass
     ×2, awaitUsingDeclarationsInForOf ×2, blockScopedVariablesUseBeforeDef,
     emitAccessExpressionOfCastedObjectLiteral…, nestedLoops,
     newLexicalEnvironmentForConvertedLoop, asyncAwait_es5,
     asyncImportedPromise_es5, emitter.asyncGenerators.classMethods,
     computedPropertyNames1/12, destructuringVariableDeclaration1ES5iterable,
     invalidNewTarget/newTarget, ES5For-of37, objectRestParameterES5,
     thisTypeInAccessors.

## 9. Acceptance

1. Focused tests per step green (oracle-byte projections; no
   hand-authored expectations); 2. full suites incl. both witness
   gates; 3. census sweep: the A-list reports zero typed-seam
   rows and the write families flip — the packet records the
   measured 185→N (target ≈0; any residual re-classified with a
   typed deferral naming its owner + review); 4. corpus ratchet
   BYTE-IDENTICAL every band; 5. full local gate + hosted at the
   final head; merge commit via PR.

Census reproduction: the CA-2b §9.4 harness at the packet head
(worktree re-synced to the new main; the scratch census patches
incl. the blocked-row contract re-applied; baseline artifacts
preserved in the session scratchpad: ca2a-census.log,
ca2a-family-cases.md, ca2a-dumps/).

## 10. Traceability

| Family | Upstream | Rust | Step/Test |
| --- | --- | --- | --- |
| C(i) | 120703-120740, 120480-120504, 105818-105917 | target_bindings finalize + generated_bindings slot | 1 |
| H | 106448-106474, 97055-97066, 116219-116238 | class_fields hoist lane | 2 |
| G | 105429-105485, 105625-105636 | es2015 simplify pass 1 | 3 |
| C(ii) | 11562-11580, 24788-24802 | es2015 get_name | 4 |
| B(i/ii) | 105263-105289, 121030-121032, 121219-121233 | es2015 synthesized-ctor body range | 5 |
| B(iii) | 119710-119719, 121075+ | printer source-file entry | 6 |
| A+D | 94410-94547, 96971-97046, 107687-107783 | builtins promote lanes + class_fields routing | 7 |

## 11. Prohibitions

No case-ID/path branches; no output substitution; no
hand-authored expectations; no witness-gate re-mint to fit; no
≥ES2015 behavior change; no checker/harness/module-lane edits; no
wiring.

## 12. Unresolved items

None at authoring. An independent design-gate review
(2026-08-23 night, fresh-context agent; every span quote-read,
every Rust anchor read, the vendored compiler probed, the witness
dumps diffed) returned NOT-READY on the first draft with 2
blockers + 6 fixes + 2 notes, ALL folded:

- **Blocker (B(i) inverted):** the draft claimed upstream leaves
  the synthesized ctor-body Block pos=-1 and that our lane ranges
  it — BOTH false: upstream ranges the Block to the class
  (`createDefaultConstructorBody` @105307) and our lane is already
  upstream-identical (es2015.rs:8407-8409); the draft had
  conflated the ctor body with `transformClassBody`'s class-body
  block. §4.B(i)/§5.5 rewritten to the review-identified real
  deltas: the missing NodeArray ranges (three sites) and the
  printer's one-edge containerPos projection not threading the
  VariableStatement's claim through the wrapper chain.
- **Blocker (H misdiagnosed):** the block-scope lane is landed and
  working (`class_fields/downlevel.rs:3331-3341`, `:7219`); the
  real defect is the parse-tree early-return in
  `should_emit_explicit_initializer_for_let_declaration`
  (es2015.rs:5386-5388) skipping synthesized declarations, where
  upstream evaluates the full predicate with per-flag false
  (`hasNodeCheckFlag` @88560-88564). §5.2/§1 rewritten.
- **Fixes:** `class_fields/downlevel.rs` (the actual ES5-band
  class-fields lane) joins the allowed surface; the A-lane
  mechanism corrected (modifiers ELIDED + a separate trailing
  export statement, never moved/`export var`); C(iii) mapped to
  step 1 with named witnesses; the assigned-name harvest
  enumerates all FOUR upstream arms; the containerPos citation
  corrected to 121012-121022 (the @121020 disjunct: NoComments
  with pos >= 0 still advances); the G locus corrected to
  `is_synthetic_super` binding-identity under eager naming (the
  pass conditions are already exact).
- **Notes:** batch span-boundary corrections (§4/§3 now carry the
  review-verified line numbers).

Implementation-time measurements (the dropped-comment rows' exact
mechanism, the statics-routing shape for wrapped classes under
downlevel.rs, the fold-predicate identity matching) are bounded
dump-driven lookups inside the pinned spans, each with a named
step and witness — a discovered NEW owner/data-model/observable
stops the train and amends this packet per the design-gate rule.

## 13. Citation status

Every §4 span verbatim-read; the wrapper chain, `_i` scoping,
containerPos cooperation, assigned-name harvest, fold passes, and
void-0 chain additionally verified empirically with an
instrumented vendored compiler during the CA-2 recon; every §5
Rust anchor read in-tree at the trusted base
(es2015.rs:1141/4908/5382/8331/8613/9098/9306,
generated_bindings.rs:193, target_bindings.rs:454/573,
class_fields.rs:34-166, builtins.rs promote guard); the baseline
counts recomputed from the full-band census at the trusted-base
bytes.
