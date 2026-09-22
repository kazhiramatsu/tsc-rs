# Codex independent recovery inspection (r20, not a compatibility verdict)

Frozen EF7 inputs were extracted unchanged for the 36 live KNOWN commands in
`recovery-inputs-r20.json`. A TypeScript 6.0.3 parse-tree census is in
`recovery-typescript-ast-r20.json`; this is not native output parity evidence.
All 32 async rows contain a zero-width Identifier. The 2 top-level-await rows
each contain 12; the 2 decorator rows contain nonzero-width MissingDeclaration
nodes with decorator modifiers, so a zero-width-only census misses them.

The earlier review statement that missing-identifier printing has no contract
is too strong. `syntax::Arena::alloc_missing` uses `NodeData::missing` and empty
Identifier text. `emitter::SourceByteRange::without_leading_trivia` explicitly
bounds scanning at `end` to preserve empty recovery ranges (`position.rs:65`).
The unchanged-node printer fallback writes that bounded slice; the changed
Identifier branch writes its empty text. This must be validated end to end,
including transforms, comments, map records, and diagnostics, but a new printer
special case is not justified solely by a grep for the word 'missing'.

`create_missing_node` currently folds message-bearing missing nodes into a
Diagnostic event. An effect attached to that existing event can preserve event
counts while exposing the structural producer. SilentMissingNode already names
the missing kind. Avoid changing historical typed-refusal counts just to add
metadata. `parse_declaration_worker` widens MissingDeclaration.pos to include
modifiers, so recoverability must use the producer kind, not just pos == end.
