## r31: structural admission for skipped-token inputs

### Topology (tsc probes; Rust facts from recovery-native-facts-r25b agree)

| shape | skips (site, span, owning array gap) | events | missing nodes |
|---|---|---|---|
| asyncFunctionDeclaration10 (4 rows) | `=>` [29,31) ListAbort, between `Parameter[19,28]` and `Parameter[31,37]` in the function's parameter array | 1 retained (1109) + 2 suppressed at 29 | Identifier@28, parent AwaitExpression |
| asyncArrowFunction9 (4 rows) | `:` [36,37) ListAbort in `VariableDeclarationList` gap; `=>` [52,54) ListAbort in SourceFile statement gap | 3 retained + 5 suppressed (36×1, 45×2, 52×2) | Identifier@51, parent **TypeAssertionExpression** (`<void>` + missing operand → tsc prints `;`) |
| import-helpers comment rows (12) | `)` [100,101) ListAbort in SourceFile statement gap | 3 retained + 1 suppressed; two are report-only: 1005 (`)` expected; `ParenthesizedExpression[69,80]` ends at the full start of `as`), 1434 on `number` [94,100) | none |
| topLevelAwaitErrors.1 (2) | 3 ListAbort inside one class statement + `Reparsed[10,682)` | 15 retained, 3 suppressed **with** missing nodes | 12, parents Binary/Paren/Await/Call/Decorator |

tsc emits skipped tokens as nothing; the gap only influences comments and maps. Rust's printer has no per-token raw copy for these shapes: the expression worker's `_ if !changed` fallback (printer.rs:8164) is reached only by kinds without an explicit arm, and the statement-level raw slice (2836-2847) is gated to `is_dormant_declaration_syntax_kind` (type-only kinds). The missing `)` is emittable structurally: `emit_token_with_comments_at_boundary` (16967-17115) writes the fixed token at `skip_trivia(expression.end)` without checking the source spelling and advances by the spelling length, exactly like `writeTokenText`, and it runs the leading-comment phase at the anchor, which is where tsc gets the first `/* value */`.

### Invariants (all computable from the record plus the reachable tree; no codes, no ids)

- I1 skip placement: each `TokenSkipped` span lies inside exactly one reachable node array gap (array `pos ≤ start`, `end ≤ array.end`, no element intersects it) and the innermost reachable node containing the span is that array's owner. Owner kinds allowed initially: SourceFile/Block/ModuleBlock statements, VariableDeclarationList declarations, parameter lists. A skip inside a leaf or non-list node refuses (this is what keeps raw-copy fallbacks unreachable).
- I2 suppressed events: `diagnostic_index: None` requires `missing_node: None` and `start` equal to a retained event's start or a `TokenSkipped.start`. Suppressed missing events (TLA rows) refuse.
- I3 report-only retained events (no missing node) must have one tie: (a) node-end tie, a reachable node whose `end` equals the event's **full start** and whose printer arm emits its own closer (ParenthesizedExpression first); (b) node-span tie, event span equals a reachable node's trivia-skipped span (the `number` statement); (c) skip tie, `start` equals a `TokenSkipped.start`. Anything else refuses.
- I4 missing nodes: as in stage B, unique reachable match by full start, retained diagnostic, parent kind in an explicit set; the set grows per group (Await now, TypeAssertionExpression with arrow9).
- I5 no `Reparsed` actions, JSDoc excluded, literal-only short-circuit untouched.

### Provenance gap

I3(a) needs the event's full start (1005 is reported at the `as` token start 91, but the paren node ends at 80). Add one field on Parser events at push time (`full_start = to_utf16(scanner.full_start_pos())`), creation-only, so checkpoints roll it back. That is the only missing fact. A producer-site enum for reports (the analogue of `ParseTokenSkipSite`) would make I3 ties explicit and is worth adding if ambiguity appears, but the ties above already hold without it. A "generic reliable emission" printer mode is not needed while I1 excludes skips from leaves; revisit only if a leaf-internal skip shape appears.

### Group order after MissingDeclaration

- G1, asyncFunctionDeclaration10 (4): B rule + I1 (parameter-list gap) + I2. Smallest new surface.
- G2, comment rows + asyncArrowFunction9 (16): + I3 with the ParenthesizedExpression closer, TypeAssertion parent in I4, statement/declaration-list gaps, EmptyStatement.
- G3, topLevelAwaitErrors.1 (2): reparse plus suppressed missing events and decorator operands; separate design.

### Controls (complete commands, ES5/ES2015/ESNext, maps on)

G1 positive: the original; `async function f(a = await /*c*/ => /*d*/ await) {}` (comments in the gap); `async (a = await => await) => {}` inside a function body (must stay refused until G2, its `:`-less form is a G1 shape only if no arrow reparse). Negative: `function f(a, => b) {}` (report-only + skip, no missing node: G2 territory), `async function f(a = await => => await) {}` (two skips at one gap).
G2 positive: both original comment shapes; `(o.x\n as number)` without comment; `x = (a\n as T)` outside `export`; ES5 optional-chain lowering with the missing closer. Negative: `(a /*c*/` at EOF (closer at EOF, tie (a) with a zero-length report), `foo(a, b` (call-argument closer; keep out of the (a) kind set until measured), `[a, b` and `{a: 1` (bracket closers), and a shape with two suppressed reports at one start.
Dedup: `async function f(a = await => await): Promise<void> {}` already carries 2 suppressed at one start; add `let x = <void> =>;` for the TypeAssertion missing operand alone.

### Must be measured natively, not inferred

The duplicated `/* value */` (tsc emits it at the closer and again as the `as;` statement's leading comment; Rust's owner-claim comment phase may suppress the second), the `;` printed for an erased TypeAssertion around a zero-width operand (goes through the PartiallyEmittedExpression comment path), ES5 `_a.sent()` default-parameter lowering followed by a second parameter, and map entries at skipped gaps (tsc maps the emitted `)` to the `as` token start).