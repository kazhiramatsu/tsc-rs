# OPS-DEBT-EMPTY-SOURCE: close the statement list before EOF comments

Base: `ccb6661c16f75cd6824e9fedb9281be68d01e542`. This bounded prerequisite for
OPS-COVER-3F repairs the existing
`h2-8b-config-source-span-commands/forced-empty` complete command. Its two tests
fail before any production edit (22 other config/library tests pass). The
unchanged prologue-only target passes all eight inputs twice.

## Source contract and current owner

The pinned TypeScript 6.0.3 `emitSourceFileWorker` emits a `MultiLine` statement
list. `emitNodeListItems` requests the closing line terminator via
`getClosingLineTerminatorCount`; that happens before
`emitBodyWithDetachedComments` emits comments at the original list end.
The checked source ranges/hashes are in [readiness.v1.json](readiness.v1.json).

Current architecture rows `E-PRINTER-BASE` and `E-COMMENTS-G` retain printer and
comment ownership. The Rust owner is
`Printer::write_transformed_source_file` in `crates/emitter/src/printer.rs`.
Its statement separators precede each statement. When the original file had no
statements but a transform inserted the module marker, the statementless-source
comment branch runs immediately after the last generated statement. The closing
newline is currently written only after those comments. Consequently `// empty`
is appended to the generated `Object.defineProperty` line and its map segment
starts at column 78 instead of column 0 on the next line.

Move that branch onto the closed statement list by writing the existing
idempotent `write_line(false)` before its original-source comments. No other
branch, comment range, generated binding, hook order, map algorithm, transform
admission or declaration behavior changes. Only `crates/emitter/src/printer.rs`
is authorized for this production prerequisite.

## Frozen denominator and checks

The existing config source-span fixture has six inputs. Only `forced-empty`
is reproduced as a difference; all its expected fields remain frozen. The other
five source-span inputs, the rest of the 96 config/library inputs, and the eight
prologue controls are the retained comparison denominator. Two module guards
remain refusal controls. The same failing complete-command comparison runs in
both the command test and the Program-facts test; these are one defect.

Before editing, run `python3 docs/design/greenfield/slices/witness-coverage/compiler-config-prologue/check_readiness.py --before`.
The schema/checker binds the source owners, baseline printer, immutable fixture
and test bytes, architecture rows, observed failure and zero unresolved owners.
After editing, the checker runs without `--before` and preserves those identities.

After: both original failing tests must pass with complete writes/metadata,
JS and declaration maps, diagnostics, ordered membership, result/status/exit.
All 24 config/library tests and prologue test must pass through the new runner.
Run the adjacent emitter printer/failure/list/comment contracts plus formatting.
The combined final branch receives the full affected hosted coverage.
No manifest shrink or profile promotion is authorized by this repair.
