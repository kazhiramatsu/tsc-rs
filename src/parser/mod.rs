//! Recursive-descent parser producing the Box-tree AST. Grammar *checks*
//! (modifier rules, const-init, duplicate object props, ...) follow tsc and
//! live in the checker; only true syntax errors are emitted here, so the
//! syntactic-vs-semantic diagnostic gating matches tsc's CLI.

mod decl;
mod expr;
mod stmt;
mod types;

use crate::ast::*;
use crate::diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain};
use crate::scanner::{Scanner, ScannerState, Tok};

/// tsc viableKeywordSuggestions: every entry of textToKeywordObj longer than

pub fn parse_with_jsx(text: &str, file: usize, jsx: bool) -> (SourceFileAst, Vec<Diagnostic>) {
    let mut p = Parser::new(text, file);
    p.jsx = jsx;
    p.next();
    let mut stmts = Vec::new();
    while p.token() != Tok::Eof {
        let before = p.scanner.token_start;
        if let Some(s) = p.parse_statement_or_recover() {
            stmts.push(s);
        }
        // hard guarantee of progress
        if p.scanner.token_start == before && p.token() != Tok::Eof {
            p.next();
        }
    }
    let is_module = p.saw_module_syntax || stmts_contain_module_syntax(&stmts);
    let span = Span::new(0, text.len());
    let mut diags = std::mem::take(&mut p.scanner.diags);
    diags.append(&mut p.diags);
    diags.sort_by_key(|d| (d.start, d.length));
    (
        SourceFileAst {
            comment_directives: p.scanner.comment_directives.clone(),
            stmts,
            is_module,
            span,
        },
        diags,
    )
}

fn stmts_contain_module_syntax(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Import(_) | Stmt::ExportNamed(_) => true,
        Stmt::Var(v) => has_modifier(&v.modifiers, ModifierKind::Export),
        Stmt::Func(f) => has_modifier(&f.modifiers, ModifierKind::Export),
        Stmt::Class(c) => has_modifier(&c.modifiers, ModifierKind::Export),
        Stmt::Interface(i) => has_modifier(&i.modifiers, ModifierKind::Export),
        Stmt::TypeAlias(t) => has_modifier(&t.modifiers, ModifierKind::Export),
        Stmt::Enum(e) => has_modifier(&e.modifiers, ModifierKind::Export),
        _ => false,
    })
}

struct Parser<'a> {
    jsx: bool,
    no_in: bool,
    for_await_span: Option<Span>,
    for_decl_init_span: Option<Span>,
    for_extra_decl_span: Option<Span>,
    jsx_tag_stack: Vec<String>,
    /// When true (parsing the extends-type of a conditional), an `infer X
    /// extends C` constraint keeps `C` even if a `?` follows; otherwise the
    /// `extends C ?` is reinterpreted as a nested conditional.
    disallow_conditional: bool,
    /// False while parsing the true branch of a conditional expression, where an
    /// arrow function may not carry a return-type annotation (so `a ? (b) : c`
    /// parses `(b)` as the branch and `:` as the conditional separator).
    allow_return_type_in_arrow: bool,
    scanner: Scanner<'a>,
    file: usize,
    diags: Vec<Diagnostic>,
    saw_module_syntax: bool,
    /// nesting depth inside a `namespace`/`module` body; an `export` here is a
    /// namespace export, not ES module syntax, so it must not mark the file a module.
    namespace_depth: u32,
    /// guards duplicate errors at the same position
    last_error_pos: usize,
    /// byte end of the previous token (for statement/expr end spans)
    prev_token_end: usize,
}

type Snapshot = (ScannerState, usize, usize, usize);

impl<'a> Parser<'a> {
    pub(crate) fn new(text: &'a str, file: usize) -> Parser<'a> {
        Parser {
            jsx: false,
            no_in: false,
            for_await_span: None,
            for_decl_init_span: None,
            for_extra_decl_span: None,
            jsx_tag_stack: Vec::new(),
            disallow_conditional: false,
            allow_return_type_in_arrow: true,
            scanner: Scanner::new(text, file),
            file,
            diags: Vec::new(),
            saw_module_syntax: false,
            namespace_depth: 0,
            last_error_pos: usize::MAX,
            prev_token_end: 0,
        }
    }

    // ── token plumbing ──────────────────────────────────────────────────────

    fn token(&self) -> Tok {
        self.scanner.token
    }
    fn next(&mut self) -> Tok {
        self.prev_token_end = self.scanner.token_end();
        self.scanner.scan()
    }
    fn token_span(&self) -> Span {
        Span::new(self.scanner.token_start, self.scanner.token_end())
    }
    fn token_value(&self) -> String {
        self.scanner.token_value.clone()
    }
    /// Faithful JS string value of the current string/template token (preserves
    /// lone surrogates that `token_value` cannot).
    fn token_wtf8(&self) -> crate::jsstr::JsString {
        crate::jsstr::JsString::from_wtf8_buf(self.scanner.token_wtf8.clone())
    }
    fn start(&self) -> usize {
        self.scanner.token_start
    }
    fn line_break_before(&self) -> bool {
        self.scanner.preceding_line_break
    }

    fn save(&self) -> Snapshot {
        (
            self.scanner.save(),
            self.diags.len(),
            self.prev_token_end,
            self.last_error_pos,
        )
    }
    fn restore(&mut self, s: Snapshot) {
        self.scanner.restore(s.0);
        self.diags.truncate(s.1);
        self.prev_token_end = s.2;
        self.last_error_pos = s.3;
    }

    /// Speculative parse: restores on None, keeps consumption on Some.
    fn try_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let snap = self.save();
        match f(self) {
            Some(v) => Some(v),
            None => {
                self.restore(snap);
                None
            }
        }
    }

