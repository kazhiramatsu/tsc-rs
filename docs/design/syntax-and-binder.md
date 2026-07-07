# Scanner, parser, binder: the front end

Companion to checker-key-functions.md and checker-foundations.md (the
checker). This doc covers the three phases that run BEFORE the checker
and produce everything it consumes: the token stream, the AST (with
recovery + error flags), and the symbol/scope/flow tables. The
milestone lesson from greenfield §12 bears repeating: **M1's
parser-with-tsc-recovery is the foundation everything downstream prices
in** — the current repo approximated it and pays the parse-error-gate
tax (archive/workstreams/parse-error-gate.md) to this day. In a rebuild
these are done
exactly, first.

Same rule: PORT the tsc source. Line anchors are vendored tsc 6.0.3.
tsc's front end lives in `scanner.ts` (bundled) and `parser.ts` /
`binder.ts` regions of `_tsc.js`.

---

## 1. Scanner — tsc `scan` 9368

A pull scanner: the parser calls `nextToken()` which calls `scan()`,
which advances `pos` and sets `token` + `tokenValue` + `tokenFlags`.

### 1.1 State and the token model

```rust
struct Scanner {
    text: Wtf8Buf,        // WTF-8, NOT String — JS source can contain lone
                          // surrogates; tsrs already uses the `wtf8` crate (keep)
    pos: usize,           // current scan position (byte offset into WTF-8)
    full_start_pos: usize,// start incl. leading trivia
    token_start: usize,   // start of the current token (excl. trivia)
    token: SyntaxKind,    // bit-compatible SyntaxKind (greenfield §3)
    token_value: String,  // identifier text / string/number literal value
    token_flags: TokenFlags, // PrecedingLineBreak, Unterminated, unicode-escape,
                          // ContainsSeparator, numeric-format bits, …
}
```

`TokenFlags` are load-bearing and OBSERVABLE via diagnostics:
`PrecedingLineBreak` drives ASI and `as`/`satisfies`/`await` line
sensitivity; unicode-escape flags drive `Keywords_cannot_contain_escape_characters`;
numeric-format flags drive octal/separator errors. Bit-compatible port.

### 1.2 The dispatch — 9368

`scan()`: at `pos>=end` → EOF; a shebang at pos 0; then a `switch` on
the code point: whitespace/newlines (set `PrecedingLineBreak`, either
skip as trivia or return a trivia token per `skipTrivia`),
comments, `/` (→ scanning, but see reScan below), strings, templates,
numbers, punctuation (maximal munch: `>` then `>=`/`>>` handled by
reScan not here), and default → `scanIdentifier` /
`isIdentifierStart`. Identifiers that match a keyword table become the
keyword `SyntaxKind` (this is where `debugger` etc. become reserved —
the current tsrs bug where `debugger` scans as an identifier is a
missing entry in this table; knowledge-base §4).

### 1.3 The reScan family — context-sensitive re-tokenization — 9866+

The scanner CANNOT tokenize some things without parser context, so the
parser asks it to re-scan the current token differently:

- `reScanGreaterToken` (9866): `>` → `>>`, `>>>`, `>=`, `>>=`, `>>>=`.
  Called at the START of every `parseBinaryExpressionRest` iteration
  (§2.2) so `A<B<C>>` parses type args (two `>`) but `a >> b` parses a
  shift. THE reason `>` is scanned as a single token by default.
- `reScanSlashToken` (9893): `/` or `/=` → a regex literal, when the
  parser is in a position where a regex is legal (`parsePrimaryExpression`).
- `reScanTemplateToken` (10871): `}` → template middle/tail, inside a
  template substitution.
- `reScanAsteriskEqualsToken`: `*=` → `=` (used in some recovery).
- Others: `reScanLessThanToken`, `reScanHashToken`, JSX text tokens.

