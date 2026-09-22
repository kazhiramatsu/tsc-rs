No blockers in any of the three diffs; I found no semantic, borrow or API problem in the class helper or the nested diff, and nothing the H1 prep changes outside the generator. The things to fix are one medium item in the H1 prep (the entry guard can silently succeed) and two hazards in the pause plan, both caused by the census binary living in a shared Cargo target directory. Read-only: I read the three diffs, the current checker/binder/syntax/schema/oracle code, and the running census process metadata.

## Blockers

None.

## Class helper (`r93-class-helper-candidate.diff`)

**Matches `getFirstTransformableStaticClassElement` (:84921-84948) on the standard-decorator path:**
- **Class decorated:** `transform_decorated` = `!experimental_decorators && target < ESNext && class has a decorator`. This equals `classOrConstructorParameterIsDecorated(false, …)`, because `nodeCanBeDecorated(false)` is true for both class kinds and standard constructor-parameter decorators never qualify.
- **Static elements:** `transform_private` = `target < ESNext`, the exact reduction. Member order and the branch priority (decorated member → the class's decorator ?? the class; else static block / static private / static initialized property when `!emit_standard_class_fields`) match upstream.
- **`is_private_identifier_class_element`** (functions.rs:3149) is exactly `isPrivateIdentifierClassElementDeclaration`: property, method or accessor, with a private name.
- **Accessor grouping:**
  - a dynamic name forms its own group;
  - otherwise it keeps the first two accessors in member order with equal `is_static_element` and equal `property_name_for_property_name_node`, which handles signed numerics and literal computed names like upstream;
  - it picks the first one that has decorators and requires it to be this member, then applies `node_can_be_decorated(false, member, class, …)`.
  - That's `getAllAccessorDeclarations` plus `classElementOrClassElementParameterIsDecorated(false)`, with the always-false parameter arms dropped.
- **Named evaluation:** identifier or string `__proto__` compared through `EscapedName::escape` identity; a computed `__proto__` stays eligible, as upstream's `isProtoSetter` ignores computed names.
- **Location:** the class's decorator when standard decorators apply and it's decorated, otherwise the first member. PropKey is checked at the same location.
- **Compile surface:** looks fine.
  - `ScriptTarget`, `EscapedName` and `NodeFlags` are already imported;
  - `has_dynamic_name` exists (binder node_util.rs:1000);
  - `EscapedName::escape(JsStr)` exists;
  - every new method takes `&self`, so there's no borrow conflict.

**Scope:** proven only for the 320 whole-command fixtures, not beyond.

## Nested parens (`r93-nested-candidate.diff`)

- **Old paths:** unchanged. `can_bridge_close_paren` (statement gaps, no missing node, `CloseParen`, report length equal to action length, actual source text `")"`) only enables map lookup and storage. When it's false, the old `preceding.end` test decides exactly as before.
- **The r92 mutations** (reversed actions, forged token, shifted start, mixed `)]`) all fall back to the old test and are refused.
- **The closer-chain rule** is as approved in r92.
- **Compile concerns:** none. `parents[*id]` returns `Option<NodeId>`; the `Filter` iterator's `.clone()` works because its closure only captures references.

## H1 prep (`r93-h1-prep-candidate.diff`)

**Correct:**
- **Schema:** both edits (`summary.absence_proofs` non-negative, `evidence.absence_proofs` `minItems: 0`).
- **Elision fields:** `disposition` and `closure` are free strings in the schema, so the new wording for the retained constructor row is valid.
- **Removed IDs:** none of the 20 removed anchor or elision IDs is referenced by `h2-transition.mjs` (its converse specs use other anchor IDs) or by `.github/ci`.
- **Aggregation:** `collectAnchorFailures` collects read, missing, non-unique and duplicate-ID failures, and the absence failures join the same vector.
- **`--check-anchors`:** it still runs `validateArtifact` and every reference check, and skips only the byte comparison.

**Medium: the main guard can silently succeed.** `path.resolve(process.argv[1]) === GENERATOR_PATH` fails when the script is reached through a symlinked path. `import.meta.url` is realpath-resolved while `argv[1]` isn't, for example via `/tmp` → `/private/tmp` or a symlinked worktree. In that case `--check` does nothing and exits 0.
- **Fix:** compare `fs.realpathSync(process.argv[1])` with `fs.realpathSync(GENERATOR_PATH)`.
- **Fail closed:** if a mode argument is present but the module isn't treated as main, throw.

**Low:**
- **Heavy import:** importing the module for the unit test runs the whole generator (tree hashing, anchors), and it throws if references are broken. The negative test only exercises the pure function, but the test can't even import when the repository's own anchors are broken. Accept it and document it.
- **`ProgramSession` needle:** it includes doc comments and two fields, so any additive field breaks it. `pub struct ProgramSession {\n    prepared: PreparedProgram,` is the stable head.

## The pause plan (`SIGSTOP` the census)

- **The xtask process is already stopped:** `ps` shows `56011 … TN`. The wrapper `run-local.py` (56007) is in `S`, waiting with no timeout. Its receipt time `seconds` and the census's per-universe `elapsed` (utf16_literal_recovery_census.rs:459/554/628/711) will include the pause, so record the pause interval. No lock descriptors are held (only the heredoc stdin and the log), so other Cargo jobs won't block. The workspace-cleanliness check runs only at start (:969-990).
- **Hazard 1, shared target dir:** the running binary is `/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/xtask`, because the process has `CARGO_TARGET_DIR=/Users/hiramatsu/dev/tsc-rs-emitter-final/target` while its working directory is the census tree. Any Recovery-next or f4 capture build using that `CARGO_TARGET_DIR` will replace that path.
  - The running process keeps its old inode (120896494 now, mtime 1789787279), so it's unaffected.
  - But a receipt that later hashes the *path* would record the wrong binary.
  - **Fix:** record the SHA-256 plus inode now, run the interleaved jobs with a different `CARGO_TARGET_DIR`, and check with `lsof -p 56011` that the `txt` inode is unchanged before `SIGCONT`.
- **Hazard 2, in-place overwrite:** on macOS, truncating and rewriting a running executable's file in place kills it with a code-signature error when a page is next loaded. Cargo uplifts artifacts by replacing the file, which is safe. A manual `cp` over the path would not be. Keep the jobs in a separate target dir and the question disappears.
- **Output validity:** nothing in the census logic depends on wall-clock time. Parse and profile outputs are unaffected; only timing fields change, and those aren't performance claims.
