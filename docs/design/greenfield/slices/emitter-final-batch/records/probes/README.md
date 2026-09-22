# Direct tsc / tsc-rs probes (start SHA `c35e00ccb`, not qualification evidence)

Scratch inputs run through the vendored `_tsc.js` and the start-SHA `target/debug/tsc-rs` CLI
(`-p tsconfig.json`), diffed on the emitted `out/main.js`. They locate causes; they are not
frozen observations (no repetition proof, no callback metadata) and add nothing to any count.

- `probe1/`: ten parse-tree access shapes with same-line trailing comments (`a.b /* t */;`,
  `a.d /* t4 */()`, `[a.b /* t6 */]`, …), ES2022. tsc-rs output identical to tsc → the printer's
  parsed-token boundaries claim these ends (EF1 gap matrix, already-exact rows).
- `probe2/`: twelve transform shapes around access expressions with trailing comments
  (class field initializer, ES5 arrow body, await, destructuring default, for-of, template,
  optional chain, CommonJS export, enum, namespace, standard decorators on class and member).
  `diff-A.txt` (ES2022): only the two decorator rows differ (EF1). `diff-B.txt` (ES5): the two
  decorator rows (EF1), the for-of expression range comments `/* t5 */` (converted loop; EF2
  candidate cause), the destructuring default comment printed twice (flattener; EF2 candidate
  cause), and the decorated ES5 class temp name `class_1` vs `D_1` (EF2 candidate cause).

## Later rounds (r5–r7; `rs/out` regenerated at the final bytes by chain8, see `diff-r7.txt`)

- `probe3/`: EF4/EF5 class shapes (`legacy-bound-this`, `escaped`, `direct-escaped`,
  `nested-computed`, `concise-arrow`); all identical at r7 except `direct-escaped`
  (EF5-BARE-CLONE-SPELLING residue, DESIGN §7.1).
- `probe6/dec`: the block-scoped decorated class of `decoratedBlockScopedClass2`; r7 leaves only
  the alias ordinal (`Foo_1` vs `Foo_2`, EF2-ALIAS-NUMBERING). `probe6/nodec` (no decorator) was
  identical from r4.
- `probe7/`: `read-comment` (EF2-READ-COMMENT) and `async-gen-super` (ES2018 capture structure +
  EF2-ASYNC-GEN-BODY-FLAG); both identical at r7.
- `probe8/imported-promise`: the two-file shape of `asyncImportedPromise_es5`
  (EF2-ASYNC-ALIAS-MARK); `test.js` identical at r7.
- `probe9/`: `field-arrow` and `legacy-static-block` (fixture sources of the EF4 rows);
  `field-arrow` identical at r7 after EF4-FILE-THIS-CAPTURE.
- `probe10/`: `iterable-es5` / `iterable-ref` (EF3-ITERABLE-2318): tsc reports the global
  TS2318 for `let [...x] = [array literal]` under `lib: es5`, tsc-rs does not (TS2304 for a
  direct `Iterable` reference is reported by both, so the lib is honored).
- `probe11/P1…P7`: lexical-`this` capture variants (top-level arrow, class declaration wrapper,
  class expression with/without arrow, arrow returning a class); localized
  EF4-FILE-THIS-CAPTURE to the class-expression static initializer path (P3 shows tsc itself
  does not capture inside the class wrapper) and EF2-ARROW-BLOCK-ORDER (P7).

## r8 rounds (`rs/out` regenerated at the r8 candidate bytes; every listed probe identical to tsc)

- `probe12/for-of37`: the `ES5For-of37` shape (a leading detached comment before a `for-of`
  that ES2015 lowers into a synthesized `try`/`finally`); EF2-DETACHED-COMMENT
  (`carried_source_detached`).
- `probe13/self-import`: `module.exports = class {}` with a JS declaration emit whose `.d.ts`
  prints `import(".")` (EF6-IMPORT-TYPE-SELF, `class_expression_assignment_container`);
  diffed on `index.d.ts`.
- `probe14/using`: `for await (await using of x)` at top level and inside an async function
  (EF2-AWAIT-USING-MISSING-NAME: synthesized temp `_e`, derived `_e_1`, `e_4_1` ordering).
- `probe15/outfile`: the CLI attempt at the `sourceMapWithNonCaseSensitiveFileNames` shape;
  the scratchpad file system is case-sensitive (tsc: TS6053), so the candidate was compared
  through the harness route instead (`records/oracle/case-canonical-proposal.md` §4).
- `probe16/A…E`: array `for-of` loops inside a generator or an async function at ES5
  (`function* g() { for (const x of [1, 2]) yield x; }` and the `await using` variants);
  EF2-LOOP-VARIABLE-POLICY (`var _i, _a, …` was `var _a, _b, …`). Identical after
  `from_existing(loop_variable)` + `merge_from` propagate the `_i` policy.