DESIGN DECISION for the port: reScan mutates the scanner in place from
`token_start`; it must be idempotent and cheap. The current tsrs
already has `reScanGreaterToken` (used in the non-LHS-`=` recovery work,
archive/workstreams/parse-error-gate-steps.md §1). A rebuild ports the
whole family — several
are needed for correct type-argument / regex / template parsing.

### 1.4 Speculation — `speculationHelper` 11099

The primitive under `lookAhead` and `tryScan` (scanner) and the
parser's `lookAhead`/`tryParse`:

```rust
fn speculation_helper<R>(&mut self, cb: impl FnOnce(&mut Self) -> R, is_lookahead: bool) -> R
    where R: SpeculationResult {
    let save = self.save_scanner_state();      // pos, full_start, token_start, token, value, flags
    let result = cb(self);
    if is_lookahead || !result.is_truthy() {   // lookahead ALWAYS rewinds; tryScan rewinds on falsy
        self.restore_scanner_state(save);
    }
    result
}
```

- `lookAhead`: run, ALWAYS rewind, return what the callback saw
  (used for grammar decisions: "is this an arrow function?").
- `tryScan`/`tryParse`: run, rewind ONLY if the callback returned
  falsy (used for optimistic parses: arrow-function attempt).

This is the ONLY backtracking mechanism — the parser is otherwise
single-pass. Port it exactly; the "rewind on falsy" asymmetry is what
makes speculative arrow-function parsing correct.

---

## 2. Parser

Recursive descent + Pratt (precedence-climbing) for binary expressions.
Produces nodes via a `factory` and `finishNode` (which stamps
pos/end and the parse-error flag).

### 2.1 Node creation, positions, and the error flag

- `getNodePos()` = `scanner.getTokenFullStart()` (incl. leading
  trivia); `finishNode(node, pos)` sets `node.pos = pos`, `node.end =
  scanner.getTokenFullStart()` (end of previous token).
- **`parseErrorBeforeNextFinishedNode`**: when a parse error is
  reported, this flag is set; the NEXT `finishNode` transfers it to the
  node as `NodeFlags.ThisNodeHasError`. This is the mechanism that
  makes `containsParseError` work (checker-foundations /
  archive/workstreams/parse-error-gate.md)
  — the flag propagates to ancestors via `ThisNodeOrAnySubNodesHasError`.
  A rebuild gets per-node parse-error gating FOR FREE by porting this;
  the current tsrs lacks it and approximates with the whole-file gate.
- `parseErrorAtPosition` (29470): **dedups by start** — no two parse
  errors at the same start position. Port this or error counts drift.

### 2.2 Expressions — the Pratt loop — `parseBinaryExpressionOrHigher` 32107

```rust
fn parse_binary_expression_or_higher(&mut self, precedence: u8) -> Node {
    let pos = self.node_pos();
    let left = self.parse_unary_expression_or_higher();
    self.parse_binary_expression_rest(precedence, left, pos)
}
fn parse_binary_expression_rest(&mut self, precedence: u8, mut left: Node, pos: usize) -> Node {
    loop {
        self.re_scan_greater_token();                      // `>`→`>>` etc. EACH iteration
        let new_prec = binary_operator_precedence(self.token());
        // `**` is right-associative: consume on >=, everything else on >
        let consume = if self.token() == AsteriskAsterisk { new_prec >= precedence } else { new_prec > precedence };
        if !consume { break; }
        if self.token() == In && self.in_disallow_in_context() { break; }   // for-in header
        if self.token() == As || self.token() == Satisfies {
            if self.scanner_has_preceding_line_break() { break; }           // ASI: `x\nas` is not a cast
            let kw = self.token(); self.next_token();
            left = if kw == Satisfies { self.make_satisfies(left, self.parse_type()) }
                   else { self.make_as(left, self.parse_type()) };
        } else {
            left = self.make_binary(left, self.parse_token_node(), self.parse_binary_expression_or_higher(new_prec), pos);
        }
    }
    left
}
```

- `getBinaryOperatorPrecedence` returns 0 for non-operators (loop
  exits). The `>` vs `>=` associativity split for `**` and the
  reScanGreater-per-iteration are the two easy-to-miss correctness bits.
