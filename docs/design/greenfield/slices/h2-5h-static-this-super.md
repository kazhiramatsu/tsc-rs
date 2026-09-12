# H2.5h ca-2a-r1: ES5 class wrapper static `this`/`super` and the class alias — design and execution record

Design and execution record for the slice selected on 2026-09-12 in
`target/decorator-prologue-followup-runs/next-slice-claude.md` (Claude's
lane): the seven rows of `ratchets/h2-5h-known-divergences.v1.json` owned by
`h2-5h-ca-2a-r1`. The claim of this record is that the seven rows and the
prepared witness commands below reproduce their frozen TypeScript 6.0.3
tuples. It is not a claim that every ES5 class-wrapper path or all of H2.8 is
complete; NC1 → MOD1 → transpile remain later slices and the A-close
dependencies are unchanged.

## Start point

- Worktree `/Users/hiramatsu/dev/tsc-rs-static-this-super`, branch
  `work/h2-5h-static-this-super`, cut from `main`
  `0aaf808bef9b0f5f64737821c63c273056dee2c4` (PR #515 merged; its tree is
  identical to the verified candidate `01398f1ee`, which contains the CFG
  head `fe502513e`). No existing worktree, exporter or frozen observation
  was changed.
- Dedicated target `target/static-this-super-acceptance`, run directories
  under `target/static-this-super-runs/` (tooling in `tools/`:
  `run-rows.sh`, `run-witnesses.sh`, `diff-captures.py`, `show-ts.py`,
  `write-witness-inputs.py`, `tsc-hash.py`, `shrink-manifest.py`). Heavy
  commands run one at a time, `taskpolicy -b nice -n 15`,
  `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`. No walk, no chain-walk, no
  `cargo xtask ci`, and — by the user's instruction during this slice — no
  local `cargo xtask acceptance`: the user runs the hosted-equivalent
  acceptance at merge time.

## Rows and the start-head re-capture

The seven rows (manifest order; `superInStaticMembers1`'s
`writes_diverging=47` counts one cause over 47 `@filename` units, not 47
cases):

| row (`#target%3Des5`) | writes | start-head divergence |
| --- | --- | --- |
| `constructorDeclarations/superCalls/derivedClassSuperStatementPosition.ts` | 1 | the `? … : …` branches of `Math.random() ? super(1) : super(0)` printed on three lines instead of one |
| `instanceAndStaticMembers/superInStaticMembers1.ts` | 48 (47 diverging) | `_super.w.call(this)` / `_super.w.call(_this)` where TypeScript prints `_super.w.call(_a)` |
| `instanceAndStaticMembers/thisAndSuperInStaticMembers3.ts` | 1 | `value: _super.f.call(this)` vs `_super.f.call(_a)` (define semantics) |
| `instanceAndStaticMembers/thisAndSuperInStaticMembers4.ts` | 1 | `C.z3 = _super.f.call(this)` vs `_super.f.call(_a)` (set semantics) |
| `instanceAndStaticMembers/typeOfThisInStaticMembers10.ts` | 1 | `D.e = (void 0).a + …` vs `_super.a + …` (legacy-decorated) |
| `instanceAndStaticMembers/typeOfThisInStaticMembers11.ts` | 1 | the same, define semantics |
| `propertyMemberDeclarations/autoAccessor5.ts` | 1 | `_C1__a_accessor_storage … _C1__d_accessor_storage` vs `_C1__b … _C1__e` |

