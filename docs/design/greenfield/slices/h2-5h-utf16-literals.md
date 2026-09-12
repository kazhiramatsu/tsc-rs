# H2.5h ca-2a-r2: UTF-16 literal value fidelity at ES5 — design and execution record

Design and execution record for the slice selected on 2026-09-12 in
`target/next-slices-20260912/claude.md` (Claude's lane): the four rows of
`ratchets/h2-5h-known-divergences.v1.json` owned by `h2-5h-ca-2a-r2`. The
claim of this record is that the four rows and the prepared witness commands
reproduce their frozen TypeScript 6.0.3 tuples. It is not a claim that every
literal-producing transform is lossless, that H2.8a/A6-41 is complete, or that
NC1 → MOD1 → transpile are unblocked; the A-close dependencies are unchanged.

## Start point

- Worktree `/Users/hiramatsu/dev/tsc-rs-utf16-literals`, branch
  `work/h2-5h-utf16-literals`, cut from the verified PR #518 candidate
  `a992ffcc4fd4094f723fafa019925caa8ed48876` (tree
  `e18cf613f6e39c63f9f1e83ec0caa88cb5264051`; hosted acceptance run
  34689556318 green; merged to `main` as `3757da2f6` with the identical
  tree while this slice was in its design gate). The start head is fixed;
  `main` is merged only when the integrator opens the next PR.
- Dedicated target `target/utf16-literals-acceptance`, run directories under
  `target/utf16-literals-runs/` (`tools/run-rows.sh`, `tools/run-witnesses.sh`,
  `tools/diff-captures.py`, `tools/tsc-hash.py`, `tools/shrink-manifest.py`).
  Heavy commands run one at a time, `taskpolicy -b nice -n 15`,
  `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, test threads 1. No walk, no
  chain-walk, no `cargo xtask ci`, and no local `cargo xtask acceptance`
  (the integrator runs it at merge); no PR from this lane.

## Rows and the start-head re-capture

All four rows are `target=es5`; on the manifest only the JavaScript write
diverges. The replay is the independent test target
`crates/compiler/tests/h2_5h_utf16_literal_rows.rs` (registered by Cargo's
test auto-discovery, not in the shared `contracts.rs` module list): each row
is prepared through the H2.5h runner's qualified-vfs route
(`load_qualified_compiler_emit_with_option_floor`, `EmitOptionFloor::Established`),
emitted twice with the harness lib bundle, and compared on the complete
tuple — every write's path, callback bytes and byte-order mark in order, the
reported diagnostics, the emit result (`emit_skipped`, diagnostics,
`emittedFiles`/`sourceMaps` presence, status writes) and the exit code —
against `ratchets/h2-5h-qualification.v1.json` (whose two TypeScript
fingerprints are re-checked). `TSC_RS_H2_8A_CAPTURE_WRITES_DIR` retains both
complete runs with decoded texts; `TSC_RS_UTF16_LITERAL_CASE_FILTER` narrows.

Baseline run `target/utf16-literals-runs/baseline-r1/` (head `a992ffcc4`,
binary sha256 `5be7a91d…`): 4/4 diverging, JS write only; diagnostics
(5107 at ES5), emit result and exit code 2 already exact.

| row (`#target%3Des5`) | source | TypeScript | Rust at the start head |
| --- | --- | --- | --- |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings10.ts` | `var x = "\u{D800}";` | `var x = "\uD800";` | `var x = "�";` |
| `…/unicodeExtendedEscapesInStrings11.ts` | `var x = "\u{DC00}";` | `var x = "\uDC00";` | `var x = "�";` |
| `…/unicodeExtendedEscapesInTemplates10.ts` | `` var x = `\u{D800}`; `` | `var x = "\uD800";` | `var x = "�";` |
| `…/unicodeExtendedEscapesInTemplates11.ts` | `` var x = `\u{DC00}`; `` | `var x = "\uDC00";` | `var x = "�";` |

## Upstream map (pinned `_tsc.js`, spans read verbatim)

| function | tsc-span | tsc-hash (sha256 of the span + `\n`) | producer/consumer role |
| --- | --- | --- | --- |
| `scanString` | 8983-9016 | `7afc9ff5ff2b72c76e2e6ef96721751c0bbecd6511d50352b0c7cf2ddb485120` | string token value: raw segments + `scanEscapeSequence(String \| ReportErrors)`; unterminated at EOF/line break |
| `scanTemplateAndSetTokenValue` | 9017-9065 | `5aa19330e93ee867ee5fd6aa6efe63a7e59b67b2d295dd6b18432f528d402e13` | template cooked value: escapes with `ReportErrors` only when `shouldEmitInvalidEscapeError` (untagged); raw CR/CRLF → LF |
| `scanEscapeSequence` | 9066-9204 | `ce2f0c9b1f13cd4b2d3e21a795a6cb494918146c8553f1ead1969bc0f1c4e54b` | branches: `\0`/octal (report → `String.fromCharCode(code)`, else raw), `\8`/`\9` (report → digit, else raw), single-char escapes, `\u{`, `\uXXXX` (malformed → raw slice), `\xXX` (malformed → raw slice), line continuations → `""`, default → the char |
| `scanExtendedUnicodeEscape` | 9205-9246 | `db627f5a9a3263792b74d5b09c528c4a42ecf5da7e18c3532b232abac2f35508` | valid → `utf16EncodeAsString(value)` + `ExtendedUnicodeEscape`; no digits / > 0x10FFFF / unterminated → `ContainsInvalidEscape` + raw slice |
| `utf16EncodeAsString` / `…Fallback` | 11213-11215 / 11203-11211 | `9b417f90…7505ae6` / `956ec1ca…d02ce4b` | `String.fromCodePoint`: a lone surrogate code point is a lone UTF-16 unit |
| `parseLiteralLikeNode` / `getTemplateLiteralRawText` | 30649-30672 / 30644-30648 | `87eabc7e…88f5ad1c` / `562a5678…dc755aa26` | `createStringLiteral(tokenValue, undefined, hasExtendedUnicodeEscape)`; template fragments carry `rawText` |
| `createStringLiteral` | 21529-21534 | `2bf21e80bf4e61e4e1af7273cc968a2d4423ba01535d7cedc31a7ed35ebc1c2e` | synthetic node: `text` (lossless JS string), `singleQuote` undefined |
| `createTemplateLiteralLikeNode` | 22885-22890 | `4d36f6cd637eb6babb29850129ab9b8a3bfea4f9e238b375705907258faf9a2b` | `text` + `rawText` |
| `cloneNode` | 24436-24466 | `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6` | own properties copied (`text` lossless); `pos`/`end` stay -1; `parent` unset |
| `visitStringLiteral` (es2015) | 107915-107920 | `681f6439ed00ed909b262b464c9792b22130db1231779524d766d3414928d92e` | `hasExtendedUnicodeEscape` → `setTextRange(createStringLiteral(node.text), node)` |
| `visitTemplateLiteral` (es2015) | 107912-107914 | `fde517b62bb80b6d1d22dd2d9dcf537b52c27350c8865d8037e4e9b105c88fe0` | every fragment → `createStringLiteral(node.text)` |
| `visitTemplateExpression` (es2015) | 107937-107952 | `f5dfead2be93a40dc2a498913d2945edfa7d29c222bb2e642422251792d07c1d` | head literal, `.concat(expr[, literal])` per span; empty span literal omitted |
| `processTaggedTemplateExpression` / `createTemplateCooked` / `getRawLiteral` | 93972-94018 / 94019-94021 / 94022-94032 | `d318d253…4782446d` / `1f8f38ee…c8253090` / `d4e11c6f…9f1bf2bf` | cooked: `IsInvalid` → `void 0` else `createStringLiteral(template.text)`; raw: `rawText` with CR/CRLF → LF |
| `getLiteralText` / `canUseOriginalText` | 13647-13688 / 13689-13702 | `35659715…4dba00c` / `bf211667…552ae76d` | parsed + parented → original source text; otherwise `escapeNonAsciiString(node.text)` (`escapeString` under `NeverAsciiEscape`/`NoAsciiEscaping`) |
| `escapeString` / `escapeNonAsciiString` / `encodeUtf16EscapeSequence` / `getReplacement` | 16311-16314 / 16316-16319 / 16296-16300 / 16301-16310 | `a41f6d59…7ff0b4d2d` / `021cee3d…983e230e` / `ef9ef4ea…14c0f8` / `0bb6e062…9aed2bcc` | per UTF-16 unit: every unit > 0x7F (a lone surrogate included) → `\uXXXX` |
| `emitLiteral` / `getLiteralTextOfNode` | 117767-117780 / 120467-120479 | `e4021e74…e3d0` / `43989b90…8201e` | printer entry (textSourceNode first) |

## The gap (current source, one cause)

1. `crates/syntax/src/scanner.rs::utf16_encode_as_string` (2204-2208) maps a
   lone surrogate code point to U+FFFD because the scanner's token value is a
   Rust `String`. The parser stores that `String` as
   `StringLiteralData.text` (`parser.rs::parse_string_literal`, 7163-7180,
   with `has_extended_unicode_escape`) and as the cooked `text` of template
   fragments (`parse_template_fragment` 7142-7161,
   `parse_no_substitution_template_literal` 7233-7248, both with the exact
   raw slice in `raw_text`). The parsed node itself keeps no lossless value:
   `LiteralNodeProperties::javascript_string_value` is set only by synthetic
   producers (`grep set_javascript_string_value`: JSX attribute lowering,
   constant folding, module export names copying an existing value, the
   checker's node builder) and copied by `clone_node`.
2. At ES2015+ nothing is lost: the printer copies a positioned, parented
   string literal verbatim (`printer.rs` 2673-2697) and a template fragment
   from `raw_text` (`emit_template_literal_token`, 9168-9216); the existing
   lossless side channel `tsc_syntax::template_text_utf16(cooked, raw)`
   re-decodes the raw fragment for the checker's template literal types and
   `printer/bundle.rs::bundle_prologue_value` (200-250) re-decodes a parsed
   string literal's original spelling for the prologue comparison.
3. At ES5 the ES2015 transform synthesizes new literals from the lossy
   `text`: `es2015.rs::visit_string_literal` (2772-2790, gated by
   `has_extended_unicode_escape`), `visit_template_literal` (2748-2767),
   `visit_template_expression` (6112-6169) and
   `tagged_template.rs::create_template_cooked` (152-162) all call
   `Es2015Visitor::create_string_literal(&text)` (1313-1325: `StringLiteralData`
   without `javascript_string_value`). The printer then reaches its synthetic
   branch (`quote_string_literal(&data.text, …)`, 2800-2820) and escapes the
   U+FFFD it was handed: `"�"`. The printer's own lossless branch
   (`quote_javascript_string(value.code_units(), …)`) is never reached because
   no producer populated the value.

The first producer that loses the value is therefore the ES2015 literal
re-creation, not the printer: the scanner's `String` is the shared lossy
surface, and the lossless information lives in the source spelling (string
literal token bytes; template `raw_text`) exactly as `bundle_prologue_value`
already assumes. Nothing is inferred from a U+FFFD already inside a `String`:
the raw spelling is decoded again, and the decode is accepted only when its
surrogate-agnostic projection equals the cooked text the scanner produced.

## Rust state lifetimes touched

| state | owner and lifetime | change |
| --- | --- | --- |
| `StringLiteralData.text` / `has_extended_unicode_escape`, template `text` / `raw_text` (`nodes.rs`, generated) | syntax arena, immutable after parse | none |
| source text (`SourceFile::text`) and positions | syntax arena; reachable from the transform arena (`arena.source(id).syntax()`) | read only, through the existing `SourceRange::from_raw` + `without_leading_trivia` qualification |
| `LiteralNodeProperties::javascript_string_value` (`metadata.rs` 430-482) | `TransformArena::literal_properties` side map per `TransformNode`, survives emit-session disposal, copied by `clone_node` (`factory.rs` 570-575, 5250, 7400), never merged by `setOriginalNode` | populated by the ES2015 producers through the existing `create_string_literal_from_code_units` |
| writer UTF-16 text (`writer.rs` `push_utf16`, `text_utf16`) and its UTF-8 sink projection | per print | none; the escaped literal is ASCII, so the final bytes are exact |

## Edit functions (per-cause commits)

1. `crates/syntax/src/scanner.rs`: `pub fn string_literal_text_utf16(cooked, raw_between_quotes)`
   — the string-grammar counterpart of `template_text_utf16`: decodes the raw
   token spelling with `scanEscapeSequence(String | ReportErrors)` semantics
   (octal → value, `\8`/`\9` → digit, malformed `\x`/`\u`/`\u{…}` → raw slice,
   line continuations → empty, no CR/CRLF normalization, quote escapes) and
   returns the cooked UTF-16 units when the decode's surrogate-agnostic
   projection equals `cooked`; otherwise the cooked text's own units.
   `template_text_utf16` gains the same guard and the report-mode octal /
   `\8` / `\9` branches (its callers — the checker's template expressions and
   template literal types, `create_template_cooked` after the invalid-cooked
   gate — all observe report-mode cooked text). Unit tests in
   `crates/syntax/tests/unit/scanner/tests.rs`.
2. `crates/emitter/src/factory.rs`: `TransformArena::literal_code_units(node)`
   — the one reader of a literal's JavaScript value: an existing
   `javascript_string_value`; else a positioned string literal's original
   spelling (leading trivia skipped, quote-delimited, closing quote stripped
   only when present) through `string_literal_text_utf16`; else a template
   fragment's `raw_text` through `template_text_utf16`; else, for a
   synthesized node whose `original` is a parsed literal with the same cooked
   `text` (the `cloneNode` shape), that original's spelling; else `None`
   (the caller keeps `text`). A value-changing synthesis (different `text`)
   never borrows a spelling. `bundle_prologue_value` is rewritten onto it.
   Contract tests in `crates/emitter/tests/unit/factory_seams/tests.rs`.
3. `crates/emitter/src/builtins/es2015.rs`: `visit_string_literal`,
   `visit_template_literal`, `visit_template_expression` create their
   literals with `create_string_literal_from_code_units` from
   `literal_code_units` (falling back to the cooked `text`); the emptiness
   test of a span literal is on the units.
4. `crates/emitter/src/builtins/tagged_template.rs::create_template_cooked`:
   the same for the cooked strings of the template object; `get_raw_literal`
   is unchanged (raw text is source bytes, never a lone surrogate).

`nodes.rs` (generated) is untouched; no expected value is edited; no
case- or path-dependent branch is added; no known-divergence row is added.

## Witness correspondence (TypeScript 6.0.3 observed twice each)

`scripts/observe-utf16-literal-witnesses.mjs` (the static-this-super observer
shape) over `crates/compiler/tests/fixtures/utf16-literals-<group>-inputs.json`
(`target/utf16-literals-runs/tools/write-witness-inputs.py`), compared by the
independent target `crates/compiler/tests/h2_5h_utf16_literal_witnesses.rs`
(`TSC_RS_UTF16_LITERAL_WITNESS_SET` / `_FILTER`). Commands: `/project/main.ts`,
`declaration` + `declarationMap` + `sourceMap` on, `module` ESNext, `newLine`
CRLF, targets ES5 (lowered) and ES2015 (retained control), ESNext for the
core surrogate cases; the bundle group uses `module` None + `outFile`.

| reached branch | witnesses (group / name) |
| --- | --- |
| es2015 `visitStringLiteral`, `\u{…}` lone high / low surrogate | `string-literals`: `extended-high-surrogate`, `extended-low-surrogate` (the rows' shape) |
| valid pair, BMP, ASCII through `\u{…}`; short `\uXXXX` control (no re-creation) | `extended-supplementary-pair`, `extended-bmp`, `extended-ascii`, `short-escape-surrogate-control`, `short-and-extended-mixed` |
| real U+FFFD vs lost surrogate; literal backslash sequences | `real-replacement-character`, `escaped-backslash-sequence` |
| quote choice, control/quote/`\0`+digit/` ` escapes, CRLF line continuation | `single-quoted`, `control-and-quote-escapes`, `null-before-digit`, `crlf-line-continuation` |
| invalid escapes with report semantics: octal, `\8`, malformed `\x`, malformed/out-of-range/unterminated `\u{`, unterminated string | `octal-escape`, `decimal-escape-8`, `malformed-hex-escape`, `unterminated-extended-escape-control`, `out-of-range-extended-escape`, `unterminated-string` |
| property-name positions (class static/member names, object literal names, computed) | `class-member-names`, `object-literal-names` |
| es2015 `visitTemplateLiteral` (no-substitution), `visitTemplateExpression` (head/middle/tail, empty tail) | `template-literals`: `no-substitution-high`, `no-substitution-low`, `head-middle-tail`, `empty-tail`, `short-escape-surrogate` |
| template raw CR/CRLF, line continuation, backtick/`${`/backslash escapes, real U+FFFD, octal with surrogate | `crlf-and-line-continuation`, `backtick-dollar-backslash`, `real-replacement-character`, `octal-with-surrogate` |
| tagged templates: cooked (valid), `void 0` invalid cooked, raw strings, no-substitution | `tagged-cooked-and-raw`, `tagged-invalid-cooked`, `tagged-no-substitution` |
| retained targets (verbatim spelling, `raw_text`) | every source at ES2015; core cases at ESNext |
| bundle prologue value comparison (parsed spelling and synthesized value) | `bundle-prologues`: `es5-dedupe`, `es2015-dedupe` |
| adjacent probes recorded, not promoted (checker string-literal type value, enum member names, module export names) | see "Un-witnessed branches" below |

## Un-witnessed branches and out-of-scope producers

Filled in at completion (§ Results).

## Results

Filled in at completion.
