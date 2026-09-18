# r24: recovery data-only implementation refinement

Continue as actual Claude read-only reviewer. No source edits, Cargo, or subagents.
Fable preferred; on a limit the caller will resume with Opus. Please keep response
to concrete issues/decisions, about 40 lines; no repeated broad source inventory.

The ordinary 551-case oracle is frozen and a focused native build is running.
Next we will freeze/push that candidate and run its validation from the separate
integration worktree while implementing recovery in the audit worktree. No Rust
source used by an active build will be edited.

Read recovery-design-r22.md and current recovery.rs/parser.rs. Codex refinement
to your r22 missing_node field: it must hold {kind, position} with FULL-start
UTF-16 position, independently of diagnostic.start (token-start after trivia).
A reporting helper can return (diagnostic_index, event_index); create_missing_node
attaches data only to that newly appended event, including same-start dedupe.
SilentMissingNode already has kind/full-start; no duplicate missing field needed.

For reparse actions, record start at the run's original first-statement pos and
end at scanner.full_start_pos after the loop. This includes a final no-progress
skip, unlike last statement.end, and is taken before restoring scanner state.
Retain TokenSkipped by statement_start in untouched ranges; retain Reparsed by
its start (currently saved_recovery cannot contain an earlier reparse action,
since there is one top-level pass). Incremental current_node should reject a
candidate intersecting the whole Reparsed range, not just its start, and a skip's
token span/statement owner. Keep literal-only admission exactly unchanged.

For the first later admission, walk reachable syntax excluding JSDoc and match
missing records by their own position, not by diagnostic.start or all arena nodes.
Require Identifier operand of AwaitExpression, retained diagnostic, no actions;
all other events must be literals. Do not change printer empty-identifier behavior
without measured failure. Preserve clean/literal admission even with Reparsed
actions. The 24 original rows plus nearby comment/map/target controls must all be
compared as complete commands before retirement.

Please find any actual flaw in this lifetime/range proposal, especially fresh vs
incremental equality for valid top-level-await reparse, and propose the smallest
necessary correction. Confirm if the FULL-start refinement avoids a real bug.
