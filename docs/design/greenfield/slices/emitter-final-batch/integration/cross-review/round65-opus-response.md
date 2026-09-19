Removing ClassStaticBlock from the skip list is the right fix, and the remaining skips match upstream exactly. I found one more mismatch in the same function: parser fields the factory never sees (post-assigned), which your 722 fixture doesn't cover. The single-load plus replay census is at least as strong as two loader passes for this delta, if you add two checks. This is a code read only; nothing was built.

## 1. `source_contains_top_level_await` (system.rs:225-256)
**Upstream, after the fix:**
- The only source of `ContainsAwait` (1<<21) is `createAwaitExpression` (22753). `for await`, `await using` and the `await` keyword token don't set it.
- `createSystemModuleBody` reads the transformed SourceFile's flags (112209).
- Only these exclusions contain the bit: Arrow, Function (declaration and expression), Constructor, MethodOrAccessor (method, get, set) and type nodes.
- These do not: Property, Class, ObjectLiteral, Module, and the default exclusion. ClassStaticBlock uses the default and passes its body up (`propagateChildFlags(body)`, 21961).

So the skip set {FunctionDeclaration, FunctionExpression, Arrow, Method, Constructor, Get, Set} is exactly right once ClassStaticBlock is removed. Skipping a method as a whole also covers its computed name and decorators, which MethodOrAccessorExcludes strips, and that matches your "computed method → non-async" probe. Type nodes can't contain an AwaitExpression, so not skipping them is harmless.

**New mismatch: parser fields assigned after the factory call.** The walker uses `for_each_child`, so it also visits fields that never entered the parse-tree node's flags. Upstream: ClassStaticBlock `modifiers` (34055), PropertyAssignment/ShorthandPropertyAssignment `modifiers` (32990), and MissingDeclaration `modifiers`. TS then reuses those nodes unchanged when nothing inside them was rewritten, so their flags stay as the parser computed them. Direct controls, ESNext System:
- `export {}; class C { @(await x) static {} }`
- `export {}; const o = { @(await x) a: 1 };`
- `export {}; const o = { @(await x) a };`

The model predicts a non-async `execute` for all three, while Rust currently finds the AwaitExpression. **Fix, if TS confirms it:** don't walk `modifiers` of ClassStaticBlockDeclaration, PropertyAssignment and ShorthandPropertyAssignment. A MissingDeclaration never reaches System, because the TS transform removes it.

Two more controls whose outcome follows from AwaitExpression being the only source:
- `export {}; await using r = source;` at ESNext: non-async `execute`.
- `export {}; for await (const v of source) {}` at ESNext: non-async `execute`.

The constructor now takes `&SystemModuleTransformer` instead of copied arguments. That has no semantic effect.

## 2. Single-load plus replay census
**It is at least as strong as two loader passes for this delta, and it isolates the delta better:** one loader pass, identical parse inputs, and the only difference being the helper. It only holds if you add these checks:

1. **Replay the export against itself.** In the current pass, re-parse every exported input with the current parser and require the digest to equal the one computed from the SourceFile the census actually evaluated. This proves the export is complete: file name, text, every ParseOptions field, JSDoc mode, and that there was no incremental cursor. `parse_prepared_source` (h2_2c_acceptance.rs:610) passes `cursor: None` and the default node-ID bases, so the digest must not depend on IDs. If the census gets its SourceFiles from anywhere other than `parse_prepared_source`, this check will expose it.
2. **Prove loading can't differ.** A single load can't show whether the old parser would have loaded different files. Add each unit's module-request set to the digest: import/export specifiers, external `import =` references, dynamic `import()`, triple-slash references, and JS `require` calls. Require that set to be identical between A and B for every unit. The reparse gate already skips declaration files.
3. **Restore exactly the helper.** Build A from the current tree with the helper functions taken from 81d5aa52e: the walker, the gate, and any helper they call, such as `modifiers_contain`. Fail if `git diff 81d5aa52e HEAD -- crates/syntax/src/parser.rs` touches anything outside those functions.
   - This measures exactly the interface fix plus the projection. Keeping the heritage admission from 7e current on both sides is correct, because it is a predicate, not parser output.
4. **Separately, prove the earlier parser changes don't affect output.** The diff from the main merge base (3b1f5fe87) to 81d5aa52e changes `parser.rs` by 147 lines of recovery metadata and event recording. The helper replay says nothing about those. The probe only needs parse output, not predicates, so run the same export once more through a probe linked against the main merge base's syntax crate. Compare diagnostics, root statements with AwaitContext, the tree-shape hash, and the module-request set, and put any differing row into the qualification union U.
   - This costs seconds, puts no old parser code into production, and replaces an assumption with a measurement.

**The export API already has what it needs.** Every field of `ParseOptions` (parser.rs:82-106) is a plain value, and `parse_source_file_from_snapshot` takes a `TextSnapshot`, so no production change is needed.
- In the probe, construct `ParseOptions` with every field listed explicitly, not with `..Default`, so a field added later fails to compile rather than being dropped silently.
- Export the snapshot text as UTF-8 bytes (base64) plus its SHA-256. `TextSnapshot::new` takes a `String`, so that round-trips losslessly.

**Suggested artifact** (one file, written by the current census):
```
{schema, head, syntax_tree_hash (git tree of crates/syntax), digest_code_sha256,
 input_manifest: {artifact hashes, plan manifest sha256, vendor tree hash}, load_failures:[...],
 inputs: [{input_id, file_name, text_utf8_base64, text_sha256, utf16_len,
           options:{script_target, language_variant, javascript_file, js_doc_parsing_mode,
                    force_external_module, detect_external_module_from_jsx}}],
 rows:   [{case_id, universe, loader, units:[{path, input_id, emit_eligible}]}],
 digests:{input_id: {diagnostics:[code,start,length,message_sha256], statements:[kind,pos,end,await_context],
          ast_shape_sha256, module_requests_sha256, recovery_counts, profiles:[5 bools], final_supported}}}
```
- Define `input_id` as the SHA-256 of the canonical (file name, text SHA-256, options).
- The replay API is `replay(inputs) -> {input_id: digest}`, run with the same digest source file in both builds; record its hash.
- Compare fail-closed: same set of input IDs; every A digest compared to the B digest; any difference puts the row into U.
- The final set to qualify is U united with the five profiles' `newly_admitted` rows.

**VFS export for the selected rows.**
- Compiler rows: use the shape from round 64, taken from `CompilerExecutionPlan`:
  - `current_directory`, `use_case_sensitive_file_names`;
  - `effective_settings` in order, the option floor, and the loader name;
  - the root selection (explicit or config, with the config unit);
  - `vfs_write_order` files with bytes and hashes (`content: None` exported as absent);
  - global then document symlinks.
- Project rows: implement an explicit snapshot from `ProjectFixtureInput`:
  - `descriptor_raw`, `scenario`, `project_root`, `current_directory`, `root_selection`;
  - every file under `mount.workspace_path` with its virtual path, bytes, hash and `case_sensitive`.

  Until that exists, fail with a named "blocked" list. None of b652's 571 refused rows use a project loader, so the blocked list may well stay empty.
