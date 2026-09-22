No blockers. Both follow-ups are sound, and I see no borrow, API, catalog-quoting, hashing or `Cargo.lock` pin problem. I read the current working diff and checked tree drift; nothing was compiled or run.

**1. Selector `current.parser_head == snapshot.head` and `current.syntax_tree_hash == snapshot.syntax_tree_hash`:** correct and binding. One operational consequence: the rerun current replay must use `--parser-tree` = the census tree, not the replay tree.
- The census tree is at `67df86615`.
- `git diff 67df86615 HEAD` in the replay tree shows no differences in `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` or `crates/{syntax,types,diagnostics}`. So the candidate-mode `source_identity(tree) == source_identity(source_tree)` still holds.
- The wrapper only reads that tree and builds in its own build dir, so the frozen census's files and output are untouched.

**2. Native successor catalog check:**
- **Catalog:** `git ls-files -z` over the same six pathspecs the wrapper's `source_identity` uses gives the same NUL-separated, unquoted catalog. `git()` trims only whitespace, and NUL isn't whitespace.
- **Exact key-set equality before reading anything:** only catalog paths are read; nothing comes from the selection.
- **Hashing:** SHA-256 of the working bytes, which equal the committed bytes because `clean()` guarantees it.
- **API:**
  - `pins[path]` works because serde's `Map` implements `Index<&str>`, and it can't panic because key equality was checked first;
  - `Value == String` comparison is implemented;
  - the `BTreeSet<&str>` comparison infers its `collect` target;
  - with no successor, `selection["successor"]["source_files_sha256"]` evaluates to `Null` without panicking.
- **Lock/manifest pin:** consistent across the chain. The wrapper already required the successor's `Cargo.lock` and root `Cargo.toml` to match the replay source tree. Native now requires the compiled workspace to match the successor, so dependency or manifest drift fails explicitly instead of hiding behind the syntax pin.

The comparator binding and the new `Cargo.lock` negative test close the loop. This review is not qualification; that still needs the Recovery-next `c1e699ab5` pipeline and the native and TS whole-command runs.
