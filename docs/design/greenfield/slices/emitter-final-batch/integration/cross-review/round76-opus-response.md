I found two concrete deviations in the emitter code, both in `legacy_decorators.rs`. The census field guard is correct at `06f5e35ea`. Option B can be done by moving the serializer out of xtask without duplicating it, and rows that fell back to a NoEmit loader need to be listed separately as "emit not qualified". This was a read-only review: no builds, no edits.

## 1. Emitter implementation (rounds 72–75)

**Deviations:**
- **`legacy_decorators.rs`, `equate_serialized_type_nodes`, Identifier arm:** a plain identifier on the left can currently equal a generated identifier on the right with the same text. Upstream says they are not equal: `isIdentifier(left) ? isIdentifier(right) && left.escapedText === right.escapedText`, and a generated name's `escapedText` never matches source text. You already require "generated on the left implies generated on the right". Also require "not generated on the left implies not generated on the right" (or compare generated identity). Otherwise the union can wrongly collapse into one constituent.
- **`legacy_decorators.rs`, qualified-name `right` (the B2 fix):** `clone_node(right)` followed by `set_text_range` prints the identifier's decoded text. Upstream reuses the parsed `node.right`, which still has its parent, so the printer emits its source spelling (for example `\u0061`). Use the existing `clone_node_with_source_spelling` so escaped member names print byte-for-byte. The `set_original_and_range` on the whole access in `entity_name_expression` is correct as it is.

**Matches the advice, with no over-broad impact:**
- `builtins.rs`: the CommonJS import-first `direct_export_storage` requires both the direct-exported set and a matching export name.
- `builtins.rs`: `promote_class_declaration_to_iife` adds `NO_TRAILING_SOURCE_MAP` only when `has_static_initialized_properties` is set.
- `es_next.rs`: `set_hoisted_initializer_range` sets only the original node, comment range and source-map range. It sets no text range, which is correct.
- `system.rs`: `source_contains_top_level_await` now walks static-block bodies.
- `system.rs`: the import-equals setter target uses `create_local_name_reference`.
- `printer.rs`: the `ClassStaticBlockDeclaration` arm writes `static`, a space, then the body, with no modifier emission.

## 2. Census field guard at `06f5e35ea`

- **Struct parsing:** every `NodeData` variant's payload is `<Variant>Data`, defined in `nodes.rs` as `pub struct … {`. The only structs where the first `}` isn't the closing line are the seven empty `{}` structs (DebuggerStatement, EmptyStatement, JSDocAllType, JSDocUnknownType, NotEmittedStatement, OmittedExpression, SyntaxList). The first-`}` split now handles them correctly.
- **Marker lookup:** there are no or-patterns and no duplicate `NodeData::X(` markers in `observable_fields.rs` or the `extra_fields` slice. The duplicate markers elsewhere in the file are outside that slice. Each variant's lookup is therefore unambiguous, and `has_field` checks word boundaries.
- **SourceFile:** `snapshot` is covered by `text_sha256`. `arena` and `root` are covered by the graph. `parse_recovery` appears in the exact-equality list but is never digested. That is correct for an AST-equality proof, but it is an unexplained exclusion. Make it explicit (an `EXCLUDED = ["parse_recovery"]` constant with its reason) so a reader can't take it as covered.

## 3. Option B: where the shared code should live

Move three things from `utf16_literal_recovery_census.rs` into a new module, `harness::upstream_suites::execution::observable_input` (next to `project.rs` and `js_paths.rs`):
- the `plan_input` and `artifact_input` bodies;
- `prepared_summary`, `limits_input` and `symlink_input`;
- `limits()` itself, so the loader limits and their serialized form can't drift apart.

**Remove the document-store side effect:** the serializer currently writes into `self.documents` as it runs. Instead, take a document sink trait (or `&mut dyn FnMut(&[u8]) -> String`) returning the SHA-256 id. The census then passes its base64 store, and the native consumer passes a hash-only sink.

**Consumer side:** rebuild the input from the fixture path plus blob SHA-1 and variant key, or from the descriptor path plus module variant for project rows. Load it with the recorded loader, call the same shared function, and assert the JSON is byte-identical, including `prepared` and `limits`. The census keeps only its `CensusParseCapture` wrapper around that call, so there is no second copy of `plan_input`.

**Caveat:** `compiler_options_debug_sha256` and `program_options_debug_sha256` hash `{:?}` output. That is only stable between builds of the same source, so treat a mismatch there as "rebuild the census", not as a divergence between the rows.

## 4. NoEmit-fallback rows

Around line 588, the loader match silently falls back from `load_*_emit` to `load_*_no_emit` and discards `emit_error`. The concerns:
- **Different programs:** `may_be_emitted` and the root set can differ between the two loaders, so a NoEmit program's `prepared` summary is not the emit program's. The consumer must reload with the recorded `loader`, never the emit loader.
- **Required fields:** each selected fallback row should carry `loader: "load_*_no_emit"`, the discarded `emit_error` text, and an explicit disposition such as "parse-admission only; emit not qualified".
- **Reporting:** keep these rows out of emit-qualified counts, and report them as a separate count so their parse-level result can't turn into an emit claim.
- **Double failures:** the `emit|no_emit` error rows should likewise carry their own disposition rather than just an error string.