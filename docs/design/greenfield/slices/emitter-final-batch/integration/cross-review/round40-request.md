Please review this exact immutable-before vs candidate diff, bounded to binary-left comment phases. No edits, Cargo/builds, or broad investigation.

Your r38 analysis read our concurrently modified printer.rs, which already had the proposed fix. That is why all your read paths said the comment should print. BEFORE source is `git show 71b1ae19bb15c373831d7d7c5ac824152d77f93a:crates/emitter/src/printer.rs` (CIrepair repository). Before BinaryExpression emits left through emit_required_node_with_forwarded_source_comments; when state is not Pending that falls back to emit_node_id_with_context with NO deferred source-comments phase. Thus no left leading comment.

Candidate working diff in CIrepair replaces that one binary-left call with emit_expression_child_with_source_comments; preserves its visited trailing anchor before operator; pending forwarding and nested suppression stay guarded. Your r38 statically traced this candidate, not the failing baseline. Please compare these two exact versions and the upstream binary maybeEmitExpression pipeline (TS _tsc.js118396 onwards) and say whether the change is justified and any specific uncovered behavioral risk. Native tests currently compiling; no success claimed.

Second correction: your wrapper range deletion recommendation is NOT accepted. Frozen full TS control `export-destructuring-boundaries/es2015/commonjs/remove-false/re-export` from fixture288 combined is:
/** leaf */
exports.y = exports.x = source.x;
exports.y = 
/** leaf */
exports.x;
There are TWO comment occurrences. `createAllExportExpressions` passes the supplied location to createExportExpression, which setTextRange's its wrapper. Please do not infer location is undefined from the plain shorthand no-alias case. Supplement72 fixture was directly observed twice; full fixtures in CIrepair. We leave wrapper ranges intact unless native controls demonstrate a real mismatch.

The remaining review should be concise and evidence-specific. Previously mistaken cloneNode and wrapper assumptions are preserved in review history with corrections. Fable remains requested unless actual limit.

New native binary from CIrepair candidate compiled: original physical CLI JSDoc case now exits0 and BOTH JS and d.ts bytes match saved TS output. Record in CIrepair integration/records/jsdoc-cli-probe-r38.json. Full contract build/comparison still running.
