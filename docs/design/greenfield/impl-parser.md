# impl: parser (phase 2) — copy-level code

Companion to m1-parser-steps.md. Module: `crates/syntax/src/parser/`
(`mod.rs` infrastructure, `stmts.rs`, `exprs.rs`, `types.rs`,
`decls.rs`, `jsx.rs` — mirroring tsc parser region order).

THE AST CONTRACT: node structs are GENERATED, tsc-field-compatible
(see [impl-nodes.md](impl-nodes.md) — it replaces the old stage 2.0):
every production constructs the generated struct for its kind, token
children are real nodes via `parse_token_node`, and the parse ends
with the parent-stamping / error-flag-aggregation `for_each_child`
pass plus the externalModuleIndicator post-step. The AST tree differ
(impl-nodes §5) is part of this phase's gate.

Stage 2.0 (AST shape) is superseded by impl-nodes.md §1-2: run
`xtask codegen nodes` and commit the generated `nodes.rs` /
`for_each_child.rs` as this stage.

Entry-point contract (lsp-and-incremental.md §2 rule 1): the public
parse entry is `parse_source_file(file_name, text, opts, cursor:
Option<&SyntaxCursor>) -> SourceFile` — `SyntaxCursor` is an empty
placeholder until the L-track; batch always passes `None`. tsc's own
parser takes the cursor as its fourth parameter (`_tsc.js` 29014);
reserving it now is what keeps incremental reparse additive.

## [COPY] Parser state + error infrastructure (stage 2.1)

