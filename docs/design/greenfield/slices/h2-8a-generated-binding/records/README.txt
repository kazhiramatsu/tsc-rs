C02 / A41-BINDING records (2026-09-16, base ccb6661c16f75cd6824e9fedb9281be68d01e542)

start.txt                    start manifest (HEAD, toolchain, vendor hashes)
source-spans.json            49 pinned _tsc.js spans (line ranges + SHA-256)
candidate.patch(.sha256/.stat)  crates/ + scripts/ diff from the base at the FINAL candidate bytes (binary fixtures included)
observer-pipeline-write.log / -check.log (+ .exit)   upstream observation of the 768-case manifest, and the independent re-observation (byte-identical)
observer-direct-write.log / -check.log              upstream observation of the 96 direct rows, and its re-check
before/   base binary (pipeline-binary.sha256) and base runs: lib-* unit tests, printer suite, direct contract (+ direct/ native observations), pipeline replay (exact 688 / failed 79) with capture-diff.txt
after/    candidate 1 (the 3 allowed files): direct contract 137/144 (7 KNOWN), adjacent suites, clippy, pipeline subsets 293/336 and the full replay 723/767 (capture-diff-*.txt, pipeline-binary.sha256), runtime-events-computed.json
after2/   candidate 2 (separated diffs: carry, system.rs, CJS, downlevel maps, class_fields): direct 142/144, adjacent suites incl. literal-update and SUPER subsets, pipeline subsets 426/452 (capture-diff-subsets.txt)
after4/   candidate 3 (adds the parsed-metadata internal flags, accessor-name maps, receiver clone range): direct 142/144, printer 25/25, adjacent suites, pipeline family subsets 434/452 (capture-diff-subsets.txt). Its full replay was stopped when decision 15 was found (no artifacts kept)
after5/   FINAL bytes (adds decision 15, descriptor forwarder range): lib-* unit tests, direct contract 142/144 (2 KNOWN dispose), adjacent suites (printer, direct, decorator-binding, literal-update, emitter contracts 451+1 inherited, SUPER followup3/extra/followup2/primary subsets), focused pipeline subset 30/30, the one full replay (pipeline-contract-full.log, pipeline-binary.sha256, capture-diff-full.txt), clippy (allow mode), fmt
Raw per-case captures (TSC_RS_H2_8A_CAPTURE_WRITES_DIR) are archived outside the repo and are regenerable; only the capture-diff-*.txt summaries are kept here.
