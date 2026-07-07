# M1a: scanner — steps

Parent design: syntax-and-binder.md §1 (state, dispatch, reScan,
speculation); core-interfaces.md §1 (positions). tsc source:
`vendor/typescript-6.0.3/lib/_tsc.js` — the scanner region starts
around line 8000 (`textToKeywordObj` 8030) and runs through
`speculationHelper` (11099). Prerequisite: M0 gate green.

Gate: token-stream parity vs the oracle scanner across the corpus
(the differ from stage 1.0).

## Stage 1.0: the token differ FIRST [P]

Build the acceptance harness before the scanner so every later stage
has a truth signal. Two sides:

`oracle/token-dump.mjs` (uses the vendored `typescript.js`):

```js
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import fs from "fs";
const text = fs.readFileSync(process.argv[2], "utf8");
const s = ts.createScanner(99 /*ESNext*/, /*skipTrivia*/ true,
                           ts.LanguageVariant.Standard, text);
let t;
while ((t = s.scan()) !== ts.SyntaxKind.EndOfFileToken) {
  console.log([t, s.getTokenStart(), s.getTokenEnd(),
               s.getTokenFlags() & 0x1 /*PrecedingLineBreak*/].join("\t"));
}
```

Rust side: `cargo xtask tokens <file>` printing the same four columns
from the new scanner. `cargo xtask token-diff [--corpus]` runs both
and diffs. Notes: the dump compares KIND + positions + the
line-break flag only; `tokenValue` is asserted separately in unit
tests (stage 1.4+) because the dump would otherwise need value
escaping rules. `.tsx` files pass `LanguageVariant.JSX` on both sides.

Commit: `m1 1.0: token differ harness`.

## Stage 1.1: scanner state + trivia [M]

Scanner struct per syntax-and-binder §1.1 (WTF-8 text — JS source can
contain lone surrogates; positions are byte offsets internally with a
byte↔UTF-16 map built per file for the diagnostic boundary,
core-interfaces §7). Port:

- `CharacterCodes` values (M0 codegen, const-inlined enum).
- Line-break/whitespace classification + `skipTrivia`; single/multi
  line comments (unterminated comment error 1010); shebang at pos 0.
- `PrecedingLineBreak` TokenFlags bit set while skipping trivia.

Verify: token-diff over 30 fixtures containing only comments/trivia
edge cases (pick from `parser/ecmascript5` by grepping for comment
fixtures). Expect: zero diffs.

Commit: `m1 1.1: scanner state + trivia`.

## Stage 1.2: punctuation + identifiers + keywords [M]

Port the `scan()` dispatch (`_tsc.js` 9368) arms for punctuation
(maximal munch EXCEPT `>` — greater-than compounds are reScan-only,
syntax-and-binder §1.3), then `scanIdentifier` with the Unicode
identifier-start/part classification tsc uses, then the keyword table
(`textToKeywordObj`, 8030) mapping identifier text to keyword
SyntaxKinds. EVERY entry of the keyword table ports — a missing entry
means a reserved word scans as an identifier (the parent repo shipped
that bug for `debugger`).

Verify: token-diff over the whole `parser/` corpus subtree. Expect:
remaining diffs only in literals/templates (stages 1.3-1.5).

Commit: `m1 1.2: punctuation, identifiers, keywords`.

## Stage 1.3: strings + escape sequences [M]

`scanString` + `scanEscapeSequence` + `scanExtendedUnicodeEscape`
(`_tsc.js` 9205). Port the extended `\u{...}` state machine exactly —
its error set (1125 hex digit expected, 1198 value must be ≤ 0x10FFFF,
1199 unterminated escape, 1002 unterminated string) and the
`ContainsInvalidEscape` token flag. Two discipline notes learned
downstream: the error POSITIONS come from where the scan cursor
actually is (do not invent recovery), and the parser layer will dedup
same-start parse errors (m1-parser 2.1) — the scanner itself emits
every error.

Verify: token-diff + a unit-pin set of ~20 escape strings whose
expected errors come from oracle probes of micro-fixtures
(`var x = "\u{r}";` etc.).

Commit: `m1 1.3: strings + escapes`.

## Stage 1.4: numbers [M]

`scanNumber` + bigint suffix + numeric separators + legacy octal and
the related TokenFlags bits (Octal, HexSpecifier, BinarySpecifier,
ContainsSeparator, ...) — several later grammar diagnostics key on
these bits, so they are observable. `checkBigIntSuffix`,
`scanExponent`.

Verify: token-diff + separator/octal micro pins.

Commit: `m1 1.4: numeric literals`.

## Stage 1.5: templates [M]

`scanTemplateAndSetTokenValue` (`_tsc.js` 9017): head/middle/tail
kinds, cooked value with escape processing, the unterminated cases,
and CR/CRLF normalization inside the cooked value. `reScanTemplateToken`
(10871) — the parser calls it at `}` inside a substitution.

Verify: token-diff over `es6/templates/**`.

Commit: `m1 1.5: template literals`.

## Stage 1.6: the reScan family + regex extent [M]

Port per syntax-and-binder §1.3: `reScanGreaterToken` (9866),
`reScanSlashToken` (9893) — at M1 only the EXTENT scan matters (find
the end of `/.../flags` or fall back to div/div-assign); the
error-reporting regex WORKER is a checker-side port that arrives with
grammar checks in M4 — plus `reScanLessThanToken`, `reScanHashToken`,
and the JSX token scanners (`scanJsxToken`, `reScanJsxToken`,
`scanJsxIdentifier`, `scanJsxAttributeValue`) for `.tsx`.

Verify: token-diff with regex-heavy and JSX fixtures. The differ
cannot exercise reScan paths that need parser context — those are
covered by M1b's syntactic parity gate instead; here assert the plain
paths plus unit pins that call the reScan entry points directly.

Commit: `m1 1.6: reScan family`.

## Stage 1.7: speculation [M]

`speculationHelper` (11099) + `lookAhead` (11138) + `tryScan` (11145)
per syntax-and-binder §1.4 — full state save/restore, and the
rewind-on-falsy asymmetry ported exactly (lookAhead ALWAYS rewinds;
tryScan rewinds only when the callback result is falsy).

Verify: unit tests exercising nested speculation with state
assertions.

Commit: `m1 1.7: speculationHelper`.

## Final gate

```sh
cargo xtask token-diff --corpus     # expect: 0 differing files
cargo xtask invariants --suite encodings
cargo xtask ledger check            # every scanner fn has its tsc-port entry
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Positions drift only on multibyte files | byte vs UTF-16 confusion in the dump layer | Internal positions are bytes; the DUMP prints UTF-16 via the per-file map, same as the oracle |
| A keyword scans as Identifier | keyword table entry missing | Re-extract textToKeywordObj; never hand-maintain |
| `a >> b` tokens differ | ported `>>` into scan() instead of reScanGreaterToken | `>` compounds exist ONLY in the reScan (syntax-and-binder §1.3) |
| Template cooked values differ on CRLF fixtures | missing \r\n → \n normalization in cooked text | Port the normalization inside scanTemplateAndSetTokenValue, not in the harness |