```rust
pub struct Parser<'t> {
    pub scanner: Scanner<'t>,
    pub arena: NodeArena,                 // stage 2.0
    pub file_index: usize,
    pub context_flags: NodeFlags,         // DisallowIn/Yield/Await/Decorator/Ambient bits
    pub parse_diagnostics: Vec<Diagnostic>,
    pub parse_error_before_next_finished_node: bool,
    pub parsing_context: u32,             // bitset of active ParsingContext values
}

impl<'t> Parser<'t> {
    /// tsc parseErrorAtPosition (_tsc.js 29467): SAME-START DEDUP —
    /// no two parse errors at one start. This single rule is why the
    /// first implementation over-reported escape errors; it is load-
    /// bearing, not an optimization.
    pub fn parse_error_at_position(&mut self, start: usize, length: usize,
            msg: &'static DiagnosticMessage, args: &[&str]) {
        let start_u16 = self.to_utf16(start);
        if self.parse_diagnostics.last()
               .is_none_or(|last| last.start != start_u16) {
            self.parse_diagnostics.push(Diagnostic::new(
                self.file_index, start_u16, self.to_utf16(start + length) - start_u16,
                msg, args));
        }
        self.parse_error_before_next_finished_node = true;
    }
    pub fn parse_error_at_current_token(&mut self, msg: &'static DiagnosticMessage, args: &[&str]) {
        self.parse_error_at_position(self.scanner.token_start,
            self.scanner.pos - self.scanner.token_start, msg, args);
    }
    /// scanner errors flow through the SAME dedup (tsc scanError → parseErrorAtPosition)
    pub fn drain_scanner_errors(&mut self) {
        for e in std::mem::take(&mut self.scanner.errors) {
            self.parse_error_at_position(e.start, e.length, e.message,
                &e.args.iter().map(String::as_str).collect::<Vec<_>>());
        }
    }

    #[inline] pub fn token(&self) -> SyntaxKind { self.scanner.token }
    pub fn next_token(&mut self) -> SyntaxKind {
        let t = self.scanner.scan(); self.drain_scanner_errors(); t
    }
    pub fn node_pos(&self) -> usize { self.scanner.full_start_pos }

    /// tsc finishNode (29778): stamp positions, transfer the error flag.
    pub fn finish_node(&mut self, id: NodeId, pos: usize) -> NodeId {
        let end = self.scanner.full_start_pos;      // end of PREVIOUS token
        let n = self.arena.node_mut(id);
        n.pos = pos as u32; n.end = end as u32;
        n.flags |= self.context_flags & NodeFlags::CONTEXT_MASK;
        if self.parse_error_before_next_finished_node {
            self.parse_error_before_next_finished_node = false;
            n.flags |= NodeFlags::THIS_NODE_HAS_ERROR;
        }
        id
    }

    pub fn parse_expected(&mut self, kind: SyntaxKind,
            msg: Option<&'static DiagnosticMessage>) -> bool {
        if self.token() == kind { self.next_token(); return true; }
        match msg {
            Some(m) => self.parse_error_at_current_token(m, &[]),
            None => self.parse_error_at_current_token(
                &gen::_0_expected, &[token_to_string(kind)]),
        }
        false
    }
    pub fn parse_optional(&mut self, kind: SyntaxKind) -> bool {
        if self.token() == kind { self.next_token(); true } else { false }
    }
    /// zero-width real node so the checker can errorType it (tsc createMissingNode)
    pub fn create_missing_node(&mut self, kind: SyntaxKind, report: bool,
            msg: Option<&'static DiagnosticMessage>, args: &[&str]) -> NodeId {
        if report { self.parse_error_at_current_token(msg.unwrap(), args); }
        let pos = self.scanner.token_start;
        let id = self.arena.alloc_missing(kind, pos);
        self.finish_node(id, pos)
    }

    /// context-flag save/restore (tsc doInsideOfContext/doOutsideOfContext)
    pub fn do_in_context<R>(&mut self, set: NodeFlags, clear: NodeFlags,
            f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.context_flags;
        self.context_flags = (self.context_flags | set) & !clear;
        let r = f(self);
        self.context_flags = saved;
        r
    }
    /// parser speculation: scanner state + parser error/context state
    pub fn try_parse<R: Truthy>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let scan = self.scanner.save();
        let (dlen, flag, ctx) = (self.parse_diagnostics.len(),
            self.parse_error_before_next_finished_node, self.context_flags);
        let r = f(self);
        if !r.is_truthy() {
            self.scanner.restore(scan);
            self.parse_diagnostics.truncate(dlen);
            self.parse_error_before_next_finished_node = flag;
            self.context_flags = ctx;
        }
        r
    }
    pub fn look_ahead<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let scan = self.scanner.save();
        let (dlen, flag) = (self.parse_diagnostics.len(),
            self.parse_error_before_next_finished_node);
        let r = f(self);
        self.scanner.restore(scan);
        self.parse_diagnostics.truncate(dlen);
        self.parse_error_before_next_finished_node = flag;
        r
    }
}
```

`ThisNodeOrAnySubNodesHasError` aggregation: computed ONCE after the
file parses (a post-pass ORing child flags upward) — tsc sets it in
`containsParseError` lazily; the post-pass is equivalent and simpler,
and the checker only reads it after parse completes.

## [COPY] The list engine (stage 2.2)

Paste; fill the two dispatch tables from the cited tsc switches —
they are pure transcription (one arm per ParsingContext value):