- **The non-LHS `=` recovery**
  (archive/workstreams/parse-error-gate.md §A) lives in
  `parseAssignmentExpressionOrHigher` (which calls
  parseBinaryExpressionOrHigher then checks
  `isLeftHandSideExpression(left) && isAssignmentOperator(reScanGreater())`).
  This is the single most valuable parser port for the current repo.
- Context flags (`disallowIn`, `await`, `yield`, `decorator`) are
  threaded as parser state (save/restore around the relevant
  productions), NOT passed as parameters. Port the `doInsideOfContext`
  / `doOutsideOfContext` save-restore helpers.

### 2.3 List parsing + RECOVERY — the heart of error tolerance

Every comma/semicolon/brace-delimited construct goes through
`parseDelimitedList` (30428) or `parseList`. The recovery behavior here
IS the parser's error tolerance, and it is what the archived parse-error-gate
work must interoperate with.

```rust
fn parse_delimited_list(&mut self, kind: ParsingContext, parse_element: impl Fn(&mut Self) -> Option<Node>, semicolon_delim: bool) -> NodeArray {
    let saved = self.parsing_context; self.parsing_context |= 1 << (kind as u32);
    let mut list = Vec::new(); let list_pos = self.node_pos(); let mut comma_start = -1;
    loop {
        if self.is_list_element(kind, /*in_error_recovery*/ false) {
            let start = self.token_full_start();
            let Some(el) = self.parse_list_element(kind, &parse_element) else { self.parsing_context = saved; return NodeArray::none(); };
            list.push(el);
            comma_start = self.token_start();
            if self.parse_optional(Comma) { continue; }
            comma_start = -1;
            if self.is_list_terminator(kind) { break; }
            self.parse_expected(Comma, self.expected_comma_diagnostic(kind));   // reports ',' expected
            if semicolon_delim && self.token() == Semicolon && !self.has_preceding_line_break() { self.next_token(); }
            if start == self.token_full_start() { self.next_token(); }          // no progress ⇒ force-advance
            continue;
        }
        if self.is_list_terminator(kind) { break; }
        if self.abort_parsing_list_or_move_to_next_token(kind) { break; }       // recovery decision
    }
    self.parsing_context = saved;
    NodeArray::new(list, list_pos, comma_start >= 0 /*hasTrailingComma*/)
}
```

- `isListTerminator(kind)` (30097): per-context terminators (a
  `TypeMembers`/`ClassMembers` list ends at `}`; `Parameters` at `)` or
  `]`; `TypeArguments` at `>`, ...). The dispatch table is large; port
  it verbatim — each entry is a recovery boundary.
