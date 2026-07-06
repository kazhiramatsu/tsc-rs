//! Statement parsing: the statement dispatcher and recovery, declarations with
//! modifiers/decorators, blocks, variable statements & binding patterns, control
//! flow (if/for/try/switch), imports/exports, and function/param/type-parameter
//! parsing. Split out of `parser/mod.rs`.

use super::Parser;
use crate::ast::*;
use crate::diagnostics::gen;
use crate::scanner::Tok;

impl<'a> Parser<'a> {
    pub(crate) fn parse_statement_or_recover(&mut self) -> Option<Stmt> {
        if self.can_start_statement() {
            Some(self.parse_statement())
        } else {
            self.error_at_current(&gen::Declaration_or_statement_expected, &[]);
            self.next();
            None
        }
    }

    fn can_start_statement(&self) -> bool {
        use Tok::*;
        match self.token() {
            CloseBrace | CloseParen | CloseBracket | Comma | Colon | Eof => false,
            _ => true,
        }
    }

    fn parse_statement(&mut self) -> Stmt {
        use Tok::*;
        match self.token() {
            Semicolon => {
                let span = self.token_span();
                self.next();
                Stmt::Empty { span }
            }
            OpenBrace => Stmt::Block(self.parse_block()),
            KConst
                if self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == Tok::KEnum {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                let start = self.start();
                self.next(); // const
                let mut e = self.parse_enum(Vec::new(), true);
                e.span.start = start as u32;
                Stmt::Enum(Box::new(e))
            }
            KVar | KConst => self.parse_var_stmt(Vec::new()),
            // `let` is contextual: it starts a lexical declaration only when
            // followed (on the same logical token) by a binding identifier or
            // the start of a destructuring pattern (`{`/`[`). Otherwise it falls
            // through to the catch-all and is parsed as a plain identifier
            // expression (sloppy-mode `let`). Mirrors tsc isLetDeclaration /
            // nextTokenIsBindingIdentifierOrStartOfDestructuring.
            KLet if self
                .lookahead(|p| {
                    p.next();
                    if p.is_ident_like()
                        || p.token().is_strict_reserved_word()
                        || matches!(p.token(), Tok::OpenBrace | Tok::OpenBracket)
                    {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some() =>
            {
                self.parse_var_stmt(Vec::new())
            }
            KFunction => Stmt::Func(Box::new(
                self.parse_function(Vec::new(), FuncKind::Declaration),
            )),
            KClass => Stmt::Class(Box::new(self.parse_class(Vec::new()))),
            KDeclare
                if self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == KImport {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                let dspan = self.token_span();
                self.next();
                let mut s = self.parse_import();
                if let Stmt::Import(i) = &mut s {
                    i.modifiers.push(Modifier {
                        kind: ModifierKind::Declare,
                        span: dspan,
                    });
                }
                s
            }
            At => {
                let decorators = self.parse_decorators();
                // decorators are only valid on class declarations (others → 1206 in the checker)
                let mods = if matches!(self.token(), KExport | KDeclare | KAbstract) {
                    self.parse_modifiers()
                } else {
                    Vec::new()
                };
                if self.token() == KClass {
                    let mut c = self.parse_class(mods);
                    if let Some(first) = decorators.first() {
                        c.span.start = first.span.start;
                    }
                    c.decorators = decorators;
                    Stmt::Class(Box::new(c))
                } else {
                    // Decorators on a non-class declaration are a grammar error
                    // (TS1206), reported by the checker — not a parse error. Parse
                    // the underlying statement so syntax stays well-formed.
                    self.parse_statement()
                }
            }
            // tsc isStartOfDeclaration for `interface`:
            KInterface => Stmt::Interface(Box::new(self.parse_interface(Vec::new()))),
            KType if self.is_type_alias_start() => {
                Stmt::TypeAlias(Box::new(self.parse_type_alias(Vec::new())))
            }
            KEnum => Stmt::Enum(Box::new(self.parse_enum(Vec::new(), false))),
            KReturn => {
                let start = self.start();
                self.next();
                let expr = if self.token() != Semicolon
                    && self.token() != CloseBrace
                    && self.token() != Eof
                    && !self.line_break_before()
                {
                    Some(self.parse_expression())
                } else {
                    None
                };
                let end = self.prev_end();
                self.parse_semicolon();
                Stmt::Return {
                    expr,
                    span: Span::new(start, end),
                }
            }
            KIf => self.parse_if(),
            KWhile => {
                let start = self.start();
                self.next();
                self.expect(OpenParen);
                let cond = self.parse_expression();
                self.expect(CloseParen);
                let body = Box::new(self.parse_statement());
                let end = self.prev_end();
                Stmt::While {
                    cond,
                    body,
                    span: Span::new(start, end),
                }
            }
            KDo => {
                let start = self.start();
                self.next();
                let body = Box::new(self.parse_statement());
                self.expect(KWhile);
                self.expect(OpenParen);
                let cond = self.parse_expression();
                self.expect(CloseParen);
                let end = self.prev_end();
                self.eat(Semicolon);
                Stmt::DoWhile {
                    body,
                    cond,
                    span: Span::new(start, end),
                }
            }
            KFor => self.parse_for(),
            KBreak | KContinue => {
                let is_break = self.token() == KBreak;
                let start = self.start();
                self.next();
                let label = if self.is_ident_like() && !self.line_break_before() {
                    Some(self.parse_ident())
                } else {
                    None
                };
                let end = self.prev_end();
                self.parse_semicolon();
                if is_break {
                    Stmt::Break {
                        label,
                        span: Span::new(start, end),
                    }
                } else {
                    Stmt::Continue {
                        label,
                        span: Span::new(start, end),
                    }
                }
            }
            KThrow => {
                let start = self.start();
                self.next();
                let expr = self.parse_expression();
                let end = self.prev_end();
                self.parse_semicolon();
                Stmt::Throw {
                    expr,
                    span: Span::new(start, end),
                }
            }
            KTry => self.parse_try(),
            KSwitch => self.parse_switch(),
            KWith => {
                let start = self.start();
                let kw_span = self.token_span();
                self.next();
                self.expect(Tok::OpenParen);
                let obj = self.parse_expression();
                self.expect(Tok::CloseParen);
                let body = Box::new(self.parse_statement());
                let span = Span::new(start, self.prev_end());
                Stmt::With {
                    obj,
                    body,
                    kw_span,
                    span,
                }
            }
            KImport
                if self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() {
                            p.next();
                            if p.token() == Tok::Eq {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some() =>
            {
                let start = self.start();
                self.next();
                let name = self.parse_ident();
                self.expect(Tok::Eq);
                let (module, is_require) = self.parse_module_reference();
                // `import a = require("m")` is an external module indicator
                // (tsc isAnExternalModuleIndicatorNode); `import a = Entity`
                // is not
                if is_require && self.namespace_depth == 0 {
                    self.saw_module_syntax = true;
                }
                let end = self.prev_end();
                self.parse_semicolon();
                Stmt::ImportEquals {
                    name,
                    module,
                    exported: false,
                    is_require,
                    span: Span::new(start, end),
                }
            }
            KImport
                if self
                    .lookahead(|p| {
                        p.next();
                        if matches!(p.token(), Tok::OpenParen | Tok::Dot | Tok::Lt) {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                // `import(...)` (dynamic import), `import.meta`, or `import<...>`
                // at statement position is an expression statement, not an
                // import clause.
                self.parse_expression_statement()
            }
            KImport => self.parse_import(),
            KExport => self.parse_export(),
            KPrivate | KProtected | KPublic | KStatic
                if self
                    .lookahead(|p| {
                        p.next();
                        if matches!(
                            p.token(),
                            KConst | KLet | KVar | KFunction | KClass | KInterface | KEnum | KType
                        ) || (p.is_ident_like()
                            && matches!(
                                p.token_value().as_str(),
                                "namespace" | "module" | "global"
                            ))
                        {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                if let Some(s) = self.try_parse_modified_declaration() {
                    s
                } else {
                    self.parse_expression_statement()
                }
            }
            KDeclare | KAbstract | KAsync => {
                if let Some(s) = self.try_parse_modified_declaration() {
                    s
                } else {
                    self.parse_expression_statement()
                }
            }
            _ if self.is_ident_like()
                && self.token_value() == "accessor"
                && self
                    .lookahead(|p| {
                        p.next();
                        if matches!(
                            p.token(),
                            Tok::KConst
                                | Tok::KLet
                                | Tok::KVar
                                | Tok::KFunction
                                | Tok::KClass
                                | Tok::KInterface
                                | Tok::KEnum
                                | Tok::KType
                                | Tok::KImport
                                | Tok::KExport
                        ) || (p.is_ident_like()
                            && matches!(
                                p.token_value().as_str(),
                                "namespace" | "module" | "global"
                            ))
                        {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                if let Some(s) = self.try_parse_modified_declaration() {
                    s
                } else {
                    self.parse_expression_statement()
                }
            }
            _ if self.token() == Tok::KAwait
                && self
                    .lookahead(|p| {
                        p.next(); // await
                        if p.is_ident_like() && p.token_value() == "using" {
                            p.next(); // using
                                      // NOTE: tsc rejects `[` here too (`await using [x]`
                                      // is an await of an element access); accepting it
                                      // keeps our historical recovery profile — flipping
                                      // it forfeits the file's semantic diagnostics to
                                      // the any-parse-error gate (see check_program_core)
                            if !p.line_break_before()
                                && (matches!(p.token(), Tok::OpenBrace | Tok::OpenBracket)
                                    || (p.is_ident_like()
                                        && p.token_value() != "of"
                                        && p.token_value() != "in"))
                            {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some() =>
            {
                let await_start = self.start();
                self.next(); // await
                let mut s = self.parse_var_stmt(Vec::new());
                if let Stmt::Var(v) = &mut s {
                    v.span.start = await_start as u32;
                }
                s
            }
            _ if self.is_ident_like()
                && self.token_value() == "using"
                && self
                    .lookahead(|p| {
                        p.next();
                        // `using x = …` — a binding identifier or `{` on the
                        // same line (tsc allows object patterns through to the
                        // grammar error; `using [x]` is an element access, and
                        // `using` before a line break or `of`/`in` is a plain
                        // identifier).
                        if !p.line_break_before()
                            && (p.token() == Tok::OpenBrace
                                || (p.is_ident_like()
                                    && p.token_value() != "of"
                                    && p.token_value() != "in"))
                        {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                self.parse_var_stmt(Vec::new())
            }
            _ if self.is_ident_like()
                && (self.token_value() == "namespace"
                    || self.token_value() == "module"
                    || self.token_value() == "global")
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() || matches!(p.token(), Tok::StrLit | Tok::OpenBrace) {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                self.parse_namespace(Vec::new())
            }
            _ => {
                // labeled statement?
                if self.is_ident_like() {
                    if let Some(labeled) = self.try_parse(|p| {
                        let label = p.parse_ident();
                        if p.token() == Colon {
                            p.next();
                            Some(label)
                        } else {
                            None
                        }
                    }) {
                        let stmt = Box::new(self.parse_statement());
                        let span = Span::new(labeled.span.start as usize, stmt.span().end as usize);
                        return Stmt::Labeled {
                            label: labeled,
                            stmt,
                            span,
                        };
                    }
                }
                self.parse_expression_statement()
            }
        }
    }

    fn is_type_alias_start(&mut self) -> bool {
        // `type X =` — `type` is contextual
        self.lookahead(|p| {
            p.next();
            if p.is_ident_like() {
                p.next();
                // possibly type params
                if p.token() == Tok::Lt || p.token() == Tok::Eq {
                    return Some(());
                }
            }
            None
        })
        .is_some()
    }

    pub(crate) fn prev_end(&self) -> usize {
        self.prev_token_end
    }

    fn parse_expression_statement(&mut self) -> Stmt {
        let start = self.start();
        let expr = self.parse_expression();
        let end = self.prev_end();
        self.parse_semicolon();
        Stmt::Expr {
            expr,
            span: Span::new(start, end),
        }
    }

    fn try_parse_modified_declaration(&mut self) -> Option<Stmt> {
        self.try_parse(|p| {
            let mods = p.parse_modifiers();
            if mods.is_empty() {
                return None;
            }
            use Tok::*;
            match p.token() {
                KConst
                    if p.lookahead(|q| {
                        q.next();
                        if q.token() == Tok::KEnum {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
                {
                    p.next();
                    Some(Stmt::Enum(Box::new(p.parse_enum(mods, true))))
                }
                KVar | KLet | KConst => Some(p.parse_var_stmt(mods)),
                // `export using x = …` / `export await using x = …`
                _ if p.is_ident_like()
                    && p.token_value() == "using"
                    && p.lookahead(|q| {
                        q.next();
                        if (q.is_ident_like() && q.token_value() != "of" && q.token_value() != "in")
                            || q.token() == Tok::OpenBrace
                        {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
                {
                    Some(p.parse_var_stmt(mods))
                }
                KAwait
                    if p.lookahead(|q| {
                        q.next();
                        if q.is_ident_like() && q.token_value() == "using" {
                            q.next();
                            if q.is_ident_like() || q.token() == Tok::OpenBrace {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some() =>
                {
                    p.next(); // await
                    Some(p.parse_var_stmt(mods))
                }
                KFunction => Some(Stmt::Func(Box::new(
                    p.parse_function(mods, FuncKind::Declaration),
                ))),
                KClass => Some(Stmt::Class(Box::new(p.parse_class(mods)))),
                KInterface => Some(Stmt::Interface(Box::new(p.parse_interface(mods)))),
                KType => Some(Stmt::TypeAlias(Box::new(p.parse_type_alias(mods)))),
                KEnum => Some(Stmt::Enum(Box::new(p.parse_enum(mods, false)))),
                // Modifiers are never valid on import/export declarations
                // (`async import …`, `accessor export …`), but tsc parses them
                // and defers the error to the checker.
                KImport => {
                    let _ = mods;
                    Some(p.parse_import())
                }
                KExport => {
                    let _ = mods;
                    Some(p.parse_export())
                }
                _ if p.is_ident_like()
                    && (p.token_value() == "namespace"
                        || p.token_value() == "module"
                        || p.token_value() == "global") =>
                {
                    let ok = p
                        .lookahead(|q| {
                            q.next();
                            // name: an identifier (possibly dotted), a string
                            // literal (ambient module), or `{` (global block)
                            if q.is_ident_like()
                                || matches!(q.token(), Tok::StrLit | Tok::OpenBrace)
                            {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some();
                    if ok {
                        Some(p.parse_namespace(mods))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
    }

    pub(crate) fn parse_decorators(&mut self) -> Vec<Decorator> {
        let mut out = Vec::new();
        while self.token() == Tok::At {
            let at_span = self.token_span();
            let start = self.start();
            self.next();
            let expr = self.parse_decorator_expr();
            out.push(Decorator {
                at_span,
                expr,
                span: Span::new(start, self.prev_end()),
            });
        }
        out
    }

    /// A decorator expression: identifier (or `(expr)`) followed by `.name`
    /// member accesses and `(...)` calls only. Computed access `[...]` is *not*
    /// part of the grammar (it terminates the decorator), so `@dec ["x"]()`
    /// reads `["x"]` as the decorated member's name.
    fn parse_decorator_expr(&mut self) -> Expr {
        let start = self.start();
        let mut e = if self.token() == Tok::KNew {
            // `@new X` / `@new X(args)` decorator
            self.parse_new_expr()
        } else if self.token() == Tok::OpenParen {
            self.next();
            let inner = self.parse_expression();
            self.expect(Tok::CloseParen);
            Expr::Paren {
                inner: Box::new(inner),
                span: Span::new(start, self.prev_end()),
            }
        } else {
            Expr::Ident(self.parse_ident())
        };
        loop {
            match self.token() {
                Tok::Dot => {
                    self.next();
                    let name = self.parse_ident_name();
                    e = Expr::PropAccess {
                        obj: Box::new(e),
                        question_dot: false,
                        name,
                        span: Span::new(start, self.prev_end()),
                    };
                }
                Tok::OpenParen => {
                    let args = self.parse_arguments();
                    e = Expr::Call {
                        callee: Box::new(e),
                        question_dot: false,
                        type_args: None,
                        args,
                        span: Span::new(start, self.prev_end()),
                    };
                }
                Tok::QuestionDot => {
                    // optional-chain member/element/call in a decorator
                    // (`@x?.y`, `@x?.["y"]`, `@x?.()`) — invalid but parsed.
                    self.next();
                    if self.token() == Tok::OpenParen {
                        let args = self.parse_arguments();
                        e = Expr::Call {
                            callee: Box::new(e),
                            question_dot: true,
                            type_args: None,
                            args,
                            span: Span::new(start, self.prev_end()),
                        };
                    } else if self.token() == Tok::OpenBracket {
                        self.next();
                        let index = self.parse_expression();
                        let end = self.scanner.token_end();
                        self.expect(Tok::CloseBracket);
                        e = Expr::ElemAccess {
                            obj: Box::new(e),
                            question_dot: true,
                            index: Box::new(index),
                            span: Span::new(start, end),
                        };
                    } else {
                        let name = self.parse_ident_name();
                        e = Expr::PropAccess {
                            obj: Box::new(e),
                            question_dot: true,
                            name,
                            span: Span::new(start, self.prev_end()),
                        };
                    }
                }
                Tok::Lt => {
                    // type arguments before a call: `@g<T>()`
                    let targs = self.try_parse_type_args();
                    if targs.is_some() && self.token() == Tok::OpenParen {
                        let args = self.parse_arguments();
                        e = Expr::Call {
                            callee: Box::new(e),
                            question_dot: false,
                            type_args: None,
                            args,
                            span: Span::new(start, self.prev_end()),
                        };
                    } else {
                        break;
                    }
                }
                Tok::NoSubTemplate | Tok::TemplateHead => {
                    // tagged template in a decorator (`@x\`\`()`) — invalid but
                    // parsed (the checker reports the error).
                    let t = self.parse_template_expr();
                    e = Expr::Call {
                        callee: Box::new(e),
                        question_dot: false,
                        type_args: None,
                        args: vec![t],
                        span: Span::new(start, self.prev_end()),
                    };
                }
                Tok::Bang if !self.line_break_before() => {
                    self.next();
                    e = Expr::NonNull {
                        expr: Box::new(e),
                        span: Span::new(start, self.prev_end()),
                    };
                }
                _ => break,
            }
        }
        e
    }

    fn parse_modifiers(&mut self) -> Modifiers {
        let mut mods = Vec::new();
        loop {
            let kind = match self.token() {
                // `export` before `{`/`*`/`default` is the export-declaration
                // keyword, not a modifier (e.g. `accessor export { V }`,
                // `accessor export default V`).
                Tok::KExport
                    if !self
                        .lookahead(|p| {
                            p.next();
                            if matches!(p.token(), Tok::OpenBrace | Tok::Star | Tok::KDefault) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some() =>
                {
                    ModifierKind::Export
                }
                Tok::KDeclare => ModifierKind::Declare,
                Tok::KAbstract => ModifierKind::Abstract,
                Tok::KAsync => ModifierKind::Async,
                Tok::KDefault => ModifierKind::Default,
                Tok::KPrivate | Tok::KProtected | Tok::KPublic
                    if self
                        .lookahead(|p| {
                            p.next();
                            if matches!(
                                p.token(),
                                Tok::KConst
                                    | Tok::KLet
                                    | Tok::KVar
                                    | Tok::KFunction
                                    | Tok::KClass
                                    | Tok::KInterface
                                    | Tok::KEnum
                                    | Tok::KType
                            ) || (p.is_ident_like()
                                && matches!(
                                    p.token_value().as_str(),
                                    "namespace" | "module" | "global"
                                ))
                            {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some() =>
                {
                    match self.token() {
                        Tok::KPrivate => ModifierKind::Private,
                        Tok::KProtected => ModifierKind::Protected,
                        _ => ModifierKind::Public,
                    }
                }
                // `static` is not a valid statement modifier, but tsc parses
                // `export static var x` and defers the error to the checker.
                Tok::KStatic
                    if self
                        .lookahead(|p| {
                            p.next();
                            if matches!(
                                p.token(),
                                Tok::KConst
                                    | Tok::KLet
                                    | Tok::KVar
                                    | Tok::KFunction
                                    | Tok::KClass
                                    | Tok::KInterface
                                    | Tok::KEnum
                                    | Tok::KType
                            ) || (p.is_ident_like()
                                && matches!(
                                    p.token_value().as_str(),
                                    "namespace" | "module" | "global"
                                ))
                            {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some() =>
                {
                    ModifierKind::Static
                }
                // `accessor` is a contextual modifier; on a non-field
                // declaration (`accessor interface I`) it is invalid but parsed,
                // with the checker reporting the error.
                _ if self.is_ident_like()
                    && self.token_value() == "accessor"
                    && self
                        .lookahead(|p| {
                            p.next();
                            if matches!(
                                p.token(),
                                Tok::KConst
                                    | Tok::KLet
                                    | Tok::KVar
                                    | Tok::KFunction
                                    | Tok::KClass
                                    | Tok::KInterface
                                    | Tok::KEnum
                                    | Tok::KType
                                    | Tok::KImport
                                    | Tok::KExport
                            ) || (p.is_ident_like()
                                && matches!(
                                    p.token_value().as_str(),
                                    "namespace" | "module" | "global"
                                ))
                            {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some() =>
                {
                    ModifierKind::Accessor
                }
                _ => break,
            };
            // only treat as modifier when followed by more declaration stuff
            let span = self.token_span();
            let snap = self.save();
            self.next();
            if self.line_break_before() && kind == ModifierKind::Async {
                self.restore(snap);
                break;
            }
            mods.push(Modifier { kind, span });
        }
        mods
    }

    pub(crate) fn parse_block(&mut self) -> Block {
        let start = self.start();
        self.expect(Tok::OpenBrace);
        let mut stmts = Vec::new();
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let before = self.scanner.token_start;
            if let Some(s) = self.parse_statement_or_recover() {
                stmts.push(s);
            }
            if self.scanner.token_start == before
                && self.token() != Tok::CloseBrace
                && self.token() != Tok::Eof
            {
                self.next();
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        Block {
            stmts,
            span: Span::new(start, end),
        }
    }

    fn parse_var_stmt(&mut self, modifiers: Modifiers) -> Stmt {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        let kw_span = self.token_span();
        let kind = match self.token() {
            Tok::KVar => VarKind::Var,
            Tok::KLet => VarKind::Let,
            _ => VarKind::Const,
        };
        // reached with a non-var/let/const head token only from the
        // `using` / `await using` statement dispatch
        let is_using = !matches!(self.token(), Tok::KVar | Tok::KLet | Tok::KConst);
        self.next();
        let mut decls = Vec::new();
        loop {
            // An empty or dangling declaration list (`var;`, `var a,`) is a
            // grammar error (TS1123) reported by the checker; stop without a
            // parse error when no binding follows.
            let binding_starts = self.is_ident_like()
                || self.token().is_strict_reserved_word()
                || matches!(self.token(), Tok::OpenBrace | Tok::OpenBracket | Tok::KThis);
            if !binding_starts {
                break;
            }
            let dstart = self.start();
            let name = self.parse_binding();
            let exclam_span = if self.token() == Tok::Bang {
                let span = self.token_span();
                self.next();
                Some(span)
            } else {
                None
            };
            let exclam = exclam_span.is_some();
            let ty = if self.eat(Tok::Colon) {
                let t = self.parse_type();
                // A trailing `?` on a variable's type annotation (`var x: T?`) is
                // a JSDoc nullable marker, invalid in TS but parsed (checker-reported).
                if self.token() == Tok::Question && !self.line_break_before() {
                    self.next();
                }
                Some(t)
            } else {
                None
            };
            let init = if self.eat(Tok::Eq) {
                Some(self.parse_assignment_expr())
            } else {
                None
            };
            let dend = self.prev_end();
            decls.push(VarDeclarator {
                name,
                exclam,
                exclam_span,
                ty,
                init,
                span: Span::new(dstart, dend),
            });
            if self.eat(Tok::Comma) {
                continue;
            }
            if !self.can_parse_semicolon() && self.token() != Tok::KOf {
                // tsc's delimited-list behavior: a non-terminator yields
                // "',' expected" here (the later ';' error at the same
                // position is suppressed by one-error-per-position).
                self.error_at_current(&gen::_0_expected, &[",".to_string()]);
            }
            break;
        }
        let end = self.prev_end();
        self.parse_semicolon();
        Stmt::Var(VarStmt {
            modifiers,
            kind,
            is_using,
            decls,
            kw_span,
            span: Span::new(start, end),
        })
    }

    pub(crate) fn parse_binding(&mut self) -> Binding {
        match self.token() {
            Tok::OpenBrace => Binding::Object(self.parse_object_pattern()),
            Tok::OpenBracket => Binding::Array(self.parse_array_pattern()),
            _ => Binding::Ident(self.parse_ident()),
        }
    }

    fn parse_object_pattern(&mut self) -> ObjectPattern {
        let start = self.start();
        self.expect(Tok::OpenBrace);
        let mut props = Vec::new();
        let mut rest = None;
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            if self.eat(Tok::DotDotDot) {
                // A rest element that is not last is a grammar error reported
                // by the checker; keep parsing so the rest of the pattern is
                // still consumed.
                let b = self.parse_binding();
                // `...name: binding` / `...name = init` — a rename or default on
                // a rest element is a grammar error but parsed (tsc keeps it).
                let b = if self.eat(Tok::Colon) {
                    self.parse_binding()
                } else {
                    b
                };
                if self.eat(Tok::Eq) {
                    let _ = self.parse_assignment_expr();
                }
                if rest.is_none() {
                    rest = Some(Box::new(b));
                }
                if !self.eat(Tok::Comma) {
                    break;
                }
                continue;
            }
            let pstart = self.start();
            let key = self.parse_prop_name();
            let binding: Binding = if self.eat(Tok::Colon) {
                self.parse_binding()
            } else if let PropName::Ident(id) = &key {
                Binding::Ident(id.clone())
            } else {
                self.error_at_current(&gen::_0_expected, &[":".to_string()]);
                Binding::Ident(Ident {
                    name: String::new(),
                    span: key.span(),
                })
            };
            let default = if self.eat(Tok::Eq) {
                Some(self.parse_assignment_expr())
            } else {
                None
            };
            props.push(ObjectPatternProp {
                key,
                binding: Box::new(binding),
                default,
                span: Span::new(pstart, self.prev_end()),
            });
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        ObjectPattern {
            props,
            rest,
            span: Span::new(start, end),
        }
    }

    fn parse_array_pattern(&mut self) -> ArrayPattern {
        let start = self.start();
        self.expect(Tok::OpenBracket);
        let mut elements = Vec::new();
        while self.token() != Tok::CloseBracket && self.token() != Tok::Eof {
            if self.token() == Tok::Comma {
                elements.push(None);
                self.next();
                continue;
            }
            let estart = self.start();
            let rest = self.eat(Tok::DotDotDot);
            let binding = self.parse_binding();
            let default = if self.eat(Tok::Eq) {
                Some(self.parse_assignment_expr())
            } else {
                None
            };
            elements.push(Some(ArrayPatternElem {
                binding: Box::new(binding),
                default,
                rest,
                span: Span::new(estart, self.prev_end()),
            }));
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBracket);
        ArrayPattern {
            elements,
            span: Span::new(start, end),
        }
    }

    fn parse_if(&mut self) -> Stmt {
        let start = self.start();
        self.next();
        self.expect(Tok::OpenParen);
        let cond = self.parse_expression();
        self.expect(Tok::CloseParen);
        let then = Box::new(self.parse_statement());
        let els = if self.eat(Tok::KElse) {
            Some(Box::new(self.parse_statement()))
        } else {
            None
        };
        let end = self.prev_end();
        Stmt::If {
            cond,
            then,
            els,
            span: Span::new(start, end),
        }
    }

    fn parse_for(&mut self) -> Stmt {
        let start = self.start();
        self.next();
        let await_span = if self.token() == Tok::KAwait {
            let s = self.token_span();
            self.next();
            Some(s)
        } else {
            None
        };
        self.for_await_span = await_span;
        self.expect(Tok::OpenParen);
        let init: Option<Box<ForInit>> = if self.token() == Tok::Semicolon {
            None
        } else if matches!(self.token(), Tok::KVar | Tok::KLet | Tok::KConst)
            || (self.is_ident_like()
                && self.token_value() == "using"
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.line_break_before() {
                            return None;
                        }
                        if p.token() == Tok::OpenBrace {
                            return Some(());
                        }
                        if p.is_ident_like() {
                            // `using of`/`using in` — `of`/`in` names the binding
                            // only when followed by `=`/`:`/`;`/`,`; otherwise it
                            // is the for-of/in keyword (empty/error binding).
                            if p.token_value() == "of" || p.token_value() == "in" {
                                p.next();
                                if matches!(
                                    p.token(),
                                    Tok::Eq | Tok::Colon | Tok::Semicolon | Tok::Comma
                                ) {
                                    return Some(());
                                }
                                return None;
                            }
                            return Some(());
                        }
                        None
                    })
                    .is_some())
            || (self.token() == Tok::KAwait
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() && p.token_value() == "using" {
                            p.next();
                            if !p.line_break_before()
                                && (p.token() == Tok::KOf
                                    || p.is_ident_like()
                                    || p.token() == Tok::OpenBrace)
                            {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some())
        {
            // `await using x of …` — consume the leading await first; the
            // list span still starts at `await` (tsc anchors 6199 there)
            let head_await_start = if self.token() == Tok::KAwait {
                let s = self.start();
                self.next();
                Some(s)
            } else {
                None
            };
            let kw_span = self.token_span();
            let kind = match self.token() {
                Tok::KVar => VarKind::Var,
                Tok::KLet => VarKind::Let,
                _ => VarKind::Const,
            };
            let head_is_using = !matches!(self.token(), Tok::KVar | Tok::KLet | Tok::KConst);
            let vstart = head_await_start.unwrap_or_else(|| self.start());
            self.next();
            let dstart = self.start();
            let empty_binding = Binding::Ident(crate::ast::Ident {
                name: String::new(),
                span: Span::new(dstart, dstart),
            });
            // Disambiguate `of`/`in` after the declaration keyword. `in` is a
            // reserved word, so `for (var in …)` always has an empty binding.
            // `of` is a valid identifier, so it is the for-of keyword (empty
            // binding) only in `for (var of <ident>)` — i.e. when an identifier
            // and then `)` follow; otherwise `of` is the binding name
            // (`for (var of of arr)`, `for (var of; ;)`).
            let name = if self.token() == Tok::KIn {
                empty_binding
            } else if self.token() == Tok::KOf
                && self
                    .lookahead(|p| {
                        p.next(); // of
                        if p.is_ident_like() || p.token().is_strict_reserved_word() {
                            p.next();
                            if p.token() == Tok::CloseParen {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some()
            {
                empty_binding
            } else {
                self.parse_binding()
            };
            let empty_decl = matches!(&name, Binding::Ident(id) if id.name.is_empty());
            let empty_decl_span = Span::new(self.prev_end(), self.prev_end());
            // optional annotation (legal only in plain for; 2483/2404 in of/in)
            let early_ty = if self.token() == Tok::Colon {
                self.next();
                Some(self.parse_type())
            } else {
                None
            };
            // optional initializer (no-in); illegal in of/in (1189/1190)
            let early_init = if self.token() == Tok::Eq {
                self.next();
                self.no_in = true;
                let e = self.parse_assignment_expr();
                self.no_in = false;
                Some(e)
            } else {
                None
            };
            // extra declarators: illegal in of/in (1091/1188)
            let mut extra_decl_names: Vec<Span> = Vec::new();
            if self.token() == Tok::Comma
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() {
                            p.next();
                            if matches!(p.token(), Tok::KIn | Tok::KOf) {
                                return Some(());
                            }
                        }
                        None
                    })
                    .is_some()
            {
                self.next();
                let extra = self.parse_ident();
                extra_decl_names.push(extra.span);
            }
            // for-in/of?
            if self.token() == Tok::KOf || self.token() == Tok::KIn {
                let is_of = self.token() == Tok::KOf;
                if let Some(init) = &early_init {
                    let _ = init;
                    self.for_decl_init_span = Some(name.span());
                }
                self.for_extra_decl_span = extra_decl_names.first().copied();
                self.next();
                // for-in iterates an Expression (commas allowed); for-of takes
                // an AssignmentExpression (no top-level comma).
                let expr = if is_of {
                    self.parse_assignment_expr()
                } else {
                    self.parse_expression()
                };
                self.expect(Tok::CloseParen);
                let body = Box::new(self.parse_statement());
                let end = self.prev_end();
                let decls = if empty_decl {
                    Vec::new()
                } else {
                    vec![VarDeclarator {
                        name,
                        exclam: false,
                        exclam_span: None,
                        ty: early_ty,
                        init: early_init,
                        span: Span::new(dstart, dstart),
                    }]
                };
                let left = Box::new(ForInit::Var(VarStmt {
                    modifiers: Vec::new(),
                    kind,
                    is_using: head_is_using,
                    decls,
                    kw_span,
                    span: if empty_decl {
                        empty_decl_span
                    } else {
                        Span::new(vstart, vstart)
                    },
                }));
                return if is_of {
                    Stmt::ForOf {
                        left,
                        expr,
                        body,
                        await_span: self.for_await_span.take(),
                        init_span: self.for_decl_init_span.take(),
                        extra_decl_span: self.for_extra_decl_span.take(),
                        span: Span::new(start, end),
                    }
                } else {
                    Stmt::ForIn {
                        left,
                        expr,
                        body,
                        init_span: self.for_decl_init_span.take(),
                        extra_decl_span: self.for_extra_decl_span.take(),
                        span: Span::new(start, end),
                    }
                };
            }
            let exclam_span = if early_ty.is_none() && self.token() == Tok::Bang {
                let span = self.token_span();
                self.next();
                Some(span)
            } else {
                None
            };
            let exclam = exclam_span.is_some();
            let ty = match early_ty {
                Some(t) => Some(t),
                None => {
                    if self.eat(Tok::Colon) {
                        Some(self.parse_type())
                    } else {
                        None
                    }
                }
            };
            let ini = match early_init {
                Some(e) => Some(e),
                None => {
                    if self.eat(Tok::Eq) {
                        Some(self.parse_assignment_expr())
                    } else {
                        None
                    }
                }
            };
            let mut decls = vec![VarDeclarator {
                name,
                exclam,
                exclam_span,
                ty,
                init: ini,
                span: Span::new(dstart, self.prev_end()),
            }];
            while self.eat(Tok::Comma) {
                let dstart = self.start();
                let name = self.parse_binding();
                let exclam_span = if self.token() == Tok::Bang {
                    let span = self.token_span();
                    self.next();
                    Some(span)
                } else {
                    None
                };
                let exclam = exclam_span.is_some();
                let ty = if self.eat(Tok::Colon) {
                    Some(self.parse_type())
                } else {
                    None
                };
                let ini = if self.eat(Tok::Eq) {
                    // `in` here is the for-in keyword, not an operator
                    self.no_in = true;
                    let e = self.parse_assignment_expr();
                    self.no_in = false;
                    Some(e)
                } else {
                    None
                };
                decls.push(VarDeclarator {
                    name,
                    exclam,
                    exclam_span,
                    ty,
                    init: ini,
                    span: Span::new(dstart, self.prev_end()),
                });
            }
            // A multi-declarator list (or declarators with initializers)
            // followed by `of`/`in` is a grammar error (checker-reported);
            // parse it as a for-of/for-in over the whole declaration list.
            if self.token() == Tok::KOf || self.token() == Tok::KIn {
                let is_of = self.token() == Tok::KOf;
                self.next();
                // for-in iterates an Expression (commas allowed); for-of takes
                // an AssignmentExpression (no top-level comma).
                let expr = if is_of {
                    self.parse_assignment_expr()
                } else {
                    self.parse_expression()
                };
                self.expect(Tok::CloseParen);
                let body = Box::new(self.parse_statement());
                let end = self.prev_end();
                let left = Box::new(ForInit::Var(VarStmt {
                    modifiers: Vec::new(),
                    kind,
                    is_using: head_is_using,
                    decls,
                    kw_span,
                    span: Span::new(vstart, self.prev_end()),
                }));
                return if is_of {
                    Stmt::ForOf {
                        left,
                        expr,
                        body,
                        await_span: self.for_await_span.take(),
                        init_span: None,
                        extra_decl_span: None,
                        span: Span::new(start, end),
                    }
                } else {
                    Stmt::ForIn {
                        left,
                        expr,
                        body,
                        init_span: None,
                        extra_decl_span: None,
                        span: Span::new(start, end),
                    }
                };
            }
            Some(Box::new(ForInit::Var(VarStmt {
                modifiers: Vec::new(),
                kind,
                is_using: head_is_using,
                decls,
                kw_span,
                span: Span::new(vstart, self.prev_end()),
            })))
        } else {
            self.no_in = true;
            let e = self.parse_expression();
            self.no_in = false;
            if self.token() == Tok::KOf || self.token() == Tok::KIn {
                let is_of = self.token() == Tok::KOf;
                self.next();
                // for-in iterates an Expression (commas allowed); for-of takes
                // an AssignmentExpression (no top-level comma).
                let expr = if is_of {
                    self.parse_assignment_expr()
                } else {
                    self.parse_expression()
                };
                self.expect(Tok::CloseParen);
                let body = Box::new(self.parse_statement());
                let end = self.prev_end();
                let left = Box::new(ForInit::Expr(e));
                return if is_of {
                    Stmt::ForOf {
                        left,
                        expr,
                        body,
                        await_span: self.for_await_span.take(),
                        init_span: None,
                        extra_decl_span: None,
                        span: Span::new(start, end),
                    }
                } else {
                    Stmt::ForIn {
                        left,
                        expr,
                        body,
                        init_span: None,
                        extra_decl_span: None,
                        span: Span::new(start, end),
                    }
                };
            }
            Some(Box::new(ForInit::Expr(e)))
        };
        self.expect(Tok::Semicolon);
        let cond = if self.token() != Tok::Semicolon {
            Some(self.parse_expression())
        } else {
            None
        };
        self.expect(Tok::Semicolon);
        let incr = if self.token() != Tok::CloseParen {
            Some(self.parse_expression())
        } else {
            None
        };
        self.expect(Tok::CloseParen);
        let body = Box::new(self.parse_statement());
        let end = self.prev_end();
        Stmt::For {
            init,
            cond,
            incr,
            body,
            span: Span::new(start, end),
        }
    }

    fn parse_try(&mut self) -> Stmt {
        let start = self.start();
        self.next();
        let block = self.parse_block();
        let catch = if self.token() == Tok::KCatch {
            let cstart = self.start();
            self.next();
            let param = if self.eat(Tok::OpenParen) {
                let pstart = self.start();
                let name = self.parse_binding();
                let ty = if self.eat(Tok::Colon) {
                    Some(self.parse_type())
                } else {
                    None
                };
                // `catch (e = 0)` — a default on the catch binding is a grammar
                // error but parsed (the checker reports it).
                if self.eat(Tok::Eq) {
                    let _ = self.parse_assignment_expr();
                }
                self.expect(Tok::CloseParen);
                Some(Param {
                    decorators: Vec::new(),
                    modifiers: Vec::new(),
                    dotdotdot: false,
                    dotdotdot_span: None,
                    name,
                    question: false,
                    question_span: None,
                    ty,
                    initializer: None,
                    span: Span::new(pstart, self.prev_end()),
                })
            } else {
                None
            };
            let cblock = self.parse_block();
            let cend = self.prev_end();
            Some(CatchClause {
                param,
                block: cblock,
                span: Span::new(cstart, cend),
            })
        } else {
            None
        };
        let finally = if self.eat(Tok::KFinally) {
            Some(self.parse_block())
        } else {
            None
        };
        if catch.is_none() && finally.is_none() {
            self.error_at_current(&gen::catch_or_finally_expected, &[]);
        }
        let end = self.prev_end();
        Stmt::Try {
            block,
            catch,
            finally,
            span: Span::new(start, end),
        }
    }

    fn parse_switch(&mut self) -> Stmt {
        let start = self.start();
        self.next();
        self.expect(Tok::OpenParen);
        let expr = self.parse_expression();
        self.expect(Tok::CloseParen);
        self.expect(Tok::OpenBrace);
        let mut cases = Vec::new();
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let cstart = self.start();
            let test = if self.eat(Tok::KCase) {
                let e = self.parse_expression();
                self.expect(Tok::Colon);
                Some(e)
            } else if self.eat(Tok::KDefault) {
                self.expect(Tok::Colon);
                None
            } else {
                self.error_at_current(&gen::_0_expected, &["case".to_string()]);
                self.next();
                continue;
            };
            let mut stmts = Vec::new();
            while !matches!(
                self.token(),
                Tok::KCase | Tok::KDefault | Tok::CloseBrace | Tok::Eof
            ) {
                let before = self.scanner.token_start;
                if let Some(s) = self.parse_statement_or_recover() {
                    stmts.push(s);
                }
                if self.scanner.token_start == before
                    && !matches!(
                        self.token(),
                        Tok::KCase | Tok::KDefault | Tok::CloseBrace | Tok::Eof
                    )
                {
                    self.next();
                }
            }
            cases.push(SwitchCase {
                test,
                stmts,
                span: Span::new(cstart, self.prev_end()),
            });
        }
        self.expect(Tok::CloseBrace);
        let end = self.prev_end();
        Stmt::Switch {
            expr,
            cases,
            span: Span::new(start, end),
        }
    }

    fn parse_import(&mut self) -> Stmt {
        let start = self.start();
        if self.namespace_depth == 0 {
            self.saw_module_syntax = true;
        }
        self.next();
        let mut type_only = false;
        if self.token() == Tok::KType {
            // Mirror tsc's default-import `type` disambiguation:
            //   import type Foo from "m"    -> type-only, default Foo
            //   import type from "m"        -> default binding named `type`
            //   import type from from "m"   -> type-only, default `from`
            //   import type from = require  -> type-only import-equals `from`
            let is_type_import = self
                .lookahead(|p| {
                    p.next(); // type  → now at T1
                    let t1_ok =
                        p.is_ident_like() || matches!(p.token(), Tok::Star | Tok::OpenBrace);
                    if !t1_ok {
                        return None;
                    }
                    let t1_is_from = p.is_ident_like() && p.token_value() == "from";
                    if !t1_is_from {
                        return Some(());
                    }
                    // T1 == `from`: type is the modifier only if `from`/`=` follows
                    p.next(); // at T2
                    if (p.is_ident_like() && p.token_value() == "from") || p.token() == Tok::Eq {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some();
            if is_type_import {
                type_only = true;
                self.next();
            }
        }
        // `defer` import modifier (deferred imports), with the same
        // binding/keyword disambiguation as `type`:
        //   import defer foo from "m"    -> defer, default foo (checker error)
        //   import defer from "m"        -> default binding named `defer`
        //   import defer from from "m"   -> defer, default `from`
        if self.is_ident_like() && self.token_value() == "defer" {
            let is_defer = self
                .lookahead(|p| {
                    p.next(); // defer  → T1
                    let t1_ok =
                        p.is_ident_like() || matches!(p.token(), Tok::Star | Tok::OpenBrace);
                    if !t1_ok {
                        return None;
                    }
                    let t1_is_from = p.is_ident_like() && p.token_value() == "from";
                    if !t1_is_from {
                        return Some(());
                    }
                    p.next(); // T2
                    if (p.is_ident_like() && p.token_value() == "from") || p.token() == Tok::Eq {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some();
            if is_defer {
                self.next();
            }
        }
        // `import A = require("m");` / `import type A = ns.Foo;` — an
        // import-equals. The common (unmodified) form is dispatched earlier;
        // this also covers modifier-prefixed forms like `accessor import N = M`.
        if self.is_ident_like()
            && self
                .lookahead(|p| {
                    p.next();
                    if p.token() == Tok::Eq {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
        {
            let name = self.parse_ident();
            self.expect(Tok::Eq);
            let (module, is_require) = self.parse_module_reference();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::ImportEquals {
                name,
                module,
                exported: false,
                is_require,
                span: Span::new(start, end),
            };
        }
        // side-effect import: import "mod";
        if self.token() == Tok::StrLit {
            let module = StrLitNode {
                value: self.token_value(),
                span: self.token_span(),
            };
            self.next();
            self.parse_import_attributes();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::Import(Box::new(ImportDecl {
                modifiers: Vec::new(),
                type_only: false,
                default_name: None,
                namespace_name: None,
                named: None,
                module,
                span: Span::new(start, end),
            }));
        }
        let mut default_name = None;
        let mut namespace_name = None;
        let mut named = None;
        if self.is_ident_like() {
            default_name = Some(self.parse_ident());
            if self.eat(Tok::Comma) {
                // fallthrough to named/namespace
            }
        }
        if self.token() == Tok::Star {
            self.next();
            self.expect(Tok::KAs);
            namespace_name = Some(self.parse_ident());
        } else if self.token() == Tok::OpenBrace {
            self.next();
            let mut specs = Vec::new();
            while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
                let sstart = self.start();
                // Resolve the `type` modifier ambiguity exactly as tsc (see the
                // matching export-specifier logic).
                let mut spec_type_only = false;
                let mut prop_name: Option<crate::ast::Ident> = None;
                let mut name = self.parse_module_export_name();
                let ident_or_kw = |p: &Self| {
                    p.is_ident_like() || p.token().is_keyword() || p.token() == Tok::StrLit
                };
                if name.name == "type" {
                    if self.token() == Tok::KAs {
                        let first_as = self.parse_ident_name();
                        if self.token() == Tok::KAs {
                            let second_as = self.parse_ident_name();
                            if ident_or_kw(self) {
                                spec_type_only = true;
                                prop_name = Some(first_as);
                                name = self.parse_module_export_name();
                            } else {
                                prop_name = Some(name);
                                name = second_as;
                            }
                        } else if ident_or_kw(self) {
                            prop_name = Some(name);
                            name = self.parse_module_export_name();
                        } else {
                            spec_type_only = true;
                            name = first_as;
                        }
                    } else if ident_or_kw(self) {
                        spec_type_only = true;
                        name = self.parse_module_export_name();
                    }
                }
                if prop_name.is_none() && self.eat(Tok::KAs) {
                    prop_name = Some(name);
                    name = self.parse_module_export_name();
                }
                specs.push(ImportSpec {
                    prop_name,
                    name,
                    type_only: spec_type_only || type_only,
                    span: Span::new(sstart, self.prev_end()),
                });
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::CloseBrace);
            named = Some(specs);
        }
        self.expect(Tok::KFrom);
        let module = if self.token() == Tok::StrLit {
            let m = StrLitNode {
                value: self.token_value(),
                span: self.token_span(),
            };
            self.next();
            m
        } else {
            self.error_at_current(&gen::String_literal_expected, &[]);
            StrLitNode {
                value: String::new(),
                span: self.token_span(),
            }
        };
        self.parse_import_attributes();
        let end = self.prev_end();
        self.parse_semicolon();
        Stmt::Import(Box::new(ImportDecl {
            modifiers: Vec::new(),
            type_only,
            default_name,
            namespace_name,
            named,
            module,
            span: Span::new(start, end),
        }))
    }

    /// Optional import/export attributes clause: `with { k: "v", … }` or the
    /// legacy `assert { … }`. Consumed and discarded.
    fn parse_import_attributes(&mut self) {
        let is_with = self.token() == Tok::KWith;
        let is_assert = self.is_ident_like() && self.token_value() == "assert";
        if !is_with && !is_assert {
            return;
        }
        if self
            .lookahead(|p| {
                p.next();
                if p.token() == Tok::OpenBrace && !p.line_break_before() {
                    Some(())
                } else {
                    None
                }
            })
            .is_none()
        {
            return;
        }
        self.next(); // with / assert
        self.expect(Tok::OpenBrace);
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            if self.token() == Tok::StrLit {
                self.next();
            } else {
                let _ = self.parse_ident_name();
            }
            self.expect(Tok::Colon);
            // the value is a string literal per spec; tsc parses any expression
            // and reports the string-literal requirement in the checker.
            if self.token() == Tok::StrLit {
                self.next();
            } else {
                let _ = self.parse_assignment_expr();
            }
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::CloseBrace);
    }

    /// The reference of an import-equals: `require("module")` or a (possibly
    /// dotted) entity name `A.B.C`. The entity-name text is stored verbatim in
    /// the module slot.
    /// Returns the reference and whether it is the external `require("m")`
    /// form (an external module reference is a module indicator; an entity
    /// name is not — tsc isAnExternalModuleIndicatorNode).
    fn parse_module_reference(&mut self) -> (StrLitNode, bool) {
        if self.is_ident_like()
            && self.token_value() == "require"
            && self
                .lookahead(|p| {
                    p.next();
                    if p.token() == Tok::OpenParen {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
        {
            self.next(); // require
            self.expect(Tok::OpenParen);
            // the argument is a string literal per spec; tsc parses any
            // expression and defers the requirement to the checker.
            let m = if self.token() == Tok::StrLit {
                let m = StrLitNode {
                    value: self.token_value(),
                    span: self.token_span(),
                };
                self.next();
                m
            } else {
                let s = self.token_span();
                let _ = self.parse_assignment_expr();
                StrLitNode {
                    value: String::new(),
                    span: s,
                }
            };
            self.expect(Tok::CloseParen);
            (m, true)
        } else {
            let estart = self.start();
            let mut name = self.parse_ident_name().name;
            while self.token() == Tok::Dot {
                self.next();
                name.push('.');
                name.push_str(&self.parse_ident_name().name);
            }
            (
                StrLitNode {
                    value: name,
                    span: Span::new(estart, self.prev_end()),
                },
                false,
            )
        }
    }

    fn parse_module_export_name(&mut self) -> crate::ast::Ident {
        if self.token() == Tok::StrLit {
            let s = crate::ast::Ident {
                name: self.token_value(),
                span: self.token_span(),
            };
            self.next();
            s
        } else {
            self.parse_ident_name()
        }
    }

    fn parse_export(&mut self) -> Stmt {
        let start = self.start();
        if self.namespace_depth == 0 {
            self.saw_module_syntax = true;
        }
        // export { a, b } [from "m"] | export <decl>
        let snap = self.save();
        self.next();
        // `export as namespace Name;` — UMD global namespace export.
        if self.token() == Tok::KAs {
            let snap2 = self.save();
            self.next(); // as
            if self.is_ident_like() && self.token_value() == "namespace" {
                self.next(); // namespace
                let _ = self.parse_ident();
                let end = self.prev_end();
                self.parse_semicolon();
                return Stmt::Empty {
                    span: Span::new(start, end),
                };
            }
            self.restore(snap2);
        }
        // `export type { ... }` / `export type * from "m"` — type-only
        // re-exports (distinct from `export type X = …`, a type alias, which
        // is followed by an identifier and handled by the declaration path).
        let type_only = if self.is_ident_like()
            && self.token_value() == "type"
            && self
                .lookahead(|p| {
                    p.next();
                    if matches!(p.token(), Tok::OpenBrace | Tok::Star) {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
        {
            self.next(); // type
            true
        } else {
            false
        };
        // `export import a = require("m");` / `export import a = M.x;` — only an
        // import-equals (`import ident =`); other forms (e.g. `export import
        // "fs"`) fall through to the general declaration path.
        if self.token() == Tok::KImport
            && self
                .lookahead(|p| {
                    p.next(); // import
                    if p.is_ident_like() {
                        p.next();
                        if p.token() == Tok::Eq {
                            return Some(());
                        }
                    }
                    None
                })
                .is_some()
        {
            self.next(); // import
            let name = self.parse_ident();
            self.expect(Tok::Eq);
            let (module, is_require) = self.parse_module_reference();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::ImportEquals {
                name,
                module,
                exported: true,
                is_require,
                span: Span::new(start, end),
            };
        }
        // `export @dec default class …` / `export @dec class …` — decorators
        // appearing between `export` and the (default) class.
        if self.token() == Tok::At {
            let decorators = self.parse_decorators();
            let is_default = self.eat(Tok::KDefault);
            if self.token() == Tok::KClass {
                let mut c = self.parse_class(Vec::new());
                if let Some(first) = decorators.first() {
                    c.span.start = first.span.start;
                }
                c.decorators = decorators;
                let end = self.prev_end();
                if is_default {
                    return Stmt::ExportDefault {
                        expr: Expr::ClassExpr(Box::new(c)),
                        span: Span::new(start, end),
                    };
                } else {
                    c.modifiers.push(Modifier {
                        kind: ModifierKind::Export,
                        span: Span::new(start, start + 6),
                    });
                    return Stmt::Class(Box::new(c));
                }
            }
            // Decorators on a non-class export are a grammar error (TS1206,
            // checker-reported). Parse the remainder so syntax stays well-formed.
            if is_default {
                let expr = self.parse_assignment_expr();
                let end = self.prev_end();
                self.parse_semicolon();
                return Stmt::ExportDefault {
                    expr,
                    span: Span::new(start, end),
                };
            }
            return self.parse_statement();
        }
        if self.token() == Tok::OpenBrace {
            self.next();
            let mut specs = Vec::new();
            while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
                let sstart = self.start();
                // Parse one export specifier, resolving the `type` modifier
                // ambiguity exactly as tsc's parseImportOrExportSpecifier:
                //   { type }          name=type
                //   { type as }       type-only, name=as
                //   { type as as }    name=as, propertyName=type
                //   { type as as X }  type-only, name=X, propertyName=as
                //   { type X }        type-only, name=X
                let mut spec_type_only = false;
                let mut prop_name: Option<crate::ast::Ident> = None;
                let mut name = self.parse_module_export_name();
                let ident_or_kw = |p: &Self| {
                    p.is_ident_like() || p.token().is_keyword() || p.token() == Tok::StrLit
                };
                if name.name == "type" {
                    if self.token() == Tok::KAs {
                        let first_as = self.parse_ident_name();
                        if self.token() == Tok::KAs {
                            let second_as = self.parse_ident_name();
                            if ident_or_kw(self) {
                                spec_type_only = true;
                                prop_name = Some(first_as);
                                name = self.parse_module_export_name();
                            } else {
                                prop_name = Some(name);
                                name = second_as;
                            }
                        } else if ident_or_kw(self) {
                            prop_name = Some(name);
                            name = self.parse_module_export_name();
                        } else {
                            spec_type_only = true;
                            name = first_as;
                        }
                    } else if ident_or_kw(self) {
                        spec_type_only = true;
                        name = self.parse_module_export_name();
                    }
                }
                if prop_name.is_none() && self.eat(Tok::KAs) {
                    prop_name = Some(name);
                    name = self.parse_module_export_name();
                }
                specs.push(ImportSpec {
                    prop_name,
                    name,
                    type_only: type_only || spec_type_only,
                    span: Span::new(sstart, self.prev_end()),
                });
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::CloseBrace);
            let module = if self.eat(Tok::KFrom) {
                if self.token() == Tok::StrLit {
                    let m = StrLitNode {
                        value: self.token_value(),
                        span: self.token_span(),
                    };
                    self.next();
                    Some(m)
                } else {
                    self.error_at_current(&gen::String_literal_expected, &[]);
                    None
                }
            } else {
                None
            };
            self.parse_import_attributes();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::ExportNamed(Box::new(ExportNamedDecl {
                star: false,
                specifiers: specs,
                module,
                span: Span::new(start, end),
            }));
        }
        // export = expr;
        if self.token() == Tok::Eq {
            self.next();
            let expr = self.parse_assignment_expr();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::ExportAssign {
                expr,
                span: Span::new(start, end),
            };
        }
        // export * from "m";
        if self.token() == Tok::Star {
            self.next();
            // optional `as name` (the alias may be a keyword like `default`)
            if self.eat(Tok::KAs) {
                if self.token() == Tok::StrLit {
                    self.next();
                } else {
                    let _ = self.parse_ident_name();
                }
            }
            self.expect(Tok::KFrom);
            let module = if self.token() == Tok::StrLit {
                let m = StrLitNode {
                    value: self.token_value(),
                    span: self.token_span(),
                };
                self.next();
                Some(m)
            } else {
                self.error_at_current(&gen::String_literal_expected, &[]);
                None
            };
            self.parse_import_attributes();
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::ExportNamed(Box::new(ExportNamedDecl {
                star: true,
                specifiers: Vec::new(),
                module,
                span: Span::new(start, end),
            }));
        }
        // export <declaration> — restore and let modifier path handle it
        self.restore(snap);
        if let Some(s) = self.try_parse_modified_declaration() {
            s
        } else {
            // export default <expr>
            let start = self.start();
            self.next();
            if self.eat(Tok::KDefault) {
                // `export default interface I {}` is a declaration, not an
                // expression; parse it directly.
                if self.token() == Tok::KInterface {
                    return Stmt::Interface(Box::new(self.parse_interface(Vec::new())));
                }
                let expr = self.parse_assignment_expr();
                let end = self.prev_end();
                self.parse_semicolon();
                Stmt::ExportDefault {
                    expr,
                    span: Span::new(start, end),
                }
            } else {
                self.parse_expression_statement()
            }
        }
    }

    // ── classes / interfaces / type aliases ────────────────────────────────

    pub(crate) fn parse_function(&mut self, modifiers: Modifiers, kind: FuncKind) -> FunctionLike {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        self.expect(Tok::KFunction);
        let is_generator = self.eat(Tok::Star);
        let name = if self.is_ident_like() || self.token().is_strict_reserved_word() {
            Some(PropName::Ident(self.parse_ident()))
        } else {
            None
        };
        let type_params = self.parse_type_params_opt();
        let params = self.parse_params();
        let return_type = if self.eat(Tok::Colon) {
            Some(self.parse_type_or_predicate())
        } else {
            None
        };
        let body = if self.token() == Tok::OpenBrace {
            Some(FuncBody::Block(self.parse_block()))
        } else {
            self.parse_semicolon();
            None
        };
        FunctionLike {
            decorators: Vec::new(),
            kind,
            modifiers,
            name,
            question: false,
            type_params,
            params,
            return_type,
            body,
            is_generator,
            span: Span::new(start, self.prev_end()),
        }
    }

    pub(crate) fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if !self.expect(Tok::OpenParen) {
            return params;
        }
        while self.token() != Tok::CloseParen && self.token() != Tok::Eof {
            if self.token() == Tok::Comma {
                // `f(,)` — no parameter where one is required
                self.error_at_current(&gen::Parameter_declaration_expected, &[]);
                self.next();
                continue;
            }
            let _was_rest = self.token() == Tok::DotDotDot;
            params.push(self.parse_param());
            // A trailing comma after a rest parameter (`f(...a,)`) is a grammar
            // error (TS1013) reported by the checker, not a syntax error.
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::CloseParen);
        params
    }

    pub(crate) fn parse_param(&mut self) -> Param {
        let start = self.start();
        let param_decorators = if self.token() == Tok::At {
            self.parse_decorators()
        } else {
            Vec::new()
        };
        let mut modifiers = Vec::new();
        loop {
            let kind = match self.token() {
                Tok::KDeclare => ModifierKind::Declare,
                Tok::KPublic => ModifierKind::Public,
                Tok::KPrivate => ModifierKind::Private,
                Tok::KProtected => ModifierKind::Protected,
                Tok::KReadonly => ModifierKind::Readonly,
                Tok::KOverride => ModifierKind::Override,
                // `static`/`export`/`async` are never valid on a parameter, but
                // tsc parses them and defers the error to the checker.
                Tok::KStatic => ModifierKind::Static,
                Tok::KExport => ModifierKind::Export,
                Tok::KAsync => ModifierKind::Async,
                _ => break,
            };
            // `readonly` could be a parameter name; only treat as modifier when
            // followed by another binding start
            let span = self.token_span();
            let snap = self.save();
            self.next();
            if self.is_ident_like()
                || matches!(
                    self.token(),
                    Tok::OpenBrace
                        | Tok::OpenBracket
                        | Tok::DotDotDot
                        | Tok::KReadonly
                        | Tok::KPublic
                        | Tok::KPrivate
                        | Tok::KProtected
                        | Tok::KOverride
                        | Tok::KStatic
                        | Tok::KExport
                        | Tok::KAsync
                )
            {
                modifiers.push(Modifier { kind, span });
            } else {
                self.restore(snap);
                break;
            }
        }
        let dotdotdot_span = if self.token() == Tok::DotDotDot {
            let span = self.token_span();
            self.next();
            Some(span)
        } else {
            None
        };
        let dotdotdot = dotdotdot_span.is_some();
        let name = if self.token() == Tok::KThis {
            let id = Ident {
                name: "this".to_string(),
                span: self.token_span(),
            };
            self.next();
            Binding::Ident(id)
        } else {
            self.parse_binding()
        };
        let question_span = if self.token() == Tok::Question {
            let span = self.token_span();
            self.next();
            Some(span)
        } else {
            None
        };
        let question = question_span.is_some();
        let ty = if self.eat(Tok::Colon) {
            Some(self.parse_type_or_predicate())
        } else {
            None
        };
        let initializer = if self.eat(Tok::Eq) {
            Some(self.parse_assignment_expr())
        } else {
            None
        };
        Param {
            decorators: param_decorators,
            modifiers,
            dotdotdot,
            dotdotdot_span,
            name,
            question,
            question_span,
            ty,
            initializer,
            span: Span::new(start, self.prev_end()),
        }
    }

    pub(crate) fn parse_type_params_opt(&mut self) -> Option<Vec<TypeParamDecl>> {
        if self.token() != Tok::Lt {
            return None;
        }
        let lt_span = self.token_span();
        self.next();
        if self.token() == Tok::Gt {
            // `<>` empty type-parameter list — a grammar error (TS1098)
            // reported by the checker, not a syntax error.
            let _ = lt_span;
            self.next();
            return Some(Vec::new());
        }
        let mut tps = Vec::new();
        while self.token() != Tok::Gt && self.token() != Tok::Eof {
            let start = self.start();
            let mut variance_span = None;
            loop {
                let is_variance = matches!(self.token(), Tok::KIn)
                    || (self.is_ident_like() && self.token_value() == "out");
                if !is_variance {
                    break;
                }
                let ok = self
                    .lookahead(|p| {
                        p.next();
                        // the modifier may be followed by another variance
                        // modifier (`out in T`, `in out T`) or the parameter name
                        if p.is_ident_like() || p.token() == Tok::KIn {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some();
                if !ok {
                    break;
                }
                let text = if self.token() == Tok::KIn {
                    "in".to_string()
                } else {
                    "out".to_string()
                };
                let s = self.token_span();
                self.next();
                if variance_span.is_none() {
                    variance_span = Some((text, s));
                }
            }
            let const_span = if self.token() == Tok::KConst
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some()
            {
                let s = self.token_span();
                self.next();
                Some(s)
            } else {
                None
            };
            let illegal_modifier = match self.token() {
                Tok::KPublic
                | Tok::KPrivate
                | Tok::KProtected
                | Tok::KStatic
                | Tok::KReadonly
                | Tok::KDeclare
                | Tok::KAbstract
                | Tok::KExport
                    if self
                        .lookahead(|p| {
                            p.next();
                            if p.is_ident_like() {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some() =>
                {
                    let kind = match self.token() {
                        Tok::KPublic => ModifierKind::Public,
                        Tok::KPrivate => ModifierKind::Private,
                        Tok::KProtected => ModifierKind::Protected,
                        Tok::KStatic => ModifierKind::Static,
                        Tok::KReadonly => ModifierKind::Readonly,
                        Tok::KDeclare => ModifierKind::Declare,
                        Tok::KAbstract => ModifierKind::Abstract,
                        _ => ModifierKind::Export,
                    };
                    let s = self.token_span();
                    self.next();
                    Some((kind, s))
                }
                _ => None,
            };
            let name = self.parse_ident();
            // `extends Constraint` and `= Default` (tsc parseTypeParameter).
            let constraint = if self.eat(Tok::KExtends) {
                Some(self.parse_type())
            } else {
                None
            };
            let default = if self.eat(Tok::Eq) {
                Some(self.parse_type())
            } else {
                None
            };
            tps.push(TypeParamDecl {
                illegal_modifier,
                const_span,
                variance_span,
                name,
                constraint,
                default,
                span: Span::new(start, self.prev_end()),
            });
            // A trailing `>` ends the list; otherwise a comma must separate
            // parameters. Breaking when no comma is consumed also guarantees
            // forward progress on malformed input (no infinite loop).
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::Gt);
        Some(tps)
    }
}
