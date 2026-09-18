# Recovery facts: joint design before admission changes

Claude Fable r22 and Codex agree on a separate append-only action record for
skipped tokens and actual reparse ranges. Mutating the last existing diagnostic
event is insufficient: some progress skips emit no diagnostic, speculative
rollback only truncates appended records, and retain_reparse_recovery handles
untouched ranges rather than the range being reparsed. Record each skip at its
producer before next_token and each reparse run where it actually executes.
Checkpoint/restore and untouched-range retention must preserve these actions;
incremental reuse must not drop them. Keep old event counts and the literal-only
admission predicate unchanged in the data-only stage.

Message-bearing missing nodes can attach provenance to the newly appended
reporting event, using its explicit new index even when diagnostic dedupe returns
None. SilentMissingNode already represents its kind. JSDoc uses the same parser
then restores the recovery checkpoint, so all new fields/actions must follow
that existing lifetime; retained JSDoc arena nodes are not committed recovery.

Codex refinement from source: the missing node's FULL-start position must be
retained separately from the diagnostic start. create_missing_node allocates at
scanner.full_start_pos, but a non-EOF diagnostic starts at scanner.token_start.
Whitespace/comments make these positions differ. A missing-node record should
therefore carry kind and its own UTF-16 position (no allocation-dependent NodeId).
A later parent-kind census must walk reachable syntax, exclude JSDoc, and use
that missing-node position, not search every arena node by diagnostic.start.
Unreachable speculative nodes can remain allocated after rollback.

The first proposed admission is narrowly missing Identifier operands of Await
expressions with no skip/reparse actions; all complete observations and nearby
comment/map/target controls must pass first. This is an AST/producer predicate,
not a case ID, diagnostic-code list, source-text heuristic, or blanket recovery
admission. The 24 candidates are not yet fixed. The 8 skipped async rows, 2
MissingDeclaration rows, 2 top-level-await reparse rows, and 12 malformed-comment
controls remain actual work. No parse row is reclassified as a separate product.

The r24 source review confirms that FULL-start is necessary for comments/trivia.
It refines two details before implementation: record a reparse run's end from the
live scanner before restoring it (including a final no-progress skip), and reject
incremental reuse for skipped-token spans but not for the Reparsed action itself.
The top-level pass executes again and reproduces that action; rejecting every
valid await range would unnecessarily reduce reuse. Fresh/incremental tests must
cover both valid and erroneous await modules. Later admission also requires a
unique reachable missing-node match per event and the converse, excluding JSDoc;
same-position recovery chains must not accidentally grant admission.
