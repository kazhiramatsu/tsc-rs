The census capture is ready for the 80-minute run as it stands; I found nothing that blocks it. The guards I suggest below belong in the native consumer and don't change what the census records. I did not re-review the emitter code, as you asked, and have no concrete wrong condition to report there. This review was read-only: I built nothing and edited nothing.

## Shared serializer extraction (census HEAD `fcbbd8a5b`)

- **Fields match the old serializer.** `limits`, `limits_input`, `symlink_input`, `prepared_summary`, `artifact_input` and both `plan_input` arms have the same keys, the same order and the same value expressions. The one intended difference is descriptor storage: `descriptor_*_base64` is replaced by pooled `descriptor_utf8.content_sha256` and `descriptor_raw.content_sha256`.
- **Hashes are identical.** The harness-local `sha256` produces the same `{:x}` SHA-256 hex as `parse_snapshot::sha256`, so the source file `sha256` values and the document ids don't change.
- **The document sink is a hash-only interface.** The census sink pools base64 bytes, and a native hash-only sink will produce the same ids.
- **Both descriptor forms still land in the pool.** When the UTF-8 and raw descriptor bytes are identical they collapse to one pool entry, which is still lossless.
- **Qualified and candidate rows embed their full input and settings inline**, so those rows are also lossless without the pool.

## Reconstruction guards for the native consumer

These don't need census changes.

1. **Make the index key unique.** Build the key from `fixture_path` + `fixture_blob_sha1` + `variant.key` + `variant.configuration_index`, or from `descriptor_path` + `descriptor_blob_sha1` + `module_variant` + `scenario`. Refuse to proceed if two recorded plans produce the same key.
2. **Check the identity after lookup.** Compare the found plan's `provenance.case_id` with the row's `case_id`. Use it only as a check, never as the lookup key, so there's still no case-id fallback.
3. **Reload with the recorded `loader` string exactly.** A `*_no_emit` row must never be reloaded with the emit loader.
4. **Assert on the reloaded program.**
   - The serialized JSON must be byte-identical, including `prepared` and `limits`.
   - Every `content_sha256` must resolve to pool bytes with that hash.
   - `prepared.roots` and each source file's path and `sha256` must match the reloaded program.
5. **Qualified and candidate rows:** pass the embedded `command_input.input` (and `settings`) directly to the moved qualified loader. Assert `route`, the `universe` artifact path, and `use_case_sensitive_file_names`.
6. **Option debug hashes:** treat a mismatch in `compiler_options_debug_sha256` or `program_options_debug_sha256` as "census binary differs, rebuild the census", not as a row divergence.

## NoEmit-fallback handling

- **What's already correct:**
  - `emit_load_error` is set only on the fallback path.
  - Fallback rows are labelled `parse-admission-only; emit-not-qualified`.
  - Rows where both loaders failed are labelled `not-loaded; emit-not-qualified`.
  - The selector asserts the fallback reason and counts selected fallbacks separately.
- **One-line selector addition:** also assert the converse. A row with an emit loader should have `emit_load_error is None` and `emit_disposition == "pending-complete-command-comparison"`. That stops a future loader rename from silently promoting a fallback row. It's a selector-script check only and doesn't block the capture.
- **Portability note:** `emit_load_error` is the error's `to_string()`. If it contains absolute workspace paths, the snapshot won't be byte-portable across machines. If you want to fix that, normalize the path before the run; it doesn't affect correctness.
- **The native consumer's tally must keep these rows separate.** Put selected NoEmit rows and all double-failure rows in their own dispositions, and never count them toward emit qualification. Your point stands that an original row may have `may_be_emitted` false for some units: parse admission for those rows covers every non-JSON source parsed through module requests, and says nothing about emit.