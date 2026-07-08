# impl: the tsc-compatible Node contract (phases 0/2)

The scanner/parser MUST produce an AST that is tsc-compatible at the
FIELD level, not merely "equivalent": every node kind carries tsc's
exact children under tsc's exact names, in tsc's visit order, plus
tsc's position/flag semantics. Rationale: the checker is a port —
every `node.expression`, `node.questionDotToken`, `node.operatorToken`
in checker.ts must transcribe to a field access that EXISTS. Any
"cleaner" AST forces per-use-site translation, and every translation
is a divergence site (the first implementation's `new.target`-
folded-into-ImportMeta bug is this class).

This doc extends m0 (codegen) and replaces impl-parser stage 2.0's
loose NodeData description.

## 1. The extraction source (M0 codegen addition, stage 0.3n)

Two sources, cross-checked:

1. **`forEachChildTable`** (`_tsc.js` 28319): per SyntaxKind, the
   NODE-VALUED children with their FIELD NAMES in VISIT ORDER, e.g.
   `TypeParameter → modifiers(nodes), name, constraint, default,
   expression`. Extract with a regex over the table's arrow bodies
   (`visitNode2(cbNode, node.<field>)` → node child;
   `visitNodes(cbNode, cbNodes, node.<field>)` → NodeArray child).
   Visit order here is AUTHORITATIVE — the binder walks children in
   this order and flow-graph shape depends on it.
2. **`typescript.d.ts` interfaces**: run a small node script using
   the vendored `typescript.js` to parse ITS OWN `typescript.d.ts`,
   walk interfaces that have a `kind: SyntaxKind.X` discriminator,
   and emit `{kindName, fields: [{name, type, optional}]}` JSON. This
   adds the NON-node payloads the table cannot see (`Identifier.
   escapedText`, `StringLiteral.text`, `NumericLiteral.text`,
   `TemplateHead.rawText/cooked`, `isTypeOnly`/`token` enum fields,
   flags-bearing booleans) and the optionality of each child.

`xtask codegen nodes` merges both into `nodes.schema.json`
(committed) and generates:

- `crates/syntax/src/nodes.rs`: one struct per node kind with tsc's
  field names in snake_case (`question_dot_token`), children typed
  `NodeId`/`Option<NodeId>`/`NodeArrayId`, payloads typed per the
  .d.ts (`String`, `bool`, enum); plus the `NodeData` enum wrapping
  them, `From` impls, and typed accessors
  (`fn as_call_expression(&self) -> &CallExpressionData`).
- `crates/syntax/src/for_each_child.rs`: `for_each_child(arena, node,
  f)` generated FROM THE SAME schema, preserving tsc's visit order
  (this is what the binder's `bindChildren`, the parent-stamping
  pass, and the error-flag aggregation walk).

Consistency check in the generator: every field the
forEachChildTable visits must exist in the .d.ts-derived field list;
every table entry's kind must map to exactly one struct. Mismatches
fail codegen.

## 2. Contract points the generator cannot decide (port rules)

- **Token nodes are REAL nodes.** tsc materializes punctuation/
  keyword children (`operatorToken`, `questionDotToken`,
  `asteriskToken`, `exclamationToken`, `dotDotDotToken`,
  `equalsGreaterThanToken`, heritage `token`, modifier nodes,
  `EndOfFileToken`) as nodes with positions. They are observable:
  grammar errors anchor on them (`grammarErrorOnFirstToken`, errors
  on `operatorToken`), and the checker dispatches on
  `modifier.kind`. The parser creates them via `parseTokenNode` —
  never booleans.
- **Positions**: `pos` = full start INCLUDING leading trivia
  (`scanner.full_start_pos` at production entry); `end` = end of the
  last token. `getStart(node)` — the error-span start — re-scans
  trivia from `pos` (port `skipTrivia`-based `getTokenPosOfNode`).
  Both byte-offset internally; UTF-16 at the diagnostic boundary.
- **`parent`**: tsc sets parents lazily/optionally at parse
  (`setParentNodes`); the checker REQUIRES them. The port runs one
  `for_each_child` pass after parseSourceFile stamping `parent` for
  every node (and asserting each node has exactly one parent — a
  reused NodeId is a parser bug this assert catches early).
  The same pass ORs `ThisNodeOrAnySubNodesHasError` upward.
