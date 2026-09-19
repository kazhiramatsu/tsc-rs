The comment-only repair is sound, and I found no review blocker. It needs two gates beyond `ledger check`, one mechanical containment rule, and a few exact corrections below. This comes from reading the ledger parser and the pinned index and from hashing `_tsc.js` slices read-only; I made no edits and ran no builds.

## Parser rules that constrain the repair (`main.rs:7061-7198`)

- **Multiple references in one function are fine.** Each `tsc-port:` line opens a block that runs to the next `tsc-port:`. The first `tsc-hash:` and `tsc-span:` inside that block belong to it.
  - To split a reference, write consecutive complete blocks in the order port, hash, span.
  - A hash or span line placed before the first port line is ignored.
- **Port line.** The first word is the name and the second must be `@6.0.3`. Anything after that is ignored, so prose goes after the version. `name (prose) @6.0.3` and `a / b @6.0.3` are what break today.
- **Span line.** It is split at the last `:` and then at the first `-`. It must contain exactly one `start-end` pair and nothing after it: no commas and no prose.
- **Hash.** SHA-256 of `split_inclusive('\n')` lines `start..=end`, which includes the final newline. Your audit script matches this.

## Two gates to add

1. **The frozen D2 disposition audit.** `exact_ledger_matches` (`main.rs:5907`) joins a ledger entry to an `m8-emitter-inventory.json` function by exact start line, end line and hash. `m8-emitter-dispositions.json` is **frozen** (5,513 rows: 1,676 ported, 3,837 deferred).
   - Every frozen *ported* row must still have a join after the repair.
   - A *deferred* row gaining a join is allowed (it only moves one way).
   - A malformed entry makes `collect_ledger_entries` fail as a whole, so the D2 audit can't run today. Run it after the repair.
   - Where a function exists in the inventory, take whole-function spans from its `source_range`, so a join is exact and not accidental.
2. **Containment check (mechanical).** Each repaired block must be one of:
   - an exact function from the index whose name matches the port's name;
   - a documented sub-span that lies strictly inside the function named on the port line.

   Anything else needs manual review. This is what rejects spans that hide where the code came from.

The whole-file pins of `printer.rs`/`parser.rs` in the oracle profiles are already expected to change in this train and are refreshed at the final freeze, so comment edits add nothing new there. D2 ignores Rust line movement.

## The four stale hashes: two wrong spans, two wrong hashes

| Entry | Finding | Fix |
|---|---|---|
| `downlevel.rs:1063` `transformClassStaticBlockDeclaration` | The recorded hash `4b66f4eb…` is exactly the whole function 96649-96682 | Set span to `96649-96682` and keep the hash. Keep "(the map write)" as prose after the version |
| `system.rs:1846` `transformSystemModule.visitVariableStatement` | The recorded hash `6571fc05…` is the whole function 112634-112683 | Set span to `112634-112683` and keep the hash |
| `ensure.rs:528` `isInternalDeclaration` 12601-12635 | The span is the exact function. The recorded hash is the same bytes without the final newline | Keep the span and recompute the hash |
| `ensure.rs:1040` `hasInternalAnnotation` 12597-12600 | Same: correct span, hash computed without the final newline | Keep the span and recompute the hash |

## Mappings for the ambiguous entries

Hash every new span with the standard rule. Prefixes shown here are ones I computed.

- **`type_nodes.rs:4612`**: split into two blocks.
  - `getTextOfJSDocComment` `11773-11775` (`3eb78aff…`)
  - `formatJSDocLink` `11776-11781` (`222396e5…`)
  - This is a checker file, but the function already carries a `tsc-port`, so its disposition count doesn't change.
- **`builtins.rs:12819`**: split into three blocks.
  - `getGeneratedNameForNode` `21652-21666` (`createNodeFactory.getGeneratedNameForNode`, `7aeec7c8…`).
  - The transformTypeScript reference as a sub-span of `visitClassDeclaration` (94434-94548). The recorded `94455-94456` is **off by one**: line 94455 is a closing `}`. The call site is `94456-94457` (`const needsName … ; const name = … factory2.getGeneratedNameForNode(node) …`). Use `visitClassDeclaration @6.0.3 (transformTypeScript needsName/name)` with span `94456-94457`.
  - `generateNameForNode` `120876-120942` (`createPrinter.generateNameForNode`). Note the upstream name is **generate**NameForNode, not getGenerated…
