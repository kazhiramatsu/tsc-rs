//! Declaration parsing: classes (members, modifiers, index signatures),
//! interfaces, namespaces, enums, and type aliases. Split out of `parser/mod.rs`.

use super::Parser;
use crate::ast::*;
use crate::diagnostics::gen;
use crate::scanner::Tok;

impl<'a> Parser<'a> {
    pub(crate) fn parse_class(&mut self, modifiers: Modifiers) -> ClassDecl {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        self.expect(Tok::KClass);
        let name = if self.is_ident_like() {
            Some(self.parse_ident())
        } else {
            None
        };
        let type_params = self.parse_type_params_opt();
        let mut extends = None;
        let mut implements = Vec::new();
        let mut seen_extends = false;
        let mut seen_implements = false;
        loop {
            if self.token() == Tok::KExtends {
                // Repeated/misordered heritage clauses are grammar errors
                // (TS1172/1174/1175), reported by the checker — not here.
                seen_extends = true;
                let hstart = self.start();
                self.next();
                // An empty extends operand (`extends implements A`, `extends {`)
                // is a grammar error (deferred); `{` starts the class body and
                // `implements` starts the next clause, so neither is consumed.
                if self.token() != Tok::KImplements && self.token() != Tok::OpenBrace {
                    let expr = self.parse_lhs_expression();
                    let type_args = if self.token() == Tok::Lt {
                        self.try_parse_type_args()
                    } else {
                        None
                    };
                    if extends.is_none() {
                        extends = Some(HeritageClause {
                            expr,
                            type_args,
                            span: Span::new(hstart, self.prev_end()),
                        });
                    }
                    // a class may only extend one class (extras are checker errors)
                    while self.token() == Tok::Comma {
                        self.next();
                        if matches!(self.token(), Tok::OpenBrace | Tok::KImplements) {
                            break;
                        }
                        let _ = self.parse_lhs_expression();
                        if self.token() == Tok::Lt {
                            let _ = self.try_parse_type_args();
                        }
                    }
                }
            } else if self.token() == Tok::KImplements {
                seen_implements = true;
                self.next();
                // `implements A, B<T>, ...` — a comma-separated type list, which
                // ends at the class body `{`.
                loop {
                    if self.token() == Tok::OpenBrace {
                        break;
                    }
                    implements.push(self.parse_type_ref());
                    if !self.eat(Tok::Comma) {
                        break;
                    }
                }
            } else {
                // Neither heritage keyword: the clauses are done (the class
                // body `{` follows). Without this break the loop would spin
                // forever on the body brace.
                break;
            }
        }
        let _ = (seen_extends, seen_implements);
        self.expect(Tok::OpenBrace);
        let mut members = Vec::new();
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            if self.eat(Tok::Semicolon) {
                continue;
            }
            let before = self.scanner.token_start;
            if let Some(m) = self.parse_class_member() {
                members.push(m);
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
        ClassDecl {
            decorators: Vec::new(),
            modifiers,
            name,
            type_params,
            extends,
            implements,
            members,
            span: Span::new(start, end),
        }
    }

    fn parse_member_modifiers(&mut self) -> Modifiers {
        let mut mods = Vec::new();
        loop {
            let kind = match self.token() {
                Tok::KPublic => ModifierKind::Public,
                Tok::KPrivate => ModifierKind::Private,
                Tok::KProtected => ModifierKind::Protected,
                Tok::KStatic => ModifierKind::Static,
                Tok::KReadonly => ModifierKind::Readonly,
                Tok::KAbstract => ModifierKind::Abstract,
                Tok::KAsync => ModifierKind::Async,
                Tok::KOverride => ModifierKind::Override,
                Tok::KDeclare => ModifierKind::Declare,
                // `export` is never valid on a class member, but tsc parses it
                // and reports the grammar error in the checker.
                Tok::KExport => ModifierKind::Export,
                // `in`/`out` variance modifiers are valid only on type
                // parameters; on a member they are parsed and deferred.
                Tok::KIn => ModifierKind::In,
                _ if self.is_ident_like() && self.token_value() == "out" => ModifierKind::Out,
                _ => break,
            };
            let span = self.token_span();
            let snap = self.save();
            self.next();
            // modifier only if followed by member-ish token (not `(`, `=`, `:`, `?`, `;`, `<`, `)`)
            if matches!(
                self.token(),
                Tok::OpenParen
                    | Tok::Eq
                    | Tok::Colon
                    | Tok::Question
                    | Tok::Semicolon
                    | Tok::Lt
                    | Tok::CloseBrace
                    | Tok::Bang
            ) {
                self.restore(snap);
                break;
            }
            mods.push(Modifier { kind, span });
        }
        mods
    }

    fn parse_class_member(&mut self) -> Option<ClassMember> {
        let decorators = if self.token() == Tok::At {
            self.parse_decorators()
        } else {
            Vec::new()
        };
        // static initialization block
        if self.token() == Tok::KStatic
            && self
                .lookahead(|p| {
                    p.next();
                    if p.token() == Tok::OpenBrace {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
        {
            self.next();
            let b = self.parse_block();
            return Some(ClassMember::StaticBlock(b));
        }
        let mut modifiers = self.parse_member_modifiers();
        // `async static {}` — a static initialization block can be preceded by
        // (invalid) modifiers, which the checker reports.
        if self.token() == Tok::OpenBrace
            && modifiers.iter().any(|m| m.kind == ModifierKind::Static)
        {
            let b = self.parse_block();
            return Some(ClassMember::StaticBlock(b));
        }
        let mut accessor_span = None;
        while self.is_ident_like()
            && self.token_value() == "accessor"
            && self
                .lookahead(|p| {
                    p.next();
                    if p.is_ident_like()
                        || p.token().is_strict_reserved_word()
                        || matches!(
                            p.token(),
                            Tok::StrLit
                                | Tok::NumLit
                                | Tok::OpenBracket
                                | Tok::PrivateIdent
                                | Tok::KPublic
                                | Tok::KPrivate
                                | Tok::KProtected
                                | Tok::KStatic
                                | Tok::KReadonly
                        )
                    {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
        {
            let s = self.token_span();
            self.next();
            if accessor_span.is_none() {
                accessor_span = Some(s);
            }
            // `accessor public x` — modifiers may follow `accessor` (the
            // ordering error is reported by the checker).
            modifiers.extend(self.parse_member_modifiers());
        }
        // `const` is never a valid member modifier (1248)
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
        // index signature
        if self.token() == Tok::OpenBracket {
            if let Some(idx) =
                self.try_parse_index_sig(modifiers.iter().any(|m| m.kind == ModifierKind::Readonly))
            {
                return Some(ClassMember::Index(idx));
            }
        }
        // accessor?
        let accessor = if matches!(self.token(), Tok::KGet | Tok::KSet) {
            let is_get = self.token() == Tok::KGet;
            let snap = self.save();
            self.next();
            if self.token() == Tok::OpenParen
                || self.token() == Tok::Colon
                || self.token() == Tok::Eq
                || self.token() == Tok::Semicolon
                || self.token() == Tok::Question
                || self.token() == Tok::Lt
            {
                self.restore(snap);
                None
            } else {
                Some(is_get)
            }
        } else {
            None
        };
        let start = modifiers
            .first()
            .map(|m| m.span.start as usize)
            .unwrap_or(self.start());
        // generator method: a `*` after any modifiers and before the name
        let is_generator = self.eat(Tok::Star);
        let name = self.parse_prop_name();
        let question_span = if self.token() == Tok::Question {
            let span = self.token_span();
            self.next();
            Some(span)
        } else {
            None
        };
        let question = question_span.is_some();
        let exclam_span = if !question && self.token() == Tok::Bang {
            let span = self.token_span();
            self.next();
            Some(span)
        } else {
            None
        };
        let exclam = exclam_span.is_some();
        if let Some(is_get) = accessor {
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
            return Some(ClassMember::Method(Box::new(FunctionLike {
                decorators,
                kind: if is_get {
                    FuncKind::Getter
                } else {
                    FuncKind::Setter
                },
                modifiers,
                name: Some(name),
                question,
                type_params,
                params,
                return_type,
                body,
                is_generator: false,
                span: Span::new(start, self.prev_end()),
            })));
        }
        if self.token() == Tok::OpenParen || self.token() == Tok::Lt {
            // method or constructor
            let is_ctor = matches!(&name, PropName::Ident(i) if i.name == "constructor");
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
            let fl = FunctionLike {
                decorators,
                kind: if is_ctor {
                    FuncKind::Constructor
                } else {
                    FuncKind::Method
                },
                modifiers,
                name: Some(name),
                question,
                type_params,
                params,
                return_type,
                body,
                is_generator,
                span: Span::new(start, self.prev_end()),
            };
            return Some(if is_ctor {
                ClassMember::Constructor(Box::new(fl))
            } else {
                ClassMember::Method(Box::new(fl))
            });
        }
        // property
        let ty = if self.eat(Tok::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        // `a?: string?` / `b?: string!` — a trailing `?` or `!` after the field
        // type is invalid but parsed (the checker reports the error).
        while matches!(self.token(), Tok::Question | Tok::Bang) {
            self.next();
        }
        let init = if self.eat(Tok::Eq) {
            Some(self.parse_assignment_expr())
        } else {
            None
        };
        let end = self.prev_end();
        self.parse_semicolon();
        Some(ClassMember::Property(PropertyDecl {
            decorators,
            const_span,
            accessor_span,
            modifiers,
            name,
            question,
            question_span,
            exclam,
            exclam_span,
            ty,
            init,
            span: Span::new(start, end),
        }))
    }

    pub(crate) fn parse_prop_name(&mut self) -> PropName {
        if self.token() == Tok::PrivateIdent {
            let span = self.token_span();
            let name = self.token_value();
            self.next();
            return PropName::Ident(Ident { name, span });
        }
        match self.token() {
            Tok::StrLit => {
                let p = PropName::String {
                    value: self.token_value(),
                    span: self.token_span(),
                };
                self.next();
                p
            }
            Tok::NumLit => {
                let p = PropName::Number {
                    value: self.token_value().parse().unwrap_or(0.0),
                    text: self.token_value(),
                    span: self.token_span(),
                };
                self.next();
                p
            }
            Tok::OpenBracket => {
                let start = self.start();
                self.next();
                let expr = self.parse_expression();
                let end = self.scanner.token_end();
                self.expect(Tok::CloseBracket);
                PropName::Computed {
                    expr: Box::new(expr),
                    span: Span::new(start, end),
                }
            }
            _ => PropName::Ident(self.parse_ident_name()),
        }
    }

    pub(crate) fn try_parse_index_sig(&mut self, readonly: bool) -> Option<IndexSig> {
        self.try_parse(|p| {
            let start = p.start();
            if p.token() != Tok::OpenBracket {
                return None;
            }
            // Disambiguate an index signature `[k: T]` from a computed property
            // name `[expr]` (tsc's isUnambiguouslyIndexSignature).
            let is_index = p
                .lookahead(|q| {
                    q.next(); // [
                    if matches!(q.token(), Tok::DotDotDot | Tok::CloseBracket) {
                        return Some(());
                    }
                    if matches!(
                        q.token(),
                        Tok::KPublic
                            | Tok::KPrivate
                            | Tok::KProtected
                            | Tok::KStatic
                            | Tok::KReadonly
                    ) {
                        q.next();
                        if q.is_ident_like() {
                            return Some(());
                        }
                        return None;
                    }
                    if !q.is_ident_like() {
                        return None;
                    }
                    q.next();
                    if matches!(q.token(), Tok::Colon | Tok::Comma) {
                        return Some(());
                    }
                    if q.token() != Tok::Question {
                        return None;
                    }
                    q.next();
                    if matches!(q.token(), Tok::Colon | Tok::Comma | Tok::CloseBracket) {
                        return Some(());
                    }
                    None
                })
                .is_some();
            if !is_index {
                return None;
            }
            p.next(); // [
                      // The bracket contents are a parameter list; the first parameter is
                      // captured for the model and any extras are parsed and discarded
                      // (multiple index parameters are a checker error).
            let p0_start = p.start();
            let rest_span = if p.token() == Tok::DotDotDot {
                let s = p.token_span();
                p.next();
                Some(s)
            } else {
                None
            };
            let modifier_span = if matches!(
                p.token(),
                Tok::KPublic | Tok::KPrivate | Tok::KProtected | Tok::KStatic | Tok::KReadonly
            ) {
                let s = p.token_span();
                p.next();
                Some(s)
            } else {
                None
            };
            let param_name = if p.is_ident_like() {
                p.parse_ident()
            } else {
                Ident {
                    name: String::new(),
                    span: Span::new(p0_start, p0_start),
                }
            };
            let question_span = if p.token() == Tok::Question {
                let s = p.token_span();
                p.next();
                Some(s)
            } else {
                None
            };
            let key_type = if p.eat(Tok::Colon) {
                p.parse_type()
            } else {
                TypeNode::Keyword(KeywordTypeKind::Any, Span::new(p.prev_end(), p.prev_end()))
            };
            // extra index parameters (invalid; checker-reported)
            while p.eat(Tok::Comma) {
                if p.token() == Tok::CloseBracket {
                    break;
                }
                let _ = p.parse_param();
            }
            if !p.eat(Tok::CloseBracket) {
                return None;
            }
            let missing_value = p.token() != Tok::Colon;
            let value_type = if p.eat(Tok::Colon) {
                p.parse_type()
            } else {
                TypeNode::Keyword(KeywordTypeKind::Any, Span::new(p.prev_end(), p.prev_end()))
            };
            let end = p.prev_end();
            p.parse_semicolon_or_comma_in_type_member();
            Some(IndexSig {
                declare_span: None,
                readonly,
                rest_span,
                modifier_span,
                question_span,
                missing_value,
                param_name,
                key_type,
                value_type,
                span: Span::new(start, end),
            })
        })
    }

    pub(crate) fn parse_interface(&mut self, modifiers: Modifiers) -> InterfaceDecl {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        self.expect(Tok::KInterface);
        let name = self.parse_ident();
        let type_params = self.parse_type_params_opt();
        let mut extends = Vec::new();
        // Repeated `extends`, or an `implements` clause, are grammar errors on
        // an interface (checker-reported); parse them so the body still parses.
        loop {
            if self.token() == Tok::KExtends || self.token() == Tok::KImplements {
                self.next();
                // `interface I extends {}` — a `{` here is the interface body,
                // not a heritage operand (grammar error, checker-reported).
                if self.token() == Tok::OpenBrace {
                    break;
                }
                loop {
                    extends.push(self.parse_type_ref());
                    if !self.eat(Tok::Comma) {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        let members = self.parse_type_members();
        InterfaceDecl {
            modifiers,
            name,
            type_params,
            extends,
            members,
            span: Span::new(start, self.prev_end()),
        }
    }

    pub(crate) fn parse_namespace(&mut self, modifiers: Modifiers) -> Stmt {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        let kw_span = self.token_span();
        let is_global = self.is_ident_like() && self.token_value() == "global";
        self.next(); // namespace / module / global
                     // The name is: nothing (for `global { … }`, the keyword is the name),
                     // a string literal (ambient module `module "x"`), or a possibly-dotted
                     // identifier (`namespace A.B.C`).
        let name = if is_global && self.token() == Tok::OpenBrace {
            crate::ast::Ident {
                name: "global".to_string(),
                span: kw_span,
            }
        } else if self.token() == Tok::StrLit {
            let n = crate::ast::Ident {
                name: self.token_value(),
                span: self.token_span(),
            };
            self.next();
            n
        } else {
            let mut n = self.parse_ident();
            while self.token() == Tok::Dot {
                self.next();
                let part = self.parse_ident_name();
                n.span = Span::new(n.span.start as usize, part.span.end as usize);
            }
            n
        };
        // A body is optional: `declare module "x";` is a bare ambient module.
        if self.token() != Tok::OpenBrace {
            let end = self.prev_end();
            self.parse_semicolon();
            return Stmt::Namespace(Box::new(NamespaceDecl {
                modifiers,
                name,
                body: Vec::new(),
                span: Span::new(start, end),
            }));
        }
        self.expect(Tok::OpenBrace);
        let mut body = Vec::new();
        self.namespace_depth += 1;
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let before = self.scanner.token_start;
            if let Some(s) = self.parse_statement_or_recover() {
                body.push(s);
            }
            if self.scanner.token_start == before
                && self.token() != Tok::CloseBrace
                && self.token() != Tok::Eof
            {
                self.next();
            }
        }
        self.namespace_depth -= 1;
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        Stmt::Namespace(Box::new(NamespaceDecl {
            modifiers,
            name,
            body,
            span: Span::new(start, end),
        }))
    }

    pub(crate) fn parse_enum(&mut self, modifiers: Modifiers, is_const: bool) -> EnumDecl {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        self.expect(Tok::KEnum);
        let name = self.parse_ident();
        self.expect(Tok::OpenBrace);
        let mut members = Vec::new();
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            if self.token() == Tok::Comma {
                self.error_at_current(&gen::Enum_member_expected, &[]);
                self.next();
                continue;
            }
            let mstart = self.start();
            let name = self.parse_prop_name();
            let init = if self.eat(Tok::Eq) {
                Some(self.parse_assignment_expr())
            } else {
                None
            };
            members.push(EnumMemberDecl {
                name,
                init,
                span: Span::new(mstart, self.prev_end()),
            });
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        EnumDecl {
            modifiers,
            is_const,
            name,
            members,
            span: Span::new(start, end),
        }
    }

    pub(crate) fn parse_type_alias(&mut self, modifiers: Modifiers) -> TypeAliasDecl {
        let start = if modifiers.is_empty() {
            self.start()
        } else {
            modifiers[0].span.start as usize
        };
        self.expect(Tok::KType);
        let name = self.parse_ident();
        let type_params = self.parse_type_params_opt();
        self.expect(Tok::Eq);
        let ty = self.parse_type();
        let end = self.prev_end();
        self.parse_semicolon();
        TypeAliasDecl {
            modifiers,
            name,
            type_params,
            ty,
            span: Span::new(start, end),
        }
    }

    // ── types ───────────────────────────────────────────────────────────────
}
