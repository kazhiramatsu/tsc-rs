# Repair candidate after r95 complete commands

At c1e699ab57cf945641f86055249cb0835aa9ea1d, the syntax library passed 199 tests and the emitter library passed 510. The six complete-command boundary tests executed 1,526 cases: 1,483 exact in both runs and 43 failures. The original 844 recovery cases all passed. The checker library step was not run after the boundary failure.

The immutable c1 contracts binary was copied and hashed before supplemental capture. The additional 442-case execution produced 841 observations (399 cases twice, 43 failing cases once). It confirms four JavaScript-only decorated-class name differences and 39 source-map-only differences, including the command fields after the first failed comparator. These supplemental executions are separate from the original qualification. See the r98 complete capture report/archive and pause receipt.

The unqualified successor candidate addresses:

- Standard decorators and ES2015 class constructors reuse the generated binding identity for the same original class. A context-local map retains the existing generated identifier; dispose clears it.
- The yield asterisk uses ordinary token-node emission, including maps and comment phases. Async delegated yield updates the original yield rather than creating an unpositioned node.
- For-await head statements, binding statements, visited block and statement-array locations follow the reference transform; the enclosing generated try remains synthetic.
- Codex and actual Claude Opus independently located premature comma-sequence parentheses in class-fields lowering. A partially-emitted wrapper needs the enclosing factory to place parentheses around the whole wrapper chain and carry its source range. Opus100 approved deferring parentheses on partially-emitted wrappers and identified the statement-callee grammar obligation that must be preserved with it. Both changes are now implemented, with 18 further whole-command controls.

Opus99 found no correctness blocker in the first three changes, verified source identity/lifecycle of the shared generated-name cache, and requested formatting (completed). Its production-path panic note was addressed with existing typed transform errors. Its object-rest observation is covered by additional whole-command controls; no object-rest correctness claim is made yet.

New immutable TypeScript controls: 288 cases in emitter-r95-neighbours plus 88 in emitter-r95-wrapper-rest and 18 in emitter-r95-call-boundaries; every command was observed twice identically. They cover target/module/comment settings for class names, erased wrappers in declaration/call/assignment/unary/array/return/arrow/property/conditional positions, yield token trivia/nesting, and for-await block/single/label/destructure/nested/object-rest bodies. Native comparison remains pending. Hosted system-control membership is now 3,584 input cases across eleven Rust tests (including the separately pinned r77 roster).

The Gate-prep pin/schema commits have been integrated as 6b60f27d7 and c3093bd03; Opus99 found no high/medium flaw. Historical observations were not re-minted, and verification-only pins cannot be auto-repaired. Final policy/fuzz pins, H1 artifact freshness, the chain walk and full gate remain outstanding.

The frozen 67df86615 census continues with its executable identity preserved. No parser KNOWN retirement or corpus qualification is claimed before its snapshot, the four parser replays, and selected native commands are verified.