- **NodeArray**: `{ nodes, pos, end, has_trailing_comma }` — a real
  arena object (impl-parser's list engine already produces it).
  `modifiers` arrays include decorator nodes interleaved in source
  order, as in tsc 5.x+ (`canHaveModifiers` model).
- **Identifiers**: store `escaped_text` (the binder's
  `escapeLeadingUnderscores` applied ONCE here, impl-binder §1), and
  `original_keyword_kind` for contextual keywords (tsc
  `identifierToKeywordKind` — several grammar checks read it).
- **SourceFile is a node** with the fields the pipeline reads:
  `statements`, `end_of_file_token` (the driver checks it),
  `file_name`, `text`, `language_variant`, `script_kind`,
  `is_declaration_file`, `line_starts` (see §3),
  `external_module_indicator` (see §4), `parse_diagnostics`,
  `node_count`/`identifier_count` (free from the arena).
- **JSDoc**: NOT parsed in the first goal (2XXX on .ts fixtures does
  not consume it). The schema generator still emits the JSDoc node
  kinds (they cost nothing); the parser skips JSDoc comment BODIES as
  trivia. Ledger-note the omission; revisit at phase 9 band
  expansion (8XXX/JS files).

## 3. Line map (M0 utility, used by every tier)

Port `computeLineStarts` (8250) and `getLineAndCharacterOfPosition`
(8328) verbatim — tsc's line-break set (LF, CR, CRLF, LS, PS, NEL)
is wider than `\n` and IS the T0 line/col contract. `line_starts` is
computed once per SourceFile in UTF-16 space (the oracle reports
UTF-16 columns); the byte↔UTF-16 map and the line map are built in
the same single pass over the text.

## 4. externalModuleIndicator (parser post-step)

Port `isFileProbablyExternalModule` (28301) +
`setExternalModuleIndicator` (28850): the first import/export/
import.meta/top-level-await token makes the file a MODULE — this
gates script-vs-module semantics that many band codes depend on
(2669 augmentation rules, 2451-vs-2403 global redeclare, 1375-family
neighbors, `export=` rules). It runs at parseSourceFile end, before
binding. CommonJS indicators are a JS-file concern — ledger-note,
skip.

## 5. Acceptance: the AST tree differ (phase 2 gate addition)

"tsc-compatible" is MEASURED, not asserted:

`oracle/ast-dump.mjs`:

```js
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import fs from "fs";
const sf = ts.createSourceFile(process.argv[2],
  fs.readFileSync(process.argv[2], "utf8"),
  { languageVersion: ts.ScriptTarget.ESNext }, /*setParentNodes*/ false,
  process.argv[2].endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
function dump(n, depth) {
  console.log(`${"  ".repeat(depth)}${n.kind} ${n.pos} ${n.end}`);
  ts.forEachChild(n, c => dump(c, depth + 1));
}
dump(sf, 0);
```

`cargo xtask ast-dump <file>` prints the same
(kind, pos-utf16, end-utf16) indented tree via the generated
`for_each_child`. `cargo xtask ast-diff --corpus` diffs both over
every fixture. **The phase-2 gate becomes twofold: syntactic-T0 ≥
99.5% AND ast-diff clean on all fixtures with zero parse errors on
both sides** (error-recovery trees may legitimately differ in
Missing-node placement; those files are excluded from the tree gate
and covered by the diagnostic gate instead — record the exclusion
count, it should be only the error fixtures).

Note `forEachChild` skips pure token children in some kinds (it
visits what the table visits) — since BOTH dumps use the same
table-driven walk, token-node divergences surface indirectly through
positions; the unit pins from impl-parser cover the token nodes the
tree walk skips.

## 6. Consequences for the impl docs (already assumed downstream)

- impl-parser stage 2.0 is REPLACED by this doc: `make_*` calls in
  its [COPY] blocks construct the GENERATED structs.
- impl-binder's `bind_children` and the parent/error-flag pass use
  the generated `for_each_child`.
- impl-checker-2xxx transcriptions read generated fields 1:1
  (`node.as_call_expression().type_arguments`), which is the point.