    /// Pure lookahead: ALWAYS restores.
    fn lookahead<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let snap = self.save();
        let r = f(self);
        self.restore(snap);
        r
    }

    fn error_at(&mut self, span: Span, msg: &'static DiagnosticMessage, args: &[String]) {
        // tsc parseErrorAtPosition: at most one error per position, and the
        // scanner's diagnostics share the same stream
        if self.scanner.diags.iter().any(|d| d.start == span.start) {
            return;
        }
        self.diags.push(Diagnostic {
            file: Some(self.file),
            start: span.start,
            length: span.len(),
            message: MessageChain::new(msg, args),
            related: Vec::new(),
        });
    }

    fn error_at_current(&mut self, msg: &'static DiagnosticMessage, args: &[String]) {
        if self.scanner.token_start == self.last_error_pos {
            return; // one syntax error per token position
        }
        self.last_error_pos = self.scanner.token_start;
        let span = self.token_span();
        self.error_at(span, msg, args);
    }

    fn eat(&mut self, t: Tok) -> bool {
        if self.token() == t {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: Tok) -> bool {
        if self.token() == t {
            self.next();
            true
        } else {
            self.error_at_current(&gen::_0_expected, &[t.text().to_string()]);
            false
        }
    }

    fn can_parse_semicolon(&self) -> bool {
        self.token() == Tok::Semicolon
            || self.token() == Tok::CloseBrace
            || self.token() == Tok::Eof
            || self.line_break_before()
    }

    fn parse_semicolon(&mut self) {
        if self.token() == Tok::Semicolon {
            self.next();
        } else if self.can_parse_semicolon() {
            // ASI
        } else {
            self.error_at_current(&gen::_0_expected, &[";".to_string()]);
        }
    }

    fn is_ident_like(&self) -> bool {
        self.token() == Tok::Ident || self.token().is_contextual_keyword()
    }

    #[allow(dead_code)]
    fn parse_ident_or_private(&mut self) -> Ident {
        if self.token() == Tok::PrivateIdent {
            let span = self.token_span();
            let name = self.token_value();
            self.next();
            return Ident { name, span };
        }
        self.parse_ident()
    }

    fn parse_ident(&mut self) -> Ident {
        if self.is_ident_like() {
            let id = Ident {
                name: self.token_value(),
                span: self.token_span(),
            };
            self.next();
            id
        } else if self.token().is_strict_reserved_word() {
            // `interface`, `public`, `yield`, … used as a plain identifier
            let id = Ident {
                name: self.token_value_or_text(),
                span: self.token_span(),
            };
            self.next();
            id
        } else {
            self.error_at_current(&gen::Identifier_expected, &[]);
            Ident {
                name: String::new(),
                span: Span::new(self.start(), self.start()),
            }
        }
    }

    /// Any identifier or keyword (property-name position).
    fn parse_ident_name(&mut self) -> Ident {
        if self.token() == Tok::PrivateIdent {
            let span = self.token_span();
            let name = self.token_value();
            self.next();
            return Ident { name, span };
        }
        if self.token() == Tok::Ident || self.token().is_keyword() {
            let id = Ident {
                name: self.token_value_or_text(),
                span: self.token_span(),
            };
            self.next();
            id
        } else {
            self.error_at_current(&gen::Identifier_expected, &[]);
            Ident {
                name: String::new(),
                span: Span::new(self.start(), self.start()),
            }
        }
    }

    fn token_value_or_text(&self) -> String {
        if self.token() == Tok::Ident || self.token() == Tok::StrLit || self.token() == Tok::NumLit
        {
            self.token_value()
        } else {
            self.token().text().to_string()
        }
    }

    // ── statements ──────────────────────────────────────────────────────────
}
