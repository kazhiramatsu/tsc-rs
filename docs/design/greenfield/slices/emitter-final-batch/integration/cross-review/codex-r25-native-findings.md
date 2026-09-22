# Native recovery facts: corrections to review assumptions

Data-only syntax comparison: all 36 original inputs match two fresh TypeScript
parses for reachable missing nodes and complete syntax diagnostics. The native
facts classify 24 rows with one event/no actions, four async-arrow rows with
eight events/two skips, four async-function rows with three events/one skip,
two MissingDeclaration rows with two events/no actions, and two top-level-await
rows with eighteen events/three skips plus one reparse action. This is syntax
proof, not emit compatibility; no production admission has changed.

The first unit run passes 177 and fails two newly added expectations. Actual
measurements correct r22/r25 review assumptions: asyncFunctionDeclaration10 has
THREE reporting attempts at the skipped arrow, one retained and two suppressed,
not two events. Its native syntax diagnostics and missing operand already match
TypeScript. Pin both suppressed events and their starts.

Also `await f()` is already AwaitExpression on the initial parse, so it correctly
produces no Reparsed action. A TypeScript source-file callback independently
confirms that `await(f())` starts as CallExpression, then becomes AwaitExpression
on the reparse. Use that genuinely ambiguous shape to test retained before/after
diagnostics and fresh/incremental equality, while preserving a direct-await
negative control. Do not manufacture Reparsed facts for already-parsed await.

The corrected rerun r25b passes all 179 library tests, the complete 36-row syntax
comparison, and the existing recovery-provenance contract (105.577 seconds).
Incremental reuse is conservatively disabled across recorded token skips so
fresh and incremental parses retain the same facts. Emit admission is unchanged.