- **`factory.rs:2220`**: the recorded hash `4d36f6cd…` is exactly `createTemplateLiteralLikeNode` `22885-22890`.
  - Keep that span with its hash.
  - Add a second block `createNodeFactory.update @6.0.3` with span `24995-25001` (`384440fe…`). Qualify the name because `update` is ambiguous in the index.
- **`factory.rs:2305`**: the same pattern.
  - `createStringLiteral` `21529-21534`: its existing hash `2bf21e80…` matches.
  - Plus `createNodeFactory.update` `24995-25001`.
- **`factory.rs:6126`**: `parenthesizeExpressionsOfCommaDelimitedList` `20479-20482` is correct as written. Add the hash `dd7a34a8…`.
- **`factory.rs:6128`**: four blocks.
  - `createArrayLiteralExpression` `22441-22449`
  - `createCallExpression` `22579-22595`
  - `createCallChain` `22602-`**`22616`**. The recorded 22617 is the first line of `updateCallChain`.
  - `createNewExpression` `22621-22631`
- **`bundle.rs:17`**: move the span out of the prose.
  - Port line: `transformRoot @6.0.3 (transformDeclarations bundle arm)`, span `114446-114513`.
  - That span is the body of `if (node.kind === 309 /* Bundle */) { … }` inside `transformDeclarations.transformRoot` (114441-114614), running from `isBundledEmit = true` to `return bundle;`, so it passes the containment check.
- **`bundle.rs:231`**: two blocks.
  - `isExternalModuleAugmentation` `13737-13739`
  - `isModuleAugmentationExternal` `13740-13748`
- **`downlevel.rs:6602`**: two blocks.
  - `visitClassExpressionInNewClassLexicalEnvironment` `97049-97129` (`5885e805…`, the same hash already used on `inline_class_expression`)
  - `transformClassMembers` (transformClassFields) `97143-`**`97237`**. The recorded 97240 runs 3 lines into `createBrandCheckWeakSetForPrivateMethods`.
- **`parser.rs:9496`**: reject `25101-25192`.
  - It covers 7 functions, including unrelated `propagateChildrenFlags`/`aggregateChildrenFlags`.
  - It also cuts off `getTransformFlagsSubtreeExclusions` (25125-25194) partway through its body.
  - Give only the functions the Rust projection actually mirrors: the top-level-await bit and the name, child and function-boundary exclusions. That is, as four blocks:
    - `propagateNameFlags` `25101-25103`
    - `propagateIdentifierNameFlags` `25104-25106`
    - `propagateChildFlags` `25110-25114`
    - `getTransformFlagsSubtreeExclusions` `25125-25194`
  - Before splitting, confirm from the Rust body that `getTransformFlagsSubtreeExclusions` really is the source of the function/arrow/constructor stripping.
- **`parser.rs:10206`**: change the port text only, to `parseIsolatedEntityName2 @6.0.3 (validity projection)`.
  - The span `29042-29061` and its hash already verify.
  - Line 29061 is `Parser2.parseIsolatedEntityName = parseIsolatedEntityName2;`, which is how the Parser exposes it, so including it is defensible.
  - Changing only the port text keeps the parser diff smallest.
- **Our es2018 helper** `prepare_for_await_object_rest`: the span `102196-` starts on the previous function's closing `}`. Use `102197-102240` with hash `646f8557108f52ce…`.

## Parser corpus and the inherited standard_decorators entries

- **Parser corpus.** Both parser edits are comments only. Pin one exact before/after full-file SHA pair for `crates/syntax/src/parser.rs`, the same way as for `context.rs`, not a path allowlist. The four full parser replays with core and fact equality still apply.
- **Inherited `standard_decorators` blocks (67).** Apply the same containment check.
  - Missing hash with a span that exactly matches the named function: compute the hash.
  - Span that is not inside the named function: fix the span. Don't compute a hash for it as written.
  - Leave existing valid blocks alone unless the check flags them.
