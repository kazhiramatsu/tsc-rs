# r28: next bounded recovery shape, MissingDeclaration

Read-only actual Claude review: no edits, Cargo or subagents. Fable remains
available (r27 succeeded); switch to Opus only if a rate limit is reported.
The missing-await candidate and 60 complete-command adjacent observations are
frozen for native tests, so do not change files. Review the next shape only.

Original esDecorators-decoratorExpression.3.ts has two MissingDeclaration nodes
and no skip/reparse actions, at both experimentalDecorators values. Native and
TS syntax diagnostics plus reachable missing nodes already agree (r25b).
Input/frozen commands: recovery-inputs-r20.json; facts:
recovery-native-facts-r25b.json in this cross-review directory.

Codex finding: TS createMissingNode(MissingDeclaration) first records the
full-start at the missing end, then setTextRangePos widens the node across its
modifiers. Both parseDeclarationWorker and parseDecoratedExpression do this.
Printer emitWorker has explicit MissingDeclaration return in both ordinary and
expression hints (typescript.js near 122292/122506). Rust ordinary printer has
no such explicit kind, but the standard-decorator visitor and isolated d.ts
visitor mention it. We must not replace with NotEmittedStatement or eagerly
erase modifiers without comparing upstream comments/maps and transform visits.

Please trace the exact TS transform/printer path and Rust path for the original
two rows. Identify the smallest prospective implementation and structural
admission rule, especially the event-to-node match after range widening,
modifier/decorator children, possible expression-position MissingDeclaration,
and whether passing through visits incorrectly evaluates decorator expressions.
Propose a bounded adjacent matrix for empty body, prefix/trailing comments,
source maps, standard/legacy decorators and ES5/ES2015/ESNext. We will first
census and measure full commands; do not claim compatibility from syntax alone.

Also correct any r27 shorthand: an unclosed async arrow at EOF can retain a
missing closing-brace report in addition to its missing await operand; that
is not automatically admitted by our strict event predicate. No need to revisit
the whole r27 implementation unless this affects the MissingDeclaration design.
