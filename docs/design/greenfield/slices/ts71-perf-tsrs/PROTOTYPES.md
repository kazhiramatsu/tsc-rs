# 性能の作業の試作一覧（2026-11-01〜11-06）

main に merge しなかった branch を、何を試して結果がどうなったかとともに並べる。GitHub 上の名前は「branch」の列（`archive/2026-11/` の下にまとめて push した；元から push 済みのものは元の名前）。採用された card の最終形は main の merge commit にあり、ここに並ぶのはその試作・段階・比較用の build。数字と判断の根拠は README の計測台帳と `target/claude-tools/handover/perf-tsrs/` の各 gate の記録（local の archive `~/dev/tsc-rs-archive/2026-11-06/`）。

種類：card = 最後の card の branch、pilot = 効果を測るための試作、card branch = 途中で止めた card、observation = 数を取るための観測用 build（merge しない前提）。

| 種類 | 日付 | branch | commit | 試したこと | 結果 |
| --- | --- | --- | --- | --- | --- |
| card | 2026-11-06 | `perf/ts71-shared-paths` | `4a79b2302` | card (a): paths share their bytes (SharedJsString) | adopted as #910 (the last card) |
| pilot | 2026-11-05 | `archive/2026-11/m8/batch-attach` | `5877accc9` | attach-from-data variant in the batch | folded into #895 |
| pilot | 2026-11-05 | `archive/2026-11/m8/batch-card10` | `608939e31` | card 10 v1 in the parse batch | superseded by v2 |
| pilot | 2026-11-05 | `archive/2026-11/m8/batch-card10b` | `83a51326b` | card 10 v2 in the parse batch | adopted as #895 |
| pilot | 2026-11-05 | `archive/2026-11/m8/inline-finish` | `1a5f4075c` | inline the node finish path (v1) | superseded by v2 |
| pilot | 2026-11-05 | `archive/2026-11/m8/inline-finish-v2` | `163377c34` | inline finish v2 | adopted as #896 |
| pilot | 2026-11-05 | `archive/2026-11/m8/intern-pilot` | `82a33f690` | parser name cache | adopted as #897 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-bind-inline` | `30f67eef9` | binder per-node helpers inline (v1) | superseded by v3 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-bind-inline2` | `b797b0202` | eight more helpers inline (v2) | dropped (no gain) |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-bind-inline3` | `5e8b17570` | v1 + tsgo bindChildren (v3) | adopted as #900 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-bind-inline3-cli` | `f6b46cd1a` | v3 as a CLI build | adopted as #900 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-buffer-pool` | `4919c247a` | keep released list mappings per thread for reuse | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-flow16` | `f021f6c23` | F10a v2: 16-byte flow nodes, kind-specialised readers | rejected (instructions +197 M) |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-no-attach` | `e3d89d4e4` | parse batch without parent attachment (bound) | bound for card 10, adopted as #895 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-node-symbols` | `7f8acc1dd` | node_symbol index V1 | parked (−40 MiB but +855 M) |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-symbol-rooms` | `ff0bada15` | symbol rooms reserved by a parse-time bound | adopted as #901 |
| pilot | 2026-11-05 | `archive/2026-11/m8/pilot-text-noutf8` | `bf8de1130` | literal texts without UTF-8 revalidation | not carded (≈ 0.09 G, below the line) |
| pilot | 2026-11-05 | `archive/2026-11/m8/reloc-k1` | `5d010ca57` | relocation variant k1 | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/m8/reloc-k2` | `e142072e3` | relocation variant k2 | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/m8/reloc-k3` | `5de88557a` | relocation variant k3 | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/m8/reloc-table` | `0c3a569d7` | relocation through per-kind slot tables | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/m8/reloc-table2` | `1b096b043` | tail records through the per-kind slot layout | not carded (see ledger) |
| pilot | 2026-11-05 | `archive/2026-11/pilot/no-await-walk` | `a06ad8dd8` | bound for card 9 (top-level await spans) | adopted as #894 |
| pilot | 2026-11-06 | `archive/2026-11/bin-fer1-on-main` | `952dd3869` | FE-R1 alone on main (interaction measurement) | FE-R1 adopted as #908 |
| pilot | 2026-11-06 | `archive/2026-11/bin-probe-a` | `d9f1020de` | identity probes stage (a): conditional-branch cache | adopted in #909 |
| pilot | 2026-11-06 | `archive/2026-11/bin-probe-ab` | `03d924783` | stage (b): identity fast path | adopted narrowed in #909 |
| pilot | 2026-11-06 | `archive/2026-11/bin-probe-abc` | `f117b6de6` | stage (c): normalize dispatch | adopted in #909 |
| pilot | 2026-11-06 | `archive/2026-11/bin-probe-abc2` | `c583f2bdf` | stage (b'): fast path only with one conditional side | adopted as #909 |
| pilot | 2026-11-06 | `archive/2026-11/bin-purge-100` | `af27aebc3` | mimalloc purge delay 10 → 100 ms | rejected (no gain) |
| pilot | 2026-11-06 | `archive/2026-11/bin-text-heap-card` | `f45a0d705` | FE-T1 card before the final squash | adopted as #907 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-extras-box` | `8c38102b1` | SymbolExtras 104 → 40 B (box on first entry) | parked (−2.6 MiB against −21) |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-flow-combined` | `6c6d280ab` | F10 combined v2: flow in store rooms + per-thread scratch | rejected (memory −12 MiB only) |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-flow16-on901` | `9d66b058a` | F10a v2 on #901 | rejected (instructions) |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-flow16b` | `444183ec7` | F10a'': re-home frozen flow at into_bind_data | led to F10a''', adopted as #902 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-links16` | `c8e4c69f9` | transient symbol records 16 B (name type in a map) | rejected (−67 MiB but +1.06 G) |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-links20` | `28c5675ee` | transient symbol links 20 B | adopted as #904 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-node-flow` | `e2633798d` | node.flowNode compact per-file form | rejected (−45.8 MiB but +860 M) |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-node-flow16` | `32c20b3a8` | node.flowNode two-byte slots | adopted as #903 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-small-maps` | `32a09ea81` | symbol tables below 9 entries without a hash index | adopted as #906 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-text-heap` | `c87d76e52` | FE-T1 pilot (a): SharedText enum design | superseded by the card, adopted as #907 |
| pilot | 2026-11-06 | `archive/2026-11/m8/pilot-text-threshold` | `3aed63493` | FE-T1 t8k: texts ≤ 8 KiB in the default heap | rejected (cost 25 MiB of the saving) |
| pilot | 2026-11-06 | `archive/2026-11/m8/proto-text-heap` | `f96fecddf` | FE-T1 bound: source texts in their own heap (routed allocator) | bound −201 MiB; card #907 |
| card branch | 2026-11-01 | `archive/2026-11/wip/perf-check-abort` | `4a7d69d67` | WIP: uninhabited CheckAbort | not pursued |
| card branch | 2026-11-04 | `archive/2026-11/perf/ts71-direct-attach` | `ac43d66dc` | parents attached through a generated child-edge visitor | withdrawn (measured +0.7%) |
| card branch | 2026-11-04 | `perf/ts71-c0-step2` | `6dd36c387` | C-0 flow memo step 2 | stopped (C-0 memos stopped 11/04) |
| card branch | 2026-11-04 | `perf/ts71-r29b` | `501c9dc1c` | R29 steps 4–7: Go sort history at the remaining sites | folded into the data-layer plan (S1) |
| card branch | 2026-11-04 | `perf/ts71-r29b-p2` | `deb3eeeab` | R29 steps 4–7, second attempt | folded into the data-layer plan (S1) |
| observation | 2026-11-04 | `archive/2026-11/scratch/c0-step3-measure` | `77c2aa34d` | C-0 flow memo enabled (step 3 measurement build) | C-0 stopped 11/04 |
| observation | 2026-11-04 | `archive/2026-11/scratch/c0-table-14` | `e30b19e06` | C-0 flow memo, 1<<14 slots (table-size measurement) | C-0 stopped 11/04 |
| observation | 2026-11-04 | `archive/2026-11/scratch/c0-table-16` | `be1d4652e` | C-0 flow memo, 1<<16 slots (table-size measurement) | C-0 stopped 11/04 |
| observation | 2026-11-04 | `perf/ts71-b2-census` | `ee42c3045` | list-copy census counters | — |
| observation | 2026-11-04 | `perf/ts71-b2-census-p2` | `ee42c3045` | list-copy census counters (copy) | — |
| observation | 2026-11-05 | `archive/2026-11/m1-observation` | `6d72adb7b` | M1 observation patch (phase ledger) plus the integrator amendment | M1 ledger (README 計測台帳) |
| observation | 2026-11-05 | `archive/2026-11/m8/batch-main` | `0cb00fa3f` | parse batch, base | — |
| observation | 2026-11-05 | `archive/2026-11/m8/binder-map-counts` | `ba0a4a36f` | binder's dense per-node maps | — |
| observation | 2026-11-05 | `archive/2026-11/m8/flow-count` | `1b86f0964` | checker's flow reads | — |
| observation | 2026-11-05 | `archive/2026-11/m8/intern-batch` | `d0d6420cf` | identifier interning batch | — |
| observation | 2026-11-05 | `archive/2026-11/m8/intern-profile` | `6c66aa26c` | interning profile | — |
| observation | 2026-11-05 | `archive/2026-11/m8/maps-count` | `96da36765` | binder per-node maps by reader | — |
| observation | 2026-11-05 | `archive/2026-11/m8/parse-batch` | `ac9ff83e9` | paired parse batch (instructions per node) | — |
| observation | 2026-11-05 | `archive/2026-11/m8/parse-counters` | `6528ee886` | parser counters for the M8 parse table | — |
| observation | 2026-11-05 | `archive/2026-11/m8/reloc-batch` | `01bc3a215` | relocation batch (parse vs parse + publication) | — |
| observation | 2026-11-05 | `archive/2026-11/m8/store-slack` | `fc8f31655` | parse-time bound for symbol rooms | informed #901 |
| observation | 2026-11-05 | `archive/2026-11/m8/textkey-sites` | `212b530af` | text-key lookup sites for the names card | informed #898 |
| observation | 2026-11-06 | `archive/2026-11/bin-diag-base` | `06d762bd8` | FE-R1 count form, base | — |
| observation | 2026-11-06 | `archive/2026-11/bin-diag-count` | `d0bae74e3` | hot-file lead counts (conditional probes, normalizations) | informed #909 |
| observation | 2026-11-06 | `archive/2026-11/bin-diag-r1` | `cd3258a6e` | FE-R1 count form, card | — |
| observation | 2026-11-06 | `archive/2026-11/bin-hw-r1` | `cec0cd248` | heap walk on FE-R1 | — |
| observation | 2026-11-06 | `archive/2026-11/bin-hw-t1` | `e2590429f` | heap walk on FE-T1 | — |
| observation | 2026-11-06 | `archive/2026-11/m8/alloc-census-d640` | `b47118ef2` | allocation-site census on main d640a5852 | — |
| observation | 2026-11-06 | `archive/2026-11/m8/base-store-report` | `0d3204265` | store mincore report, base | — |
| observation | 2026-11-06 | `archive/2026-11/m8/diag-instr-main` | `92f219c15` | instructions retired per phase mark (base) | — |
| observation | 2026-11-06 | `archive/2026-11/m8/diag-instr-nf1` | `ddc9ac3ec` | per-phase instructions, node-flow pilot 1 | — |
| observation | 2026-11-06 | `archive/2026-11/m8/diag-instr-nf16` | `6b808af11` | per-phase instructions, two-byte slots | — |
| observation | 2026-11-06 | `archive/2026-11/m8/diag-instr-nf16b` | `4d24c5361` | per-phase instructions, two-byte slots b | — |
| observation | 2026-11-06 | `archive/2026-11/m8/diag-instr-nf2` | `6fd7e04a0` | per-phase instructions, node-flow pilot 2 | — |
| observation | 2026-11-06 | `archive/2026-11/m8/fer1-p2-bound` | `700dacc74` | FE-R1 phase 2 exact bound; any-check counts | phase 2 not carded (11.4 MiB) |
| observation | 2026-11-06 | `archive/2026-11/m8/flow-combined-store-report` | `3299ab3bf` | store mincore report, combined flow | — |
| observation | 2026-11-06 | `archive/2026-11/m8/heap-walk-bind` | `15cfc673a` | heap walk after load, adopt, bind and check | — |
| observation | 2026-11-06 | `archive/2026-11/m8/links-census` | `43e198f14` | transient symbol links census | informed #904 |
| observation | 2026-11-06 | `archive/2026-11/m8/mem-account-d640` | `704e914ba` | M7 memory account on main d640a5852 | — |
| observation | 2026-11-06 | `archive/2026-11/m8/node-census` | `da8b43218` | node record census for N28 | N28 not started (NO-GO for now) |
| observation | 2026-11-06 | `archive/2026-11/m8/paths-diag` | `9091c1d72` | card (a) memory diagnosis (heap walk, forced purge) | — |
| observation | 2026-11-06 | `archive/2026-11/m8/phase-instr-base` | `e9ca299c5` | per-phase instructions and allocations, base | — |
| observation | 2026-11-06 | `archive/2026-11/m8/phase-instr-card` | `6f0d55653` | per-phase instructions and allocations, card (a) | — |
| other | 2026-11-03 | `archive/2026-11/perf/ts71-frontend-prep` | `5609c3756` | Merge remote-tracking branch 'origin/main' into perf/ts71-frontend-prep | see the ledger |
| other | 2026-11-03 | `archive/2026-11/perf/ts71-small-cuts` | `ef5da50d5` | memory report: count the union, intersection and indexed-access maps (an assignment droppe | see the ledger |
| other | 2026-11-04 | `archive/2026-11/perf/ts71-r29` | `db61521b9` | types: Go's union-builder input and binary search (R29 steps 1-3: goslices module, append- | see the ledger |
| other | 2026-11-04 | `perf/ts71-b5-steps-3abc` | `a0fc19b2f` | checker: completed base-type caches persist across overload candidates (B-5 step 3c) | see the ledger |
| other | 2026-11-04 | `perf/ts71-m4-cuts` | `9504acdeb` | checker, syntax: the instruction audit's small cuts — hoisted flow file-index checks, inli | see the ledger |

Codex lane の成果物（patch・報告・SPEC）は branch ではなく `handover/perf-tsrs/*-lane-record/` にある：r0（rayon：不採用）、fer1（→ #908）、dup4・rel1・sl1・m4b（観測）、ic1（途中で停止）、det1（→ #905）ほか。
