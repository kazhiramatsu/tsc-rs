//! Type parsing: the type grammar — unions/intersections, function & mapped
//! types, type references with arguments, predicates, and object type members.
//! Split out of `parser/mod.rs`.

use super::Parser;
use crate::ast::*;
use crate::diagnostics::gen;
use crate::scanner::Tok;

impl<'a> Parser<'a> {
    pub(crate) fn parse_type(&mut self) -> TypeNode {
        let start = self.start();
        let t = self.parse_non_conditional_type();
        if self.token() == Tok::KExtends && !self.line_break_before() {
            self.next();
            let saved = self.disallow_conditional;
            self.disallow_conditional = true;
            let extends_ty = self.parse_non_conditional_type();
            self.disallow_conditional = false;
            self.expect(Tok::Question);
            let true_ty = self.parse_type();
            self.expect(Tok::Colon);
            let false_ty = self.parse_type();
            self.disallow_conditional = saved;
            let span = Span::new(start, self.prev_end());
            return TypeNode::Conditional(Box::new(ConditionalTypeNode {
                check: t,
                extends_ty,
                true_ty,
                false_ty,
                span,
            }));
        }
        t
    }

    fn parse_non_conditional_type(&mut self) -> TypeNode {
        // A leading `?` (legacy/Flow optional-type syntax) or `!` (JSDoc
        // non-null) is not valid TS, but tsc consumes it and parses the
        // following type, deferring the error. A bare marker with no following
        // type (`x: ?`) is modeled as `any`.
        if matches!(self.token(), Tok::Question | Tok::Bang) {
            let q = self.token_span();
            self.next();
            if matches!(
                self.token(),
                Tok::Eq
                    | Tok::Semicolon
                    | Tok::Comma
                    | Tok::CloseParen
                    | Tok::CloseBracket
                    | Tok::CloseBrace
                    | Tok::Gt
                    | Tok::Eof
            ) {
                return TypeNode::Keyword(KeywordTypeKind::Any, q);
            }
        }
        // Constructor type: `new (...) => T` and `abstract new (...) => T`.
        // (`abstract` is contextual, so only treat it as a ctor-type prefix
        // when an actual `new` follows.)
        if self.token() == Tok::KNew
            || (self.token() == Tok::KAbstract
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == Tok::KNew {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some())
        {
            let start = self.start();
            let is_abstract = self.eat(Tok::KAbstract);
            self.expect(Tok::KNew);
            let mut f = self.parse_function_type_tail(start, true);
            f.is_abstract = is_abstract;
            return TypeNode::Ctor(Box::new(f));
        }
        if self.is_function_type_start() {
            let start = self.start();
            let f = self.parse_function_type_tail(start, false);
            return TypeNode::Function(Box::new(f));
        }
        self.parse_union_type()
    }

    pub(crate) fn parse_type_or_predicate(&mut self) -> TypeNode {
        // `asserts x`, `asserts x is T`, and `x is T` predicates. The leading
        // `asserts` is contextual (an identifier).
        if self.is_ident_like() && self.token_value() == "asserts" {
            if let Some(t) = self.try_parse(|p| {
                let start = p.start();
                p.next(); // asserts
                if !(p.is_ident_like() || p.token() == Tok::KThis) {
                    return None;
                }
                let pname = Ident {
                    name: p.token_value(),
                    span: p.token_span(),
                };
                p.next();
                let ty = if p.token() == Tok::KIs {
                    p.next();
                    Some(Box::new(p.parse_type()))
                } else {
                    None
                };
                Some(TypeNode::Predicate {
                    param_name: pname,
                    asserts: true,
                    ty,
                    span: Span::new(start, p.prev_end()),
                })
            }) {
                return t;
            }
        }
        // `x is T` predicates behave as boolean but keep their parts for 2677
        if self.is_ident_like() || self.token() == Tok::KThis {
            if let Some(t) = self.try_parse(|p| {
                let start = p.start();
                let pname = Ident {
                    name: p.token_value(),
                    span: p.token_span(),
                };
                p.next();
                // A predicate `x is T` requires `is` on the same line; otherwise
                // `is` begins the next construct (e.g. an interface member named
                // `is`), as in tsc's parseTypePredicatePrefix.
                if p.token() == Tok::KIs && !p.line_break_before() {
                    p.next();
                    let t = p.parse_type();
                    Some(TypeNode::Predicate {
                        param_name: pname,
                        asserts: false,
                        ty: Some(Box::new(t)),
                        span: Span::new(start, p.prev_end()),
                    })
                } else {
                    None
                }
            }) {
                return t;
            }
        }
        self.parse_type()
    }

    fn is_function_type_start(&mut self) -> bool {
        if self.token() == Tok::Lt {
            return true;
        }
        if self.token() != Tok::OpenParen {
            return false;
        }
        // lookahead: `(` ... `)` `=>`
        self.lookahead(|p| {
            p.next();
            if p.token() == Tok::CloseParen || p.token() == Tok::DotDotDot {
                return Some(());
            }
            // skip leading parameter modifiers (`(public x) => …`); invalid on a
            // function type but parsed and deferred by the checker.
            while matches!(
                p.token(),
                Tok::KPublic | Tok::KPrivate | Tok::KProtected | Tok::KReadonly
            ) {
                p.next();
            }
            // param start then `:`/`,`/`?`/`)` patterns
            if p.is_ident_like()
                || p.token() == Tok::KThis
                || matches!(p.token(), Tok::OpenBrace | Tok::OpenBracket)
            {
                // `this` is a valid (type-only) parameter name but not a
                // binding; consume it directly so a `(this: T) => U` head is
                // recognized as a function type.
                if p.token() == Tok::KThis {
                    p.next();
                } else {
                    p.parse_binding();
                }
                if matches!(p.token(), Tok::Colon | Tok::Comma | Tok::Question | Tok::Eq) {
                    return Some(());
                }
                if p.token() == Tok::CloseParen {
                    p.next();
                    if p.token() == Tok::Arrow {
                        return Some(());
                    }
                }
            }
            None
        })
        .is_some()
    }

    fn parse_function_type_tail(&mut self, start: usize, is_ctor: bool) -> FunctionTypeNode {
        let type_params = self.parse_type_params_opt();
        let params = self.parse_params();
        self.expect(Tok::Arrow);
        let return_type = self.parse_type_or_predicate();
        FunctionTypeNode {
            type_params,
            params,
            return_type,
            is_abstract: is_ctor && false,
            span: Span::new(start, self.prev_end()),
        }
    }

    fn parse_union_type(&mut self) -> TypeNode {
        self.eat(Tok::Bar); // leading |
        let start = self.start();
        let first = self.parse_intersection_type();
        if self.token() != Tok::Bar {
            return first;
        }
        let mut members = vec![first];
        while self.eat(Tok::Bar) {
            members.push(self.parse_intersection_type());
        }
        TypeNode::Union {
            members,
            span: Span::new(start, self.prev_end()),
        }
    }

    fn parse_intersection_type(&mut self) -> TypeNode {
        self.eat(Tok::Amp);
        let start = self.start();
        let first = self.parse_postfix_type();
        if self.token() != Tok::Amp {
            return first;
        }
        let mut members = vec![first];
        while self.eat(Tok::Amp) {
            members.push(self.parse_postfix_type());
        }
        TypeNode::Intersection {
            members,
            span: Span::new(start, self.prev_end()),
        }
    }

    fn parse_postfix_type(&mut self) -> TypeNode {
        let start = self.start();
        let mut t = self.parse_primary_type();
        loop {
            if self.token() == Tok::OpenBracket && !self.line_break_before() {
                self.next();
                if self.token() == Tok::CloseBracket {
                    let end = self.scanner.token_end();
                    self.next();
                    t = TypeNode::Array {
                        elem: Box::new(t),
                        span: Span::new(start, end),
                    };
                } else {
                    let index = self.parse_type();
                    let end = self.scanner.token_end();
                    self.expect(Tok::CloseBracket);
                    t = TypeNode::IndexedAccess {
                        obj: Box::new(t),
                        index: Box::new(index),
                        span: Span::new(start, end),
                    };
                }
            } else if self.token() == Tok::Bang && !self.line_break_before() {
                // `number!` — a trailing JSDoc non-null marker is invalid TS but
                // parsed (the checker reports the error).
                self.next();
            } else {
                break;
            }
        }
        t
    }

    fn parse_primary_type(&mut self) -> TypeNode {
        use Tok::*;
        let span = self.token_span();
        match self.token() {
            // A type keyword used as a namespace qualifier (`string.X`) is a
            // qualified entity name, not the keyword type.
            t if t.is_keyword()
                && !matches!(t, Tok::KImport | Tok::KTypeof | Tok::KThis | Tok::KNew)
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == Tok::Dot {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                TypeNode::Ref(self.parse_type_ref())
            }
            // `*` is a legacy/JSDoc any-like type; tsc parses it.
            Star => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Any, span)
            }
            // Legacy JSDoc-style function type `function(params): ret`; tsc
            // parses it (modeled as `any`).
            KFunction => {
                self.next();
                if self.token() == Tok::OpenParen {
                    self.next();
                    let mut depth = 1u32;
                    while depth > 0 && self.token() != Tok::Eof {
                        match self.token() {
                            Tok::OpenParen => depth += 1,
                            Tok::CloseParen => depth -= 1,
                            _ => {}
                        }
                        self.next();
                    }
                    if self.eat(Tok::Colon) {
                        let _ = self.parse_type();
                    }
                }
                TypeNode::Keyword(
                    KeywordTypeKind::Any,
                    Span::new(span.start as usize, self.prev_end()),
                )
            }
            KImport => {
                self.next();
                self.expect(Tok::OpenParen);
                // tsc parses any type as the argument; the "argument must be a
                // string literal" rule is enforced by the checker.
                let _ = self.parse_type();
                // optional import-attributes argument: `import("m", { with: … })`
                if self.eat(Tok::Comma) {
                    let _ = self.parse_assignment_expr();
                }
                self.expect(Tok::CloseParen);
                while self.token() == Tok::Dot {
                    self.next();
                    let _ = self.parse_ident_name();
                }
                if self.token() == Tok::Lt {
                    let _ = self.parse_type_args();
                }
                TypeNode::Keyword(
                    KeywordTypeKind::Any,
                    Span::new(span.start as usize, self.prev_end()),
                )
            }
            KAny => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Any, span)
            }
            KUnknown => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Unknown, span)
            }
            KString => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::String, span)
            }
            KNumber => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Number, span)
            }
            KBoolean => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Boolean, span)
            }
            KObject => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Object, span)
            }
            KSymbol => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Symbol, span)
            }
            KBigint => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Bigint, span)
            }
            KVoid => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Void, span)
            }
            KUndefined => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Undefined, span)
            }
            KNever => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Never, span)
            }
            KNull => {
                self.next();
                TypeNode::Keyword(KeywordTypeKind::Null, span)
            }
            KThis => {
                self.next();
                if self.token() == Tok::KIs {
                    self.next();
                    let t = self.parse_type();
                    TypeNode::Predicate {
                        param_name: crate::ast::Ident {
                            name: "this".to_string(),
                            span,
                        },
                        asserts: false,
                        ty: Some(Box::new(t)),
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                } else {
                    TypeNode::This(span)
                }
            }
            StrLit => {
                let v = self.token_wtf8();
                self.next();
                TypeNode::LiteralString { value: v, span }
            }
            NumLit => {
                let v: f64 = self.token_value().parse().unwrap_or(0.0);
                let text = self.token_value();
                self.next();
                TypeNode::LiteralNumber {
                    value: v,
                    text,
                    span,
                }
            }
            BigIntLit => {
                let text = self.token_value();
                self.next();
                TypeNode::LiteralBigInt { text, span }
            }
            KTrue => {
                self.next();
                TypeNode::LiteralBool { value: true, span }
            }
            KFalse => {
                self.next();
                TypeNode::LiteralBool { value: false, span }
            }
            Minus => {
                // negative numeric literal type
                self.next();
                if self.token() == NumLit {
                    let v: f64 = self.token_value().parse().unwrap_or(0.0);
                    let text = format!("-{}", self.token_value());
                    let end = self.scanner.token_end();
                    self.next();
                    TypeNode::LiteralNumber {
                        value: -v,
                        text,
                        span: Span::new(span.start as usize, end),
                    }
                } else if self.token() == BigIntLit {
                    let text = format!("-{}", self.token_value());
                    let end = self.scanner.token_end();
                    self.next();
                    TypeNode::LiteralBigInt {
                        text,
                        span: Span::new(span.start as usize, end),
                    }
                } else {
                    self.error_at_current(&gen::Type_expected, &[]);
                    TypeNode::Keyword(KeywordTypeKind::Any, span)
                }
            }
            KTypeof => {
                self.next();
                if self.token() == Tok::KImport {
                    // `typeof import("m").Member` — import type query, modeled
                    // as `any` like other import types.
                    self.next();
                    self.expect(Tok::OpenParen);
                    if self.token() == Tok::StrLit {
                        self.next();
                    } else {
                        self.error_at_current(&gen::String_literal_expected, &[]);
                    }
                    // optional import-attributes argument: `import("m", { with: … })`
                    if self.eat(Tok::Comma) {
                        let _ = self.parse_assignment_expr();
                    }
                    self.expect(Tok::CloseParen);
                    while self.token() == Tok::Dot {
                        self.next();
                        let _ = self.parse_ident_name();
                    }
                    if self.token() == Tok::Lt {
                        let _ = self.parse_type_args();
                    }
                    TypeNode::Keyword(
                        KeywordTypeKind::Any,
                        Span::new(span.start as usize, self.prev_end()),
                    )
                } else {
                    let name = self.parse_entity_name();
                    // `typeof X<Args>` instantiation expression — consume the
                    // type arguments (modeled by the query itself).
                    let type_args = if self.token() == Tok::Lt {
                        self.try_parse_type_args()
                    } else {
                        None
                    };
                    TypeNode::TypeQuery {
                        name,
                        type_args,
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                }
            }
            KKeyof => {
                self.next();
                let ty = self.parse_postfix_type();
                TypeNode::Keyof {
                    ty: Box::new(ty),
                    span: Span::new(span.start as usize, self.prev_end()),
                }
            }
            KReadonly => {
                self.next();
                let ty = self.parse_postfix_type();
                TypeNode::ReadonlyOp {
                    ty: Box::new(ty),
                    span: Span::new(span.start as usize, self.prev_end()),
                }
            }
            OpenParen => {
                self.next();
                let saved = self.disallow_conditional;
                self.disallow_conditional = false;
                let inner = self.parse_type();
                self.disallow_conditional = saved;
                let end = self.scanner.token_end();
                self.expect(CloseParen);
                TypeNode::Paren {
                    inner: Box::new(inner),
                    span: Span::new(span.start as usize, end),
                }
            }
            NoSubTemplate => {
                let v = self.token_wtf8();
                self.next();
                TypeNode::LiteralString { value: v, span }
            }
            TemplateHead => {
                let start = self.start();
                let head = self.token_value();
                self.next();
                let mut parts: Vec<(TypeNode, String)> = Vec::new();
                loop {
                    let t = self.parse_type();
                    if self.token() != Tok::CloseBrace {
                        self.error_at_current(&gen::_0_expected, &["}".to_string()]);
                        parts.push((t, String::new()));
                        break;
                    }
                    let tok = self.scanner.scan_template(false);
                    let text = self.token_value();
                    parts.push((t, text));
                    if tok == Tok::TemplateTail {
                        self.next();
                        break;
                    }
                    self.next();
                }
                TypeNode::TemplateLit {
                    head,
                    parts,
                    span: Span::new(start, self.prev_end()),
                }
            }
            OpenBrace if self.is_mapped_type_start() => self.parse_mapped_type(),
            OpenBrace => {
                let members = self.parse_type_members();
                TypeNode::TypeLiteral {
                    members,
                    span: Span::new(span.start as usize, self.prev_end()),
                }
            }
            OpenBracket => {
                self.next();
                let mut elems = Vec::new();
                while self.token() != CloseBracket && self.token() != Eof {
                    let estart = self.start();
                    let dotdotdot = self.eat(DotDotDot);
                    // Named tuple member `name: T` / `name?: T` (and the rest
                    // form `...name: T`). The label carries no type information,
                    // so only the element type and optionality are kept — but it
                    // must be recognized so the `:` is consumed. tsc
                    // isTupleElementName: identifier/keyword followed by `:` or
                    // `?:`.
                    let is_named = self.is_ident_like()
                        && self
                            .lookahead(|p| {
                                p.next();
                                if p.token() == Question {
                                    p.next();
                                }
                                if p.token() == Colon {
                                    Some(())
                                } else {
                                    None
                                }
                            })
                            .is_some();
                    let (question, ty) = if is_named {
                        let _label = self.parse_ident();
                        let q = self.eat(Question);
                        self.expect(Colon);
                        // `[name: ...T]` — a rest marker after the label is
                        // invalid but parsed (the checker reports the error).
                        let _ = self.eat(DotDotDot);
                        (q, self.parse_type())
                    } else {
                        // Unnamed element: `T` or optional `T?`.
                        let t = self.parse_type();
                        (self.eat(Question), t)
                    };
                    // A trailing `?` after the element type (`[name: T?]`) is
                    // invalid but parsed (the checker reports the error).
                    let _ = self.eat(Question);
                    elems.push(TupleElem {
                        dotdotdot,
                        question,
                        ty,
                        span: Span::new(estart, self.prev_end()),
                    });
                    if !self.eat(Comma) {
                        break;
                    }
                }
                let end = self.scanner.token_end();
                self.expect(CloseBracket);
                TypeNode::Tuple {
                    elems,
                    span: Span::new(span.start as usize, end),
                }
            }
            // `unique symbol` — the only valid `unique` type operator. `unique`
            // is contextual (arrives as an identifier); keep the operator so the
            // checker can validate its declaration context.
            _ if self.is_ident_like()
                && self.token_value() == "unique"
                && self
                    .lookahead(|p| {
                        p.next();
                        // the `unique` type operator must be followed by a type
                        if p.is_ident_like()
                            || p.token().is_keyword()
                            || matches!(
                                p.token(),
                                Tok::OpenParen | Tok::OpenBrace | Tok::OpenBracket
                            )
                        {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                let start = span.start as usize;
                self.next(); // unique
                let valid_symbol = self.token() == Tok::KSymbol;
                let ty = self.parse_postfix_type();
                let end = ty.span().end as usize;
                TypeNode::Unique {
                    ty: Box::new(ty),
                    span: Span::new(start, end),
                    valid_symbol,
                }
            }
            _ if self.is_ident_like()
                && self.token_value() == "infer"
                && self
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
                let start = self.start();
                self.next();
                let name = self.parse_ident();
                // Optional constraint `infer X extends C`. When a `?` follows the
                // constraint outside a disallow-conditional context, the
                // `extends C ? … : …` is a nested conditional instead, so the
                // constraint is backtracked (mirrors tsc's parseConstraintOfInferType).
                let mut constraint = None;
                if self.token() == Tok::KExtends {
                    let snap = self.save();
                    self.next();
                    let parsed_constraint = self.parse_non_conditional_type();
                    if !self.disallow_conditional && self.token() == Tok::Question {
                        self.restore(snap);
                    } else {
                        constraint = Some(Box::new(parsed_constraint));
                    }
                }
                TypeNode::Infer {
                    name,
                    constraint,
                    span: Span::new(start, self.prev_end()),
                }
            }
            _ if self.is_ident_like() || self.token().is_keyword() => {
                TypeNode::Ref(self.parse_type_ref())
            }
            _ => {
                self.error_at_current(&gen::Type_expected, &[]);
                TypeNode::Keyword(
                    KeywordTypeKind::Any,
                    Span::new(span.start as usize, span.start as usize),
                )
            }
        }
    }

    fn is_mapped_type_start(&mut self) -> bool {
        self.lookahead(|p| {
            p.next(); // {
                      // optional readonly modifier (with +/-)
            if p.token() == Tok::Plus || p.token() == Tok::Minus {
                p.next();
            }
            if p.token() == Tok::KReadonly {
                p.next();
            }
            if p.token() != Tok::OpenBracket {
                return None;
            }
            p.next();
            if !p.is_ident_like() {
                return None;
            }
            p.next();
            if p.token() == Tok::KIn {
                Some(())
            } else {
                None
            }
        })
        .is_some()
    }

    fn parse_mapped_type(&mut self) -> TypeNode {
        let start = self.start();
        // A mapped type's contents are a fresh type context: conditional types
        // are allowed again, so an `infer X extends C ? … : …` constraint in the
        // `in` clause is reinterpreted as a conditional (matching tsc).
        let saved_disallow = self.disallow_conditional;
        self.disallow_conditional = false;
        self.expect(Tok::OpenBrace);
        let mut readonly_mod = None;
        if self.token() == Tok::Plus {
            self.next();
            if self.eat(Tok::KReadonly) {
                readonly_mod = Some(MappedModifier::Add);
            }
        } else if self.token() == Tok::Minus {
            self.next();
            if self.eat(Tok::KReadonly) {
                readonly_mod = Some(MappedModifier::Remove);
            }
        } else if self.eat(Tok::KReadonly) {
            readonly_mod = Some(MappedModifier::Add);
        }
        self.expect(Tok::OpenBracket);
        let key = self.parse_ident();
        self.expect(Tok::KIn);
        let constraint = self.parse_type();
        let name_type = if self.eat(Tok::KAs) {
            Some(self.parse_type())
        } else {
            None
        };
        self.expect(Tok::CloseBracket);
        let mut optional_mod = None;
        if self.token() == Tok::Plus {
            self.next();
            if self.eat(Tok::Question) {
                optional_mod = Some(MappedModifier::Add);
            }
        } else if self.token() == Tok::Minus {
            self.next();
            if self.eat(Tok::Question) {
                optional_mod = Some(MappedModifier::Remove);
            }
        } else if self.eat(Tok::Question) {
            optional_mod = Some(MappedModifier::Add);
        }
        let value = if self.eat(Tok::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        self.eat(Tok::Semicolon);
        // A mapped type may not declare additional members, but tsc parses any
        // trailing members into the node and defers the error to the checker.
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let before = self.scanner.token_start;
            let _ = self.parse_type_member();
            if self.scanner.token_start == before {
                self.next();
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        self.disallow_conditional = saved_disallow;
        TypeNode::Mapped(Box::new(MappedTypeNode {
            readonly_mod,
            key,
            constraint,
            name_type,
            optional_mod,
            value,
            span: Span::new(start, end),
        }))
    }

    fn parse_entity_name(&mut self) -> EntityName {
        let start = self.start();
        let first = if self.token() == Tok::KThis {
            let id = crate::ast::Ident {
                name: "this".to_string(),
                span: self.token_span(),
            };
            self.next();
            id
        } else {
            self.parse_ident_name()
        };
        let mut parts = vec![first];
        while matches!(self.token(), Tok::Dot | Tok::QuestionDot) {
            if self.token() == Tok::Dot {
                // stop at `.<` (Closure-style `Array.<T>`); the caller consumes
                // the dot before the type arguments.
                let dot_then_lt = self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == Tok::Lt {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some();
                if dot_then_lt {
                    break;
                }
            }
            // `?.` is not valid in a type name (checker error); accept it.
            self.next();
            parts.push(self.parse_ident_name());
        }
        EntityName {
            parts,
            span: Span::new(start, self.prev_end()),
        }
    }

    pub(crate) fn parse_type_ref(&mut self) -> TypeRef {
        let start = self.start();
        let name = self.parse_entity_name();
        // Closure-style dotted type args: `Array.<number>`
        if self.token() == Tok::Dot {
            self.next();
        }
        let type_args = if self.token() == Tok::Lt {
            self.parse_type_args()
        } else {
            None
        };
        TypeRef {
            name,
            type_args,
            span: Span::new(start, self.prev_end()),
        }
    }

    pub(crate) fn parse_type_args(&mut self) -> Option<Vec<TypeNode>> {
        if !self.eat(Tok::Lt) {
            return None;
        }
        let mut args = Vec::new();
        while self.token() != Tok::Gt && self.token() != Tok::Eof {
            args.push(self.parse_type());
            if self.token() == Tok::Comma {
                let comma_span = self.token_span();
                self.next();
                if self.token() == Tok::Gt {
                    self.error_at(comma_span, &gen::Trailing_comma_not_allowed, &[]);
                    break;
                }
                continue;
            }
            break;
        }
        self.expect(Tok::Gt);
        Some(args)
    }

    /// Speculative: `<T, U>` followed by `(` — used for call type arguments.
    pub(crate) fn try_parse_type_args(&mut self) -> Option<Vec<TypeNode>> {
        self.try_parse(|p| {
            let args = p.parse_type_args()?;
            Some(args)
        })
    }

    pub(crate) fn parse_type_members(&mut self) -> Vec<TypeMember> {
        let mut members = Vec::new();
        if !self.expect(Tok::OpenBrace) {
            return members;
        }
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let before = self.scanner.token_start;
            if let Some(m) = self.parse_type_member() {
                members.push(m);
            }
            if self.scanner.token_start == before
                && self.token() != Tok::CloseBrace
                && self.token() != Tok::Eof
            {
                self.next();
            }
        }
        self.expect(Tok::CloseBrace);
        members
    }

    pub(crate) fn parse_semicolon_or_comma_in_type_member(&mut self) {
        if self.token() == Tok::Semicolon || self.token() == Tok::Comma {
            self.next();
        }
    }

    fn parse_type_member(&mut self) -> Option<TypeMember> {
        let start = self.start();
        // visibility/static modifiers are illegal on type members (TS1070)
        // but parse so the member itself still checks
        let mut illegal_modifiers: Modifiers = Vec::new();
        let mut declare_span: Option<Span> = None;
        loop {
            let kind = match self.token() {
                Tok::KPublic => ModifierKind::Public,
                Tok::KPrivate => ModifierKind::Private,
                Tok::KProtected => ModifierKind::Protected,
                Tok::KStatic => ModifierKind::Static,
                Tok::KDeclare => {
                    let ok = self
                        .lookahead(|p| {
                            p.next();
                            if p.token() == Tok::OpenBracket || p.is_ident_like() {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some();
                    if !ok {
                        break;
                    }
                    declare_span = Some(self.token_span());
                    self.next();
                    continue;
                }
                _ => break,
            };
            let span = self.token_span();
            let ok = self
                .lookahead(|p| {
                    p.next();
                    if p.is_ident_like()
                        || p.token() == Tok::OpenBracket
                        || p.token() == Tok::KReadonly
                    {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some();
            if !ok {
                break;
            }
            self.next();
            illegal_modifiers.push(Modifier { kind, span });
        }
        // call signature
        if self.token() == Tok::OpenParen || self.token() == Tok::Lt {
            let type_params = self.parse_type_params_opt();
            let params = self.parse_params();
            let return_type = if self.eat(Tok::Colon) {
                Some(self.parse_type_or_predicate())
            } else {
                None
            };
            self.parse_semicolon_or_comma_in_type_member();
            return Some(TypeMember::Call(CallSig {
                type_params,
                params,
                return_type,
                span: Span::new(start, self.prev_end()),
            }));
        }
        // construct signature
        if self.token() == Tok::KNew {
            let snap = self.save();
            self.next();
            if self.token() == Tok::OpenParen || self.token() == Tok::Lt {
                let type_params = self.parse_type_params_opt();
                let params = self.parse_params();
                let return_type = if self.eat(Tok::Colon) {
                    Some(self.parse_type_or_predicate())
                } else {
                    None
                };
                self.parse_semicolon_or_comma_in_type_member();
                return Some(TypeMember::Ctor(CallSig {
                    type_params,
                    params,
                    return_type,
                    span: Span::new(start, self.prev_end()),
                }));
            }
            self.restore(snap);
        }
        let readonly = if self.token() == Tok::KReadonly {
            let snap = self.save();
            self.next();
            if matches!(
                self.token(),
                Tok::Colon
                    | Tok::Question
                    | Tok::OpenParen
                    | Tok::Semicolon
                    | Tok::Comma
                    | Tok::CloseBrace
                    | Tok::Lt
            ) {
                self.restore(snap);
                false
            } else {
                true
            }
        } else {
            false
        };
        // index signature
        if self.token() == Tok::OpenBracket {
            if let Some(mut idx) = self.try_parse_index_sig(readonly) {
                idx.declare_span = declare_span;
                return Some(TypeMember::Index(idx));
            }
        }
        if self.token() == Tok::CloseBrace || self.token() == Tok::Eof {
            return None;
        }
        let name = self.parse_prop_name();
        let question = self.eat(Tok::Question);
        if self.token() == Tok::OpenParen || self.token() == Tok::Lt {
            let type_params = self.parse_type_params_opt();
            let params = self.parse_params();
            let return_type = if self.eat(Tok::Colon) {
                Some(self.parse_type_or_predicate())
            } else {
                None
            };
            self.parse_semicolon_or_comma_in_type_member();
            return Some(TypeMember::Method(MethodSig {
                name,
                question,
                type_params,
                params,
                return_type,
                span: Span::new(start, self.prev_end()),
            }));
        }
        let ty = if self.eat(Tok::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        let end = self.prev_end();
        self.parse_semicolon_or_comma_in_type_member();
        Some(TypeMember::Prop(PropSig {
            illegal_modifiers: illegal_modifiers.clone(),
            readonly,
            name,
            question,
            ty,
            span: Span::new(start, end),
        }))
    }

    // ── expressions ─────────────────────────────────────────────────────────
}