- `abortParsingListOrMoveToNextToken(kind)` (30356): reports the
  context-specific error (`parsingContextErrors`: "Declaration or
  statement expected", "Property or signature expected", "Parameter
  declaration expected", ...) then, if the current token could start an
  element of ANY enclosing parsing context (`isInSomeParsingContext`),
  returns true to ABORT this list and let the outer context handle the
  token; otherwise skips one token and continues. This nested-context
  awareness is what lets `[a, {b: , c]` recover sanely. THE recovery
  engine; the archived parse-error-gate work reads the RESULT of this.
- The `start == token_full_start ⇒ next_token()` guard is the
  no-infinite-loop backstop (force progress when a bad element didn't
  consume anything).
- `createMissingList()` / `isMissingList` and missing IDENTIFIER nodes
  (zero-width, `NodeFlags` marking them synthesized) are how a required
  element that's absent still yields a node the checker can errorType.

### 2.4 Statement dispatch — `parseStatement` 33528

A `switch` on the leading token: `{`→block, `var/let/const`→variable
statement, `function`→function decl, `if/do/while/for/switch/...`, and
`@`→decorated declaration, and the tricky ones (`async`, `await using`,
`export`, `import`, identifier-followed-by-`:`→labeled). Modifiers are
parsed by `parseModifiers` with lookahead. The default (an expression
that could start a statement) → `parseExpressionOrLabeledStatement`.
Port the dispatch order; ambiguities (e.g. `async` as identifier vs
modifier) are resolved by `lookAhead`.

---

## 3. Binder

Walks the AST once, creating symbols (into `locals`/`members`/`exports`
tables), assigning `parent` pointers, computing strict-mode, and
building the control-flow graph (the FlowNode graph checker-key-functions
§4.8 walks). Three responsibilities braided into one pass.

### 3.1 declareSymbol — the merge engine — tsc 42602

THE symbol model. The class-method-overload-orphaning bug
(relation-core-1) was a hand-rolled divergence from this; port it and
that class of bug is structurally impossible.

```rust
fn declare_symbol(&mut self, table: &mut SymbolTable, parent: Option<SymbolId>, node: NodeId,
        includes: SymbolFlags, excludes: SymbolFlags, replaceable_by_method: bool, computed: bool) -> SymbolId {
    let name = if computed { "__computed" } else if self.is_default_export(node) && parent.is_some() { "default" }
               else { self.declaration_name(node) };   // None ⇒ "__missing"
    let sym = match name {
        None => self.create_symbol(SymbolFlags::None, "__missing"),
        Some(name) => {
            match table.get(name) {
                None => { let s = self.create_symbol(SymbolFlags::None, name); table.set(name, s); s }
                Some(existing) if replaceable_by_method && !self.is_replaceable(existing) => return existing,
                Some(existing) if self.flags(existing).intersects(excludes) => {
                    // CONFLICT unless it's a var/assignment merge
                    if self.is_replaceable(existing) {
                        let s = self.create_symbol(SymbolFlags::None, name); table.set(name, s); s
                    } else if !(includes.intersects(VARIABLE) && self.flags(existing).contains(ASSIGNMENT)) {
                        // report Duplicate_identifier / Cannot_redeclare_block_scoped_variable
                        // (+ enum-merge, multiple-default-export special messages),
                        // with relatedInformation pointing at every prior declaration,
                        // then start a FRESH symbol so later refs don't compound the error
                        self.report_duplicate(existing, node, includes);
                        self.create_symbol(SymbolFlags::None, name)
                    } else { existing }
                }
                Some(existing) => existing,   // MERGE: no excluded-flag overlap ⇒ same symbol
            }
        }
    };
    self.add_declaration_to_symbol(sym, node, includes);   // pushes decl, updates flags, sets valueDeclaration
    // parent must be consistent across merged declarations
    if self.symbol(sym).parent.is_none() { self.symbol_mut(sym).parent = parent; }
    sym
}
```

- **includes/excludes masks** are the merge rule: two declarations
  merge iff neither's `includes` hits the other's `excludes`. Function
  overloads (`FunctionExcludes` doesn't exclude Function),
  namespace+function, namespace+class, enum+namespace, interface+interface,
  interface+class all merge; var+function does not (except the
  var/assignment JS special case). These masks are generated from tsc's
  `SymbolFlags` — port them, don't hand-enumerate (that is exactly what
  went wrong with class-method overloads).
- **On conflict, a FRESH symbol replaces the table entry** so the error
  is reported ONCE and later references don't re-trigger it. tsrs's
  `duplicate_losers` set (U5) approximates the visibility half of this.
- `addDeclarationToSymbol` sets `valueDeclaration` (first value decl
  wins), accumulates `declarations`, ORs in the `includes` flags. The
  checker's `getTypeOfSymbol` dispatch (checker-foundations §1.1) reads
  exactly these.

### 3.2 declareModuleMember — locals vs exports — tsc 42672

Whether a declaration goes into `locals` (module-internal) or `exports`
(visible) or BOTH (`local.exportSymbol` linking them) depends on the
`export` modifier and container kind. The `local + exportSymbol` split
(a local symbol whose `exportSymbol` points at the export-table entry)
is how `export const x` is both a local reference target and an export.
Port the three-way branch; the current tsrs export handling (U5 import/
export work) is a partial version.

### 3.3 bind / bindContainer / getContainerFlags — tsc 44226 / 42734

```rust
fn bind(&mut self, node: NodeId) {
    self.set_parent(node, self.parent);
    let save_strict = self.in_strict_mode;
    self.update_strict_mode_by_node(node);   // "use strict", class bodies, modules
    self.bind_worker(node);                   // create THIS node's symbol(s) + flow effects
    let cf = self.container_flags(node);
    if cf.is_empty() { self.bind_children(node); }
    else { self.bind_container(node, cf); }   // sets up a new locals/flow-container scope
    self.in_strict_mode = save_strict;
}
```

- **`getContainerFlags`** classifies a node as: IsContainer (has
  `locals` — function/module/source-file/type-alias/mapped-type),
  IsBlockScopedContainer (block/for/catch/switch/module-block),
  IsControlFlowContainer (function/module/source-file/property-initializer
  — gets its own FlowNode graph), HasLocals, IsFunctionLike,
  IsFunctionExpression. These flags decide scope creation and where the
  flow graph resets. Port the classification; it defines the scope tree
  the checker's name resolution walks.
- **`bindContainer`** saves/restores `container`, `blockScopeContainer`,
  `currentFlow`, `activeLabelList`, and the break/continue targets, then
  binds children in the new scope. Control-flow containers reset
  `currentFlow` to a fresh `Start` node — this is where the per-function
  flow graph begins.
- **Flow graph construction** happens in `bindWorker`'s statement arms:
  `bindWhileStatement`, `bindIfStatement` (via `bindCondition`),
  `bindTryStatement` (exception edges), `bindReturnOrThrow`, etc. — the
  `createFlowCondition/Assignment/Call/BranchLabel/LoopLabel/ReduceLabel`
  + `addAntecedent` + `finishFlowLabel` family (checker-key-functions
  §4.8). The current tsrs `flow_graph.rs` already implements this
  family, byte-identical-gated (Tier-2) — KEEP that design; a rebuild
  ports `FlowFlags` bit-compatibly and the bind*Statement family
  verbatim.

### 3.4 Strict mode

`inStrictMode` is threaded through bind and affects both binder
diagnostics (`with` in strict, duplicate params, octal literals) and
LATER checker behavior (`delete` of an identifier, arguments/eval
assignment). It is set by `"use strict"` prologues, class bodies
(always strict), and ES modules. Port the propagation; several grammar
diagnostics gate on it.

---

## 4. Where the front end sits in the porting order

Front end is M1–M2 in the greenfield milestones (§12) and gates
everything:

1. **Scanner** (M1): token model + `TokenFlags` + `scan` dispatch +
   the reScan family + speculationHelper. Acceptance: token stream
   matches tsc on a sample (a small oracle that dumps
   `scanner.getToken()` in a loop).
2. **Parser** (M1): recursive descent + the Pratt loop + list-parsing
   recovery (`parseDelimitedList`/`isListTerminator`/
   `abortParsingListOrMoveToNextToken`) + the parse-error flag
   (`parseErrorBeforeNextFinishedNode` → `ThisNodeHasError`) + the
   non-LHS `=` recovery. Acceptance: SYNTACTIC diagnostics ≥ 99.5%
   parity (greenfield M1 gate) — this is the foundation the whole
   conformance number rests on.
3. **Binder** (M2): `declareSymbol` merge engine + `declareModuleMember`
   + `getContainerFlags`/`bindContainer` scope tree + the flow-graph
   bind*Statement family + strict-mode propagation. Acceptance:
   crash-free bind of the corpus + a symbol-table spot-audit vs tsc
   (greenfield M2 gate).

The parse-error flag (2) and the declareSymbol merge (3) are the two
front-end ports that, done right, DELETE whole classes of downstream
work the current repo still carries (archive/workstreams/parse-error-gate.md and the
overload-merge family respectively).
