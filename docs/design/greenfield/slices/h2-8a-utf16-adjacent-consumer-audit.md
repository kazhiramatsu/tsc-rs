# UTF-16 adjacent repair: consumer audit of name/value conversions

Date: 2026-09-14. Worktree `tsc-rs-declaration-comment-design`, HEAD `b652451f0` plus the uncommitted A/B/C migration after the 40-error test-adapter repair and `cargo fmt --all`.
Evidence directory: `target/declaration-comment-ranges-runs/utf16-consumer-audit-20260914-004515/` (inventory, per-group rows and results, merged JSON, receipt).

## 1. Scope and method

- Inventory: every production line (`crates/*/src/**/*.rs`, `#[cfg(test)]` modules excluded) matching one of `.to_utf8(`, `.to_string_lossy(`, `from_utf8_lossy(`, `from_utf16_lossy(`, `.as_str()` (split into `expect`/`unwrap`/`?`/`map`/`unwrap_or`/`is_*`/other), `escaped_name`, `.text.as_str(`. 1,262 rows found, 1,253 in production code (9 inside `cfg(test)` modules). The pre-migration search inventory (245 `escaped_name`/`data.text` rows, all pending) is superseded by this wider, classified table.
- Classification: seven read-only reviews grouped by crate, each row classified exactly once at function level with the receiver kind, a category and a justification; every listed row appears in exactly one result (validated by file:line key). Suspects were re-verified by the integrator against the current source and the vendored `_tsc.js` before any change.
- Type facts used: `JsString`/`JsStr::as_str()` is `None` exactly on an unpaired surrogate; `to_string_lossy()` is the explicit U+FFFD output boundary; `EscapedName::as_str()` and `TemplateText::to_utf8()` are the same scalar-only faces; `cmp_utf16()` orders in code units; identifiers, keywords, numeric/regex text, JSX text and raw source are scalar by grammar, and lone surrogates enter only through escape sequences in string/template literals, JSON config strings and everything derived from them.
- Line numbers in the evidence rows are the inventory snapshot taken before `cargo fmt --all` reflowed the migrated files; each row records its source text and function, and the reviews located every row by text.

Categories:

| category | meaning |
| --- | --- |
| identity | the value is a name/key/path identity and the function keeps it lossless (JsStr/EscapedName comparisons, explicit None arms that reproduce tsc's verdict, lossless fallbacks) |
| grammar-scalar | a JS value whose producer guarantees a Unicode scalar (identifier/keyword/numeric text, generated or fixed ASCII names, directive text) |
| display | final human-facing rendering (Display impls, CLI stdout) where U+FFFD is the accepted boundary and nothing feeds back into identity |
| native-io | conversion to OS paths/bytes at the real filesystem or process boundary with the JS value retained beside it |
| scalar-observer | evidence/acceptance tooling that fails loudly (`expect`/`Err`) on a non-scalar value, so a difference cannot be masked |
| not-js-value | the receiver is an ordinary Rust string, serde JSON evidence, native listing or file bytes |
| suspect | a lone surrogate would be replaced, dropped or would panic on a reachable path, or a lossy string feeds a comparison/emit |

## 2. Summary

| group | rows | identity | grammar-scalar | display | native-io | scalar-observer | not-js-value | suspect |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| syntax / binder / types / diagnostics | 63 | 19 | 26 | 3 | 1 | 7 | 6 | 1 |
| checker (node builder, declaration emit, jsx, literals, modules, structural, annotate) | 175 | 144 | 27 | 0 | 0 | 0 | 4 | 0 |
| checker (remaining files) | 158 | 132 | 18 | 3 | 1 | 0 | 4 | 0 |
| emitter | 36 | 10 | 19 | 3 | 2 | 1 | 1 | 0 |
| program / host / compiler | 109 | 39 | 3 | 52 | 11 | 0 | 4 | 0 |
| harness / conformance / fuzz (evidence tooling on the acceptance path) | 151 | 5 | 54 | 0 | 2 | 3 | 87 | 0 |
| xtask (acceptance runners and evidence producers) | 561 | 1 | 0 | 3 | 0 | 94 | 458 | 5 |
| **total** | **1253** | **350** | **147** | **64** | **17** | **105** | **564** | **6** |

Receiver kinds over all rows: rust-str 347, js-value 310, escaped-name 249, serde-json 220, bytes 51, other 43, os-path 26, template-text 7.

## 3. Suspects and dispositions

| # | site | finding | disposition in this train |
| --- | --- | --- | --- |
| 1 | `crates/syntax/src/parser.rs` `parse_new_expression_stub` → `current_token_text` | After `new` `.` the parser called `parse_identifier()` without checking the token, so a string/template token whose value carries an unpaired surrogate (`new."\uD800"`) reached `current_token_text`'s scalar-only `expect` and panicked; tsc `parseNewExpressionOrNewDotTarget` uses `parseIdentifierName()` (keywords accepted, other tokens give a missing identifier + TS1003). | **Fixed**: `parse_identifier_name(None)`; fixture `crates/syntax/tests/fixtures/utf16-new-meta-property-name.json` minted from tsc by `scripts/observe-utf16-new-meta-property-name.mjs` (2 repetitions) and checked by `crates/syntax/tests/new_meta_property_name.rs`. The same change removes the pre-existing divergence where `new."x"` fabricated an identifier from the literal and `new.if` reported TS1003. |
| 2 | `crates/xtask/src/h1_emit_acceptance.rs` `assert_outcome` (emittedFiles) | `to_string_lossy()` kept from the PathBuf era silently became surrogate-lossy after `emitted_files()` moved to `JsString`; the sibling sites fail closed with `as_str().expect(..)`. | **Fixed**: fails closed. |
| 3 | `crates/xtask/src/h1_emit_acceptance.rs` `assert_writes` (source provenance) | Same fail-open conversion on `source_files()`. | **Fixed**: fails closed. |
| 4 | `crates/xtask/src/h2_2c_acceptance.rs` `source_maps_value` | `input_source_files()` rendered lossy into the source-map facet compared by `compare_vector_value`, while `source_maps_match`/`emitted_files_value` guard the same data with `expect`. | **Fixed**: fails closed. |
| 5 | `crates/xtask/src/symbol_audit.rs` `audit_source_file` (members/exports keys) | Symbol-diff line compared textually with `symbol-dump.mjs`; a Rust-side lone surrogate became U+FFFD instead of failing closed (the oracle transport already rejects a TS-side lone surrogate). | **Fixed**: fails closed (`as_str().expect`). |
| 6 | `crates/xtask/src/symbol_audit.rs` `audit_source_file` (escapedName column) | Same as 5. | **Fixed**: fails closed. |

Practical exposure of 2–6 was bounded: the expected side is serde JSON, which cannot carry a lone surrogate, so masking needed an oracle-side U+FFFD; they were nevertheless the only PathBuf→JsString conversions in comparators not converted to fail-closed guards.

## 4. Recorded observations that are not suspects

- `crates/harness/src/upstream_suites.rs` `decode_utf16` (called from `decode_source` on the acceptance program-construction path) maps a lone surrogate in a UTF-16-BOM corpus source to U+FFFD, whereas tsc's `Buffer.toString("utf16le")` keeps it. Unreachable on the pinned corpus: the seven UTF-16-BOM fixtures decode without error. Left unchanged; optional hardening is to fail (or produce a WTF-8 `JsString`) like the production loader's documented rejection.
- The emitter sink projection happens at `PrintedText` → `EmitArtifact.callback_text` (U+FFFD per unpaired unit, byte-identical to Node's UTF-8 `fs.writeSync`), so in-process callback observers see the projection rather than tsc's in-memory JS string; `text_utf16()`/`units()` stay lossless. Recorded, unchanged.
- `crates/program/src/loader.rs` `resolve_runtime_dependency_symlinks` tests a `/node_modules/` substring through `to_string_lossy()`; the ASCII test is invariant under replacement (`JsStr::contains` would express it losslessly). Unchanged.
- Windows-only boundary: native output/probe names with a lone surrogate would be spelled U+FFFD by tsc-rs where Node passes WTF-16 to the W APIs; the JS key is retained everywhere. Platform limitation, unchanged.
- Scalar-side fidelity observations found while reading (outside this repair's scope, not changed): `crates/checker/src/engine.rs` `report_incompatible_stack` decides the `.prop` join with an `is_alphanumeric` test rather than `isIdentifierText`; `crates/emitter/src/builtins/system.rs` `identifier_or_literal_text` has no Numeric/BigInt arm for a `{ 0: a }` binding element on the System flattening path; emitter JSX `decode_entities` accepts `&#+65;` and drops code points above 0x10FFFF where tsc rejects/throws; `crates/emitter/src/builtins.rs` activity canary lowercases the path where tsc's `fileExtensionIs` is case-sensitive (observer only).

## 5. Function-level table

One line per (file, function); `rows` counts inventory hits, `categories` lists the categories those hits received. The per-row justifications are in `audit-merged.json` in the evidence directory.

### syntax / binder / types / diagnostics

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/binder/src/assignment.rs` | `get_assignment_declaration_property_access_kind` | 1 | grammar-scalar 1 | Identifier escaped_text (Rust String, grammar-scalar) compared with ASCII constants; element-access names stay EscapedName. |
| `crates/binder/src/bind.rs` | `check_private_identifier` | 1 | grammar-scalar 1 | Private identifier text (scalar) compared with "#constructor". |
| `crates/binder/src/bind.rs` | `lookup_symbol_for_property_access` | 1 | grammar-scalar 1 | Identifier text (scalar) and EscapedName element-access keys both reach SymbolTable::get(impl Into<JsStr>) losslessly. |
| `crates/binder/src/bind.rs` | `lookup_symbol_for_property_access_in` | 1 | grammar-scalar 1 | Same lossless lookup shape with an explicit container. |
| `crates/binder/src/declare.rs` | `apply_declared_identity_relocation` | 1 | identity 1 | Relocates EscapedName serials in place, lossless. |
| `crates/binder/src/declare.rs` | `name` | 1 | grammar-scalar 1 | Private-name serial relocation: expect guarded by an ASCII-digit check; suffix re-attached with push_js. |
| `crates/binder/src/symbols.rs` | `None` | 1 | identity 1 | Symbol.escaped_name is declared as the branded EscapedName (WTF-8 JsString); storage is lossless, and the table keyed by it hashes/compares raw bytes (Borrow<[u8]>). |
| `crates/binder/src/symbols.rs` | `alloc` | 2 | identity 2 | Forwards the EscapedName to try_alloc. |
| `crates/binder/src/symbols.rs` | `new` | 2 | identity 2 | Symbol::new moves the EscapedName in unchanged. |
| `crates/binder/src/symbols.rs` | `try_alloc` | 2 | identity 2 | Stores the EscapedName via Symbol::new. |
| `crates/diagnostics/src/js_string.rs` | `as_str` | 1 | identity 1 | Lossless-optional accessor definition (None on lone surrogate). |
| `crates/diagnostics/src/js_string.rs` | `ends_with_js` | 1 | identity 1 | Scalar fast path with code-unit fallback. |
| `crates/diagnostics/src/js_string.rs` | `from` | 1 | not-js-value 1 | impl From<&String> for JsString calls String::as_str on a Rust UTF-8 String and copies its bytes; UTF-8 is a subset of WTF-8 so the conversion is lossless and no surrogate can be present. |
| `crates/diagnostics/src/js_string.rs` | `starts_with_js` | 1 | identity 1 | Scalar fast path with code-unit fallback. |
| `crates/diagnostics/src/js_string.rs` | `to_string_lossy` | 2 | display 2 | Definition of JsStr::to_string_lossy: as_str() borrows when scalar, otherwise decode_utf16 with REPLACEMENT_CHARACTER — this is the designated lossy sink; it performs no identity work itself.; Definition of the explicit UTF-8 output boundary (JsString::to_string_lossy -> JsStr::to_string_lossy); it is the documented U+FFFD sink itself, and does not feed anything on its own — acceptability is decid |
| `crates/diagnostics/src/js_string/tests.rs` | `assert_contract` | 3 | scalar-observer 3 | Unit-test contract assertions over arbitrary code units; failures are visible. |
| `crates/diagnostics/src/js_string/tests.rs` | `last_separator_preserves_path_components_and_matches_scalar_str` | 1 | scalar-observer 1 | unwrap on scalar fixtures only; non-scalar fixture uses to_utf16. |
| `crates/diagnostics/src/js_string/tests.rs` | `lossy_conversion_is_only_at_the_explicit_sink_and_debug_preserves_units` | 2 | scalar-observer 2 | Pins the sink's replacement and Debug's unit preservation. |
| `crates/diagnostics/src/js_string/tests.rs` | `prefix_views_and_buffer_reuse_preserve_canonical_values` | 1 | scalar-observer 1 | Scalar as_str assertion after buffer reuse. |
| `crates/diagnostics/src/lib.rs` | `format_message` | 1 | grammar-scalar 1 | Scalar-only formatting API; UTF-8 inputs make the expect unreachable. |
| `crates/diagnostics/src/render.rs` | `absolute_current_directory` | 1 | native-io 1 | Native cwd (OS path) decoded at the process boundary; JS directory appended losslessly. |
| `crates/diagnostics/src/render.rs` | `lookup_source_text` | 2 | identity 1, not-js-value 1 | JS file name compared against a scalar-keyed host map without lossy projection. |
| `crates/diagnostics/src/render.rs` | `push_utf16` | 1 | display 1 | Final code-frame render of scalar source-line units. |
| `crates/syntax/src/parser.rs` | `current_token_text` | 1 | suspect 1 | SUSPECT: expect on the token value is reachable with a lone-surrogate string/template token via parse_new_expression_stub's unguarded parse_identifier after `new.`. |
| `crates/syntax/src/parser.rs` | `entity_name_scalar_input` | 1 | identity 1 | Scalar fast path; non-scalar values projected only inside comments for the grammar predicate, rejected outside comments, never returned as names. |
| `crates/syntax/src/parser.rs` | `parse_error_for_missing_semicolon_after` | 1 | grammar-scalar 1 | Identifier text matched against keywords. |
| `crates/syntax/src/parser.rs` | `parse_jsx_text` | 1 | grammar-scalar 1 | JSX text token value is a raw UTF-8 source slice. |
| `crates/syntax/src/parser/jsdoc.rs` | `parse_child_parameter_or_property_tag` | 1 | grammar-scalar 1 | Tag-name identifier text matched against ASCII tag names. |
| `crates/syntax/src/parser/jsdoc.rs` | `parse_jsdoc_link` | 1 | grammar-scalar 1 | Identifier link kind matched against ASCII constants. |
| `crates/syntax/src/parser/jsdoc.rs` | `parse_jsdoc_link_prefix` | 1 | grammar-scalar 1 | Identifier/keyword token value matched against ASCII constants. |
| `crates/syntax/src/parser/jsdoc.rs` | `parse_tag` | 1 | grammar-scalar 1 | Tag-name identifier text matched against ASCII tag names. |
| `crates/syntax/src/parser/jsdoc.rs` | `slice_indent` | 1 | not-js-value 1 | Rust &str indentation whitespace; lossy branch unreachable. |
| `crates/syntax/src/parser/jsdoc.rs` | `token_value` | 1 | grammar-scalar 1 | All callers are identifier/keyword or raw comment-text tokens under JSDoc scanning. |
| `crates/syntax/src/regex.rs` | `code_unit_string` | 1 | grammar-scalar 1 | Diagnostic argument for one code unit; all callers pass ASCII syntax characters. |
| `crates/syntax/src/regex.rs` | `scan_character_class_escape` | 4 | grammar-scalar 4 | ASCII word-character property names/values looked up in constant tables. |
| `crates/syntax/src/regex.rs` | `scan_escape_sequence` | 1 | grammar-scalar 1 | Lossy call only on verified ASCII hex digits; decoded pair kept as u16 units. |
| `crates/syntax/src/regex.rs` | `scan_hex_digits` | 1 | grammar-scalar 1 | ASCII hex digits only. |
| `crates/syntax/src/regex.rs` | `scan_identifier_parts` | 3 | grammar-scalar 3 | Group-name identifier parts: escapes admitted only as identifier-part scalars, hex digits ASCII. |
| `crates/syntax/src/regex.rs` | `slice_string` | 1 | grammar-scalar 1 | Slices of scalar regex source at digit/word/identifier boundaries; replacement unreachable. |
| `crates/syntax/src/scanner.rs` | `finish_identifier_token` | 1 | grammar-scalar 1 | Identifier value built from chars only; keyword lookup exact. |
| `crates/syntax/src/scanner.rs` | `js_number_to_string` | 1 | not-js-value 1 | Local Rust String of ASCII numeric text. |
| `crates/syntax/src/scanner.rs` | `numeric_token_value` | 1 | grammar-scalar 1 | ASCII numeric token value on numeric-scanning paths only. |
| `crates/types/src/escaped_name.rs` | `as_str` | 1 | identity 1 | Lossless-optional accessor definition on the branded name. |
| `crates/types/src/lib.rs` | `None` | 2 | not-js-value 2 | `pub mod escaped_name;` module declaration; no value is converted.; `pub use escaped_name::EscapedName;` re-export; no value is converted. |
| `crates/types/src/tables.rs` | `create_unique_es_symbol_type` | 2 | identity 2 | EscapedName stored unchanged in the UniqueESSymbol payload. |
| `crates/types/src/tables.rs` | `get_string_literal_type` | 1 | identity 1 | Scalar cache fast path with explicit lossless TemplateText fallback. |
| `crates/types/src/tables.rs` | `get_string_literal_type_from_text` | 1 | identity 1 | Primary cache keyed by lossless TemplateText; to_utf8 only gates the scalar secondary cache. |
| `crates/types/src/ty.rs` | `bitor` | 1 | identity 1 | Field declaration `escaped_name: crate::EscapedName` of the TypeData::UniqueESSymbol variant (nearest_fn attribution is incidental); the payload is the branded WTF-8 name, no conversion. |

### checker (node builder, declaration emit, jsx, literals, modules, structural, annotate)

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/checker/src/annotate.rs` | `add_inherited_members` | 1 | identity 1 | Base member lookup/insert by cloned EscapedName. |
| `crates/checker/src/annotate.rs` | `combine_common_js_export_members` | 1 | identity 1 | Byte comparison against InternalSymbolName::EXPORT_EQUALS. |
| `crates/checker/src/annotate.rs` | `create_instantiated_symbol_table` | 1 | identity 1 | Instantiated table keyed by cloned EscapedName. |
| `crates/checker/src/annotate.rs` | `get_annotated_type_for_assignment_declaration` | 1 | identity 1 | Annotated-type property lookup by JsStr name; lossless. |
| `crates/checker/src/annotate.rs` | `get_declared_type_of_type_alias` | 1 | identity 1 | Byte comparison against `BuiltinIteratorReturn`. |
| `crates/checker/src/annotate.rs` | `get_es_symbol_like_type_for_node` | 3 | identity 3 | Builds the `__@name@id` unique-symbol key entirely in JsString/EscapedName; lossless. |
| `crates/checker/src/annotate.rs` | `get_intended_type_from_jsdoc_type_reference` | 2 | grammar-scalar 2 | Matches a JSDoc type-reference identifier String against ASCII intrinsic names; identifier grammar. |
| `crates/checker/src/annotate.rs` | `get_signature_from_declaration` | 1 | identity 1 | Parameter-property name passed to resolve_name as JsStr. |
| `crates/checker/src/annotate.rs` | `get_type_alias_instantiation` | 1 | grammar-scalar 1 | Type-alias (identifier) name passed to intrinsic_type_kind, whose None arm is the correct non-intrinsic result. |
| `crates/checker/src/annotate.rs` | `get_type_for_variable_like_declaration` | 1 | identity 1 | Parameter-name byte comparison against `this`. |
| `crates/checker/src/annotate.rs` | `get_type_of_property_in_base_class` | 1 | identity 1 | Base-class property lookup by JsStr name; lossless. |
| `crates/checker/src/annotate.rs` | `resolve_reverse_mapped_type_members` | 2 | identity 2 | Reverse-mapped property symbols and the members table reuse cloned EscapedNames; lossless. |
| `crates/checker/src/annotate.rs` | `symbol_list_to_table` | 1 | identity 1 | createSymbolTable port keyed by cloned EscapedName. |
| `crates/checker/src/annotate.rs` | `unresolved_symbol_path` | 1 | grammar-scalar 1 | Joins synthetic unresolved-symbol names that are created only from identifier text, so the scalar expect is grammar-guaranteed. |
| `crates/checker/src/declaration_emit.rs` | `declaration_replay_invoke` | 1 | not-js-value 1 | Matches replay resolver member ids (Rust Strings, ASCII) — not JS values. |
| `crates/checker/src/declaration_emit.rs` | `declaration_replay_prepare_root` | 2 | not-js-value 2 | Matches replay resolver member ids (Rust Strings, ASCII) — not JS values. |
| `crates/checker/src/declaration_emit.rs` | `declaration_replay_project_symbol` | 1 | identity 1 | Projects the symbol name as a lossless replay JSON string. |
| `crates/checker/src/declaration_emit.rs` | `emit_get_properties_of_container_function` | 1 | identity 1 | Publishes property names to the emitter as owned EscapedName values. |
| `crates/checker/src/declaration_emit.rs` | `is_any_symbol_accessible` | 1 | identity 1 | Type-parameter name passed to resolve_name as JsStr. |
| `crates/checker/src/declaration_emit.rs` | `track_symbol` | 1 | identity 1 | Replay decision record carries the JsString name through the lossless replay JSON value type; wire escaping handles unpaired surrogates. |
| `crates/checker/src/declaration_emit/replay_json.rs` | `json_part` | 1 | not-js-value 1 | JsonPart for Rust String delegates to the str impl; JsString/JsStr have separate lossless impls. |
| `crates/checker/src/jsx.rs` | `collect` | 1 | grammar-scalar 1 | Lower-cased pragma names sliced from comment text (scalar source text) matched against ASCII pragma names. |
| `crates/checker/src/jsx.rs` | `create_jsx_attributes_type_from_attributes_property` | 6 | grammar-scalar 1, identity 5 | JSX attribute names are cloned EscapedNames throughout; the children-name check is a lossless byte comparison with a possibly string-literal ElementChildrenAttribute key. |
| `crates/checker/src/jsx.rs` | `first_entity_escaped_identifier` | 1 | grammar-scalar 1 | Escapes the first identifier component (Vec<String>) of a parsed entity name; scalar by identifier grammar. |
| `crates/checker/src/jsx.rs` | `get_jsx_fragment_type` | 1 | identity 1 | Byte comparison against `Fragment`. |
| `crates/checker/src/jsx.rs` | `get_name_from_jsx_element_attributes_container` | 1 | identity 1 | Returns the cloned EscapedName of the ElementAttributesProperty member; lossless. |
| `crates/checker/src/literals.rs` | `check_object_literal_members` | 4 | identity 4 | Object-literal member names are cloned EscapedNames for symbol creation, contextual lookup and table keys; lossless. |
| `crates/checker/src/literals.rs` | `check_spread_prop_overrides` | 1 | identity 1 | SymbolTable lookup by cloned EscapedName. |
| `crates/checker/src/literals.rs` | `get_anonymous_partial_type` | 1 | identity 1 | Partial property symbols created with cloned EscapedNames. |
| `crates/checker/src/literals.rs` | `get_spread_symbol` | 1 | identity 1 | Spread property symbol created with the cloned EscapedName. |
| `crates/checker/src/literals.rs` | `get_spread_type` | 2 | identity 2 | Spread member bookkeeping in EscapedName-keyed collections; lossless. |
| `crates/checker/src/literals.rs` | `is_symbol_with_numeric_name` | 1 | identity 1 | Numeric-name test via is_numeric_literal_name, which is false for non-scalar names (correct). |
| `crates/checker/src/literals.rs` | `is_symbol_with_symbol_name` | 1 | identity 1 | Byte-prefix test for `__@`. |
| `crates/checker/src/literals.rs` | `literal_text_of` | 1 | grammar-scalar 1 | NumericLiteral String text (scalar) and StringLiteral JsString text (as_js) both yield a lossless JsStr. |
| `crates/checker/src/modules.rs` | `check_alias_symbol` | 2 | identity 2 | Unescaped names flow into diagnostic text via concat_js/JsStr arguments; lossless. |
| `crates/checker/src/modules.rs` | `checked_js_automatic_export_related_info` | 3 | identity 3 | Export name escaped from StringLiteral JsString/Identifier text, used as a lossless exports key and diagnostic argument. |
| `crates/checker/src/modules.rs` | `clone_type_as_module_type` | 2 | identity 2 | Module-type clone reuses the cloned EscapedName. |
| `crates/checker/src/modules.rs` | `combine_value_and_type_symbols` | 2 | identity 2 | Combined symbol reuses the value symbol's cloned EscapedName. |
| `crates/checker/src/modules.rs` | `get_symbol_flags_full` | 1 | identity 1 | Type-only export-star target lookup by EscapedName. |
| `crates/checker/src/modules.rs` | `get_type_only_alias_declaration_ex` | 1 | identity 1 | Exports lookup by the (fallback) cloned EscapedName. |
| `crates/checker/src/modules.rs` | `is_applicable_versioned_types_key` | 1 | identity 1 | Non-scalar `types@` range keys evaluate false, matching tsc's VersionRange.tryParse failure. |
| `crates/checker/src/modules.rs` | `is_node_core_module` | 1 | identity 1 | Explicit None branch: a non-scalar specifier is not a Node core module, as in tsc. |
| `crates/checker/src/modules.rs` | `is_symbol_of_declaration_with_colliding_name` | 1 | identity 1 | Variable name passed to resolve_name as JsStr. |
| `crates/checker/src/modules.rs` | `mark_symbol_of_alias_declaration_if_type_only` | 1 | identity 1 | Export-star name compared and stored as JsStr/EscapedName without conversion. |
| `crates/checker/src/modules.rs` | `parse_resolution_mode_override` | 3 | identity 3 | `as_str()` matches against ASCII attribute names/values; the None arm is the same InvalidName/InvalidValue verdict tsc gives for any other text. |
| `crates/checker/src/modules.rs` | `report_non_default_export` | 1 | identity 1 | exports.contains_key by cloned EscapedName. |
| `crates/checker/src/modules.rs` | `visit_module_exports` | 1 | grammar-scalar 1 | Diagnostic argument is the raw source text of the specifier (scalar Rust String), the id is a lossless JsStr. |
| `crates/checker/src/node_builder/chains.rs` | `alias_for_symbol_in_module` | 2 | identity 2 | Module exports lookup by cloned EscapedName. |
| `crates/checker/src/node_builder/chains.rs` | `build` | 1 | identity 1 | Qualified-name identifiers created from unescaped JsStr via the factory's JsStr identifier constructor; lossless. |
| `crates/checker/src/node_builder/chains.rs` | `chains_get_property_name_node_for_symbol` | 1 | identity 1 | Unescaped JsStr name fed to the lossless property-name builder. |
| `crates/checker/src/node_builder/chains.rs` | `chains_symbol_to_type_node` | 1 | identity 1 | Unescaped JsStr diagnostic argument for the unsafe-import error. |
| `crates/checker/src/node_builder/chains.rs` | `create_access_from_symbol_chain` | 1 | identity 1 | Parent members lookup by EscapedName. |
| `crates/checker/src/node_builder/chains.rs` | `create_expression_from_symbol_chain` | 1 | identity 1 | Numeric-spelling check compares the JsString name byte-wise with a Rust String; non-matching names become lossless identifiers/string literals as in tsc. |
| `crates/checker/src/node_builder/chains.rs` | `create_property_name_for_identifier_or_literal` | 1 | identity 1 | Scalar fast path for identifier/numeric names with a lossless string-literal fallback (documented). |
| `crates/checker/src/node_builder/chains.rs` | `describe_symbol` | 1 | identity 1 | Tracker symbol description copies the EscapedName as a JsString; lossless. |
| `crates/checker/src/node_builder/chains.rs` | `get_module_specifier_override` | 1 | identity 1 | Unescaped JsStr diagnostic argument. |
| `crates/checker/src/node_builder/chains.rs` | `get_property_name_node_for_symbol_from_name_type` | 2 | identity 2 | nameType value converted losslessly (to_js_string); as_str() gates only identifier/numeric forms, non-scalar names become string literals as in tsc. |
| `crates/checker/src/node_builder/chains.rs` | `lookup_symbol_chain_worker` | 3 | identity 3 | Synthetic-scope and globals lookups by WTF-8 bytes/JsStr; lossless. |
| `crates/checker/src/node_builder/chains.rs` | `symbol_is_shadowed_in_synthetic_scope` | 2 | identity 2 | Byte-keyed synthetic-scope lookup; lossless. |
| `crates/checker/src/node_builder/chains.rs` | `type_parameter_shadows_other_type_parameter_in_scope` | 4 | identity 4 | Name carried as JsStr through the byte-keyed lookup and resolve_name. |
| `crates/checker/src/node_builder/mod.rs` | `describe_symbol` | 1 | identity 1 | Tracker symbol description copies the EscapedName as a JsString; lossless. |
| `crates/checker/src/node_builder/serialize.rs` | `describe_symbol` | 1 | identity 1 | Tracker symbol description copies the EscapedName as a JsString; lossless. |
| `crates/checker/src/node_builder/serialize.rs` | `node_modules_resolution_candidates` | 1 | identity 1 | `as_str()` detects only the `/`/`.` sentinels; non-scalar directories are split losslessly and still terminate. |
| `crates/checker/src/node_builder/serialize.rs` | `serialize_parameter_name_from_parse` | 4 | grammar-scalar 4 | Fallback identifiers use the parameter symbol's display name; parameter symbols are identifier/`__N`/JSDoc-identifier named, so the scalar expects are grammar-guaranteed. |
| `crates/checker/src/node_builder/signatures.rs` | `collect_binding_element_symbols` | 1 | identity 1 | Binding element names collected as cloned EscapedNames. |
| `crates/checker/src/node_builder/signatures.rs` | `expanded_tuple_element_label` | 2 | grammar-scalar 2 | `{rest}_{index}` label from the rest parameter symbol's name; parameter names are scalar so the expect is grammar-guaranteed. |
| `crates/checker/src/node_builder/signatures.rs` | `get_expanded_parameters` | 1 | grammar-scalar 1 | Expanded parameter names come from Vec<String> tuple labels or get_parameter_name_at_position; scalar. |
| `crates/checker/src/node_builder/signatures.rs` | `parameter_scope_symbols` | 1 | identity 1 | Parameter name collected as a cloned EscapedName. |
| `crates/checker/src/node_builder/signatures.rs` | `parameter_to_parameter_declaration_name` | 5 | grammar-scalar 5 | Fallback identifiers use the parameter symbol's display name; parameter symbols are scalar-named so the expects are grammar-guaranteed. |
| `crates/checker/src/node_builder/signatures.rs` | `signature_to_signature_declaration_helper` | 2 | identity 2 | Synthetic-scope locals keyed by cloned EscapedName. |
| `crates/checker/src/node_builder/specifier.rs` | `ambient_symbol_name` | 1 | identity 1 | Ambient module name unquoted on the JsStr; lossless. |
| `crates/checker/src/node_builder/specifier.rs` | `no_host_specifier_for_module_symbol` | 1 | identity 1 | Ambient module name unquoted on the JsStr and returned/normalized as JsString; lossless. |
| `crates/checker/src/node_builder/specifier.rs` | `normalize_path_text` | 1 | identity 1 | Only `.`/`..`/empty components are special-cased; all others are kept as JsStr and rejoined losslessly. |
| `crates/checker/src/node_builder/specifier.rs` | `path_is_relative` | 1 | identity 1 | Sentinel match plus lossless byte-prefix checks. |
| `crates/checker/src/node_builder/specifier.rs` | `selected_types_versions_paths` | 1 | identity 1 | Non-scalar typesVersions keys are skipped, matching tsc's VersionRange.tryParse rejection. |
| `crates/checker/src/node_builder/specifier.rs` | `try_directory_with_package_json` | 1 | identity 1 | `as_str()` only detects ASCII `index.*` names; non-scalar names correctly fall through. |
| `crates/checker/src/node_builder/statements.rs` | `get_name_candidate_worker` | 2 | identity 2 | as_str() gates only ASCII internal names and identifier validity; non-scalar names take tsc's `_`-per-unit sanitization. |
| `crates/checker/src/node_builder/statements.rs` | `get_namespace_members_for_serialization` | 1 | identity 1 | Non-identifier (incl. non-scalar) member names are excluded exactly as tsc's isIdentifierText filter does. |
| `crates/checker/src/node_builder/statements.rs` | `get_non_inherited_properties` | 1 | identity 1 | EscapedName equality between inherited and own properties. |
| `crates/checker/src/node_builder/statements.rs` | `get_unused_name` | 1 | grammar-scalar 1 | Scalar expect only on symbol-less temporaries, whose sole caller passes a generated `{root}_base` &str-derived name. |
| `crates/checker/src/node_builder/statements.rs` | `identifier_text` | 1 | grammar-scalar 1 | Transform-arena identifier text is a scalar Rust String. |
| `crates/checker/src/node_builder/statements.rs` | `include_private_symbol` | 1 | identity 1 | Unescaped JsString name passed to get_unused_name with a symbol (worker path). |
| `crates/checker/src/node_builder/statements.rs` | `is_namespace_member` | 1 | identity 1 | Byte comparison against `prototype`. |
| `crates/checker/src/node_builder/statements.rs` | `is_string_a_non_contextual_keyword` | 1 | identity 1 | Keyword lookup is None (false) for non-scalar text; keywords are ASCII. |
| `crates/checker/src/node_builder/statements.rs` | `is_type_representable_as_function_namespace_merge` | 3 | identity 3 | Non-identifier (incl. non-scalar) property names make the merge non-representable, as tsc's `!isIdentifierText` does. |
| `crates/checker/src/node_builder/statements.rs` | `make_serialize_property_symbol` | 1 | identity 1 | Byte comparison against `constructor`. |
| `crates/checker/src/node_builder/statements.rs` | `property_in_base_type` | 2 | identity 2 | Base property search by EscapedName equality. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_as_alias` | 4 | grammar-scalar 1, identity 3 | Target/exported names kept as JsString/JsStr; internal names are generated scalar Strings; internal-name detection via as_str() only matches ASCII constants. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_as_class` | 1 | identity 1 | Byte comparison against `prototype`. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_as_namespace_declaration` | 1 | identity 1 | Fakespace table keyed by cloned EscapedName. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_enum` | 1 | identity 1 | Unescaped member name used for approximate length only; lossless. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_import_equals_alias` | 2 | identity 2 | EXPORT_EQUALS byte comparison and len_units accounting; lossless. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_maybe_alias_assignment` | 5 | identity 5 | Unescaped JsString name compared byte-wise with generated variable names and passed as JsStr to the specifier/unused-name helpers. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_module` | 4 | identity 4 | Member/target names stay JsString; internal names are generated scalar Strings compared byte-wise; export specifiers built from lossless JsStr. |
| `crates/checker/src/node_builder/statements.rs` | `serialize_symbol_worker` | 1 | identity 1 | Escaped/unescaped symbol names stay JsString/JsStr through keyword checks and internal-name derivation. |
| `crates/checker/src/node_builder/statements.rs` | `symbol_to_declarations_worker` | 1 | identity 1 | Single-entry SymbolTable keyed by cloned EscapedName. |
| `crates/checker/src/node_builder/type_nodes.rs` | `add_property_to_element_list` | 1 | identity 1 | Byte-prefix test for late-bound `__@` names. |
| `crates/checker/src/node_builder/type_nodes.rs` | `preserve_comments_on` | 1 | grammar-scalar 1 | JSDoc comment text is a scalar Rust String. |
| `crates/checker/src/node_builder/type_nodes.rs` | `restore_direct_symbol_visibility` | 1 | identity 1 | Cloned EscapedName passed to resolve_name as JsStr. |
| `crates/checker/src/node_builder/type_nodes.rs` | `type_to_type_node_worker` | 2 | grammar-scalar 1, identity 1 | Enum member names split identifier vs string-literal forms losslessly; the variance-marker expect is on a type-parameter identifier. |
| `crates/checker/src/structural.rs` | `create_symbol_with_type` | 1 | identity 1 | Symbol clone with the cloned EscapedName. |
| `crates/checker/src/structural.rs` | `discriminate_type_by_discriminable_items` | 1 | identity 1 | Discriminator name passed as JsStr; lossless. |
| `crates/checker/src/structural.rs` | `exclude_properties` | 1 | identity 1 | HashSet<EscapedName> membership; lossless. |
| `crates/checker/src/structural.rs` | `find_discriminant_properties` | 1 | identity 1 | Discriminant test by JsStr name. |
| `crates/checker/src/structural.rs` | `for_each_property_leaf` | 1 | identity 1 | Constituent property lookup by JsStr name. |
| `crates/checker/src/structural.rs` | `get_parameter_name_at_position` | 3 | grammar-scalar 1, identity 2 | Returns cloned parameter EscapedNames or re-escapes a scalar tuple label String. |
| `crates/checker/src/structural.rs` | `get_properties_of_union_or_intersection_type` | 1 | identity 1 | Seen-name dedup in Vec<EscapedName> and lookup by JsStr; lossless. |
| `crates/checker/src/structural.rs` | `get_signatures_of_type` | 1 | identity 1 | Union-constituent method names compared as EscapedNames. |
| `crates/checker/src/structural.rs` | `get_unmatched_property` | 1 | identity 1 | Property lookup by JsStr name. |
| `crates/checker/src/structural.rs` | `is_applicable_index_type` | 1 | identity 1 | TemplateText numeric-name test is false for lone-surrogate values, as in tsc. |
| `crates/checker/src/structural.rs` | `is_global_function_type` | 1 | identity 1 | Byte comparison against `Function`. |
| `crates/checker/src/structural.rs` | `is_valid_type_for_template_literal_placeholder` | 1 | identity 1 | to_utf8 only gates number/bigint validity (false for lone surrogates in tsc too); other arms use eq_utf8 or the source type losslessly. |
| `crates/checker/src/structural.rs` | `members_related_to_index_info` | 1 | identity 1 | Lossless `contains("-")` byte search for JSX attribute names. |
| `crates/checker/src/structural.rs` | `properties_identical_to` | 1 | identity 1 | Property lookup by JsStr name. |
| `crates/checker/src/structural.rs` | `properties_related_to` | 2 | identity 2 | Property lookups by JsStr name plus `length`/numeric checks with correct non-scalar verdicts. |
| `crates/checker/src/structural.rs` | `report_unmatched_property` | 1 | identity 1 | Property lookup by JsStr name. |
| `crates/checker/src/structural.rs` | `type_related_to_discriminated_type` | 2 | identity 2 | Discriminant property names tracked in HashSet<EscapedName> and looked up as JsStr; lossless. |

### checker (remaining files)

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/checker/src/access.rs` | `check_private_identifier_property_access` | 1 | grammar-scalar 1 | Compares private identifier text (Rust String, grammar-scalar) between declaration and access. |
| `crates/checker/src/access.rs` | `check_property_access_expression_or_qualified_name` | 2 | grammar-scalar 2 | globalThis export probe uses the property identifier text (scalar) for table lookups. |
| `crates/checker/src/access.rs` | `container_seems_to_be_empty_dom_element` | 1 | identity 1 | Symbol names compared against ASCII DOM names with scalar-pattern byte tests. |
| `crates/checker/src/access.rs` | `for_each_property_bool` | 1 | identity 1 | Synthetic property recursion keys constituent lookups by the cloned EscapedName. |
| `crates/checker/src/access.rs` | `get_private_identifier_property_of_type` | 1 | identity 1 | Private-name property lookup by branded EscapedName. |
| `crates/checker/src/access.rs` | `get_suggested_lib_for_non_existent_property` | 1 | identity 1 | Container/member names compared byte-wise against an ASCII feature table. |
| `crates/checker/src/access.rs` | `is_property_declared_in_ancestor_class` | 1 | identity 1 | Base-type property lookup by EscapedName. |
| `crates/checker/src/access.rs` | `raw_symbol_path_below_source_module` | 1 | identity 1 | Collects the symbol path as Vec<EscapedName>. |
| `crates/checker/src/access.rs` | `report_nonexistent_property` | 1 | identity 1 | Suggestion name is unescaped into a JsString diagnostic argument. |
| `crates/checker/src/access.rs` | `resolve_alias_with_deprecation_check` | 1 | identity 1 | Deprecated alias target name reaches the diagnostic as a JsStr argument. |
| `crates/checker/src/calls.rs` | `has_exact_optional_unassignable_properties` | 1 | identity 1 | Property lookup by EscapedName. |
| `crates/checker/src/check.rs` | `alias_for_symbol_in_container_slice` | 1 | identity 1 | Exports lookup keyed by EscapedName. |
| `crates/checker/src/check.rs` | `check_deprecated_type_reference_or_import` | 1 | identity 1 | Unescaped name -> JsString diagnostic argument. |
| `crates/checker/src/check.rs` | `check_type_assignable_to_worker` | 1 | identity 1 | JSX intrinsic-attributes name test via as_str with a correct None arm. |
| `crates/checker/src/check.rs` | `entity_symbol_name_as_written_slice` | 1 | identity 1 | Byte compare against InternalSymbolName::DEFAULT. |
| `crates/checker/src/check.rs` | `get_late_bound_symbol` | 1 | identity 1 | Byte compare against the internal __computed key. |
| `crates/checker/src/check.rs` | `identifier_or_literal_name_slice` | 1 | identity 1 | Identifier / numeric / escaped-string faces over a JsStr, lossless. |
| `crates/checker/src/check.rs` | `map_to_type_string_nodes_slice` | 1 | identity 1 | Collision-retry enrollment only for identifier heads; rendered text kept as JsString. |
| `crates/checker/src/check.rs` | `mapped_type_to_string_slice_node` | 1 | grammar-scalar 1 | Mapped type parameter name is a grammar-scalar identifier. |
| `crates/checker/src/check.rs` | `missing_property_display_name` | 1 | identity 1 | All display arms stay JS values (as_js/unescape/declaration text). |
| `crates/checker/src/check.rs` | `needs_qualification_slice` | 1 | identity 1 | Scope table lookups keyed by EscapedName. |
| `crates/checker/src/check.rs` | `property_name_slice` | 2 | identity 2 | Non-scalar literal names return the escaped quoted face; escaped names unescape to JsString. |
| `crates/checker/src/check.rs` | `report_unmatched_property_head` | 1 | identity 1 | Private-name skip via scalar-pattern prefix tests on the EscapedName. |
| `crates/checker/src/check.rs` | `reused_computed_property_name_text_slice` | 2 | identity 2 | Non-scalar evaluated names take the `["..."]` escaped face on explicit None arms. |
| `crates/checker/src/check.rs` | `should_elide_iterable_default_arguments_slice` | 1 | identity 1 | Protocol-name match with a correct None arm. |
| `crates/checker/src/check.rs` | `specifier_for_module_symbol_slice` | 1 | identity 1 | Quoted ambient module name unwrapped on the JsStr, lossless. |
| `crates/checker/src/check.rs` | `symbol_expression_face_slice` | 1 | not-js-value 1 | Element-access face compares the JsString name against an ASCII numeric spelling and keeps it verbatim otherwise. |
| `crates/checker/src/check.rs` | `symbol_name_from_name_type_slice` | 2 | identity 2 | Literal name-type display: identifier/numeric tests fall to a lossless quoted face for non-scalar names. |
| `crates/checker/src/check.rs` | `try_symbol_table_slice` | 2 | identity 2 | Scope table lookups keyed by EscapedName. |
| `crates/checker/src/check.rs` | `tuple_element_label_slice` | 2 | grammar-scalar 2 | Rest parameter symbol names are grammar-scalar (identifier/__N/synthetic). |
| `crates/checker/src/check.rs` | `type_parameter_to_declaration_slice` | 1 | grammar-scalar 1 | Type parameter declaration name is a grammar-scalar identifier. |
| `crates/checker/src/check.rs` | `type_to_string_slice_node` | 3 | grammar-scalar 2, identity 1 | Type-parameter/infer names are grammar-scalar identifiers; enum member names take a lossless escaped string-literal face. |
| `crates/checker/src/check.rs` | `type_to_string_slice_structured` | 2 | identity 2 | Array/ReadonlyArray sugar decided by byte compare plus symbol identity. |
| `crates/checker/src/class.rs` | `are_type_parameters_identical` | 1 | identity 1 | Branded name equality between identifier text and symbol name. |
| `crates/checker/src/class.rs` | `check_class_for_static_property_name_conflicts` | 1 | identity 1 | Built-in static name match via as_str with a correct None arm; display via unescape. |
| `crates/checker/src/class.rs` | `check_index_constraint_for_property` | 1 | identity 1 | Base property lookup by EscapedName. |
| `crates/checker/src/class.rs` | `check_inherited_properties_are_identical` | 5 | identity 5 | HashMap keyed by EscapedName. |
| `crates/checker/src/class.rs` | `check_member_for_override_modifier` | 3 | identity 3 | Property lookups by EscapedName. |
| `crates/checker/src/class.rs` | `issue_member_specific_error` | 3 | identity 3 | Property lookups by EscapedName. |
| `crates/checker/src/class.rs` | `quoted_member_list` | 3 | identity 3 | Property lookups by EscapedName. |
| `crates/checker/src/contextual.rs` | `discriminate_contextual_type_by_jsx_attributes` | 5 | identity 5 | Discriminator names carried as EscapedName. |
| `crates/checker/src/contextual.rs` | `discriminate_contextual_type_by_object_members` | 6 | identity 6 | Discriminator names carried as EscapedName. |
| `crates/checker/src/contextual.rs` | `element_or_property_access_name` | 1 | grammar-scalar 1 | Numeric literal text (scalar String) and string literal JsStr escaped into EscapedName. |
| `crates/checker/src/contextual.rs` | `get_contextual_type_for_object_literal_element` | 1 | identity 1 | Contextual property lookup by EscapedName. |
| `crates/checker/src/contextual.rs` | `get_contextual_type_for_static_property_declaration` | 1 | identity 1 | Contextual property lookup by EscapedName. |
| `crates/checker/src/contextual.rs` | `get_matching_union_constituent_for_object_literal` | 1 | identity 1 | EscapedName equality. |
| `crates/checker/src/elaboration.rs` | `elaborate_jsx_children` | 1 | grammar-scalar 1 | JSX tag name source text (scalar String) becomes a JsStr diagnostic argument. |
| `crates/checker/src/engine.rs` | `excess_properties_worker` | 1 | identity 1 | EscapedName clone; scalar-pattern contains test; is_known_property lookup. |
| `crates/checker/src/engine.rs` | `find_matching_discriminant_type` | 1 | identity 1 | Discriminator names carried as EscapedName. |
| `crates/checker/src/engine.rs` | `get_key_property_name` | 1 | identity 1 | Key property name kept as EscapedName. |
| `crates/checker/src/engine.rs` | `get_regular_type_of_object_literal` | 1 | identity 1 | SymbolTable insert keyed by EscapedName. |
| `crates/checker/src/engine.rs` | `has_common_properties` | 1 | identity 1 | EscapedName clone; contains test; is_known_property lookup. |
| `crates/checker/src/engine.rs` | `report_incompatible_stack` | 1 | identity 1 | Property path join: non-identifier names take the bracket arms with push_js. |
| `crates/checker/src/engine.rs` | `resolve_excess_property_report` | 1 | identity 1 | JSX for/class special-case via as_str with a correct None arm. |
| `crates/checker/src/engine.rs` | `simple_type_relation` | 2 | identity 2 | EscapedName equality of enum symbol names. |
| `crates/checker/src/evaluate.rs` | `is_numeric_literal_name` | 1 | not-js-value 1 | Byte-exact compare against the ASCII numeric spelling. |
| `crates/checker/src/evaluate.rs` | `js_string_to_number` | 1 | identity 1 | Explicit None -> NaN, JS ToNumber semantics for non-scalar strings. |
| `crates/checker/src/expr.rs` | `get_unique_type_parameter_name` | 1 | grammar-scalar 1 | Type parameter base name is a grammar-scalar identifier. |
| `crates/checker/src/expr.rs` | `get_unique_type_parameters` | 1 | identity 1 | Type parameter names handled as EscapedName. |
| `crates/checker/src/expr.rs` | `has_type_parameter_by_name` | 1 | identity 1 | EscapedName equality. |
| `crates/checker/src/expr.rs` | `pad_object_literal_type` | 1 | identity 1 | SymbolTable insert keyed by EscapedName. |
| `crates/checker/src/flow.rs` | `escaped_text_of` | 2 | grammar-scalar 2 | Identifier/PrivateIdentifier escaped_text are scalar Rust Strings. |
| `crates/checker/src/flow.rs` | `get_flow_type_of_property_synthetic` | 3 | identity 3 | Private-name description split on the JsStr, lossless EscapedName rebuild. |
| `crates/checker/src/flow.rs` | `property_initialization_flow_type` | 3 | identity 3 | Private-name description split on the JsStr, lossless EscapedName rebuild. |
| `crates/checker/src/flow.rs` | `try_get_name_from_type` | 2 | identity 2 | Unique-symbol EscapedName cloned. |
| `crates/checker/src/functions.rs` | `check_async_function_return_type` | 1 | grammar-scalar 1 | Locals lookup by scalar identifier text. |
| `crates/checker/src/functions.rs` | `create_symbol_without_type` | 1 | identity 1 | EscapedName passed to create_symbol. |
| `crates/checker/src/indexed.rs` | `get_literal_type_from_property` | 1 | identity 1 | Byte compare against the internal default key. |
| `crates/checker/src/indexed.rs` | `is_numeric_literal_name` | 1 | identity 1 | Explicit None -> false for non-scalar names. |
| `crates/checker/src/indexed.rs` | `property_name_from_type` | 2 | identity 1, not-js-value 1 | String values escaped via as_js, numbers via ASCII spelling, unique symbols cloned. |
| `crates/checker/src/inference.rs` | `infer_from_properties` | 1 | identity 1 | Property lookup by EscapedName. |
| `crates/checker/src/inference.rs` | `infer_to_template_literal_type` | 1 | identity 1 | to_utf8 None correctly drops number/bigint families; TemplateText passed on. |
| `crates/checker/src/inference.rs` | `template_constraint_match` | 2 | identity 2 | Numeric arms unreachable for non-scalar values; string/template arms compare TemplateText losslessly. |
| `crates/checker/src/instantiate.rs` | `apply_template_string_mapping` | 1 | identity 1 | Alias name -> intrinsic kind; template texts mapped per unit. |
| `crates/checker/src/instantiate.rs` | `get_string_mapping_type` | 1 | identity 1 | Alias name -> intrinsic kind; literal mapped as TemplateText. |
| `crates/checker/src/instantiate.rs` | `instantiate_symbol` | 2 | identity 2 | EscapedName cloned into the instantiated symbol. |
| `crates/checker/src/instantiate.rs` | `intrinsic_type_kind` | 1 | identity 1 | as_str()? None means not an intrinsic; kinds are fixed identifiers. |
| `crates/checker/src/iterate.rs` | `get_iteration_diagnostic_details` | 2 | identity 2 | ES2015 iterable name test with a correct None arm. |
| `crates/checker/src/lib.rs` | `check_directive` | 1 | grammar-scalar 1 | Pragma name from comment source text (scalar). |
| `crates/checker/src/lib.rs` | `check_program_with_prebound_libs_at_observed` | 1 | identity 1 | package.json type match with `_ => Other`. |
| `crates/checker/src/lib.rs` | `fmt` | 3 | display 3 | Display of the fail-closed AuthoritativeModuleFailure; lossy only in the rendered error text. |
| `crates/checker/src/lib.rs` | `resolve_host_current_directory` | 2 | identity 1, native-io 1 | Native cwd converted at the host boundary; JS path segments kept verbatim. |
| `crates/checker/src/merge.rs` | `add_undefined_to_globals_or_error_on_redeclaration` | 1 | grammar-scalar 1 | Fixed internal `undefined` name; lossless lookup and diagnostic. |
| `crates/checker/src/merge.rs` | `clone_symbol` | 2 | identity 2 | EscapedName cloned into the new symbol. |
| `crates/checker/src/merge.rs` | `symbol_display_name` | 1 | identity 1 | Unescape to JsString. |
| `crates/checker/src/merge.rs` | `symbol_name_as_written_slice` | 2 | identity 2 | Literal name-type display with a lossless quoted face for non-scalar names. |
| `crates/checker/src/narrow.rs` | `create_type_predicate_from_type_predicate_node` | 1 | grammar-scalar 1 | Parameter identifier text (scalar) compared against parameter EscapedNames. |
| `crates/checker/src/narrow.rs` | `narrow_type_by_type_name` | 1 | identity 1 | typeof name match with tsc's host-object default arm. |
| `crates/checker/src/narrow.rs` | `typeof_ne_facts` | 1 | identity 1 | None defaults to TYPEOF_NE_HOST_OBJECT at both callers. |
| `crates/checker/src/operators.rs` | `check_assignment_declaration` | 1 | identity 1 | resolve_name by EscapedName. |
| `crates/checker/src/operators.rs` | `get_rest_type` | 1 | identity 1 | SymbolTable insert keyed by EscapedName. |
| `crates/checker/src/program.rs` | `create_symbol` | 2 | identity 2 | EscapedName passed through to the transient allocator. |
| `crates/checker/src/program.rs` | `fmt` | 1 | not-js-value 1 | Host document version (Arc<str>) rendered in an error Display. |
| `crates/checker/src/relate.rs` | `enum_type_relation` | 2 | identity 2 | EscapedName equality and lookups. |
| `crates/checker/src/relpin.rs` | `mark_fresh_probe_source` | 2 | identity 2 | EscapedName cloned into the transient twin. |
| `crates/checker/src/resolve.rs` | `is_es2015_or_later_constructor_name` | 1 | identity 1 | Constructor-name match with a correct None arm. |
| `crates/checker/src/resolve.rs` | `is_primitive_type_name` | 1 | identity 1 | Keyword match with a correct None arm. |
| `crates/checker/src/resolve.rs` | `on_successfully_resolved_symbol` | 3 | identity 3 | EscapedName kept for table lookups and unescaped JsStr diagnostic arguments. |
| `crates/checker/src/resolve.rs` | `resolve_name_full` | 1 | identity 1 | Byte-exact JsStr compare of the export-default local name. |
| `crates/checker/src/spell.rs` | `get_spelling_suggestion_for_name` | 1 | identity 1 | Unescaped JsString candidates compared per UTF-16 code unit. |
| `crates/checker/src/syntactic_type_node_builder.rs` | `visit_existing_node_tree_symbols_worker` | 1 | identity 1 | Computed-name identifier collapse only for identifier text; string literal node kept otherwise. |
| `crates/checker/src/unions.rs` | `remove_subtypes` | 1 | identity 1 | Key property name kept as EscapedName. |
| `crates/checker/src/unused.rs` | `check_unused_locals_and_parameters` | 1 | identity 1 | Unescape to JsString diagnostic argument. |
| `crates/checker/src/unused.rs` | `error_unused_local` | 1 | identity 1 | Unescape to JsString diagnostic argument. |
| `crates/checker/src/widen.rs` | `get_properties_of_context` | 1 | identity 1 | HashMap keyed by EscapedName. |
| `crates/checker/src/widen.rs` | `get_undefined_property` | 1 | identity 1 | Cache keyed by EscapedName. |
| `crates/checker/src/widen.rs` | `get_widened_property` | 1 | identity 1 | Widening context name kept as EscapedName. |
| `crates/checker/src/widen.rs` | `get_widened_type_of_object_literal` | 2 | identity 2 | SymbolTable keyed by EscapedName. |

### emitter

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/emitter/src/builtins.rs` | `create_export_access_from_module_name` | 1 | grammar-scalar 1 | Existing name nodes are cloned losslessly; the expect is limited to &str-built synthesized identifiers. |
| `crates/emitter/src/builtins.rs` | `create_import_binding_access` | 1 | grammar-scalar 1 | Import property access: node-backed names (including non-scalar string-literal import names) are cloned losslessly; the expect covers only the ASCII default / identifier-text node-less properties. |
| `crates/emitter/src/builtins.rs` | `create_preinitialized_export_access` | 1 | grammar-scalar 1 | String-literal export names become a lossless element access before the identifier-only expect. |
| `crates/emitter/src/builtins.rs` | `export_assignment_plan` | 2 | grammar-scalar 2 | Builds the export plan from identifier text (String) and a Box<str> set of directly exported names; export names stay owned JsStrings and are compared byte-exactly. |
| `crates/emitter/src/builtins.rs` | `exports_for_assignment` | 1 | grammar-scalar 1 | Same lossless export/local comparison for assignment targets. |
| `crates/emitter/src/builtins.rs` | `observe_script_source_routing` | 1 | scalar-observer 1 | Activity-canary routing observer; the lossy lowercase path copy only feeds an ASCII suffix predicate that is invariant under U+FFFD replacement (aside: lowercasing is more permissive than tsc's case-sensitive fileExtensionIs). |
| `crates/emitter/src/builtins.rs` | `visit_postfix_unary_expression` | 1 | grammar-scalar 1 | Same lossless export/local comparison as the prefix arm before publishing aliases. |
| `crates/emitter/src/builtins.rs` | `visit_prefix_unary_expression` | 1 | grammar-scalar 1 | Compares the scalar local identifier text with each ModuleExportName via lossless JsStr==&str byte equality to skip re-publishing direct export storage. |
| `crates/emitter/src/builtins/class_fields/downlevel.rs` | `private_environment_class_name` | 1 | identity 1 | Ports tsc's className selection (identifier source first, then isIdentifierText-gated literal text); the lone-surrogate None arm matches tsc. |
| `crates/emitter/src/builtins/class_fields/downlevel.rs` | `visit_class_expression` | 1 | identity 1 | Named-evaluation class name for private-name prefixes gated by isIdentifierText exactly as tsc; non-scalar assigned names correctly produce no prefix. |
| `crates/emitter/src/builtins/jsx.rs` | `collect` | 1 | not-js-value 1 | Pragma tag matching on a Rust String sliced from comment text (scalar source text). |
| `crates/emitter/src/builtins/jsx.rs` | `create_entity_expression` | 2 | grammar-scalar 1, identity 1 | Factory root may be the raw reactNamespace value and is emitted losslessly via create_unchecked_identifier; the resolver query is correctly skipped for non-identifier spellings; tail parts are parsed identifier components. |
| `crates/emitter/src/builtins/jsx.rs` | `decode_entities` | 1 | identity 1 | Entity decoding on JsStr with the same ASCII grammar as tsc's regex; undecodable candidates (including non-scalar ones) are left intact unit-for-unit (asides: '+'-prefixed decimals and >0x10FFFF values diverge on the scalar side). |
| `crates/emitter/src/builtins/standard_decorators.rs` | `create_access_object` | 5 | grammar-scalar 5 | Decorator context `access` object: literal member names take the lossless computed-literal branch as in tsc, private arms use PrivateIdentifier text, and the identifier-without-source-node arms are practically unreachable and scalar. |
| `crates/emitter/src/builtins/standard_decorators.rs` | `decorator_helper_stem` | 2 | grammar-scalar 1, identity 1 | getHelperVariableName port: identifier/private text stems are scalar, string-literal stems are gated by isIdentifierText(ESNext) exactly as tsc, so lone surrogates fall back to `member`. |
| `crates/emitter/src/builtins/standard_decorators.rs` | `identifier_text` | 1 | grammar-scalar 1 | Identifier String text accessor. |
| `crates/emitter/src/builtins/standard_decorators.rs` | `transform_class_like` | 1 | grammar-scalar 1 | Private auto-accessor backing storage name from PrivateIdentifier text (scalar). |
| `crates/emitter/src/builtins/system.rs` | `create_system_binding_property_access` | 1 | grammar-scalar 1 | Literal property names are cloned as nodes; only identifier text reaches the as_str filter, and the JsString fallback/exclusion list is lossless (aside: numeric/bigint literal names appear to hit identifier_or_literal_text's error arm before the kind match). |
| `crates/emitter/src/declarations/statements.rs` | `expando_declaration_arm` | 1 | identity 1 | Expando property names are filtered by isIdentifierText exactly as tsc, omitting non-identifier (lone-surrogate) names losslessly. |
| `crates/emitter/src/error.rs` | `fmt` | 3 | display 3 | Display impls for EmitFailure and EmitIoError; the JsString path/message are retained and the diagnostic path uses the lossless accessors, so U+FFFD appears only in final error text. |
| `crates/emitter/src/execute.rs` | `encode_uri` | 1 | identity 1 | encodeURI port: explicit typed MalformedSourceMapUrl failure on a lone surrogate mirrors the JS URIError; scalar text percent-encoded per UTF-8 byte. |
| `crates/emitter/src/factory.rs` | `create_unchecked_identifier` | 1 | identity 1 | Non-scalar synthetic identifier text is retained in emit metadata and printed via UTF-16, lossless. |
| `crates/emitter/src/printer.rs` | `emit_transformed_node_worker` | 1 | grammar-scalar 1 | String literal with an Identifier text source re-derives the scalar identifier spelling (tsc getTextOfNode) and re-quotes it into UTF-16 units. |
| `crates/emitter/src/printer.rs` | `text` | 1 | native-io 1 | PrintedText::text is the UTF-8 sink projection (U+FFFD per unpaired unit) matching Node's utf8 file write byte-for-byte, with text_utf16() lossless; taken before the OS boundary at artifact construction. |
| `crates/emitter/src/resolver.rs` | `union` | 1 | identity 1 | EmitTrackerSymbolDescription.escaped_name is a verbatim owned JsString (lossless, unbranded) for harness recording. |
| `crates/emitter/src/writer.rs` | `has_trailing_whitespace` | 1 | identity 1 | Last-char whitespace predicate on the projection; U+FFFD and surrogate units are both non-whitespace, so the result equals tsc's charCodeAt check. |
| `crates/emitter/src/writer.rs` | `text` | 1 | native-io 1 | TextWriter::text is the same UTF-8 sink projection; PrintedText is built from the lossless generated_text() and in-crate consumers only use byte-length deltas. |

### program / host / compiler

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/compiler/src/cli.rs` | `create_directory` | 1 | native-io 1 | Output directory converted only for fs::create_dir; JS path kept for the error text. |
| `crates/compiler/src/cli.rs` | `directory_exists` | 1 | native-io 1 | Pure is_dir probe at the OS boundary; no value derived from the lossy copy. |
| `crates/compiler/src/cli.rs` | `directory_exists_js` | 1 | identity 1 | Scalar-only embedded-library-directory intercept with lossless fallthrough. |
| `crates/compiler/src/cli.rs` | `file_exists_js` | 1 | identity 1 | Scalar-only embedded-library intercept with lossless fallthrough. |
| `crates/compiler/src/cli.rs` | `get_directories_js` | 1 | identity 1 | Scalar-only embedded-library-directory intercept with lossless fallthrough. |
| `crates/compiler/src/cli.rs` | `parse_arguments` | 1 | not-js-value 1 | Process argv Strings; not JS values. |
| `crates/compiler/src/cli.rs` | `parse_target` | 1 | not-js-value 1 | Process argv token; not a JS value. |
| `crates/compiler/src/cli.rs` | `read_directory_js` | 1 | identity 1 | Scalar-only embedded-library-directory intercept; entries are ASCII names appended to the JS path. |
| `crates/compiler/src/cli.rs` | `read_file_js` | 1 | identity 1 | Scalar-only embedded-library intercept with lossless fallthrough to the filesystem host. |
| `crates/compiler/src/cli.rs` | `realpath_js` | 1 | identity 1 | Scalar-only embedded intercept returning the original JsStr; lossless fallthrough. |
| `crates/compiler/src/cli.rs` | `rendered_diagnostics_with_exit_work_status_and_h2` | 3 | display 3 | Final terminal rendering of diagnostics/status writes; lossy conversion is the stdout boundary (same as Node). |
| `crates/compiler/src/cli.rs` | `resolve_project_file` | 1 | not-js-value 1 | argv-derived native path; scalar by construction, spelled into TS5057/TS5058. |
| `crates/compiler/src/cli.rs` | `write_file` | 1 | native-io 1 | Emit output path converted only for the fs::write call; JS path kept for the error text. |
| `crates/compiler/src/lib.rs` | `fmt` | 2 | display 2 | DriverError Display: fail-closed driver text only. |
| `crates/compiler/src/lib.rs` | `programmatic_option_diagnostics` | 1 | identity 1 | ignoreDeprecations strict-equality port; None arm reports TS5103 like tsc. |
| `crates/host/src/error.rs` | `new_js` | 1 | native-io 1 | Native display PathBuf beside the retained js_path; consumers use js_path(). |
| `crates/host/src/js_path.rs` | `component_spelling` | 1 | identity 1 | Offset-preserving U+E000 spelling used only for std::path component offsets; never escapes. |
| `crates/host/src/js_path.rs` | `filesystem_path` | 2 | native-io 2 | Actual OS query boundary; U+FFFD spelling matches Node's fs encoding; errors keep the JS path. |
| `crates/host/src/js_path.rs` | `to_file_name_lower_case_js` | 1 | identity 1 | Lossless case fold with per-unit surrogate re-emission. |
| `crates/host/src/memory.rs` | `build` | 4 | display 4 | Builder validation error details; keys and realpath map are JsString-based. |
| `crates/host/src/memory.rs` | `scalar_native_result` | 1 | native-io 1 | Legacy native listing API fails closed (JS path retained) on non-scalar entries. |
| `crates/program/src/config.rs` | `config_diagnostic_owners` | 1 | identity 1 | ASCII container-name match; None arm correct. |
| `crates/program/src/config.rs` | `config_option_lib` | 1 | grammar-scalar 1 | expect() on catalogue-mapped &'static str lib values; unreachable for raw strings. |
| `crates/program/src/config.rs` | `current_directory` | 1 | native-io 1 | Legacy native cwd callback fails closed with the JS path retained; JS callback is lossless. |
| `crates/program/src/config.rs` | `deprecation_option_diagnostics` | 1 | identity 1 | Strict 5.0/6.0 match; None -> TS5103 like tsc. |
| `crates/program/src/config.rs` | `emit_option_validation_diagnostic_for_properties` | 1 | identity 1 | ASCII option-name match for locations; None arm falls back correctly. |
| `crates/program/src/config.rs` | `javascript_array_index` | 1 | identity 1 | Canonical ASCII index grammar; non-scalar names are ordinary properties. |
| `crates/program/src/config.rs` | `no_lib_lib_option_diagnostics` | 1 | identity 1 | ASCII lib/noLib name match; None arm correct. |
| `crates/program/src/config.rs` | `program_config_file` | 1 | identity 1 | ASCII files/include match; element texts cloned as JsString. |
| `crates/program/src/config_host.rs` | `is_implicit_excluded_directory` | 2 | identity 2 | ASCII excluded-directory catalog match on the JsStr basename. |
| `crates/program/src/config_options.rs` | `named_string_value` | 2 | identity 2 | Custom map lookup; None reports the invalid-argument diagnostic like tsc. |
| `crates/program/src/config_options.rs` | `named_value` | 2 | identity 2 | Custom map lookup; None reports the invalid-argument diagnostic like tsc. |
| `crates/program/src/config_options.rs` | `option_spelling_suggestion` | 1 | identity 1 | UTF-16 Levenshtein port; short-candidate skip semantics preserved. |
| `crates/program/src/error.rs` | `new_js` | 1 | native-io 1 | Native display PathBuf beside the retained js_path. |
| `crates/program/src/js_path.rs` | `starts_with_ignore_case` | 1 | identity 1 | ${configDir} case-insensitive prefix; surrogate slices correctly fail. |
| `crates/program/src/js_string_ops.rs` | `js_replace_stars` | 1 | identity 1 | Scalar fast path plus lossless piecewise construction for non-scalar inputs. |
| `crates/program/src/json.rs` | `json_object_get` | 2 | identity 2 | Array-prototype index lookup; non-scalar property is not an index, as in JS. |
| `crates/program/src/library.rs` | `reference_file_name` | 1 | identity 1 | Catalog lookup; None for surrogate spellings like libMap.get. |
| `crates/program/src/library.rs` | `source_file_priority` | 1 | identity 1 | Catalog rank; non-scalar basename ranks as non-member like tsc. |
| `crates/program/src/library.rs` | `spelling_suggestion` | 1 | identity 1 | UTF-16 Levenshtein port; short-candidate skip semantics preserved. |
| `crates/program/src/loader.rs` | `bind_module_resolution` | 2 | display 2 | Fail-closed unsupported/invalid detail text; membership decided on CanonicalPath. |
| `crates/program/src/loader.rs` | `bind_type_resolution` | 1 | display 1 | Fail-closed invalid_data detail text; lookup on CanonicalPath. |
| `crates/program/src/loader.rs` | `error_display_path` | 1 | native-io 1 | Infrastructure error native display path; js_path retained via with_js_path. |
| `crates/program/src/loader.rs` | `process_module_requests` | 1 | display 1 | Fail-closed unsupported-extension detail text. |
| `crates/program/src/loader.rs` | `resolve_runtime_dependency_symlinks` | 1 | identity 1 | Lossy copy only feeds an ASCII substring predicate invariant under U+FFFD; JsStr::contains would be cleaner. |
| `crates/program/src/loader.rs` | `resolved_library_path` | 1 | grammar-scalar 1 | Synthetic ASCII lookup name from the &str catalog file name. |
| `crates/program/src/module_requests.rs` | `file_emit_module_kind` | 1 | display 1 | Fail-closed unsupported detail text. |
| `crates/program/src/module_requests.rs` | `string_literal_like_text` | 2 | identity 2 | resolution-mode exact ASCII compare projection; None = no override like tsc. |
| `crates/program/src/module_requests.rs` | `unsupported` | 1 | display 1 | ResolutionError::Unsupported detail prefix. |
| `crates/program/src/module_resolution.rs` | `declaration_extension_probe_plan` | 1 | display 1 | Unsupported-extension error text; suffix stripping on JsStr. |
| `crates/program/src/module_resolution.rs` | `extension_probe_plan` | 1 | display 1 | Unsupported-extension error text; suffix stripping on JsStr. |
| `crates/program/src/module_resolution.rs` | `implementation_extension_probe_plan` | 1 | display 1 | Unsupported-extension error text; suffix stripping on JsStr. |
| `crates/program/src/module_resolution.rs` | `into_resolved_module` | 2 | display 2 | Mismatch error text; CanonicalPath comparison. |
| `crates/program/src/module_resolution.rs` | `into_resolved_type_reference_directive` | 2 | display 2 | Mismatch error text; CanonicalPath comparison. |
| `crates/program/src/module_resolution.rs` | `js_array_index` | 1 | identity 1 | Canonical ASCII index grammar. |
| `crates/program/src/module_resolution.rs` | `js_generic_array_property_index` | 1 | identity 1 | Canonical ASCII index grammar. |
| `crates/program/src/module_resolution.rs` | `js_number_from_text` | 1 | identity 1 | Number() coercion; non-scalar -> NaN like JS. |
| `crates/program/src/module_resolution.rs` | `load_package` | 1 | display 1 | Decode-failure detail text; cache/program path use the JsStr. |
| `crates/program/src/module_resolution.rs` | `normalize_absolute_path_worker` | 1 | grammar-scalar 1 | Native scalar path through scalar-closed lexical normalization; expect unreachable. |
| `crates/program/src/module_resolution.rs` | `normalize_package_subpath` | 2 | display 2 | Escape-check error text; candidate returned as JsString. |
| `crates/program/src/module_resolution.rs` | `package_condition_matches` | 1 | identity 1 | types@range ASCII grammar; other conditions compared losslessly. |
| `crates/program/src/module_resolution.rs` | `probe_legacy_directory_worker` | 2 | display 2 | invalid_data error text; logical name built from JsStr. |
| `crates/program/src/module_resolution.rs` | `realpath_program_path` | 1 | display 1 | invalid_data detail text; JS path used losslessly for the host queries. |
| `crates/program/src/module_resolution.rs` | `search_package_exports` | 1 | display 1 | Unsupported detail text. |
| `crates/program/src/module_resolution.rs` | `search_package_types_versions` | 2 | display 1, identity 1 | ASCII version-range keys (None = no match, like VersionRange.tryParse); error text display. |
| `crates/program/src/module_resolution.rs` | `validate_path_context` | 1 | display 1 | canonicalization_js detail text; JsString comparisons. |
| `crates/program/src/option_validation.rs` | `validate_compiler_options` | 1 | identity 1 | reactNamespace identifier check; None -> violation with the original JS value, like isIdentifierText. |
| `crates/program/src/prepared.rs` | `build` | 1 | display 1 | Package-scope validation detail text. |
| `crates/program/src/prepared.rs` | `register_text_owner` | 1 | display 1 | Text-owner conflict detail text. |
| `crates/program/src/prepared.rs` | `try_add_auxiliary_file` | 1 | display 1 | Identity-conflict detail text. |
| `crates/program/src/prepared.rs` | `try_add_package_metadata` | 1 | display 1 | Identity-conflict detail text. |
| `crates/program/src/prepared.rs` | `try_add_package_redirect` | 2 | display 2 | Identity-conflict detail text; keys are CanonicalPath. |
| `crates/program/src/prepared.rs` | `try_add_root` | 2 | display 2 | Root mismatch detail text; CanonicalPath comparison. |
| `crates/program/src/prepared.rs` | `try_add_source_file` | 4 | display 4 | Identity-conflict detail text; keys are CanonicalPath, js_path retained. |
| `crates/program/src/prepared.rs` | `validate_canonical_case` | 1 | display 1 | Expected-fold detail text; lossless fold comparison. |
| `crates/program/src/prepared.rs` | `validate_module_target` | 1 | display 1 | Extension mismatch detail text. |
| `crates/program/src/prepared.rs` | `validate_owned_resolution_paths` | 6 | display 6 | Path-ownership validation detail text; canonical() equality. |
| `crates/program/src/resolution.rs` | `fmt` | 1 | display 1 | MissingResolutionError Display text. |
| `crates/program/src/resolution.rs` | `is_valid` | 1 | identity 1 | Arbitrary extension validity; ASCII known-extension exclusion with lossless JsStr checks. |
| `crates/program/src/resolution_error.rs` | `canonicalization_js` | 1 | native-io 1 | Native display PathBuf beside the retained js_path. |
| `crates/program/src/text.rs` | `decode_host_text` | 1 | not-js-value 1 | Native file bytes decoded like Node's readFile; UTF-16 unpaired surrogates rejected explicitly. |

### harness / conformance / fuzz (evidence tooling on the acceptance path)

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/conformance/src/families.rs` | `validate_structure` | 2 | not-js-value 2 | Validates the diag-families.json structure with String family names; no JS values. |
| `crates/conformance/src/host_resolution.rs` | `classify` | 1 | not-js-value 1 | Matches the String fixture path against literal corpus paths. |
| `crates/conformance/src/host_resolution.rs` | `expected_owner_names` | 1 | not-js-value 1 | Matches the String fixture path against literal corpus paths to select owner names. |
| `crates/conformance/src/host_resolution.rs` | `fixture_count` | 1 | not-js-value 1 | Counts distinct String fixture paths. |
| `crates/conformance/src/host_resolution.rs` | `git_resolve_commit` | 1 | not-js-value 1 | git subprocess; stderr rendered lossily into the error only. |
| `crates/conformance/src/host_resolution.rs` | `load_resolution_requests` | 3 | not-js-value 3 | Runs the Node request producer keyed by String row ids; stderr is error text only. |
| `crates/conformance/src/host_resolution.rs` | `negative_canary_spec` | 2 | not-js-value 2 | Matches String fixture/matrix keys against literal corpus paths. |
| `crates/conformance/src/host_resolution.rs` | `owner_evidence` | 1 | not-js-value 1 | Matches the String fixture path to pick evidence text. |
| `crates/conformance/src/host_resolution.rs` | `row_seed_projection_sha256` | 1 | not-js-value 1 | Hashes String evidence text into a projection digest. |
| `crates/conformance/src/host_resolution.rs` | `scope_seed_projection_sha256` | 1 | not-js-value 1 | Hashes String evidence text into a projection digest. |
| `crates/conformance/src/host_resolution.rs` | `summarize` | 1 | not-js-value 1 | Counts distinct String fixture paths. |
| `crates/conformance/src/host_resolution.rs` | `validate_registry_with_options` | 2 | not-js-value 2 | Validates the H0 registry rows by String id; registry metadata only. |
| `crates/conformance/src/host_resolution.rs` | `validate_trusted_baseline_at_head` | 2 | not-js-value 2 | Pairs head and trusted registry rows by String id. |
| `crates/conformance/src/host_resolution.rs` | `validate_tsc_owners` | 1 | not-js-value 1 | Checks the vendored tsc owner chain by String declaration ids. |
| `crates/conformance/src/host_resolution.rs` | `workspace_history_rel` | 1 | not-js-value 1 | Native canonical path relative to the git root, stringified for git. |
| `crates/conformance/src/lib.rs` | `classify_fn_partial_boundaries` | 1 | identity 1 | Compares checker JsString file names with oracle scalar file names by byte equality (report-only FN audit). |
| `crates/conformance/src/lib.rs` | `fixture_key` | 1 | not-js-value 1 | Stringifies a native corpus-relative path as the golden key. |
| `crates/conformance/src/lib.rs` | `line_col` | 1 | identity 1 | Locates a tsrs diagnostic's file by lossless JsStr equality with scalar program file names; unmatched names later error in scalar_observation. |
| `crates/conformance/src/lib.rs` | `measure_conformance_with_schedule` | 3 | not-js-value 3 | Pairs expanded programs and goldens by ASCII matrix key before execution. |
| `crates/conformance/src/lib.rs` | `observe_case` | 1 | not-js-value 1 | Uses the String oracle CLI hash as the T4 pin. |
| `crates/conformance/src/lib.rs` | `orphan_golden_case` | 1 | not-js-value 1 | Finds a golden matrix key with no expanded program. |
| `crates/conformance/src/lib.rs` | `produce_ci_grading_events` | 1 | not-js-value 1 | Pairs expanded programs and goldens by ASCII matrix key. |
| `crates/conformance/src/lib.rs` | `scalar_observation` | 1 | scalar-observer 1 | Scalar golden-wire observer that returns Err (never a replacement) for unpaired surrogates in tsrs diagnostic names/messages before the T0-T4 golden comparison. |
| `crates/conformance/src/lib.rs` | `validate_ci_summary_identity_shape` | 1 | not-js-value 1 | Matches the String band name of the CI summary. |
| `crates/conformance/src/ratchet.rs` | `atomic_write` | 1 | not-js-value 1 | Derives a native temp file name from the artifact path. |
| `crates/conformance/src/ratchet.rs` | `comparator_state` | 1 | not-js-value 1 | Checks String tier names of the oracle-inputs comparator map. |
| `crates/conformance/src/ratchet.rs` | `git` | 1 | not-js-value 1 | git subprocess wrapper; stderr rendered lossily into the error only. |
| `crates/conformance/src/ratchet.rs` | `git_rel_path` | 1 | not-js-value 1 | Native canonical path relative to the git root, stringified for git. |
| `crates/conformance/src/ratchet.rs` | `vendor_pins` | 1 | not-js-value 1 | Lists native vendored lib file names to hash the vendor pins. |
| `crates/conformance/src/ratchet.rs` | `version_ancestry` | 1 | not-js-value 1 | Indexes commit SHA Strings for the ancestry walk. |
| `crates/conformance/src/rendered.rs` | `run_t4_report` | 2 | not-js-value 2 | Pairs expanded programs and goldens by ASCII matrix key for the T4 report. |
| `crates/conformance/src/scope.rs` | `is_ancestor` | 1 | not-js-value 1 | git merge-base; stderr rendered lossily into the error only. |
| `crates/conformance/src/scope.rs` | `run_cross_language_check` | 1 | not-js-value 1 | Node encoder cross-check; stderr rendered lossily into the error only. |
| `crates/conformance/src/scope.rs` | `validate_identity_pass` | 1 | not-js-value 1 | Matches the String pass name of a scope identity. |
| `crates/conformance/src/scope.rs` | `verify_scope_baseline` | 2 | not-js-value 2 | Pairs band pins by String band name against the trusted base. |
| `crates/fuzz/src/adapters/oracle.rs` | `session_failure_detail` | 1 | not-js-value 1 | Builds oracle-session failure text from captured stderr bytes (lossy, error text only). |
| `crates/fuzz/src/adapters/tsrs.rs` | `diagnostic_location` | 1 | identity 1 | Locates the Rust diagnostic's file by lossless JsStr equality with scalar case file names; misses and non-scalar names are Errs. |
| `crates/fuzz/src/adapters/tsrs.rs` | `related_diagnostic` | 1 | identity 1 | Locates a related diagnostic's file by lossless JsStr equality; misses and non-scalar names are Errs. |
| `crates/fuzz/src/adapters/tsrs.rs` | `scalar_wire_text` | 1 | scalar-observer 1 | Scalar M9 wire observer that returns Err (never a replacement) for unpaired surrogates in Rust diagnostic text/names before the oracle comparison. |
| `crates/fuzz/src/classify.rs` | `terminal_key` | 1 | not-js-value 1 | Formats a static enum boundary name into the terminal key. |
| `crates/fuzz/src/executor.rs` | `session_failure_detail` | 1 | not-js-value 1 | Builds Rust-worker failure text from captured stderr bytes (lossy, error text only). |
| `crates/fuzz/src/normalize.rs` | `for_validated_case` | 1 | grammar-scalar 1 | Builds normalization mappings from generator identifier Strings (scalar by construction). |
| `crates/fuzz/src/normalize.rs` | `validate_mapping_ordinals` | 1 | grammar-scalar 1 | Validates mapping ordinals and duplicate String sources. |
| `crates/fuzz/src/preflight.rs` | `blocker_ids` | 1 | not-js-value 1 | Iterates String blocker ids. |
| `crates/fuzz/src/preflight.rs` | `derived_manifest_status_check` | 1 | not-js-value 1 | Formats a static enum status into evidence text. |
| `crates/fuzz/src/preflight.rs` | `load_preflight_inventory` | 1 | not-js-value 1 | Validates TOML manifest check ids for duplicates. |
| `crates/fuzz/src/preflight.rs` | `render_text` | 1 | not-js-value 1 | Renders preflight status text from enums and String ids. |
| `crates/fuzz/src/preflight.rs` | `string_array` | 1 | not-js-value 1 | Reads a TOML string array, erroring on non-strings. |
| `crates/fuzz/src/preflight.rs` | `validate_check` | 1 | not-js-value 1 | Validates a manifest check; enum status in messages. |
| `crates/fuzz/src/process_session.rs` | `to_string_lossy` | 1 | not-js-value 1 | Lossy conversion of captured child stderr bytes, used only for failure detail text. |
| `crates/fuzz/src/schema.rs` | `validate` | 2 | not-js-value 2 | Validates the oracle Node argument Strings of the process policy. |
| `crates/fuzz/src/schema.rs` | `validate_files_structure` | 1 | grammar-scalar 1 | Checks duplicate encoded file name Strings. |
| `crates/fuzz/src/schema.rs` | `validate_with_decoded_sources` | 9 | grammar-scalar 5, not-js-value 4 | Validates CaseSpec ids, option names, identifiers and file names, all Rust Strings from serde (scalar by construction). |
| `crates/fuzz/src/worker_protocol.rs` | `validate` | 2 | not-js-value 2 | Validates worker hello implementation/version Strings. |
| `crates/harness/src/lib.rs` | `compiler_option_kind` | 1 | grammar-scalar 1 | Maps a lowercased option key (fixed identifier) to its kind. |
| `crates/harness/src/lib.rs` | `jsx_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `module_detection_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `module_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `module_resolution_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `new_line_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `parse_bool` | 1 | grammar-scalar 1 | Parses a boolean directive value, erroring on unknown text. |
| `crates/harness/src/lib.rs` | `project_compiler_options` | 1 | grammar-scalar 1 | Projects directive option values (comment text) to CompilerOptions, ignoring unknown spellings per the historical adapter. |
| `crates/harness/src/lib.rs` | `target_option_value` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> None. |
| `crates/harness/src/lib.rs` | `validate_compiler_options` | 1 | grammar-scalar 1 | Checks option keys for ASCII-fold duplicates. |
| `crates/harness/src/upstream_suites.rs` | `case_id` | 1 | not-js-value 1 | Formats the case id from the SuiteName enum and percent-encoded Strings. |
| `crates/harness/src/upstream_suites.rs` | `decode_source` | 2 | native-io 2 | Decodes corpus file bytes to text: UTF-8 arms match Node's utf8 replacement; the UTF-16 arm replaces lone surrogates with U+FFFD (Node preserves them) but no pinned UTF-16 fixture contains one. |
| `crates/harness/src/upstream_suites.rs` | `expand_project_fixture` | 1 | grammar-scalar 1 | Reads project descriptor inputFiles from serde_json, erroring on non-strings. |
| `crates/harness/src/upstream_suites.rs` | `generate_manifest` | 1 | not-js-value 1 | Builds project case variants from the ProjectModule enum. |
| `crates/harness/src/upstream_suites.rs` | `source_inventory_sha256` | 1 | not-js-value 1 | Hashes manifest source Strings and the SuiteName enum. |
| `crates/harness/src/upstream_suites.rs` | `validate_corpus_identity` | 3 | not-js-value 3 | Checks suite identity against the immutable pin using the SuiteName enum. |
| `crates/harness/src/upstream_suites.rs` | `validate_fixtures_and_cases` | 6 | not-js-value 6 | Validates expanded cases against fixtures using manifest String ids/paths/variants. |
| `crates/harness/src/upstream_suites.rs` | `validate_ordered_settings` | 1 | grammar-scalar 1 | Checks directive setting names for empties/duplicates. |
| `crates/harness/src/upstream_suites.rs` | `validate_sources` | 4 | not-js-value 4 | Reconstructs the blob inventory from manifest Strings and the SuiteName enum. |
| `crates/harness/src/upstream_suites/compiler.rs` | `is_config_file_name` | 1 | grammar-scalar 1 | Lowercases a directive-derived unit base name to detect tsconfig/jsconfig. |
| `crates/harness/src/upstream_suites/execution.rs` | `apply_compiler_setting` | 3 | grammar-scalar 3 | Maps a lowercased directive key/value to CompilerOptions, erroring on unknown spellings. |
| `crates/harness/src/upstream_suites/execution.rs` | `apply_compiler_settings` | 1 | grammar-scalar 1 | Applies each directive setting by lowercased key. |
| `crates/harness/src/upstream_suites/execution.rs` | `build_compiler_fixture` | 2 | grammar-scalar 2 | Copies @symlink directive Strings into symlink operations. |
| `crates/harness/src/upstream_suites/execution.rs` | `build_compiler_plan` | 3 | not-js-value 3 | Copies manifest variant metadata Strings into the plan. |
| `crates/harness/src/upstream_suites/execution.rs` | `compiler_root_allow_js` | 1 | grammar-scalar 1 | Reads allowJs/checkJs by lowercased directive key. |
| `crates/harness/src/upstream_suites/execution.rs` | `compiler_root_selection` | 1 | identity 1 | Selects root units by lossless JsStr==&str comparison of tsconfig file names with normalized unit paths. |
| `crates/harness/src/upstream_suites/execution.rs` | `contains_reference_path` | 1 | grammar-scalar 1 | Scans decoded source text for a reference-path directive. |
| `crates/harness/src/upstream_suites/execution.rs` | `decoded_source` | 2 | not-js-value 2 | Copies manifest path/blob Strings into VerifiedSource alongside the decoded text. |
| `crates/harness/src/upstream_suites/execution.rs` | `exact_setting` | 1 | grammar-scalar 1 | Returns a directive setting value String. |
| `crates/harness/src/upstream_suites/execution.rs` | `load_compiler_program` | 2 | grammar-scalar 2 | Builds the VFS host from directive-derived unit/alias path Strings. |
| `crates/harness/src/upstream_suites/execution.rs` | `load_qualified_compiler_emit_with_symlinks` | 2 | grammar-scalar 2 | Reads the virtual tsconfig via a lossless &str->JsStr conversion and applies directive settings. |
| `crates/harness/src/upstream_suites/execution.rs` | `load_recorded_execution_plans` | 1 | not-js-value 1 | Copies manifest case ids into provenance. |
| `crates/harness/src/upstream_suites/execution.rs` | `lookup` | 1 | grammar-scalar 1 | Matches a lowercased directive key against baseline-metadata keys. |
| `crates/harness/src/upstream_suites/execution.rs` | `parse_jsx` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> error. |
| `crates/harness/src/upstream_suites/execution.rs` | `parse_module` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> error. |
| `crates/harness/src/upstream_suites/execution.rs` | `parse_module_detection` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> error. |
| `crates/harness/src/upstream_suites/execution.rs` | `parse_module_resolution` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> error. |
| `crates/harness/src/upstream_suites/execution.rs` | `parse_target` | 1 | grammar-scalar 1 | Maps a lowercased directive value to an enum number, unknown -> error. |
| `crates/harness/src/upstream_suites/execution.rs` | `project_compiler_options` | 1 | grammar-scalar 1 | Applies manifest-recorded directive settings. |
| `crates/harness/src/upstream_suites/execution.rs` | `project_root_selection` | 2 | grammar-scalar 2 | Selects the project root from serde_json descriptor strings (scalar by construction). |
| `crates/harness/src/upstream_suites/execution.rs` | `required_ordered_string` | 1 | grammar-scalar 1 | Reads a required descriptor string, erroring when absent or non-string. |
| `crates/harness/src/upstream_suites/execution.rs` | `suite_identity` | 1 | not-js-value 1 | Reports the suite-name enum in a manifest error. |
| `crates/harness/src/upstream_suites/execution.rs` | `verify_project_inputs` | 2 | grammar-scalar 2 | Re-reads descriptor inputFiles from serde_json, erroring on non-strings, and compares with manifest paths. |
| `crates/harness/src/upstream_suites/execution.rs` | `verify_suite_path_sets` | 1 | not-js-value 1 | Reports suite-name enum in a manifest mismatch error. |
| `crates/harness/src/upstream_suites/execution/js_paths.rs` | `scalar_json_observation` | 1 | scalar-observer 1 | Scalar observer for the config-host trace that returns Err (never a projection) for unpaired surrogates before the trace is recorded. |
| `crates/harness/src/upstream_suites/execution/js_paths.rs` | `visit_directory` | 1 | grammar-scalar 1 | Wraps directive-derived unit paths losslessly as JsStr for readDirectory matching. |
| `crates/harness/src/upstream_suites/execution/project.rs` | `apply_project_emit_options` | 5 | grammar-scalar 5 | Copies descriptor path options losslessly into JsString options (serde-scalar), leaving non-strings None. |
| `crates/harness/src/upstream_suites/execution/project.rs` | `project_named_i32` | 2 | grammar-scalar 2 | Maps a descriptor module option value to an enum number, erroring on unknown or non-string values. |
| `crates/harness/src/upstream_suites/h1_conformance.rs` | `derive_summary` | 1 | not-js-value 1 | Counts unique manifest blob SHA Strings. |
| `crates/harness/src/upstream_suites/h1_conformance.rs` | `validate_fixtures_and_cases` | 1 | not-js-value 1 | Checks duplicate manifest case ids. |
| `crates/harness/src/upstream_suites/h1_conformance.rs` | `validate_sources` | 1 | not-js-value 1 | Collects manifest blob SHA Strings for the inventory. |

### xtask (acceptance runners and evidence producers)

| file | function | rows | categories | summary |
| --- | --- | ---: | --- | --- |
| `crates/xtask/src/acceptance_slices.rs` | `git_head` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/acceptance_slices.rs` | `run` | 2 | not-js-value 2 | All 2 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/acceptance_slices.rs` | `stable_message` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/ci_test_receipts.rs` | `receipt_decision` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/ci_test_receipts.rs` | `rustc_version` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/codegen_common.rs` | `rustfmt_text` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/completion.rs` | `parse_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h1_conformance.rs` | `parse_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h1_emit_acceptance.rs` | `assert_outcome` | 2 | not-js-value 1, suspect 1 | Compares emitSkipped/emittedFiles/sourceMaps with the H1 qualification; emitted_files() (now JsString) is rendered lossy before the comparison (suspect). |
| `crates/xtask/src/h1_emit_acceptance.rs` | `assert_writes` | 7 | not-js-value 5, scalar-observer 1, suspect 1 | Byte-exact write comparison with the H1 qualification; the path uses expect() but source_files() (now JsString) is rendered lossy before the provenance comparison (suspect). |
| `crates/xtask/src/h1_emit_acceptance.rs` | `normalize_chain` | 1 | scalar-observer 1 | Serializes MessageChain.text for the H1 diagnostics comparison; panics (fails closed) on a non-scalar text. |
| `crates/xtask/src/h1_emit_acceptance.rs` | `normalize_diagnostic` | 3 | identity 1, scalar-observer 2 | Serializes diagnostic file names for the H1 comparison with expect() guards and looks up line/column with a lossless JsStr comparison. |
| `crates/xtask/src/h1_emit_acceptance.rs` | `source_texts` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `promoted_to_h2_1e` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `promoted_to_h2_2a` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `promoted_to_h2_2b` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `promoted_to_h2_2c` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `promoted_to_h2_4b` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1a_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `promoted_to_h2_2b` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1b_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1c_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_1c_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1c_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1c_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1c_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `promoted_to_h2_2b` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1d_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1e_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_1e_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_1e_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1e_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_1e_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `promoted_to_h2_2b` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2a_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `expected_node_format_sources` | 4 | not-js-value 4 | All 4 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2b_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `assert_exact_writes` | 4 | not-js-value 1, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers js-value, serde-json; see rows. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `assert_h2_6c_vector_population` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `assert_h2_7b_effective_declaration_options` | 3 | scalar-observer 3 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `canonicalize_diagnostic_paths` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `case_input_with_floor` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `census_fingerprint_verifies` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `compact_typescript_observation` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `count_diverging_writes` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `emitted_files_match` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `emitted_files_value` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `exact_write_difference` | 2 | not-js-value 2 | All 2 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `execute_slice_observed_with_inputs` | 12 | not-js-value 12 | All 12 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `expected_declaration_members` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `expected_node_format_sources` | 4 | not-js-value 4 | All 4 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `expected_typed_activity` | 1 | scalar-observer 1 | Classifies prepared source files by ASCII path suffix using a lossy rendering of ProgramPath::display(); replacement cannot change the classification. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `h2_7b_nocheck_case_count` | 4 | not-js-value 4 | All 4 hit(s) are on bytes/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `is_transform_source` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `load` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `load_h2_vector_divergence_manifest` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `report_h2_6c_suite_outcomes` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `run_h2_5a` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `source_files_value` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `source_maps_match` | 2 | not-js-value 1, scalar-observer 1 | Mixed: categories not-js-value, scalar-observer on receivers js-value, rust-str; see rows. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `source_maps_value` | 1 | suspect 1 | Builds the source_maps vector facet; input_source_files (now JsString) are rendered lossy before compare_vector_value (suspect), unlike the sibling expect() sites. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `transform_source_paths` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `validate_h2_7b_adjacency` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `validate_h2_7b_qualification` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2c_acceptance.rs` | `vectorize_writes` | 3 | scalar-observer 3 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `execute_observed` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `expected_node_format_sources` | 4 | not-js-value 4 | All 4 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `promotes_historical_case` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_2d_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `execute_observed` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `expected_node_format_sources` | 4 | not-js-value 4 | All 4 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3a_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `owner_options` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3b_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `case_input` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `owner_options` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `promotes_historical_case` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3c_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `assert_exact_writes` | 5 | not-js-value 2, scalar-observer 3 | Mixed: categories not-js-value, scalar-observer on receivers bytes, js-value, serde-json; see rows. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `expected_owner_activity` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `flatten_message_chain` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `normalize_diagnostic` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `owner_options` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_3d_acceptance.rs` | `string` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_6c_refusal_migrations.rs` | `observe` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/h2_6c_refusal_migrations.rs` | `partition` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_7c_acceptance.rs` | `validate_artifact` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/h2_7de_acceptance.rs` | `validate_artifact` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/host_resolution.rs` | `parse_check_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/host_resolution.rs` | `parse_draft_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/invariant_attestation.rs` | `changed_fingerprint_names` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/invariant_attestation.rs` | `normalize_path` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/l0_identity_stress.rs` | `parse_arguments` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/l0_text_stress.rs` | `parse_arguments` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/l0_text_stress.rs` | `run` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/l1_incremental_stress.rs` | `parse_arguments` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/local_ci_resume.rs` | `collect` | 2 | not-js-value 2 | All 2 hit(s) are on bytes/os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/local_ci_resume.rs` | `environment_affects_ci` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/local_ci_resume.rs` | `repository_paths` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `compare_program_with_mutation_canary` | 1 | scalar-observer 1 | Differential fuzz comparison of tsrs vs oracle diagnostics; a non-scalar rendered wire text returns an Err (fails loudly). |
| `crates/xtask/src/m8_evidence.rs` | `coverage_emitters` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `divergence_signature` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `evidence` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `first_affected_diagnostic_key` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `fuzz_run` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `message_chain_value` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/m8_evidence.rs` | `perf_conformance` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `produce_ci_conformance_outputs` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `produce_fuzz` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `produce_performance` | 2 | not-js-value 2 | All 2 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `produce_runtime` | 4 | not-js-value 4 | All 4 hit(s) are on bytes/rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `reduce_source_preserving_signature` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `related_value` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/m8_evidence.rs` | `run_coverage_process` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `run_node_jsonl` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `runtime_artifact_is_current` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `t4_divergence_signature` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `timed_conformance` | 2 | not-js-value 2 | All 2 hit(s) are on bytes/rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `tsrs_value` | 2 | scalar-observer 2 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/m8_evidence.rs` | `validate_runtime_artifact` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `verify_for_readiness` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_evidence.rs` | `workspace_relative` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `apply_review` | 6 | not-js-value 6 | All 6 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `audit_plan` | 28 | not-js-value 28 | All 28 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `audit_reviewed_overrides` | 5 | not-js-value 5 | All 5 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `build_static_closure` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `check` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `coverage_set` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `draft` | 14 | not-js-value 14 | All 14 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `ensure_raw_trace` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `freeze` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `git_head` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `materialize_programs` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `parse_draft_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `repository_relative_path` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `sibling_candidates` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `validate_plan_transition` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `validate_raw_trace` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `validate_raw_trace_header` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `validate_scc_decisions` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `verify_frozen_anchor` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_plan.rs` | `verify_review_input` | 2 | not-js-value 2 | All 2 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `current_node_version` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `parse_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `run` | 2 | not-js-value 2 | All 2 hit(s) are on bytes/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `run_node_jsonl` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `validate_instrumentation` | 3 | not-js-value 3 | All 3 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/m8_trace.rs` | `validate_probe` | 4 | not-js-value 4 | All 4 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `acceptance_plan_command` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `ast_diff` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `ast_dump` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `audit_m8_emitter_dispositions` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `bind_corpus` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `build_m8_emitter_dispositions` | 5 | not-js-value 5 | All 5 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `cargo_test_executables` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `ci_oracle_gates` | 2 | not-js-value 2 | All 2 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `codegen_band_inventory` | 3 | not-js-value 3 | All 3 hit(s) are on bytes/rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `codegen_emitter_dispositions` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `combine_completion_probes` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `conformance_diff` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `display_relative` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `disposition_stats` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `escape_reason_after` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `escapes` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `expand_fixture` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `families_check` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `families_report` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `git_repository_root` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `goldens_diff` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `h2_5g_inventory` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `h2_5g_probe` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `h2_7b_acceptance` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `invariants` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `jsdoc_ast_diff` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `lib_gate` | 3 | not-js-value 3 | All 3 hit(s) are on bytes/rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `load_sample_programs` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `m8_emitter_dispositions_at` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `m8_git_is_ancestor` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `m8_readiness` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `m8_readiness_inner` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `m8_resolve_git_commit` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `mechanical_family_rows` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `nearest_planning_boundaries` | 4 | not-js-value 4 | All 4 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `oracle_smoke` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_ci_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_conformance_args` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_diagnostic_entry` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_diagnostics` | 2 | display 1, not-js-value 1 | Dev CLI that prints parse diagnostics of one file; the message text is rendered lossy to stdout only (display). |
| `crates/xtask/src/main.rs` | `parse_fuzz_preflight_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_invariant_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_semantic_history_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_string` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `parse_token_diff_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `port_plan` | 6 | not-js-value 6 | All 6 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `project_oracle_jsdoc_dump` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `project_oracle_jsdoc_ref` | 1 | not-js-value 1 | All 1 hit(s) are on serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `ratchet_check` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `ratchet_update` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `readme_status` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `render_readme_status` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `repository_relative_display_path` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `resolve_ci_baseline` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `run_encodings` | 1 | display 1 | Final human-facing rendering of a JS value (failure/stdout text) that feeds no comparison. |
| `crates/xtask/src/main.rs` | `run_prefix_determinism` | 2 | display 1, scalar-observer 1 | Mixed: categories display, scalar-observer on receivers js-value; see rows. |
| `crates/xtask/src/main.rs` | `rust_jsdoc_ast_dump` | 2 | scalar-observer 2 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/main.rs` | `rust_symbol_dump` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `scope_audit` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `symbol_diff` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `validate_d2_inventory` | 11 | not-js-value 11 | All 11 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `validate_m8_emitter_dispositions` | 8 | not-js-value 8 | All 8 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/main.rs` | `validate_sample_program_semantics` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/node_codegen.rs` | `collect_dts_nodes` | 5 | not-js-value 5 | All 5 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/node_codegen.rs` | `parse_dts_field` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/node_codegen.rs` | `render_for_each_child_rs` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/node_codegen.rs` | `render_observable_fields_rs` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/node_codegen.rs` | `rust_field_name` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/node_codegen.rs` | `schema_audit` | 11 | not-js-value 11 | All 11 hit(s) are on bytes/rust-str/serde-json receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `None` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `declaration_ref` | 1 | scalar-observer 1 | Acceptance/evidence observer over JS values (js-value); every conversion is an as_str().expect()/ok_or that PANICS or FAILS loudly on a lone surrogate, so no difference can be masked. |
| `crates/xtask/src/recovery_census.rs` | `region_dump` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `run` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `run_census` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `rust_file_dump` | 2 | scalar-observer 2 | Builds the recovery census file dump from the binder; escaped names and file names use expect() guards (panic on non-scalar). |
| `crates/xtask/src/recovery_census.rs` | `selected_cases` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/recovery_census.rs` | `shape_dump` | 3 | not-js-value 3 | All 3 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/relpin.rs` | `finish` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/relpin.rs` | `fixture_text` | 1 | not-js-value 1 | All 1 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/relpin.rs` | `gen` | 2 | not-js-value 2 | All 2 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/relpin.rs` | `rewrite_expects` | 1 | not-js-value 1 | All 1 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/relpin.rs` | `run` | 3 | not-js-value 3 | All 3 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/slice_evidence.rs` | `git_output` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/slice_evidence.rs` | `parse_snapshot_args` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/slice_evidence.rs` | `parse_verify_args` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/slice_evidence.rs` | `validate_before_manifest` | 2 | not-js-value 2 | All 2 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/symbol_audit.rs` | `audit_source_file` | 2 | suspect 2 | Renders per-name symbol audit lines (escapedName, members, exports) with to_string_lossy for the textual symbol-diff comparison with the oracle (suspect, low severity). |
| `crates/xtask/src/upstream_suites.rs` | `atomic_write_manifest` | 1 | not-js-value 1 | All 1 hit(s) are on os-path receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/upstream_suites.rs` | `parse_args` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_catalog.rs` | `discover` | 1 | not-js-value 1 | All 1 hit(s) are on bytes receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `automation_cargo_selector` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `cargo_command_index` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `cargo_subcommand` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `collect_automation_steps` | 1 | not-js-value 1 | All 1 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `collect_step_automation` | 3 | not-js-value 3 | All 3 hit(s) are on other receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
| `crates/xtask/src/workspace_maintenance.rs` | `run_workspace_command` | 1 | not-js-value 1 | All 1 hit(s) are on rust-str receivers (evidence JSON, CLI arguments, process output, or native paths); no JS value is converted. |
