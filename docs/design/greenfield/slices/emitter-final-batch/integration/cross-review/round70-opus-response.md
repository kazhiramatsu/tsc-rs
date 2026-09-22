The design is sound, but there are five blockers before the full census. Four are in the digest and command-input capture; the fifth is replay-harness hygiene. The selective baseline loader pass is not needed if you use the rule below. This is a code read only; nothing was built or run.

## Blockers
**B1. The AST scalar payload is incomplete** (`recovery_parse_snapshot.rs` `digest`, the `payload` match). Today it hashes Identifier/PrivateIdentifier `escaped_text`, string/NoSubstitution/numeric/BigInt/regex text, and the prefix/postfix operator. The script over `nodes.rs` finds these non-child scalar fields that are not hashed:
- **SyntaxKind values:** `HeritageClause.token` (`extends` vs `implements`), `ImportAttributes.token` (`with` vs `assert`), `ImportClause.phase_modifier`, `MetaProperty.keyword_token` and `TypeOperator.operator`.
- **Booleans:** `is_type_only` on Import/Export declarations, clauses and specifiers; `ExportAssignment.is_export_equals`; `ImportType.is_type_of`; `ImportAttributes.multi_line`; `StringLiteral.has_extended_unicode_escape`; `RegularExpressionLiteral.is_unterminated`; `JsxText.contains_only_trivia_white_spaces`.
- **Text:** TemplateHead/Middle/Tail `text` and `raw_text`, NoSubstitution `raw_text`, `JsxText.text`, `NotEmittedStatement.text`, and Identifier `text`.
- **JSDoc payloads:** `comment` on the doc and tag nodes, JSDoc link/text `text`, the `postfix`/`is_name_first`/`is_bracketed`/`is_array_type` flags.

Two parses that differ only in these fields hash equal. That's a false-equality path, so the digest can't be claimed exact.

**Fix:** drive the payload from the generated `for_each_observable_field`, keeping its field names. It covers every Bool/String/JsString field and every Node/NodeArray field, so use it for child edges too instead of `for_each_child`. Then:
- add an explicit arm for the five SyntaxKind-valued fields, plus `ImportAttributes.multi_line` and `JSDocComment` if the generator skips them;
- add a unit test that parses `nodes.rs`' field list and fails if any non-child field is not covered.

`nodes.rs`, `observable_fields.rs`, `for_each_child.rs`, and the `types` and `diagnostics` crates are all unchanged since 3b1f5fe87. So the same digest source compiles against the merge base and produces identical field sets, and there is no API compatibility issue.

**B2. SourceFile-level facts are incomplete.** The digest records references, pragmas and the external-module indicator, but not these fields:
- `comment_directives`: `@ts-ignore`/`@ts-expect-error` feed diagnostic suppression, and a reparse rescans comments;
- `module_name`, `renamed_dependencies`, and `has_jsx_import_source_pragma`/`has_jsx_runtime_pragma`;
- `language_version`, `language_variant`, `is_declaration_file` and `js_doc_parsing_mode`.

**Fix:** destructure `SourceFile` exhaustively, with no `..`, the way `options_json` destructures `ParseOptions`. Map node references to UTF-16 ranges and leave out only the arena and recovery internals. Any field added later then fails to compile.

**B3. Don't use the module-request summary as proof of anything.** It misses import attributes (`resolution-mode`), ambient and module augmentations, and implicit helper requests, as you noted. The proof below doesn't need it; keep it as information only.

**B4. Command inputs lose information.**
- **Qualified and candidate routes:** `artifact_input` hard-codes `"use_case_sensitive_file_names": true`. Take the value from the loader, or assert it against the artifact.
- **Recorded compiler plans:** symlinks are exported as `global_symlinks` followed by the per-unit `document_symlinks`, normalized only. That drops the raw target/link/anchor spellings and the effective FileSet order, where a repeated key replaces its target in place.
  - Export `global_symlink_directives` (lossless), the effective `global_symlinks`, and each unit's `document_symlinks` separately, each with its phase.
  - Better still, also export the host VFS the loader actually mounted (path → content hash, plus the symlink map) from `PreparedProgram`, if it exposes it. The `prepared.source_files` summary lists only program sources, not package.json, config, or files that are present but not loaded.

**B5. Replay-harness hygiene** (`replay-recovery-parse.py`).
- `Cargo.lock` is copied only if absent, so a build directory shared between labels keeps a stale lock. Always overwrite it, or require a fresh `--build-dir` per label.
- For the reverted-helper tree, record `parser_diff_sha256`, as the script already does, and also enforce the round-65 guard. Fail unless `git diff 81d5aa52e -- crates/syntax/src/parser.rs` of that tree is exactly the helper function spans, or pin a scripted patch by hash.

## Showing loading is unchanged without a selective loader pass
The module planner (`crates/program/src/module_requests.rs:291-335`) parses each source with options identical to `prepared_parse_options`: the case-preserving path, ParseAll JSDoc mode, and the same module-detection logic. It then derives every request from that parse output alone. So:

- **Rule:** a row goes into U if any of its parse inputs, in either the acceptance role or the module role, has a different full core digest (after B1 and B2) or different profiles under any baseline (reverted helper or merge base).
- **Why this is enough:** suppose the old parser would have loaded a different file set for a row. Walk the planner's load order. The first file whose parse differs was loaded by both programs, because every earlier file parsed identically. It is therefore among the captured module-role inputs, so the row is flagged. Rows in U are qualified by complete TS/native commands, so their loading under the old parser never has to be reproduced.
- **What's left is tiny:** the reparse gate skips declaration files, so library `.d.ts` inputs can't change. JSON parsing is untouched, so skipping JSON is fine. In practice, changed module inputs are the external-module sources that are already units.
- **Two guards:**
  - the module role must cover every non-JSON `program.source_files()` entry, which it does;
  - its options must equal the planner's. That logic is duplicated today, so either expose the planner's option builder and call it (a behaviour-neutral production accessor), or add a test asserting that `prepared_parse_options(path)` equals the planner's options across the corpus.

## Checks that pass
- **Acceptance role:** exact input (name, text, all eight options), deduplicated by ID. Each input is self-replayed against the actual census parse, and both the core digest and the full `ParseRecovery` must be equal.
- **Clean-tree pins:** the digest-code self-hash, and the source, probe, binary, lock and build-log hashes.
- **Feature-gated profiles:** on for the current and reverted builds, off for the merge base.
- **Recorded compiler and project routes:** unit contents via a hash pool, `content: None` preserved, settings, write order, root IDs, config unit, the raw project descriptor, module variant, and mount-file hashes.

## Required before the full census
1. Apply B1 and B2, and re-run the 285-case self-replay smoke under all three builds (current, reverted helper, merge base) to confirm identical digests where expected.
2. Fix B4 and B5.
3. The union selector implements the rule above. The TS observer and native consumer read `command_input` directly, with no fallback to looking up inputs by case ID.