```rust
impl<'t> Parser<'t> {
    pub fn parse_delimited_list<T: Into<NodeId>>(&mut self, ctx: ParsingContext,
            parse_element: impl Fn(&mut Self) -> Option<NodeId>,
            consider_semicolon_as_delimiter: bool) -> NodeArrayId {
        let saved = self.parsing_context;
        self.parsing_context |= 1 << (ctx as u32);
        let mut list = Vec::new();
        let list_pos = self.node_pos();
        let mut comma_start: isize = -1;
        loop {
            if self.is_list_element(ctx, /*in_error_recovery*/ false) {
                let start_pos = self.scanner.full_start_pos;
                match parse_element(self) {
                    None => { self.parsing_context = saved;
                              return self.arena.empty_array(list_pos); }
                    Some(el) => list.push(el),
                }
                comma_start = self.scanner.token_start as isize;
                if self.parse_optional(SyntaxKind::CommaToken) { continue; }
                comma_start = -1;
                if self.is_list_terminator(ctx) { break; }
                self.parse_expected(SyntaxKind::CommaToken,
                    self.expected_comma_diagnostic(ctx));
                if consider_semicolon_as_delimiter
                    && self.token() == SyntaxKind::SemicolonToken
                    && !self.scanner.token_flags.contains(TokenFlags::PRECEDING_LINE_BREAK)
                { self.next_token(); }
                if start_pos == self.scanner.full_start_pos { self.next_token(); } // force progress
                continue;
            }
            if self.is_list_terminator(ctx) { break; }
            if self.abort_parsing_list_or_move_to_next_token(ctx) { break; }
        }
        self.parsing_context = saved;
        self.arena.alloc_array(list, list_pos, self.node_pos(), comma_start >= 0)
    }

    /// tsc isListTerminator (30097): TRANSCRIBE the whole switch.
    fn is_list_terminator(&mut self, ctx: ParsingContext) -> bool {
        if self.token() == SyntaxKind::EndOfFileToken { return true; }
        todo_port!("isListTerminator switch, _tsc.js 30097 — one arm per context")
    }
    /// tsc isListElement → isStartOf* per context
    fn is_list_element(&mut self, ctx: ParsingContext, in_error_recovery: bool) -> bool {
        todo_port!("isListElement switch — one arm per context, _tsc.js near 29900")
    }
    /// tsc 30356: report the context error, then abort if an ENCLOSING
    /// context could start an element here; else skip one token.
    fn abort_parsing_list_or_move_to_next_token(&mut self, ctx: ParsingContext) -> bool {
        self.parsing_context_error(ctx);   // parsingContextErrors table: transcribe
        if self.is_in_some_parsing_context() { return true; }
        self.next_token();
        false
    }
    fn is_in_some_parsing_context(&mut self) -> bool {
        for c in ParsingContext::ALL {
            if self.parsing_context & (1 << (c as u32)) != 0 {
                if self.is_list_element(c, true) || self.is_list_terminator(c) {
                    return true;
                }
            }
        }
        false
    }
}
```

## [COPY] Pratt loop + assignment gate (stage 2.4)

```rust
impl<'t> Parser<'t> {
    pub fn parse_binary_expression_or_higher(&mut self, precedence: u8) -> NodeId {
        let pos = self.node_pos();
        let left = self.parse_unary_expression_or_higher();
        self.parse_binary_expression_rest(precedence, left, pos)
    }
    fn parse_binary_expression_rest(&mut self, precedence: u8,
            mut left: NodeId, pos: usize) -> NodeId {
        loop {
            self.scanner.re_scan_greater_token();          // EVERY iteration head
            let new_prec = binary_operator_precedence(self.token());
            let consume = if self.token() == SyntaxKind::AsteriskAsteriskToken
                { new_prec >= precedence } else { new_prec > precedence };
            if !consume { break; }
            if self.token() == SyntaxKind::InKeyword
                && self.context_flags.contains(NodeFlags::DISALLOW_IN_CONTEXT) { break; }
            if matches!(self.token(), SyntaxKind::AsKeyword | SyntaxKind::SatisfiesKeyword) {
                if self.scanner.token_flags.contains(TokenFlags::PRECEDING_LINE_BREAK) { break; }
                let kw = self.token(); self.next_token();
                let ty = self.parse_type();
                left = self.make_as_or_satisfies(kw, left, ty, pos);
            } else {
                let op = self.parse_token_node();
                let right = self.parse_binary_expression_or_higher(new_prec);
                left = self.make_binary(left, op, right, pos);
            }
        }
        left
    }

    /// tsc parseAssignmentExpressionOrHigher: THE recovery-defining rule —
    /// `=` binds only to a LEFT-HAND-SIDE expression; otherwise it is left
    /// unconsumed for the outer context to report.
    pub fn parse_assignment_expression_or_higher(&mut self) -> NodeId {
        // yield / async-arrow / parenthesized-arrow speculations first:
        todo_port!("isYieldExpression / tryParseParenthesizedArrowFunctionExpression / async arrow lookAhead");
        #[allow(unreachable_code)] {
        let pos = self.node_pos();
        let expr = self.parse_binary_expression_or_higher(0);
        if is_left_hand_side_expression(&self.arena, expr)
            && is_assignment_operator(self.scanner.re_scan_greater_token()) {
            let op = self.parse_token_node();
            let right = self.parse_assignment_expression_or_higher();
            return self.make_binary(expr, op, right, pos);
        }
        self.parse_conditional_expression_rest(expr, pos)
        }
    }
}
```

