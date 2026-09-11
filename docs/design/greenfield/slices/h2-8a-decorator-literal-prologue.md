# A6-41 literal computed names and lexical prologues: design and execution record

Design and execution record for the literal-name / lexical-prologue follow-up
prepared in [the handoff](h2-8a-decorator-literal-prologue-handoff.md) and
pinned by [the start manifest](h2-8a-decorator-literal-prologue-start.v1.json).
Passing the 120 prepared commands plus the reachability extension observed
below is the claim of this record. It is not a claim that every
`transformESDecorators` source path or all of H2.8 is complete.

## Start point

- Worktree `/Users/hiramatsu/dev/tsc-rs-dec-literal-prologue`, branch
  `prep/h2-8a-decorator-literal-prologue`, HEAD `e8281f286` (the manifest's
  production head `5134bb018` plus the preparation docs commit; the
  production tree `1e632db73…` differs from HEAD's tree only by the four
  preparation documents). The twelve production inputs, the observer, the
  witness harness and `_tsc.js` were re-hashed at the start and match the
  manifest byte for byte (`standard_decorators.rs` `47ec5c4afcc7…`,
  `printer.rs` `cd0ba7f9e7c2…`, `metadata.rs` `117da2d57102…`,
  `_tsc.js` `1c59e77a54b1…`).
