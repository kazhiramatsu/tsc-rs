# Native configuration name order

These inputs and expected CLI bytes were observed with the pinned tsgo 7.1
binary. Its SHA-256 is in `cases.json`. Reobserve with:

```sh
python3 record.py /path/to/pinned/tsc
```

The recorder verifies the inputs, captures the native filesystem case profile,
and records `--showConfig`, `--listFilesOnly`, and ordinary diagnostic output.
Only the fixture's absolute directory prefix becomes `<fixture>` in expected
text; other bytes and exit status are preserved. The Rust root-plan regression
uses a memory host with that recorded case profile, so it remains portable.
Complete native CLI comparisons use the capture's filesystem profile.

`directory` distinguishes BMP and supplementary names, ASCII case, combining
marks and literal-file priority. `buckets` keeps the requested b/a include order
even though native directory enumeration visits a first. `case-alias` pins the
value replacement and insertion position of literal names differing in case.
`diagnostic` changes the TS2717 primary declaration when the two Unicode roots
are reversed; its expected error is on U+10000.ts.

The native host uses byte order for directory display names. Include base paths
use the native path comparer; root-plan assembly retains literal and include
bucket order. A final global filename sort would violate these fixtures.

CFG1b's 24 frozen input cases have a separate native root-order recording in
config-discovery-roots.json. Its recorder is record_config_roots.py; run it
with the same pinned executable. It preserves both original JSON files and
verifies the native root multiset before recording. Only the root-order field
is substituted in the historical integration observation; diagnostics, options
and other fields keep their original independent reference.