The re-capture is `crates/compiler/tests/integration/h2_5h_static_this_super_rows.rs`:
each row is prepared through the H2.5h runner's qualified-vfs route
(`load_qualified_compiler_emit_with_option_floor`, `EmitOptionFloor::Established`),
emitted twice with the harness lib bundle, and compared on the complete
tuple — every write's path, callback bytes and byte-order mark in order,
the reported diagnostics, the emit result (`emit_skipped`, diagnostics,
`emittedFiles`/`sourceMaps` presence, status writes) and the exit code —
against `ratchets/h2-5h-qualification.v1.json` (whose two TypeScript
fingerprints are re-checked). With `TSC_RS_H2_8A_CAPTURE_WRITES_DIR` it
retains both complete runs with decoded texts. Baseline run
`target/static-this-super-runs/baseline-r1/`: 7/7 diverging, all on the JS
write only (diagnostics, emit result and exit code already exact).

## Upstream map (pinned `_tsc.js`, spans read verbatim)

- classFields transformer flags `95871-95872`:
  `shouldTransformThisInStaticInitializers = languageVersion < ES2022`,
  `shouldTransformSuperInStaticInitializers = shouldTransformThisInStaticInitializers && languageVersion >= ES2015`
  — below ES2015 class-fields never rewrites a static `super` access; the
  ES2015 lowering prints `_super.x` / `_super.f.call(this)`.
- `getClassFacts` `96844-96898`: `NeedsSubstitutionForThisInClassStaticField`
  (bit 8) from `ContainsLexicalThis` on a static property/block, and
  `NeedsClassConstructorReference` unless `ClassWasDecorated`;
  `visitInNewClassLexicalEnvironment` `96921-96967` enables the print-time
  `this` substitution (`enableSubstitutionForClassStaticThisOrSuperReference`
  `97583-97596`: `ThisKeyword` substitution plus emit notifications for
  function declarations/expressions, constructors, accessors, methods,
  property declarations and computed property names).
- `transformProperty` `97488-97500`: a static property's transformed
  expression gets `AdviseOnEmitNode` and `lexicalEnvironmentMap.set(getOriginalNode(property), lexicalEnvironment)`
  while the class has facts; `transformClassStaticBlockDeclaration`
  `96649-96682` maps every static block and advises its IIFE arrow.
- `onEmitNode` `97932-97982`: a mapped original installs its class
  environment (`shouldSubstituteThisWithClassThis` unless a
  `TransformPrivateStaticElements` static block); a function expression
  whose original is not an arrow (or is an async body), function
  declarations, constructors, accessors, methods and property declarations
  clear it; a computed property name switches to `.previous`.
  `substituteThisExpression` `97999-98017`: `classThis ?? classConstructor`
  (or `classConstructor` alone), else `(void 0)` for a legacy-decorated
  class. The es2015 substitution runs after it in the chain, so the
  es2015 super-property call receiver `createThis()` (`createCallBinding`)
  reaches the class alias before the captured-`this` substitution can see
  it — this is why TypeScript prints `_super.w.call(_a)` even inside the
  converted arrow functions where es2015 would otherwise print `_this`.
- es2015 `visitCallExpressionWithPotentialCapturedThisAssignment`
  `107784-107828`: the `super(...)` result `_this = _super.call(this, …) || this`
  is only `setOriginalNode`'d — it has no text range. Printer
  `getLinesBetweenNodes` `120408-120432`: a synthesized parent or operand
  (own `pos`/`end`, after `skipSynthesizedParentheses` `120436-120441`)
  yields 0 lines; `preserveSourceNewlines` is never set for this emit.