## [PORT TABLE] productions, in port order

Transcription targets; every row is one commit-sized unit unless
marked (grouped). Anchors: grep `function <name>(` in `_tsc.js`.

| # | Function group | Key members | Notes |
|---|---|---|---|
| 1 | source file | `parseSourceFile` 29014, statement list | spine first |
| 2 | statements dispatch | `parseStatement` 33528 switch | stubs → missing statements |
| 3 | variable statements | `parseVariableStatement/DeclarationList/Declaration`, binding patterns | let/const/using NodeFlags |
| 4 | control statements (grouped) | if/do/while/for/forIn-Of/switch/try/labeled/break/continue/return/throw | for-header DisallowIn handling |
| 5 | blocks + expression statement + ASI | `parseExpressionOrLabeledStatement`, `canParseSemicolon`/`parseSemicolon` | ASI is observable in 1005 positions |
| 6 | primary expressions | `parsePrimaryExpression`, literals, array/object literals, paren, template expr | list engine contexts |
| 7 | member/call chains | `parseMemberExpressionRest`/`parseCallExpressionRest`, optional chain, tagged templates, type-args-in-expr lookAhead | `new` + `new.target` MetaProperty |
| 8 | unary/update | `parseUnaryExpressionOrHigher`, await/yield context gates | |
| 9 | binary/assignment/conditional | [COPY] blocks above + `parseConditionalExpressionRest` | arrow speculation fills the todo_port |
| 10 | arrow functions | `parseParenthesizedArrowFunctionExpression` + lookAhead classifiers | tryParse discipline |
| 11 | functions/classes as expressions | shared with decl parsers (#13) | |
| 12 | type grammar (grouped ~20 fns) | `parseType`, union/intersection/operator/array/tuple/function/typeLiteral/conditional/mapped/templateLiteral/import/typeof/predicate | TypeArguments/TypeMembers contexts |
| 13 | declarations | modifiers lookahead, function/class(members incl. static blocks, `#`names, accessors, index sigs)/interface/typeAlias/enum/module(dotted→NESTED)/import/export | largest group; split per kind |
| 14 | JSX | `parseJsxElementOrSelfClosingElementOrFragment` family | .tsx variant |

Per-row verification: `cargo xtask conformance --syntactic-only`
rate recorded in the commit body + targeted fixtures listed in
m1-parser-steps.md for that stage.

## Parser-emitted 2XXX (band accounting, phase 2 scope)

The parser emits a handful of BAND codes directly — they count
toward 2XXX parity and land here, not in the checker phases
(impl-checker-2xxx §10 full-band accounting): 2809 (`Declaration or
statement expected. This '=' follows a block…` — the recovery
companion of the LHS-gated assignment rule, `parseBlock` region
@33077), 2754 (`'super' may not use type arguments`,
`parseSuperExpression` @32323), 2819 (`Namespace name cannot be
'{0}'`, `parseErrorForMissingSemicolonAfter` @29620). The full
current list = `xtask codegen band-inventory` rows with
region=parser; pin each with an oracle-probed micro.
