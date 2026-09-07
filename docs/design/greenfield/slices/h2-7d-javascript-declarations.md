# H2.7d original JavaScript declaration residuals

The original Bundle census exposed four declaration differences. This packet
repairs their existing checker/NodeBuilder producers. It does not change the
Bundle visitor, module-specifier naming policy, printer, options or admission.
The semantic baseline remains TypeScript 6.0.3 commit
`050880ce59e30b356b686bd3144efe24f875ebc8`.

| Original input | Prior Rust output | TypeScript behavior and producer |
| --- | --- | --- |
| `jsDeclarationsImportTypeBundled.ts` | `import("folder/mod1").Item[]` | Retain `(typeof import("folder/mod1"))[]` by finding the variable statement's owned JSDoc type annotation. |
| `jsdocAccessibilityTagsDeclarations.ts` | The private method's JSDoc disappears. | The collapsed property retains the method's original location. |
| `jsdocReadonlyDeclarations.ts` | `readonly x: number` | Effective JSDoc readonly keeps the initializer literal `6`. |
| `uniqueSymbolsDeclarationsInJs.ts` | A readonly static `Symbol()` property has type `symbol`. | Effective readonly makes this a valid `unique symbol` declaration. |

`getJSDocType` (`_tsc.js:11721-11726`) and `getEffectiveTypeAnnotationNode`
(`16761-16766`) consult the owned JSDoc chain. Rust's syntactic builder had
searched only tags directly attached to the variable declaration, missing its
containing statement. The existing binder `get_jsdoc_type_tag` already models
that AST ownership walk, including initializer attachments and assignment
locations. Reusing it allows the existing annotation-reuse and bundled module
specifier logic to run. No type-equality condition is weakened and no source
text is reparsed.

`isDeclarationReadonly` (`14128-14130`) uses combined effective modifiers and
excludes parameter properties. The checker and syntactic builder now use that
same existing binder query, retaining the parameter-property predicate.
`isValidESSymbolDeclaration` (`14377-14379`) uses effective readonly for both
property declarations and signatures, so JavaScript JSDoc participates in
unique-symbol eligibility. Static and declaration-shape checks are unchanged.

`makeSerializePropertySymbol` (`55099-55229`) gets call signatures before the
private-method branch. Its collapsed property uses the first function-like
declaration, then the first signature's declaration, then the first symbol
declaration as its location. Restoring that selection retains JSDoc through
the existing `setTextRange2` and comment printer; no synthetic comment copy is
introduced.

The pinned native source at `1f70213d4922b434345f639b441681e470c7cfc1` was
reviewed at `checker/utilities.go:843-845` and `976-984`. It retains the combined
readonly/parameter-property distinction, while its symbol eligibility and
JavaScript support differ from the TS6 JSDoc path. Those differences do not
replace the fixed TS6 observations.

## Verification contract

The focused compiler test joins the four original case IDs directly from
`ratchets/h2-7de-candidate-inputs.v1.json` and
`ratchets/h2-7de-observations.v1.json`. Files, roots, current directory, case
behavior, effective options and complete frozen TypeScript observations remain
unchanged. Serialized TS library filenames are projected to the existing Rust
catalog keys with an exact filename roundtrip assertion. No fixture is copied,
no option is dropped, and no alternate input enters the original denominator.

Two fresh Programs per original input compare full JavaScript and declaration
bytes, BOM, source association, URL/map presence and declaration diagnostics
through the same internal live-resolver Bundle seam. The production corpus
runner separately compares the complete command/Program tuple, semantic and
program diagnostics, callbacks and activity. The previous 122 internal Bundle
comparisons run alongside these eight focused comparisons. The combined four
tests pass all 130 comparisons (54.83 s), and emitter/checker/compiler
all-target clippy passes with warnings denied (1m 08s). Final source review
tightened the private-method location predicate to the exact TS
`isFunctionLikeDeclaration` family; the focused eight comparisons pass again
(3.25 s), followed by all-target clippy with warnings denied (27.55 s).
Adjacent frozen declaration replay remains the integration owner's check
because these producers are also shared with non-Bundle declarations.
