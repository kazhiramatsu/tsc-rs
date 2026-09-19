# Standalone parser replay feature parity

The completed r78 census accounts for all 14,329 claimed IDs: 14,219 loaded, 110 explicit load failures, and 16,994 distinct parse inputs. Its original snapshot remains SHA256 `1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75`. This is parse coverage, not complete-command emission qualification.

The first standalone replay failed before parsing its first input because its generated manifest omitted `serde_json/preserve_order`, enabled in the original workspace. Input IDs and graph digests hash JSON serialization bytes. The manifest now selects the original feature, and a literal original input guards against recurrence. The shared digest source, original snapshot, parser sources, and all identity/digest assertions are unchanged.

The added regression test was run with the previous feature set in a separate build directory. It failed with exit 101 and the same input-ID mismatch (`0000b94d…` versus `e9f1c740…`). Its log and the read-only census feature tree are retained under `records/`. An initial cargo invocation put `--exact` before the argument separator and exited 1 without running tests; the corrected negative-control invocation is the retained regression log. Opus round 126 independently reviewed both digest paths and the literal-input guard. Full positive replays and the selector remain required before changing any KNOWN disposition.

The failed r126 replay and r129 candidate's blocked-before-build receipts are retained with the original census archive. The retry uses a fresh directory and does not replace those artifacts.
