# r25: review data-only recovery implementation

Actual Claude read-only review; no edits, Cargo, or subagents. Please examine
the current git diff in crates/syntax/src/{recovery,parser,lib}.rs and the two
unit-test files, plus the new tests/emitter_recovery.rs. The 36-input syntax
observer runs each TS parse twice and compares reachable missing nodes and
diagnostics. Native tests are not run yet: the sole heavy local job is validating
the frozen ordinary candidate in the separate integration worktree.

The implementation follows r22/r24: explicit fresh event index for messaged
missing provenance, independent FULL-start UTF-16 position, append-only actions,
checkpoint truncation, skip span/owner reuse guard, untouched-range action
retention, live scanner reparse-end. Reparsed itself does not reject reuse.
Existing literal-only admission is unchanged. No emit admission change yet.

One detail from source: MissingDeclaration nodes are later widened to include
their modifiers (parse_declaration/parse_decorated_expression), so missing-node
position documents creation FULL-start, not final declaration.pos. The first
Identifier/Await admission will be unaffected; later MissingDeclaration admission
must account for the final reachable node and its modifiers explicitly.

Please identify concrete bugs or omitted skip producers in TS/TSX parsing, and
whether the speculative/JSDoc/incremental/reparse record lifetimes remain sound.
Pay attention to the new tests' expectations. About 40 lines, source-backed.
