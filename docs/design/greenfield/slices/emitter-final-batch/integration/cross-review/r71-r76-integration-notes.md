# Native r71 and repairs r72–r76 (qualification pending)

Frozen native r71 source: `5f32b6041`. Receipt: `records/local/system-factory-native-r71.json`.

| Complete-command corpus | Exact | Failed | Selected |
|---|---:|---:|---:|
| Async-arrow body ranges | 372 | 0 | 372 |
| Await boundaries | 598 | 6 | 604 |
| Export destructuring and adjacent controls | 1098 | 32 | 1130 |
| System binding publication and adjacent controls | 816 | 42 | 858 |

The namespace contract also passed. The test process exited 101: 2 test functions passed, 3 failed. No failed row is retired or qualified by this run. `native-r71-differences.json` retains decoded first-failure observations; map mismatches can precede and conceal JavaScript mismatches.

Repairs agreed with actual Claude Opus in rounds 72–76:

- The TypeScript ES5 class wrapper adds `NoTrailingSourceMap` only when the original class facts include initialized static properties. Member decorators alone must not inherit that flag.
- Static-block printing ignores parser-attached modifiers. The System ContainsAwait projection visits its body only, as the upstream factory does.
- Import-equals setter targets preserve their generated binding identity. This repairs the `tslib` versus `tslib_1` regression after replacing spelling-keyed temporary registration.
- A variable initializer hoisted by the ESNext using transform keeps a synthetic text range and receives the declaration's explicit comment/source-map ranges. This prevents an automatically inserted parenthesis from claiming those maps.
- CommonJS import-first export lookup applies the existing direct-storage filter to internal export-import aliases, avoiding duplicate own-name publication.
- Legacy metadata compares serialized nodes using the upstream structural rules, including temporary equivalence. Distinct unknown unions collapse to Object while still allocating both constituent temporaries. Qualified property names retain their parsed name ranges.

Controls: await 604 → 690, System 858 → 1044. Previous case objects must remain identical; new expected outputs are collected through two equal complete TypeScript 6.0.3 commands. These are fixture counts, not a native pass claim. Current native validation is still pending. CI expects 1734 complete System/await observations, in addition to separate export-destructuring and other witness groups.

The first emitter library test attempt found a missing SyntaxKind qualification in the previously added MissingDeclaration unit test; only that enum reference was fixed before rerunning. Preserve both receipts.

Recovery qualification is separate: no original 36 parse-recovery KNOWN rows have been retired. The parser replay tool compiled against current, pre-projection, and merge-base parsers; all three graph mutation controls passed in each build. Current 285 statement shapes and diagnostic code/ranges match TypeScript. Full corpus census and complete-command qualification remain required.

The 24 final metadata controls preserve escaped qualified names. The emitter library passed 510/510 before this spelling amendment; all final complete-command comparisons remain pending.