- [PR #513](https://github.com/kazhiramatsu/tsc-rs/pull/513) at the start:
  OPEN, head `5134bb018`, hosted `gates` run 34587126551 IN_PROGRESS. It was
  merged by Codex while this work ran (2026-09-11 10:50:45 UTC, hosted
  `gates` SUCCESS, merge commit `9806a4cb9` = `origin/main`, parents
  `720f2d4b9` and `5134bb018`). The merge commit's tree is
  `1e632db73fbcd8e37a6e3c63b03daf57dfbdafdc`, byte-identical to the
  production base's tree, and `5134bb018` is an ancestor of `origin/main`:
  recorded as an identity, not as a re-verification, and nothing on this
  branch was reset or rebased. Nothing here is folded into that PR or into
  `main`. The root checkout, `dec-next`, `dec53`, `dec-followup` and
  `dec-merge` worktrees, their targets and captures are untouched.
- Dedicated target `target/decorator-literal-prologue-acceptance`, run
  directories under `target/decorator-literal-prologue-runs/` (tooling in
  `tools/`: `run-witnesses.py`, `run-full530.py`, `run-emitter-suites.py`,
  `witness-chain.sh`, `diff-captures.py`, `diff-maps.py`, `show-ts.py`, and
  the previous work's `compare-with-full62.py` copied unchanged, SHA-256
  `23507016ab00…`). Heavy commands run one at a time,
  `taskpolicy -b nice -n 15`, `CARGO_BUILD_JOBS=2`.

## Upstream observations (TypeScript 6.0.3, twice each)

The 120 proposed inputs were split by group into
`crates/compiler/tests/fixtures/decorator-<group>-inputs.json` and observed
with the existing observer (`scripts/observe-decorator-next-witnesses.mjs`,
the three groups registered; the five existing groups' inputs and
observations are unchanged). Run directory
`target/decorator-literal-prologue-prep/observe-r1/`.

| group | sources | commands | Program executions | diagnostics | exceptions | exit |
| --- | --- | --- | --- | --- | --- | --- |
| `literal-member-kinds` | 8 (method/getter/setter/auto-accessor with `["x"]` and the `[key]` control, instance + static) | 48 | 96 | 0 | 0 | 0 |
| `literal-key-spelling` | 8 (decimal/hex/separator numerics, plain/escaped templates, escaped-string control, template method, substitution-template control) | 48 | 96 | 0 | 0 | 0 |
| `lexical-prologue` | 4 (function/arrow body with and without directives) | 24 | 48 | 0 | 0 | 0 |

Observation SHA-256: `decorator-literal-member-kinds.json` `775e60a15d2b…`,
`decorator-literal-key-spelling.json` `eca126749d00…`,
`decorator-lexical-prologue.json` `8b84990296c7…`.

TypeScript facts read from the observations (not from the source alone):

- A decorated method/getter/setter with `["x"]` keeps `["x"]` as the element
  name, names the context `"x"`, and builds `access: { has: obj => "x" in obj,
  get: obj => obj["x"] }` (getter/method) or `{ has, set }` (setter); no temp,
  no `__propKey`. The `[key]` control uses the cache temp. Auto-accessors go
  through the class-fields owner (`get [(_C__a_accessor_storage = new WeakMap(), "x")]()`
  at ES2015).
- Numeric keys: the context name is the cooked text quoted (`"16"` for
  `0x10`, `"1000"` for `1_000`), the element name prints from source
  (`this[0x10]`; `this[1000]` at ES2015 and `this[1_000]` at ES2022+, the
  `AllowNumericSeparator` rule).
- Template keys: the context name and the access object print the template
  source text with backticks (`` name: `xa` ``, `` `xa` in obj ``,
  `` obj[`xa`] ``); the string control keeps `"xa"`.
- A function or arrow body with directives receives the hoisted `var _a;`
  after both directives (`"use strict"; "custom directive"; var _a;`).

## Rust baseline (production unchanged)

Test registration only (`h2_8a_decorator_next_witnesses.rs` GROUPS + the three
fixtures), one contracts binary `d1476afb7499…` built from the unchanged
production. Receipts copied to
`ratchets/h2-8a-decorator-literal-prologue-baseline-<group>.v1.json`; run
directories `baseline-r1-<group>/` (log, two complete captures per case,
archived binary, `classification.txt`, `map-classification.txt`).

| group | exact ×2 | failed | exit | families |
| --- | --- | --- | --- | --- |
| `literal-member-kinds` | 18 | 30 | 101 | method/getter/setter × {`["x"]`, `[key]` control} × 5 lowered configurations failed; both auto-accessor sources exact ×6; ESNext/define exact (native decorators) |
| `literal-key-spelling` | 28 | 20 | 101 | `field-template-plain`, `field-template-escaped`, `method-template-escaped`, `method-template-substitution-control` × 5 lowered configurations failed; decimal/hex/separator numerics and the escaped-string control exact ×6 |
| `lexical-prologue` | 14 | 10 | 101 | `function-directives`, `arrow-directives` × 5 lowered configurations failed; both no-directive controls exact ×6 |

Classification of the failures (every failing case was captured twice,
identically):

| family | difference | cause |
| --- | --- | --- |
| method/getter/setter, literal and identifier keys (30) and `method-template-substitution-control` (5) | JavaScript identical; `main.js.map` only: on `__runInitializers(_a, _staticExtraInitializers);` tsc emits an end segment after the semicolon (column 64 → class name end) in addition to the segment at the semicolon; Rust emits only the latter | cause 1: the pending static initializer statement carries no source map range (tsc sets the class name's range on both the `__runInitializers` call and its statement) |
| `field-template-plain`, `field-template-escaped`, `method-template-escaped` (15) | JavaScript: Rust prints `"x"` / `"xa"` where tsc prints `` `x` `` / `` `xa` `` in the context name and the access object (the method rows also carry cause 1 in their map) | cause 2: the synthesized context string literal has no text source for a template key, and the printer's text-source branch handles only identifier and string sources |
| `function-directives`, `arrow-directives` (10) | JavaScript: `var _a;` printed before `"use strict"; "custom directive";` (tsc: after them); the map shifts accordingly | cause 3: `merge_block_environment` inserts the hoisted declaration at index 0 |

Numerics were already exact: the Rust scanner cooks `0x10` → `16` and
`1_000` → `1000` (`check_big_int_suffix`, the `"" + numericValue` rule), the
decorator helper names the context from that text, and the printer already
applies the numeric-separator target rule to the element name. They are
recorded, not changed.

## Reachability extension: later-pass readers of the hoisted `var` (cause 4)

The hoisted `var` statement of `endLexicalEnvironment` carries
`EmitFlags.CustomPrologue` upstream; the decorator transform's
`create_hoisted_declarations` set no flag. No command among the 120
distinguishes the two, so the readers of that flag in the passes that run
after `transformESDecorators` (`getScriptTransformers`: ESNext →
ESDecorators → ClassFields → ES2021 … ES2015 → module transformer) were
mapped before any change:

| hoist owner (decorator transform) | later-pass reader of the flag | reachable from ordinary source? |
| --- | --- | --- |
| source file (`transform_root` → `merge_source_file_environment`) | CommonJS/AMD/UMD `transform_source_file` (`builtins.rs` 5013-5022, `copyCustomPrologue`): flagged statements are copied before the `__esModule` marker and the export pre-initializers | yes: `module: commonjs/amd/umd` with an undecorated top-level class whose computed field holds a decorated anonymous class |
| source file | System `createSystemModuleBody` (`copyPrologue`) | yes, same source with `module: system` |
| source file | ES2017 `visitSourceFile` (`copyPrologue`, target < ES2017) | copied first either way; no ordering difference for these sources |
| function/arrow body (`visit_function_like_body` → `merge_block_environment`) | class-fields `merge_lexical_environment` (`class_fields.rs` 3152): hoisted variables of the later pass splice at `leftHoistedFunctionsEnd` (before an existing hoisted `var`, flag-independent), custom-prologue statements at `leftHoistedVariablesEnd` (after it only when flagged) | yes: a parameter initializer holding the same class makes class-fields lower the default into a custom-prologue `if (p === void 0) {…}` statement |
| function body | ES2015 `transformFunctionBody` (`copyCustomPrologue(isHoistedVariableStatement)`), generators | target ES5 only; outside the ES2015/ES2022/ESNext matrix |
| decorator IIFE / property-initializer IIFE (`transform_class_like`, `visit_property_initializer`) | class-fields `visitFunctionBody` of the synthesized arrow bodies | no custom-prologue statements are added there; no ordering difference |
| any | ESNext `count_prologues`, ES2021/ES2018 `is_custom_prologue` | ESNext runs before the decorators; ES2021/ES2018 read it only around syntax these sources do not use |

A fourth group, `lexical-prologue-readers` (18 commands), was therefore
observed with the same observer (`observe-r2/`; observation SHA-256
`097c2489317c…`): the top-level undecorated hoist under `module: commonjs`
on all six configurations and under AMD/UMD/System at ES2022 (set/define),
and a parameter-initializer owner of the same hoist on the six
configurations. The AMD/UMD/System commands report TS5107 (the 6.0
deprecation of those module kinds; exit 2) and still emit; the observation
is kept as recorded, with the diagnostic in the tuple.

Rust baseline of the group on the unchanged production (binary
`30322e5e1217…`, receipt
`ratchets/h2-8a-decorator-literal-prologue-baseline-lexical-prologue-readers.v1.json`,
run directory `baseline-r2-lexical-prologue-readers/`): 2 exact (the two
native ESNext/define rows), 16 failed:

| family | difference | cause |
| --- | --- | --- |
| CommonJS ×5, AMD ×2, UMD ×2 | JavaScript: tsc `"use strict"; var _a; Object.defineProperty(exports, "__esModule", …); exports.Plain = void 0;`, Rust prints `var _a;` after `exports.Plain = void 0;` (TS5107 reproduced by Rust on AMD/UMD) | cause 4 (the CommonJS copy of custom prologues) |
| parameter initializer ×5 | JavaScript: tsc `var _a; var _b; if (p === void 0) { p = (_a = class …, _b = __propKey(key), _a); } return p;`, Rust `var _a; if (…) {…} var _b; return p;` | cause 4 (class-fields splices its custom-prologue statement at `leftHoistedVariablesEnd`, which excludes the unflagged `var _b;`); the map difference follows the order |
| System ×2 | JavaScript identical (the System body copies the hoist as tsc does); `main.js.map` differs because the Rust System output carries no mappings at all | not a decorator owner: the System module transform's source map; recorded as an open row, not fixed here |

Because the two System rows cannot become exact through any decorator-pass
change, the gating group registered in the harness is version 2,
`lexical-prologue-readers-v2` (16 commands: the CommonJS/AMD/UMD and
parameter-initializer rows), observed again with the observer
(`observe-r3/`, SHA-256 `8f81436975d2…`) and checked to be byte-identical to
the corresponding version-1 rows (inputs and observations, the case-id
prefix aside). Version 1 stays on disk as the record of the System rows and
is not registered in the harness; its baseline receipt is the per-case
baseline of version 2's rows (same binary, same production, same inputs).

## Upstream callees and predicates (pinned `_tsc.js`, sha `1c59e77a…`)

- `partialTransformClassElement` 99831-99944: `isPropertyNameLiteral(expression)
  && !isIdentifier(expression)` (99870) names the context by
  `createStringLiteralFromNode(expression)` for every member kind; `access`
  is `{ get }` for methods/getters and fields, `{ set }` for setters and
  fields (99882-99885); methods and accessors then build
  `createESDecorateHelper(this, descriptor ?? null, memberDecoratorsName,
  context, null, methodExtraInitializersName)` (99898).
- `isPropertyNameLiteral` 15888-15896 (Identifier, StringLiteral,
  NoSubstitutionTemplateLiteral, NumericLiteral);
  `getTextOfIdentifierOrLiteral` 15899-15901 (`node.text`, the cooked text).
- `createStringLiteralFromNode` 21535-21543 sets `textSourceNode = sourceNode`
  for every source kind.
- `getLiteralTextOfNode` 120467-120479: an identifier, private identifier,
  numeric literal or JSX namespaced name source prints as `"…"` around the
  (numeric: `textSourceNode.text`) escaped text; any other source delegates
  to `getLiteralTextOfNode(textSourceNode, …)` → `getLiteralText`
  13647-13690: `canUseOriginalText` (parsed, parented, and for a numeric
  literal no separator unless `AllowNumericSeparator`) prints the source
  text verbatim, otherwise a template prints `` ` `` + (`rawText` ??
  `escapeTemplateSubstitution(escapeText(text))`) + `` ` ``.
- `createClassInfo` 99262-99290: the first decorated static method or accessor
  creates `_staticExtraInitializers` and `initializer =
  createRunInitializersHelper(classThis ?? this, name)` with
  `setSourceMapRange(initializer, node.name ?? moveRangePastDecorators(node))`
  pushed to `pendingStaticInitializers`; `transformClassLike` 99502-99510 and
  `visitClassStaticBlockDeclaration` 100005-100040 wrap each pending static
  initializer in `createExpressionStatement(initializer)` and
  `setSourceMapRange(initializerStatement, getSourceMapRange(initializer))`.
  `prepareConstructor` 99747-99758 inlines the instance initializers into one
  statement without a statement range (the constructor line was already exact).
- The decorators `visitor` 99134-99143 visits FunctionDeclaration and
  FunctionExpression through `visitEachChild(node, fallbackVisitor, context)`
  and ArrowFunction through the default `visitEachChild`; both reach
  `visitParameterList` 91168-91181 (`startLexicalEnvironment`,
  `InParameters`, `addDefaultValueAssignmentsIfNeeded` when
  `VariablesHoistedInParameters` at target ≥ ES2015) and `visitFunctionBody`
  91277-91290 (`resumeLexicalEnvironment`, `endLexicalEnvironment`,
  `convertToFunctionBlock`, `mergeLexicalEnvironment`).
- `mergeLexicalEnvironment` 24889-24936: hoisted `var` statements splice at
  `leftHoistedFunctionsEnd` (after the `isPrologueDirective` span and the
  `isHoistedFunction` span), custom-prologue statements at
  `leftHoistedVariablesEnd`.
- `hoistVariableDeclaration` 116104-116116 (`NoNestedSourceMaps` on each
  declaration); `endLexicalEnvironment` 116163-116200 (`CustomPrologue` on the
  `var` statement); `isHoistedVariableStatement` 14173-14175;
  `copyPrologue`/`copyCustomPrologue` 24827-24870;
  `transformCommonJSModule` 110167-110200.

## Source → Rust owner → witness map

| `_tsc.js` (6.0.3) | Rust owner (start SHA) | witness | change |
| --- | --- | --- | --- |
| `partialTransformClassElement` literal branch for methods/accessors, `createESDecorateClassElementAccess{Has,Get,Set}Method`, `getHelperVariableName` | `decorator_property_name` 5526, `collect_method_plan` 1360, `partial_transform_method_plan` 2470, `create_method_decorator_context` 3454, `create_access_object` 3502, `create_computed_access_key` 3569 (`standard_decorators.rs`) | `literal-member-kinds` (48) | none: already exact in the JavaScript, the identifier controls and literal rows differ only through cause 1 |
| `createClassInfo` pending static initializer + `transformClassLike`/`visitClassStaticBlockDeclaration` statement range | `materialize_pending_initializer_expressions` 3651 (class range on the call), `materialize_pending_initializer_statements` 3767 (statement without range) | `literal-member-kinds` method/getter/setter rows, `method-template-substitution-control`, `method-template-escaped` | cause 1 |
| `createStringLiteralFromNode` (`textSourceNode` for every source), `getLiteralTextOfNode` numeric branch and delegation, `getLiteralText` template branch / `canUseOriginalText` | `create_string_literal_from_property_literal` 3586 (text source set only for strings); `printer.rs` StringLiteral 2735-2841 (text-source branch: Identifier 2767-2811, StringLiteral 2812-2821, otherwise the created node's own text is quoted); `emit_template_literal_token` 9060 | `literal-key-spelling` template rows (15) | cause 2 |
| `getLiteralText` numeric branch, `canUseOriginalText` separator rule; scanner cooking | scanner `check_big_int_suffix`; printer NumericLiteral | `field-numeric-*` (18), `field-string-escaped-control` (6) | none (exact) |
| `visitFunctionBody` → `mergeLexicalEnvironment` (function and arrow bodies) | `visit_function_like_body` 5314, `merge_block_environment` 5841 (`statements.insert(0, …)`), `merge_source_file_environment` 5760 (already prologue-aware), `is_prologue_directive` 5808, `is_hoisted_function` 5823 | `lexical-prologue` (24) | cause 3 |
| `hoistVariableDeclaration` / `endLexicalEnvironment` flags; readers `copyCustomPrologue` (CommonJS/AMD/UMD), class-fields `mergeLexicalEnvironment` | `create_hoisted_declarations` 5738 (no flags); readers `builtins.rs` 5013-5022, `class_fields.rs` 3152 | `lexical-prologue-readers` CommonJS/AMD/UMD/parameter rows (14) | cause 4 |
| `visitParameterList` default-value lowering in the decorators pass | none in `standard_decorators.rs` (class-fields lowers the default later, `downlevel.rs` `lower_function_parameter_defaults`; tsc's text is reached that way) | `lexical-prologue-readers` parameter rows | recorded; see the execution record |
| class-fields `visitFunctionBody` → `mergeLexicalEnvironment` (the reader of the flag inside a function body) | `class_fields/downlevel.rs` `prepend_function_prelude_to_block` 7878 (standard-prologue span only: no `isHoistedFunction` / `isHoistedVariableStatement` spans) | `lexical-prologue-readers` parameter rows (text) | cause 5 |
| `visitParameterList` in the decorators pass: `InParameters`, `VariablesHoistedInParameters`, `addDefaultValueAssignmentsIfNeeded` (target ≥ ES2015), `addDefaultValueAssignmentForInitializer` / `ForBindingPattern`, `addInitializationStatement`; `mergeLexicalEnvironment`'s custom-prologue splice | `visit_function_like_body` (parameters visited by the generic child visit inside the environment, no window, no lowering), `merge_block_environment` (hoisted `var` only) | `lexical-prologue-readers` parameter rows (map at ES2015/ES2022, text at ESNext/set) | cause 6 |
| System `createSystemModuleBody` | `system.rs` | `lexical-prologue-readers` System rows | JavaScript exact; the empty System source map is another owner (open row) |

## Ownership decisions

- **Scope of temporaries and helpers.** No cause allocates a temporary or
  requests a helper. Cause 1 touches only a source map range, cause 2 only
  the spelling of an already synthesized literal, cause 3 only the splice
  index of the already created `var` statement, cause 4 only emit flags.
- **No quote normalization.** The template spelling is produced by the same
  delegation tsc performs (`getLiteralTextOfNode(textSourceNode)`): the
  parsed template's own token text prints verbatim when it has a source
  range; a template without one prints through the template-token writer
  (`rawText` or the escaped cooked text between backticks). Nothing rewrites
  quotes into backticks by string comparison. A numeric source prints its
  cooked text quoted, as the identifier branch already did.
- **Function directives and `CustomPrologue` are separate causes.** Cause 3
  is the splice position inside the decorators pass (the pass's own
  `mergeLexicalEnvironment`); cause 4 is the flag read by *later* passes.
  Cause 3 has its own witnesses (`lexical-prologue`) and cause 4 its own
  (`lexical-prologue-readers`); each is verified alone on the binary built
  from its commit before the next cause is applied.
- **Flags, not positions.** Cause 4 sets `CUSTOM_PROLOGUE` on the statement
  and `NO_NESTED_SOURCE_MAPS` on each declaration exactly where tsc sets them
  (`endLexicalEnvironment`, `hoistVariableDeclaration`); it is applied to the
  one constructor of hoisted declarations shared by the source file, the
  function-body, the class IIFE and the property-initializer environments,
  because upstream marks all four the same way. No reader is changed.
- **Ranges.** Cause 1 copies the initializer's explicitly set source map
  range (the class name, or the range past the decorators) onto the new
  statement; an expression without an explicit range leaves the statement
  unmapped, which is what tsc's `-1` position does in the printer.
- **Class-fields boundary.** `class_fields.rs` and the module transforms
  are unchanged; the shapes handed over (`["x"]` element names, flagged
  hoisted `var` statements) are the ones those owners already consume from
  the class-fields pass itself. One reader in `class_fields/downlevel.rs`
  is changed (cause 5): its function-prelude installation is the port of
  the same `mergeLexicalEnvironment`, and the parameter rows reproduced the
  missing `isHoistedFunction` / `isHoistedVariableStatement` spans as a
  text difference. Nothing else in the class-fields owners moves.
- **Who lowers a parameter default.** In tsc the pass that hoists while
  visiting the parameters lowers the default (`visitParameterList`); for the
  parameter rows that is the decorators pass, and class-fields then sees a
  parameter without an initializer. The Rust decorator pass lacked that
  window, so the lowering fell to class-fields' downlevel port, which
  builds the statement with different source-map ownership (no text range
  on the block and the assignment, no `NoSourceMap` on the initializer)
  and, at ESNext/set, does not hoist in the parameters at all, leaving the
  default unlowered. Cause 6 ports `visitParameterList` into the decorators
  pass with tsc's exact flags and ranges; the downlevel port's own map
  fidelity is a class-fields-owner observation, unwitnessed by a command of
  its own, and is not changed.

## Execution record

Per-cause commits follow this record; each was verified with its own group
run on the binary built from that commit before the next cause was applied
(`tools/witness-chain.sh <run> <group:count>`: one `--no-run` build, then
`run-witnesses.py`). Every run directory holds `prelaunch.json` (head, git
status, production/fixture/test/runner/Cargo.lock/`_tsc.js` SHA-256),
`production-and-witness-inputs.tar.gz`, `run.log`, `captures/` (two complete
captures per case), `binaries/` (the executed test binary) and
`receipt.json`; the receipts are copied into `ratchets/`.

### Cause 1 — source map range of the pending static initializer statements (commit `a41921f1a`)

`materialize_pending_initializer_statements` (the port of
`visitClassStaticBlockDeclaration` and of `transformClassLike`'s trailing
block) now copies the initializer's explicitly set source map range onto the
expression statement it creates (`copy_source_map_range`); the range itself
is still set on the `__runInitializers` call by
`materialize_pending_initializer_expressions` (`set_class_source_map_range`:
the class name, or the range past the decorators). An initializer without an
explicit range leaves its statement unmapped, as tsc's negative position
does. `prepareConstructor`'s inlined instance statement is unchanged (it was
already exact).

Focused run `cause1-r1-literal-member-kinds/`
([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-cause1-focused.v1.json)):
the whole `literal-member-kinds` group, 48 exact twice, exit 0, binary
`9a84a263c80d…`, sources and witness inputs unchanged after the run.

### Cause 2 — template and numeric key sources of the context name (commit `60a6a8a94`)

`create_string_literal_from_property_literal` now records the parsed literal
as the created string literal's text source for string, numeric and
no-substitution-template sources alike (`createStringLiteralFromNode` sets
`textSourceNode` unconditionally). The printer's text-source branch of
`StringLiteral` (`printer.rs`) gains the two remaining cases of
`getLiteralTextOfNode`: a numeric source prints its cooked text quoted and
escaped exactly as the identifier branch does, and any other source
delegates to the source literal's own text — a parsed template with a
source range prints its token text verbatim (`canUseOriginalText`), a
template without one goes through `emit_template_literal_token` (`rawText`,
else the escaped cooked text between backticks). The string-source path is
unchanged. Nothing compares or rewrites quotes; the numeric rows, already
exact, now reach the same output through the numeric branch.

Focused run `cause2-r1-literal-key-spelling/`
([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-cause2-focused.v1.json)):
the whole `literal-key-spelling` group, 48 exact twice (the
substitution-template control needed cause 1, the template rows cause 2, the
`method-template-escaped` rows both), exit 0, binary `88785a7c3c92…`, sources and
witness inputs unchanged after the run.

### Cause 3 — hoisted declarations after a function body's directives (commit `5bcb2c600`)

`merge_block_environment` no longer inserts the hoisted `var` statement at
index 0: `hoisted_declaration_insertion_index`, shared with
`merge_source_file_environment`, skips the standard prologue directives
(`is_prologue_directive`) and the custom-prologue hoisted function
declarations of earlier transforms (`is_hoisted_function`), which is
`mergeLexicalEnvironment`'s `leftHoistedFunctionsEnd` for the single
declaration this pass produces. Expression-bodied arrows still take index 0
of the block `convertToFunctionBlock` synthesizes. This is the decorator
pass's own splice; the `CustomPrologue` flag that later passes read is
cause 4 and was left untouched here.

Focused run `cause3-r1-lexical-prologue/`
([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-cause3-focused.v1.json)):
the whole `lexical-prologue` group, 24 exact twice, exit 0, binary `46f6dcc1a911…`,
sources and witness inputs unchanged after the run.

### Cause 4 — `CustomPrologue` on the hoisted `var` statements (commit `940d62e4a`)

`create_hoisted_declarations` now sets `NO_NESTED_SOURCE_MAPS` on each
declaration (`hoistVariableDeclaration`) and `CUSTOM_PROLOGUE` on the
statement (`endLexicalEnvironment`), for every environment that declares
temporaries through it. No reader was changed in this commit.

Focused run `cause4-r1-lexical-prologue-readers-v2/`
([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-cause4-focused.v1.json)):
11 exact twice (the five CommonJS rows, the two AMD and two UMD rows with
TS5107 reproduced, the two native rows), 5 failed (the parameter-initializer
rows on the five lowered configurations), exit 101, binary `4da04c6adc70…`. The
remaining difference is unchanged from the baseline: Rust prints
`var _a; if (p === void 0) { … } var _b; return p;` where tsc prints
`var _a; var _b; if (…) { … } return p;`. A temporary probe (not committed)
showed the decorator's `var _b;` carrying the flag (`2097152`) and
class-fields' `merge_lexical_environment` never running for `make`: the
lowered-target class-fields visitor installs its function prelude through
`downlevel.rs` `prepend_function_prelude_to_block`, which counts only the
standard prologue directives before inserting its hoisted `var` and its
initialization statements, so the flagged statement is not skipped. That
reader is cause 5.

### Cause 5 — the class-fields downlevel reader of the flag (commit `8df67d1c1`)

`class_fields/downlevel.rs` `prepend_function_prelude_to_block` (the
port of `mergeLexicalEnvironment` for the declarations that pass hoists into
a function body or constructor) now scans the standard prologue directives,
then the custom-prologue hoisted functions (`is_hoisted_function`), then the
custom-prologue hoisted `var` statements (`is_hoisted_variable_statement`):
its initialization statements splice at `leftHoistedVariablesEnd`, its
generated `var` statement at `leftHoistedFunctionsEnd`, the custom prologues
first, exactly as upstream. Before the change the flagged decorator hoist
was treated as an ordinary statement and class-fields' parameter-default
`if` landed before it.

Focused run `cause5-r1-lexical-prologue-readers-v2/`: 11 exact twice, 5
failed, exit 101, binary `88eb536d2a91…`; the four ES2015/ES2022 parameter rows now
print tsc's text (`var _a; var _b; if (p === void 0) { … } return p;`) and
differ only in the source map of the lowered default (tsc maps the block
and the assignment to the parameter and suppresses the initializer's map;
the class-fields port maps the initializer and not the block); the
ESNext/set row still prints `make(p = class …)` because nothing hoists in
the parameters there. Both residues belong to the pass that lowers the
default, cause 6.

### Cause 6 — `visitParameterList` in the decorators pass (commit `8884fcc05`)

`visit_function_like_body` now visits a function-like's parameters first,
under the environment's `InParameters` window (`visit_parameter_list`);
`hoist_temp_variable` and `hoist_existing_temp_variable` set
`VariablesHoistedInParameters` while the window is open. When it is set and
the target is ES2015 or later, `add_default_value_assignments_if_needed`
lowers the visited parameters: an initializer becomes
`if (name === void 0) { name = initializer; }` with tsc's flags and ranges
(a positionless clone for the condition, a `NoSourceMap` clone as the
assignment target, `NoSourceMap | NoComments` added to the initializer,
the parameter's text range on the assignment and the block, the block
`SingleLine | NoTrailingSourceMap | NoTokenSourceMaps | NoComments`), added
through `add_initialization_statement` (which marks it `CustomPrologue`);
a binding pattern is aliased by `getGeneratedNameForNode(parameter)` (an
ordinary temp binding declared by the parameter, not hoisted) with a
`var <pattern> = alias === void 0 ? initializer : alias;` statement — the
same callee's unwitnessed arm, implemented without a claim. The lowered
array replaces the memoized parameter visit, so `update_generic`'s child
visit reuses it, and `merge_block_environment` splices the initialization
statements at `leftHoistedVariablesEnd` after the hoisted `var` statement.
Constructors are not routed here (tsc's `visitConstructorDeclaration`
visits their parameters plainly).

Focused run `cause6-r1-lexical-prologue-readers-v2/`
([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-cause6-focused.v1.json),
binary `4a8e07b7ea03…` built with causes 5 and 6): the whole
`lexical-prologue-readers-v2` group, 16 exact twice, exit 0, sources and
witness inputs unchanged after the run. The parameter rows now match tsc
in text and map at ES2015, ES2022 and ESNext/set: the decorator pass owns
the lowering, class-fields sees a parameter without an initializer and
hoists its class-expression temp in the body.

## Candidate patches

`scripts/export-decorator-literal-prologue-candidates.sh` exports, with
both endpoints fixed by default (start `5134bb018`, the production base and
PR #513's head; end `8884fcc05`, the cause 6 commit; explicit arguments only
reproduce another range), one `.candidate.patch` per commit that changes
`crates/` or `scripts/` plus the cumulative production diff, under
`docs/design/greenfield/slices/h2-8a-decorator-literal-prologue-NN-<name>.candidate.patch`
and `h2-8a-decorator-literal-prologue-production.cumulative.patch`. The
earlier exporters (`export-decorator-next-candidates.sh` @`306930ab7`,
`export-decorator-followup-candidates.sh` @`0fda49509..a4c089c7b`) and
their default endpoints are untouched.

## Final measurements

All runs below execute the production of the last production commit `8884fcc05` (`standard_decorators.rs` SHA-256 `14ee9d92657a…`, `printer.rs` `506932da77c7…`, `class_fields/downlevel.rs` `5a13751ee3ad…`; `class_fields.rs`, `execute.rs`, `jsx.rs`, `es2018.rs`, `generated_bindings.rs`, `target_bindings.rs`, `metadata.rs`, `es2015.rs`, `system.rs` unchanged since the start manifest) through one contracts test binary built once by `tools/final-chain.sh` (build log `final-build/build.log`; executed binary SHA-256 `4a8e07b7ea03…`, 1 distinct binary in every receipt). Every receipt records the head, the production/fixture/test SHA-256, the environment, the actual exit, the log SHA-256, the archived binary and every capture's SHA-256, and attests that sources and witness inputs were unchanged after the run.

| suite | run dir | cases | exact ×2 | failed | exit | binary | log | receipt |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `literal-member-kinds` | `final-literal-member-kinds` | 48 | 48 | 0 | 0 | `4a8e07b7ea03…` | `23bff75d9406…` | [`h2-8a-decorator-literal-prologue-final-literal-member-kinds.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-literal-member-kinds.v1.json) |
| `literal-key-spelling` | `final-literal-key-spelling` | 48 | 48 | 0 | 0 | `4a8e07b7ea03…` | `51b0b0f8ce03…` | [`h2-8a-decorator-literal-prologue-final-literal-key-spelling.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-literal-key-spelling.v1.json) |
| `lexical-prologue` | `final-lexical-prologue` | 24 | 24 | 0 | 0 | `4a8e07b7ea03…` | `27937b151f5d…` | [`h2-8a-decorator-literal-prologue-final-lexical-prologue.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-lexical-prologue.v1.json) |
| `lexical-prologue-readers-v2` | `final-lexical-prologue-readers-v2` | 16 | 16 | 0 | 0 | `4a8e07b7ea03…` | `39d4cfc5e371…` | [`h2-8a-decorator-literal-prologue-final-lexical-prologue-readers-v2.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-lexical-prologue-readers-v2.v1.json) |
| `transform-order` | `final-transform-order` | 60 | 60 | 0 | 0 | `4a8e07b7ea03…` | `6290dd562a8b…` | [`h2-8a-decorator-literal-prologue-final-transform-order.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-transform-order.v1.json) |
| `super-paths` | `final-super-paths` | 42 | 42 | 0 | 0 | `4a8e07b7ea03…` | `b8adc600ff68…` | [`h2-8a-decorator-literal-prologue-final-super-paths.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-super-paths.v1.json) |
| `name-owners` | `final-name-owners` | 24 | 24 | 0 | 0 | `4a8e07b7ea03…` | `93fefb3101ad…` | [`h2-8a-decorator-literal-prologue-final-name-owners.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-name-owners.v1.json) |
| `source-followup` | `final-source-followup` | 48 | 48 | 0 | 0 | `4a8e07b7ea03…` | `653cceb33802…` | [`h2-8a-decorator-literal-prologue-final-source-followup.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-source-followup.v1.json) |
| `source-followup-top-level` | `final-source-followup-top-level` | 18 | 18 | 0 | 0 | `4a8e07b7ea03…` | `872c645cd05e…` | [`h2-8a-decorator-literal-prologue-final-source-followup-top-level.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final-source-followup-top-level.v1.json) |
| 530 existing commands (`h2_8a_retained_accessor_owners`) | `final-full530` | 530 | 530 | 0 | 0 | `4a8e07b7ea03…` | `0870e604c1c0…` | [`h2-8a-decorator-literal-prologue-final530.v1.json`](../../../../ratchets/h2-8a-decorator-literal-prologue-final530.v1.json) |

- full62 comparison of the 530 current captures against the previous work's reference captures (manifest `badaa9c02d9e…`, comparison script `23507016ab00…`, both verified before the comparison): exact twice 530, changed 0, missing 0, extra 0, exit 0 ([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-final530-full62.v1.json)).
- Emitter suites `cargo test -p tsc-rs-emitter --lib --test contracts -- --test-threads=1`: 495 passed / 0 failed (ok), 452 passed / 0 failed (ok), exit 0 ([receipt](../../../../ratchets/h2-8a-decorator-literal-prologue-final-emitter-suites.v1.json)).

Baseline → final, per group: `literal-member-kinds` 18 → 48 exact, `literal-key-spelling` 28 → 48, `lexical-prologue` 14 → 24, `lexical-prologue-readers` 2/18 → `-v2` 16/16 (the two System rows of version 1 are not in the gating set); the existing 192 and 530 commands were exact before and stay exact with unchanged expectations.