- Name generator: `makeTempVariableName` `120703-120740` keeps one temp
  sequence per formatted prefix/suffix key (`#_accessor_storage` for a
  generated private temp) and always reserves a private name in nested
  scopes; `createHoistedVariableForClass` `97772-97789` then makes the
  hoisted `_C__<temp>_accessor_storage` optimistically unique. Names are
  generated in print order, so a file-level `#_a_accessor_storage`
  (an unwrapped class's hoist printed at the top of the file) pushes the
  wrapped class printed after it to `_b`.

## Causes and fixes (one commit each, focused run per commit)

| # | cause | Rust locus | fix |
| --- | --- | --- | --- |
| 1 | the `this` synthesized by es2015 for a super-property call/tag/spread inside a lowered static initializer or static block is printed as `this` (or as es2015's captured `_this`), never as the class-fields constructor alias | `class_fields.rs`, `class_fields/downlevel.rs` | port of the print-time class lexical environment: `StaticEmitEnvironment` recorded per relocated static statement (`lexicalEnvironmentMap`), `before_emit_node`/`after_emit_node` (onEmitNode), `substitute_this_expression`, enabled by `NeedsSubstitutionForThisInClassStaticField` |
| 2 | class-fields rewrote (or invalidated, for legacy-decorated classes) static `super` accesses at ES5 | `downlevel.rs` `static_super_access` | `shouldTransformSuperInStaticInitializers` requires ES2015+: below it every consumer leaves `super` to the ES2015 lowering |
| 3 | the printer derived a conditional expression's line breaks from the operands' ORIGINAL nodes, so the synthesized `_this = _super.call(this, 1) \|\| this` branch inherited `super(1)`'s lines | `printer.rs` `lines_between_optional_nodes` | faithful `getLinesBetweenNodes`: synthesized parentheses skipped, own ranges only, synthesized parent/operand → 0 |
| 4 | the private temp letter of a computed auto-accessor's backing field was fixed at transform time in source order per scope; TypeScript assigns it in print order with nested-scope reservation | `downlevel.rs`, `target_bindings.rs`, `generated_bindings.rs`, `metadata.rs` | the hoisted variable is a `PreferredNameDomain::PrivateTemp` binding (`<prefix>` + private temp + `_accessor_storage`); the finalizer allocates the letter in the printing scope's private-name sequence (`allocate_private_temp_hoisted_name`) |

## Witnesses (TypeScript 6.0.3 observed twice each, `scripts/observe-static-this-super-witnesses.mjs`)

The 73 inputs are `crates/compiler/tests/fixtures/static-this-super-<group>-inputs.json`
(written by `target/static-this-super-runs/tools/write-witness-inputs.py`,
the decorator-witness command shape: `/project/main.ts`, `declaration`,
`declarationMap` and `sourceMap` on, `module` ESNext), observed twice with
`scripts/observe-static-this-super-witnesses.mjs` (SHA-256 `b8e0c7fd244d…`)
into `static-this-super-<group>.json`; every ES5 command reports exactly the
ES5 deprecation diagnostic 5107 (exit 2) and every other command reports
none (exit 0). Two earlier drafts were discarded for type errors of their
own (`this` in an unannotated function expression, an instance-side
`super.f()` against a static-only `B`, `prop = 1` next to a conditional
`super()`, a call-expression accessor key, `this` in a decorated static
initializer at ES2015); the observed inputs are diagnostic-free.

| group | sources | commands | targets / semantics | observation SHA-256 |
| --- | --- | --- | --- | --- |
| `static-super-calls` | 10 (derived field/block/arrow/function-boundary/spread/tagged-template/nested-class/anonymous-expression/instance-side/computed-name super calls) | 40 | ES5 + ES2015 × set/define | `353c2f692135…` |
| `conditional-super-calls` | 5 (multi-line, one-line, plain-conditional control, value-used, parenthesized branches) | 10 | ES5 + ES2015 | `57c8c04d7ef1…` |
| `auto-accessor-storage-names` | 5 (wrapped-then-unwrapped, unwrapped-then-wrapped, two-wrapped, nested-function-scope, anonymous-class-expression) | 15 | ES5 + ES2015 + ES2022 | `297cbdb1c0b9…` |
| `legacy-decorated-static-super` | 2 (static super read + call, static block super) | 8 | ES5 + ES2015 × set/define, `experimentalDecorators` | `c3ed9b0e8367…` |

TypeScript facts read from the observations: a `super.f()` receiver inside
a converted static-block arrow prints `_a` while the file still receives an
unused `var _this = this;` capture (`derived-block-super-call`, ES5); a
legacy-decorated class prints `_super.f.call((void 0))` for the same call
and `_super.g` for a read (`decorated-derived-static-super-read-and-call`,
`decorated-derived-static-block-super`, ES5); a nested class inside a
static block gets its own alias (`Inner.y = _super.f.call(_b)` under
`_a = Outer`); a wrapped class printed after an unwrapped one starts its
accessor storages at `_b`, a class inside a sibling function starts at `_a`
again, and two wrapped classes continue one sequence (`_a…_d`, `_e…_h`).

The witness test is `crates/compiler/tests/integration/h2_5h_static_this_super_witnesses.rs`
(`TSC_RS_STATIC_THIS_SUPER_WITNESS_SET=<group>|all`,
`TSC_RS_STATIC_THIS_SUPER_WITNESS_FILTER`, captures under
`TSC_RS_H2_8A_CAPTURE_WRITES_DIR`), running each command through the same
complete-command comparator as the decorator witness groups.

## Results

Every run: one binary per stage (`target/static-this-super-runs/<run>/prelaunch.txt`
records the head, the production/fixture SHA-256s and the binary SHA-256;
`receipt.txt` the exit codes and counts; `row-captures/` and
`witness-captures/` both complete runs of every command).

| run | tree | rows exact ×2 | witnesses exact ×2 |
| --- | --- | --- | --- |
| `baseline-r1` | start head + row replay | 0/7 | — |
| `fix1-r1` | causes 1–4 | 7/7 | — |
| `full-witnesses-r1` | causes 1–4 | — | 46/73 (27 map-only failures: 20 ES2015 static-super controls, 7 accessor-name commands; 1 ES5 JS failure `nested-function-scope`) |
| `full-r2` | + naming-moment frames, Reflect.get call range, accessor key temp range | 7/7 | 68/73 (ES2015 tagged-template bind range ×2, ES2015 setter key temp range ×3) |
| `full-r3` | final tree | 7/7 | 73/73 |

Per-cause commits, each verified on its own cumulative tree (`stage<k>` runs;
`tools/stage-tree.sh` derives the tree from the final files by reverse
patches and `tools/verify-stage.py` checks it line by line against the
cause patches):

| stage / commit | rows exact ×2 | focused witness group |
| --- | --- | --- |
| 1 · cause 1 (`80dcf5649`) | 3/7 (superInStaticMembers1, thisAndSuperInStaticMembers3/4) | static-super-calls 20/40: every ES5 command exact, the 20 ES2015 controls map-only (their Reflect.get ranges, commit 5) |
| 2 · cause 2 (`ff6189718`) | 5/7 (+ typeOfThisInStaticMembers10/11) | legacy-decorated-static-super 8/8 |
| 3 · cause 3 (`f31f0d3a6`) | 6/7 (+ derivedClassSuperStatementPosition) | conditional-super-calls 10/10 |
| 4 · cause 4 (`9298ce03f`) | 7/7 (+ autoAccessor5) | auto-accessor-storage-names 9/15: every JS write exact incl. `nested-function-scope`/`two-wrapped`; the 6 `[k]`-keyed commands map-only (the key temp ranges, commit 5) |
| 5 · control repairs (`f7b465e53`) | 7/7 | all four groups 73/73 |

Adjacent regression suites at the final head: `target/static-this-super-runs/regression-r1/` — emitter lib
`cargo test -p tsc-rs-emitter --lib`: 495 passed, 0 failed. Compiler contracts
`cargo test -p tsc-rs-compiler --test contracts` (`--test-threads=2`): 415
passed, 15 failed, 16 ignored. Every one of the 15 failures also fails at the
START head: `target/static-this-super-runs/baseline-failing-set/` rebuilt the
contracts binary with `crates/emitter` reset to `0aaf808b` (this branch's
tests and fixtures kept) and ran the 15 tests serially — all 15 exit 101
there too, and `class-field-alias-map-positions` fails on the identical set
of 20 commands (`nested-computed-name` ES5/ES2015, `concise-arrow`,
`legacy-bound-this`, `legacy-static-block` ES5: an unused `var _this = this;`
capture and a `default_1`/`class_1` internal name; the rest map-only). They
are therefore inherited from `main`, not introduced by this slice, and are
left to their owners: `emit_session_contract` ×2, `h2_7a_m4_controls` ×3,
`program_session_contract::programmatic_node_module_resolution_relationships_keep_exact_module_names`,
and the witness suites `class_field_alias_map_positions`,
`export_name_syntax_maps`, `ellipsis_comment_owners`,
`hoisted_declaration_export_ranges`, `import_helpers`,
`static_initializer_map_ranges`, `token_comment_phases`,
`source_map_emit_witness_contract` ×2. The new row replay and witness tests
pass in the same run (`static_this_super_rows`, `static_this_super_witnesses`).

Manifest: `ratchets/h2-5h-known-divergences.v1.json` 28 → 21 rows by promotion of the seven `h2-5h-ca-2a-r1` rows (owners `h2-5h-ca-2a-r2` 4 and `h2-5h-ca-2a-r4` 17 remain); the receipt `ratchets/h2-5h-static-this-super-shrink.v1.json` names the two exact replay runs (`stage5`, `full-r3`) and the manifest SHA-256 before/after. No row was added and no expected tuple changed.

Open (not claimed): the `.previous` switch for a `this` inside a computed
property name of a static member in a static context, and the
`TransformPrivateStaticElements` static-block arm of
`shouldSubstituteThisWithClassThis`, are ported but have no witness command
of their own; the ES2015 `var _this = this;` capture quirk is reproduced by
the existing es2015 port, not changed here.

## Prohibitions kept

- No known-divergence row was added and no expected tuple was edited; the
  manifest changed only by promotion of rows proven exact against their
  frozen upstream tuples.
- No production file of another lane was edited; the root checkout and
  every other worktree are untouched.

## Merge validation: generated private accessor receivers

[PR #518](https://github.com/kazhiramatsu/tsc-rs/pull/518) combines this slice
with main `85713fd1b`. Hosted run `34687171001` passed diagnostic conformance
and every emit band through H2.6b, including H2.5h's expected 867 exact / 21
known / 44 deferred rows. It then found a new H2.6c regression in the frozen
ES2015 `esDecorators-classDeclaration-sourceMap.ts` command. Its JS write
changed `_static_private_z_descriptor.get.call(this)` (and the setter's
receiver) to `_classThis`; the JS map, declaration, and declaration map
remained exact. This was a previously exact command, not an inherited
known divergence.

The new class-fields emit hook followed `set_semantic_original_node`'s
Rust-only resolver bridge from a generated private accessor function to its
erased property. That property's initializer was in the lexical environment
map, so the lookup reinstated static `this` substitution before the ordinary
function boundary could clear it. TypeScript's
`visitMethodOrAccessorDeclaration` (`_tsc.js:96194-96228`) creates the
hoisted function without an original-node link.

The correction records whether an original edge is a resolver-only bridge
and uses `get_emit_original_node` for class-fields notification identities
and map keys. Emit provenance stops at that synthetic function; existing
resolver projections keep their full original chain. The edge marker is
not inherited by emit-metadata merging, and a real `setOriginalNode` call
replaces it even when the target node is unchanged.

The row replay now includes the unchanged ES2015, ES2022 and ESNext frozen
H2.6c commands, comparing ordered write bytes/BOM, diagnostics, result
presence and exit code twice. A factory test covers the distinction through
cloning and reattachment while checking that resolver projection still
works. No frozen observation, manifest or acceptance rule is changed by
this correction. Merge validation logs and captures are retained in
`target/merge-validation/` in the `tsc-rs-static-this-super-merge` worktree;
the PR body records the final gate results.
