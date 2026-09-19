# Round135: noEmit census commands

The two bundlerImportTsExtensions noEmit=true inputs were refused by the original emit-only census loader. The original snapshot and all110load-failure dispositions remain unchanged. r147 derives identical parse tuples from their loaded noEmit=false siblings; this is parse coverage, not a native command measurement.

Actual Opus135 found the existing test observer already selects load_program when noEmit=true. A focused emitter_final_batch entry now compares just these two original input identities to the frozen TS whole-command tuples twice, including diagnostics, empty writes, status, exit and emit result. It does not widen the public loader API, candidate owner admission or other corpus selection. The witness registry explicitly includes the two IDs and all original inputs.

Preparation checks: cargo fmt and witness input/membership checks. Native measurement is pending the serial Cargo queue; this commit makes no qualification claim.
