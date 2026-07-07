//! Expression parsing: assignment/conditional/binary/unary precedence climb,
//! arrow functions, JSX, call/member tails, template literals, and primary
//! expressions including object literals. Split out of `parser/mod.rs`.

use super::Parser;
use crate::ast::*;
use crate::diagnostics::gen;
use crate::scanner::Tok;

/// tsc isLeftHandSideExpressionKind (_tsc.js:12210): expression forms allowed
/// to the left of an assignment operator during parsing.
fn is_left_hand_side_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Ident(_)
            | Expr::NumLit { .. }
            | Expr::StrLit { .. }
            | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. }
            | Expr::NullLit { .. }
            | Expr::RegexLit { .. }
            | Expr::Template { .. }
            | Expr::Array { .. }
            | Expr::Object { .. }
            | Expr::FunctionExpr(_)
            | Expr::ClassExpr(_)
            | Expr::Call { .. }
            | Expr::New { .. }
            | Expr::PropAccess { .. }
            | Expr::ElemAccess { .. }
            | Expr::Paren { .. }
            | Expr::NonNull { .. }
            | Expr::This { .. }
            | Expr::Super { .. }
            | Expr::ImportCall { .. }
            | Expr::ImportMeta { .. }
    )
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_expression(&mut self) -> Expr {
        let start = self.start();
        let mut e = self.parse_assignment_expr();
        while self.token() == Tok::Comma {
            let op_span = self.token_span();
            self.next();
            let right = self.parse_assignment_expr();
            let span = Span::new(start, right.span().end as usize);
            e = Expr::Binary {
                op: BinOp::Comma,
                op_span,
                left: Box::new(e),
                right: Box::new(right),
                span,
            };
        }
        e
    }

    pub(crate) fn parse_assignment_expr(&mut self) -> Expr {
        // arrow function lookahead
        if let Some(arrow) = self.try_parse_arrow() {
            return arrow;
        }
        if self.token() == Tok::KYield {
            let start = self.start();
            self.next();
            // `yield*` delegates to another iterable.
            let delegate = self.eat(Tok::Star);
            let expr = if !self.line_break_before()
                && !matches!(
                    self.token(),
                    Tok::Semicolon
                        | Tok::CloseParen
                        | Tok::CloseBracket
                        | Tok::CloseBrace
                        | Tok::Comma
                        | Tok::Eof
                ) {
                Some(Box::new(self.parse_assignment_expr()))
            } else {
                None
            };
            return Expr::Yield {
                expr,
                delegate,
                span: Span::new(start, self.prev_end()),
            };
        }
        let start = self.start();
        let left = self.parse_conditional_expr();
        let op = match self.token() {
            Tok::Eq => Some(BinOp::Assign),
            Tok::PlusEq => Some(BinOp::AddAssign),
            Tok::MinusEq => Some(BinOp::SubAssign),
            Tok::StarEq => Some(BinOp::MulAssign),
            Tok::StarStarEq => Some(BinOp::ExpAssign),
            Tok::SlashEq => Some(BinOp::DivAssign),
            Tok::PercentEq => Some(BinOp::ModAssign),
            Tok::LtLtEq => Some(BinOp::ShlAssign),
            Tok::GtGtEq => Some(BinOp::ShrAssign),
            Tok::GtGtGtEq => Some(BinOp::UShrAssign),
            Tok::AmpEq => Some(BinOp::AmpAssign),
            Tok::BarEq => Some(BinOp::BarAssign),
            Tok::CaretEq => Some(BinOp::CaretAssign),
            Tok::AmpAmpEq => Some(BinOp::AmpAmpAssign),
            Tok::BarBarEq => Some(BinOp::BarBarAssign),
            Tok::QuestionQuestionEq => Some(BinOp::QuestionQuestionAssign),
            _ => None,
        };
        if let Some(op) = op {
            if !is_left_hand_side_expr(&left) {
                // tsc leaves the assignment token unconsumed here; statement
                // recovery reports the parse error at that token.
                return left;
            }
            let op_span = self.token_span();
            self.next();
            let right = self.parse_assignment_expr();
            let span = Span::new(start, right.span().end as usize);
            return Expr::Binary {
                op,
                op_span,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        left
    }

    fn try_parse_arrow(&mut self) -> Option<Expr> {
        // async arrow?
        if self.token() == Tok::KAsync {
            let r = self.try_parse(|p| {
                let start = p.start();
                let mod_span = p.token_span();
                p.next();
                if p.line_break_before() {
                    return None;
                }
                p.parse_arrow_tail(
                    start,
                    vec![Modifier {
                        kind: ModifierKind::Async,
                        span: mod_span,
                    }],
                )
            });
            if r.is_some() {
                return r;
            }
        }
        if self.is_ident_like() || self.token() == Tok::OpenParen || self.token() == Tok::Lt {
            return self.try_parse(|p| {
                let start = p.start();
                p.parse_arrow_tail(start, Vec::new())
            });
        }
        None
    }

    fn parse_arrow_tail(&mut self, start: usize, modifiers: Modifiers) -> Option<Expr> {
        let type_params = if self.token() == Tok::Lt {
            // A generic arrow's type-parameter list must begin with a type
            // parameter (identifier / variance / const / modifier) or be empty
            // (`<>`). If `(` or some other token follows the `<`, this is not a
            // generic arrow — bail so an angle type assertion of a function
            // type, e.g. `<(s: string) => number>x`, is parsed instead.
            let looks_like_type_params = self
                .lookahead(|p| {
                    p.next();
                    if p.is_ident_like()
                        || p.token().is_strict_reserved_word()
                        || matches!(p.token(), Tok::KIn | Tok::KConst | Tok::Gt)
                    {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some();
            if !looks_like_type_params {
                return None;
            }
            Some(self.parse_type_params_opt()?)
        } else {
            None
        };
        let mut ambiguous = false;
        let mut has_return_colon = false;
        let (params, return_type) = if self.is_ident_like() {
            // single ident param, no parens, no return type
            let id = self.parse_ident();
            let span = id.span;
            (
                vec![Param {
                    decorators: Vec::new(),
                    modifiers: Vec::new(),
                    dotdotdot: false,
                    dotdotdot_span: None,
                    name: Binding::Ident(id),
                    question: false,
                    question_span: None,
                    ty: None,
                    initializer: None,
                    span,
                }],
                None,
            )
        } else if self.token() == Tok::OpenParen {
            let snap_diags = self.diags.len();
            let params = self.parse_params();
            if self.diags.len() != snap_diags {
                return None; // param list had syntax errors → not an arrow
            }
            // A parameter list of only bare identifiers or destructuring
            // patterns (no types, `?`, rest, or modifiers) could equally be a
            // parenthesized expression; tsc disambiguates such "ambiguous"
            // arrows from a conditional via lookahead past the body.
            ambiguous = params
                .iter()
                .all(|p| p.ty.is_none() && !p.question && !p.dotdotdot && p.modifiers.is_empty());
            has_return_colon = self.token() == Tok::Colon;
            let return_type = if has_return_colon {
                self.next();
                Some(self.parse_type_or_predicate())
            } else {
                None
            };
            (params, return_type)
        } else {
            return None;
        };
        if self.token() != Tok::Arrow {
            return None;
        }
        self.next();
        let body = if self.token() == Tok::OpenBrace {
            FuncBody::Block(self.parse_block())
        } else {
            FuncBody::Expr(Box::new(self.parse_assignment_expr()))
        };
        // In the true branch of a conditional, an ambiguous parenthesized form
        // with a return type is only an arrow if a `:` (the conditional
        // separator) follows the body; otherwise rewind and treat `(…)` as a
        // parenthesized expression.
        if !self.allow_return_type_in_arrow
            && ambiguous
            && has_return_colon
            && self.token() != Tok::Colon
        {
            return None;
        }
        Some(Expr::Arrow(Box::new(FunctionLike {
            decorators: Vec::new(),
            kind: FuncKind::Arrow,
            modifiers,
            name: None,
            question: false,
            type_params,
            params,
            return_type,
            body: Some(body),
            is_generator: false,
            span: Span::new(start, self.prev_end()),
        })))
    }

    fn parse_conditional_expr(&mut self) -> Expr {
        let start = self.start();
        let cond = self.parse_binary_expr(0);
        if self.token() == Tok::Question {
            self.next();
            // The true branch never allows an arrow return type; the false
            // branch inherits the incoming flag (so a conditional nested in a
            // true branch keeps the restriction in its own false branch).
            let saved = self.allow_return_type_in_arrow;
            self.allow_return_type_in_arrow = false;
            let when_true = self.parse_assignment_expr();
            self.allow_return_type_in_arrow = saved;
            self.expect(Tok::Colon);
            let when_false = self.parse_assignment_expr();
            let span = Span::new(start, when_false.span().end as usize);
            return Expr::Cond {
                cond: Box::new(cond),
                when_true: Box::new(when_true),
                when_false: Box::new(when_false),
                span,
            };
        }
        cond
    }

    fn binary_op_for_token(&mut self) -> Option<(BinOp, u8)> {
        use BinOp::*;
        // (op, precedence) — higher binds tighter
        let r = match self.token() {
            // logical operators bind looser than bitwise-or; `??` shares `||`'s
            // level (mixing the two without parentheses is a separate grammar
            // error, not modeled here).
            Tok::QuestionQuestion => (QuestionQuestion, 2),
            Tok::BarBar => (BarBar, 2),
            Tok::AmpAmp => (AmpAmp, 3),
            Tok::Bar => (BitOr, 4),
            Tok::Caret => (BitXor, 5),
            Tok::Amp => (BitAnd, 6),
            Tok::EqEq => (EqEq, 7),
            Tok::BangEq => (NotEq, 7),
            Tok::EqEqEq => (EqEqEq, 7),
            Tok::BangEqEq => (NotEqEq, 7),
            Tok::Lt => (Lt, 8),
            Tok::LtEq => (LtEq, 8),
            Tok::KIn if !self.no_in => (In, 8),
            Tok::KInstanceof => (Instanceof, 8),
            Tok::Gt => {
                // rescan for >> >= etc.
                match self.scanner.rescan_greater() {
                    Tok::Gt => (Gt, 8),
                    Tok::GtEq => (GtEq, 8),
                    Tok::GtGt => (Shr, 9),
                    Tok::GtGtGt => (UShr, 9),
                    Tok::GtGtEq | Tok::GtGtGtEq => return None, // assignment handled elsewhere
                    _ => return None,
                }
            }
            // A prior rescan_greater() (e.g. from an inner precedence level that
            // declined to consume) may have already widened `>` into one of
            // these; handle them directly so the shift isn't dropped.
            Tok::GtGt => (Shr, 9),
            Tok::GtGtGt => (UShr, 9),
            Tok::GtGtEq | Tok::GtGtGtEq => return None,
            Tok::LtLt => (Shl, 9),
            Tok::Plus => (Add, 10),
            Tok::Minus => (Sub, 10),
            Tok::Star => (Mul, 11),
            Tok::Slash => (Div, 11),
            Tok::Percent => (Mod, 11),
            Tok::StarStar => (Exp, 12),
            _ => return None,
        };
        Some(r)
    }

    fn parse_binary_expr(&mut self, min_prec: u8) -> Expr {
        let start = self.start();
        let left = self.parse_unary_expr();
        self.parse_binary_continuation(left, start, min_prec)
    }

    fn parse_binary_continuation(&mut self, mut left: Expr, start: usize, min_prec: u8) -> Expr {
        loop {
            // `as` / `satisfies`
            if (self.token() == Tok::KAs || self.token() == Tok::KSatisfies)
                && !self.line_break_before()
            {
                let kind = if self.token() == Tok::KAs {
                    AssertionKind::As
                } else {
                    AssertionKind::Satisfies
                };
                let kw_span = self.token_span();
                self.next();
                if kind == AssertionKind::As && self.token() == Tok::KConst {
                    let cspan = self.token_span();
                    self.next();
                    let span = Span::new(start, cspan.end as usize);
                    left = Expr::Assertion {
                        expr: Box::new(left),
                        ty: TypeNode::Keyword(KeywordTypeKind::Any, cspan),
                        kind: AssertionKind::ConstAssert,
                        kw_span,
                        span,
                    };
                    continue;
                }
                let ty = self.parse_type();
                let span = Span::new(start, self.prev_end());
                left = Expr::Assertion {
                    expr: Box::new(left),
                    ty,
                    kind,
                    kw_span,
                    span,
                };
                continue;
            }
            let Some((op, prec)) = self.binary_op_for_token() else {
                break;
            };
            if prec < min_prec {
                break;
            }
            let op_span = self.token_span();
            self.next();
            // ** is right-associative
            let right = if op == BinOp::Exp {
                self.parse_binary_expr(prec)
            } else {
                self.parse_binary_expr(prec + 1)
            };
            let span = Span::new(start, right.span().end as usize);
            left = Expr::Binary {
                op,
                op_span,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        left
    }

    fn parse_unary_expr(&mut self) -> Expr {
        use Tok::*;
        let start = self.start();
        let op = match self.token() {
            Plus => Some(UnaryOp::Plus),
            Minus => Some(UnaryOp::Minus),
            Bang => Some(UnaryOp::Bang),
            Tilde => Some(UnaryOp::Tilde),
            KTypeof => Some(UnaryOp::Typeof),
            KVoid => Some(UnaryOp::Void),
            KDelete => Some(UnaryOp::Delete),
            _ => None,
        };
        if let Some(op) = op {
            self.next();
            let operand = self.parse_unary_expr();
            let span = Span::new(start, operand.span().end as usize);
            return Expr::Unary {
                op,
                operand: Box::new(operand),
                span,
            };
        }
        if self.token() == PlusPlus || self.token() == MinusMinus {
            let op_plus = self.token() == PlusPlus;
            self.next();
            let operand = self.parse_unary_expr();
            let span = Span::new(start, operand.span().end as usize);
            return Expr::Update {
                op_plus,
                prefix: true,
                operand: Box::new(operand),
                span,
            };
        }
        if self.token() == KAwait {
            // `await` only special inside async or modules; parse as unary always
            let snap = self.save();
            self.next();
            if matches!(
                self.token(),
                Semicolon | CloseParen | CloseBrace | CloseBracket | Comma | Eof | Colon
            ) {
                self.restore(snap);
            } else {
                let expr = self.parse_unary_expr();
                let span = Span::new(start, expr.span().end as usize);
                return Expr::Await {
                    expr: Box::new(expr),
                    span,
                };
            }
        }
        // JSX (in .tsx) replaces angle assertions
        if self.token() == Lt && self.jsx {
            let estart = self.start();
            let first = self.parse_jsx_element();
            // adjacent JSX elements need one parent (2657)
            let mut reported = false;
            while self.token() == Lt
                && self
                    .lookahead(|p| {
                        p.next();
                        if p.is_ident_like() || p.token() == Gt {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some()
            {
                if !reported {
                    reported = true;
                    self.error_at(
                        Span::new(estart, estart + 1),
                        &gen::JSX_expressions_must_have_one_parent_element,
                        &[],
                    );
                }
                let _ = self.parse_jsx_element();
            }
            return first;
        }
        // type assertion <T>expr
        if self.token() == Lt {
            // `<const>expr` const assertion
            if self
                .lookahead(|p| {
                    p.next();
                    if p.token() == Tok::KConst {
                        p.next();
                        if p.token() == Gt {
                            return Some(());
                        }
                    }
                    None
                })
                .is_some()
            {
                self.next(); // <
                let cspan = self.token_span();
                self.next(); // const
                self.next(); // >
                let expr = self.parse_unary_expr();
                let span = Span::new(start, expr.span().end as usize);
                return Expr::Assertion {
                    expr: Box::new(expr),
                    ty: TypeNode::Keyword(KeywordTypeKind::Any, cspan),
                    kind: AssertionKind::ConstAssert,
                    kw_span: Span::new(start, start),
                    span,
                };
            }
            if let Some(e) = self.try_parse(|p| {
                p.next();
                let ty = p.parse_type();
                if !p.eat(Gt) {
                    return None;
                }
                let expr = p.parse_unary_expr();
                let span = Span::new(start, expr.span().end as usize);
                Some(Expr::Assertion {
                    expr: Box::new(expr),
                    ty,
                    kind: AssertionKind::Angle,
                    kw_span: Span::new(start, start),
                    span,
                })
            }) {
                return e;
            }
        }
        let mut e = self.parse_lhs_expression();
        // postfix ++/--
        if (self.token() == PlusPlus || self.token() == MinusMinus) && !self.line_break_before() {
            let op_plus = self.token() == PlusPlus;
            let end = self.scanner.token_end();
            self.next();
            let span = Span::new(start, end);
            e = Expr::Update {
                op_plus,
                prefix: false,
                operand: Box::new(e),
                span,
            };
        }
        e
    }

    /// A JSX tag or attribute name: an identifier (or `this`) optionally
    /// extended with hyphen, namespace (`:`) and member (`.`) parts, e.g.
    /// `data-x`, `xml:lang`, `Foo.Bar`, `this._tagName`. The composed name is
    /// stored verbatim in a single identifier.
    fn parse_jsx_name(&mut self) -> Ident {
        let start = self.start();
        let mut name = if self.token() == Tok::KThis {
            self.next();
            "this".to_string()
        } else {
            self.parse_ident_name().name
        };
        loop {
            match self.token() {
                Tok::Minus | Tok::Colon if !self.line_break_before() => {
                    let sep = if self.token() == Tok::Minus { '-' } else { ':' };
                    self.next();
                    name.push(sep);
                    name.push_str(&self.parse_ident_name().name);
                }
                Tok::Dot => {
                    self.next();
                    name.push('.');
                    name.push_str(&self.parse_ident_name().name);
                }
                _ => break,
            }
        }
        Ident {
            name,
            span: Span::new(start, self.prev_end()),
        }
    }

    /// `<tag attrs>children</tag>` | `<tag attrs/>` | `<>children</>`
    fn parse_jsx_element(&mut self) -> Expr {
        let start = self.start();
        self.expect(Tok::Lt);
        // fragment?
        if self.token() == Tok::Gt {
            // children begin right after `>`
            let (children, closing_span) = self.parse_jsx_children(start);
            let span = Span::new(start, self.prev_end());
            return Expr::JsxElement(Box::new(JsxElement {
                tag: None,
                attrs: Vec::new(),
                children,
                closing_span,
                self_closing: false,
                span,
            }));
        }
        let tag = self.parse_jsx_name();
        // optional type arguments: `<Foo<T> .../>`
        if self.token() == Tok::Lt {
            let _ = self.try_parse_type_args();
        }
        let mut attrs = Vec::new();
        while self.is_ident_like() || self.token().is_keyword() || self.token() == Tok::OpenBrace {
            let astart = self.start();
            // spread attribute: `{...expr}` (carried with an empty name)
            if self.token() == Tok::OpenBrace {
                self.next();
                self.expect(Tok::DotDotDot);
                let e = self.parse_assignment_expr();
                self.expect(Tok::CloseBrace);
                attrs.push(JsxAttr {
                    name: crate::ast::Ident {
                        name: String::new(),
                        span: Span::new(astart, astart),
                    },
                    value: Some(e),
                    span: Span::new(astart, self.prev_end()),
                });
                continue;
            }
            let name = self.parse_jsx_name();
            let value = if self.token() == Tok::Eq {
                // A JSX attribute value string treats backslash literally; flag
                // the value scan so the scanner skips escape processing.
                self.scanner.jsx_attr_string = true;
                self.next();
                self.scanner.jsx_attr_string = false;
                if self.token() == Tok::OpenBrace {
                    self.next();
                    let e = self.parse_expression();
                    self.expect(Tok::CloseBrace);
                    Some(e)
                } else {
                    Some(self.parse_primary_expr())
                }
            } else {
                None
            };
            attrs.push(JsxAttr {
                name,
                value,
                span: Span::new(astart, self.prev_end()),
            });
        }
        if self.token() == Tok::Slash {
            self.next();
            self.expect(Tok::Gt);
            let span = Span::new(start, self.prev_end());
            return Expr::JsxElement(Box::new(JsxElement {
                tag: Some(tag),
                attrs,
                children: Vec::new(),
                closing_span: None,
                self_closing: true,
                span,
            }));
        }
        self.jsx_tag_stack.push(tag.name.clone());
        let (children, closing_span) = self.parse_jsx_children(start);
        self.jsx_tag_stack.pop();
        if closing_span.is_none() {
            // unclosed: 17008 at the tag name
            self.error_at(
                tag.span,
                &gen::JSX_element_0_has_no_corresponding_closing_tag,
                &[tag.name.clone()],
            );
            if self.token() == Tok::Eof {
                self.error_at_current(&gen::_0_expected, &["</".to_string()]);
            }
        }
        let span = Span::new(start, self.prev_end());
        Expr::JsxElement(Box::new(JsxElement {
            tag: Some(tag),
            attrs,
            children,
            closing_span,
            self_closing: false,
            span,
        }))
    }

    /// children after the opening tag's `>` (current token), through `</x>`
    fn parse_jsx_children(&mut self, _elem_start: usize) -> (Vec<JsxChild>, Option<Span>) {
        if self.token() != Tok::Gt {
            // malformed opening tag (EOF / unexpected token): no children
            self.error_at_current(&gen::_0_expected, &[">".to_string()]);
            return (Vec::new(), None);
        }
        let mut children = Vec::new();
        // raw text follows the `>` directly: scan without normal tokenization
        self.scanner.scan_jsx_text();
        loop {
            match self.token() {
                Tok::JsxText => {
                    if !self.scanner.token_value.is_empty() {
                        children.push(JsxChild::Text);
                    }
                    self.next(); // normal scan: next char is `<`, `{`, `}` or EOF
                }
                Tok::OpenBrace => {
                    self.next();
                    if self.token() == Tok::DotDotDot {
                        // spread child `{...expr}`
                        let sstart = self.start();
                        self.next();
                        let e = self.parse_assignment_expr();
                        let span = Span::new(sstart, e.span().end as usize);
                        children.push(JsxChild::Expr(Expr::Spread {
                            expr: Box::new(e),
                            span,
                        }));
                    } else if self.token() == Tok::CloseBrace {
                        // empty container `{}` / `{/* comment */}` — no child
                    } else {
                        let e = self.parse_expression();
                        children.push(JsxChild::Expr(e));
                    }
                    if self.token() == Tok::CloseBrace {
                        self.scanner.scan_jsx_text();
                    } else {
                        self.error_at_current(&gen::_0_expected, &["}".to_string()]);
                    }
                }
                Tok::Lt => {
                    let lt_start = self.start();
                    // closing tag?
                    let is_closing = self
                        .lookahead(|p| {
                            p.next();
                            if p.token() == Tok::Slash {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some();
                    if is_closing {
                        // does the closing name match an ENCLOSING tag instead
                        // of ours? then we are unclosed: bail without consuming
                        let close_name = self.lookahead(|p| {
                            p.next();
                            p.next();
                            if p.token() != Tok::Gt {
                                Some(p.parse_jsx_name().name)
                            } else {
                                None
                            }
                        });
                        if let (Some(cn), Some(own)) = (&close_name, self.jsx_tag_stack.last()) {
                            if cn != own
                                && self.jsx_tag_stack[..self.jsx_tag_stack.len() - 1]
                                    .iter()
                                    .any(|t| t == cn)
                            {
                                return (children, None);
                            }
                        }
                        self.next(); // <
                        self.next(); // /
                        if self.token() != Tok::Gt {
                            let _ = self.parse_jsx_name();
                        }
                        let cend = self.scanner.token_end();
                        self.expect(Tok::Gt);
                        return (children, Some(Span::new(lt_start, cend)));
                    }
                    let e = self.parse_jsx_element();
                    if let Expr::JsxElement(j) = e {
                        children.push(JsxChild::Element(*j));
                    }
                    // the child's closing `>` pre-scanned one token; re-read
                    // it as raw text
                    self.scanner.rescan_jsx_text();
                }
                _ => return (children, None),
            }
        }
    }

    pub(crate) fn parse_lhs_expression(&mut self) -> Expr {
        let start = self.start();
        let mut e = if self.token() == Tok::KNew {
            self.parse_new_expr()
        } else {
            self.parse_primary_expr()
        };
        e = self.parse_call_tail(start, e, true);
        e
    }

    pub(crate) fn parse_new_expr(&mut self) -> Expr {
        let start = self.start();
        self.next(); // new
                     // `new.target` meta-property (and similar `new.<name>`): resolves to a
                     // dynamic value; modeled like other meta-properties.
        if self.token() == Tok::Dot {
            self.next();
            let _name = self.parse_ident_name();
            return Expr::ImportMeta {
                span: Span::new(start, self.prev_end()),
            };
        }
        let callee_start = self.start();
        let mut callee = self.parse_primary_expr();
        // member access on callee (no calls)
        loop {
            if self.token() == Tok::Dot {
                self.next();
                let name = self.parse_ident_name();
                let span = Span::new(callee_start, name.span.end as usize);
                callee = Expr::PropAccess {
                    obj: Box::new(callee),
                    question_dot: false,
                    name,
                    span,
                };
            } else if self.token() == Tok::OpenBracket {
                self.next();
                let index = self.parse_expression();
                let end = self.scanner.token_end();
                self.expect(Tok::CloseBracket);
                callee = Expr::ElemAccess {
                    obj: Box::new(callee),
                    question_dot: false,
                    index: Box::new(index),
                    span: Span::new(callee_start, end),
                };
            } else if matches!(self.token(), Tok::NoSubTemplate | Tok::TemplateHead) {
                // tagged template in member position: ``new f`...` ``
                let t = self.parse_template_expr();
                let span = Span::new(callee_start, t.span().end as usize);
                callee = Expr::Call {
                    callee: Box::new(callee),
                    question_dot: false,
                    type_args: None,
                    args: vec![t],
                    span,
                };
            } else {
                break;
            }
        }
        let type_args = if self.token() == Tok::Lt {
            // Accept `<…>` as type arguments only when they close cleanly on
            // `>`. A malformed close (e.g. `new Date < A ? 1 : 2`, where the
            // `<` is really a comparison) makes parse_type_args emit a
            // diagnostic; detect that and backtrack so the binary-expression
            // parser handles the `<`. (try_parse rolls back the diagnostic.)
            self.try_parse(|p| {
                let before = p.diags.len();
                let targs = p.parse_type_args()?;
                if p.diags.len() != before {
                    None
                } else {
                    Some(targs)
                }
            })
        } else {
            None
        };
        let args = if self.token() == Tok::OpenParen {
            Some(self.parse_arguments())
        } else {
            None
        };
        let end = self.prev_end();
        Expr::New {
            callee: Box::new(callee),
            type_args,
            args,
            span: Span::new(start, end),
        }
    }

    pub(crate) fn parse_arguments(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        self.expect(Tok::OpenParen);
        while self.token() != Tok::CloseParen && self.token() != Tok::Eof {
            if self.token() == Tok::DotDotDot {
                let sstart = self.start();
                self.next();
                let e = self.parse_assignment_expr();
                let span = Span::new(sstart, e.span().end as usize);
                args.push(Expr::Spread {
                    expr: Box::new(e),
                    span,
                });
            } else {
                args.push(self.parse_assignment_expr());
            }
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::CloseParen);
        args
    }

    /// Tokens that may follow the type arguments of a bare instantiation
    /// expression `f<T>` (mirrors tsc's `canFollowTypeArgumentsInExpression`).
    /// These cannot begin an operand, so the preceding `<…>` is type arguments
    /// rather than a `<` comparison.
    fn can_follow_type_args_in_expr(tok: Tok) -> bool {
        use Tok::*;
        matches!(
            tok,
            OpenParen
                | NoSubTemplate
                | TemplateHead
                | Dot
                | QuestionDot
                | CloseParen
                | CloseBracket
                | Colon
                | Semicolon
                | Question
                | EqEq
                | EqEqEq
                | BangEq
                | BangEqEq
                | AmpAmp
                | BarBar
                | QuestionQuestion
                | Caret
                | Amp
                | Bar
                | CloseBrace
                | Eof
                | Comma
                | At
        )
    }

    fn parse_call_tail(&mut self, start: usize, mut e: Expr, allow_call: bool) -> Expr {
        loop {
            match self.token() {
                Tok::Dot => {
                    self.next();
                    let name = self.parse_ident_name();
                    let span = Span::new(start, name.span.end as usize);
                    e = Expr::PropAccess {
                        obj: Box::new(e),
                        question_dot: false,
                        name,
                        span,
                    };
                }
                Tok::QuestionDot => {
                    self.next();
                    if self.token() == Tok::OpenParen {
                        let args = self.parse_arguments();
                        let span = Span::new(start, self.prev_end());
                        e = Expr::Call {
                            callee: Box::new(e),
                            question_dot: true,
                            type_args: None,
                            args,
                            span,
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
                    } else if matches!(self.token(), Tok::NoSubTemplate | Tok::TemplateHead) {
                        // `a?.\`...\`` — a tagged template in an optional chain is
                        // invalid but parsed (the checker reports the error).
                        let args = self.parse_tagged_template_args();
                        let span = Span::new(
                            start,
                            args.first()
                                .map(|arg| arg.span().end as usize)
                                .unwrap_or(start),
                        );
                        e = Expr::Call {
                            callee: Box::new(e),
                            question_dot: true,
                            type_args: None,
                            args,
                            span,
                        };
                    } else {
                        let name = self.parse_ident_name();
                        let span = Span::new(start, name.span.end as usize);
                        e = Expr::PropAccess {
                            obj: Box::new(e),
                            question_dot: true,
                            name,
                            span,
                        };
                    }
                }
                Tok::OpenBracket => {
                    self.next();
                    if self.token() == Tok::CloseBracket {
                        self.error_at_current(
                            &gen::An_element_access_expression_should_take_an_argument,
                            &[],
                        );
                        let end = self.scanner.token_end();
                        self.next();
                        let index = Expr::Missing {
                            span: Span::new(end, end),
                        };
                        e = Expr::ElemAccess {
                            obj: Box::new(e),
                            question_dot: false,
                            index: Box::new(index),
                            span: Span::new(start, end),
                        };
                        continue;
                    }
                    let index = self.parse_expression();
                    let end = self.scanner.token_end();
                    self.expect(Tok::CloseBracket);
                    e = Expr::ElemAccess {
                        obj: Box::new(e),
                        question_dot: false,
                        index: Box::new(index),
                        span: Span::new(start, end),
                    };
                }
                Tok::OpenParen if allow_call => {
                    let args = self.parse_arguments();
                    let span = Span::new(start, self.prev_end());
                    e = Expr::Call {
                        callee: Box::new(e),
                        question_dot: false,
                        type_args: None,
                        args,
                        span,
                    };
                }
                Tok::Lt if allow_call => {
                    // possible call with explicit type args `f<T>(...)`, or a
                    // bare instantiation expression `f<T>` when the type args
                    // are followed by a token that cannot start an operand
                    // (tsc's canFollowTypeArgumentsInExpression).
                    let r = self.try_parse(|p| {
                        let before = p.diags.len();
                        let targs = p.parse_type_args()?;
                        // a malformed close (`x < y;` is a comparison, not type
                        // args) emits a diagnostic — reject and let it parse as
                        // a `<` comparison instead.
                        if p.diags.len() != before {
                            return None;
                        }
                        if p.token() == Tok::OpenParen {
                            let args = p.parse_arguments();
                            Some((targs, Some(args)))
                        } else if Self::can_follow_type_args_in_expr(p.token()) {
                            Some((targs, None))
                        } else {
                            None
                        }
                    });
                    match r {
                        Some((targs, Some(args))) => {
                            let span = Span::new(start, self.prev_end());
                            e = Expr::Call {
                                callee: Box::new(e),
                                question_dot: false,
                                type_args: Some(targs),
                                args,
                                span,
                            };
                        }
                        Some((targs, None))
                            if matches!(self.token(), Tok::NoSubTemplate | Tok::TemplateHead) =>
                        {
                            let args = self.parse_tagged_template_args();
                            let span = Span::new(
                                start,
                                args.first()
                                    .map(|arg| arg.span().end as usize)
                                    .unwrap_or(start),
                            );
                            e = Expr::Call {
                                callee: Box::new(e),
                                question_dot: false,
                                type_args: Some(targs),
                                args,
                                span,
                            };
                        }
                        Some((_targs, None)) => {
                            // bare instantiation expression — type args consumed
                            // but not separately modeled in the AST.
                        }
                        None => break,
                    }
                }
                Tok::Bang if !self.line_break_before() => {
                    let end = self.scanner.token_end();
                    self.next();
                    e = Expr::NonNull {
                        expr: Box::new(e),
                        span: Span::new(start, end),
                    };
                }
                Tok::NoSubTemplate | Tok::TemplateHead => {
                    let args = self.parse_tagged_template_args();
                    let span = Span::new(
                        start,
                        args.first()
                            .map(|arg| arg.span().end as usize)
                            .unwrap_or(start),
                    );
                    e = Expr::Call {
                        callee: Box::new(e),
                        question_dot: false,
                        type_args: None,
                        args,
                        span,
                    };
                }
                _ => break,
            }
        }
        e
    }

    fn parse_tagged_template_args(&mut self) -> Vec<Expr> {
        let template = self.parse_template_expr();
        let span = template.span();
        let mut args = vec![Expr::TemplateStringsArray { span }];
        if let Expr::Template { parts, .. } = template {
            for part in parts {
                if let TemplatePart::Expr(expr) = part {
                    args.push(expr);
                }
            }
        }
        args
    }

    pub(crate) fn parse_template_expr(&mut self) -> Expr {
        let start = self.start();
        let mut parts = Vec::new();
        if self.token() == Tok::NoSubTemplate {
            parts.push(TemplatePart::Str(self.token_wtf8()));
            let end = self.scanner.token_end();
            self.next();
            return Expr::Template {
                parts,
                span: Span::new(start, end),
            };
        }
        // TemplateHead
        parts.push(TemplatePart::Str(self.token_wtf8()));
        self.next();
        loop {
            let e = self.parse_expression();
            parts.push(TemplatePart::Expr(e));
            if self.token() != Tok::CloseBrace {
                self.error_at_current(&gen::_0_expected, &["}".to_string()]);
                break;
            }
            let t = self.scanner.scan_template(false);
            parts.push(TemplatePart::Str(self.token_wtf8()));
            if t == Tok::TemplateTail {
                let end = self.scanner.token_end();
                self.next();
                return Expr::Template {
                    parts,
                    span: Span::new(start, end),
                };
            }
            self.next(); // move past middle into expression
        }
        Expr::Template {
            parts,
            span: Span::new(start, self.prev_end()),
        }
    }

    fn parse_primary_expr(&mut self) -> Expr {
        use Tok::*;
        let span = self.token_span();
        match self.token() {
            NumLit => {
                let value: f64 = self.token_value().parse().unwrap_or(0.0);
                let text = self.token_value();
                self.next();
                Expr::NumLit { value, text, span }
            }
            BigIntLit => {
                let text = self.token_value();
                self.next();
                Expr::BigIntLit { text, span }
            }
            StrLit => {
                let value = self.token_wtf8();
                self.next();
                Expr::StrLit { value, span }
            }
            NoSubTemplate | TemplateHead => self.parse_template_expr(),
            KImport => {
                self.next(); // import
                if self.token() == Tok::Dot {
                    // `import.meta` (or other meta-properties) — consume `.name`
                    self.next();
                    let _meta = self.parse_ident_name();
                    Expr::ImportMeta {
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                } else if self.token() == Tok::OpenParen {
                    let args = self.parse_arguments();
                    Expr::ImportCall {
                        args,
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                } else if self.token() == Tok::Lt {
                    // `import<Args>(...)` — type arguments on a dynamic import
                    // are invalid but parsed (the checker reports the error).
                    let _ = self.try_parse_type_args();
                    let args = if self.token() == Tok::OpenParen {
                        self.parse_arguments()
                    } else {
                        Vec::new()
                    };
                    Expr::ImportCall {
                        args,
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                } else {
                    self.error_at_current(&gen::Expression_expected, &[]);
                    Expr::Missing { span }
                }
            }
            PrivateIdent => {
                // `#field` as a primary expression is only valid as the left
                // operand of `in` (an ergonomic brand check). Validity in
                // context is a checker concern; carry the name with its `#`.
                let name = self.token_value();
                self.next();
                Expr::Ident(crate::ast::Ident { name, span })
            }
            KTrue => {
                self.next();
                Expr::BoolLit { value: true, span }
            }
            KFalse => {
                self.next();
                Expr::BoolLit { value: false, span }
            }
            KNull => {
                self.next();
                Expr::NullLit { span }
            }
            KThis => {
                self.next();
                Expr::This { span }
            }
            KSuper => {
                self.next();
                if !matches!(
                    self.token(),
                    Tok::OpenParen | Tok::Dot | Tok::OpenBracket | Tok::Lt
                ) {
                    self.error_at_current(
                        &gen::super_must_be_followed_by_an_argument_list_or_member_access,
                        &[],
                    );
                }
                Expr::Super { span }
            }
            Slash | SlashEq => {
                self.scanner.rescan_slash_as_regex();
                let text = self.token_value();
                let span = self.token_span();
                self.next();
                Expr::RegexLit { text, span }
            }
            OpenParen => {
                let start = self.start();
                self.next();
                let inner = self.parse_expression();
                let end = self.scanner.token_end();
                self.expect(CloseParen);
                Expr::Paren {
                    inner: Box::new(inner),
                    span: Span::new(start, end),
                }
            }
            OpenBracket => {
                let start = self.start();
                self.next();
                let mut elements = Vec::new();
                while self.token() != CloseBracket && self.token() != Eof {
                    // elision: a comma with no preceding element is a hole
                    if self.token() == Comma {
                        let hspan = self.token_span();
                        elements.push(Expr::Missing {
                            span: Span::new(hspan.start as usize, hspan.start as usize),
                        });
                        self.next();
                        continue;
                    }
                    if self.token() == DotDotDot {
                        let sstart = self.start();
                        self.next();
                        let e = self.parse_assignment_expr();
                        let span = Span::new(sstart, e.span().end as usize);
                        elements.push(Expr::Spread {
                            expr: Box::new(e),
                            span,
                        });
                    } else {
                        elements.push(self.parse_assignment_expr());
                    }
                    if !self.eat(Comma) {
                        break;
                    }
                }
                let end = self.scanner.token_end();
                self.expect(CloseBracket);
                Expr::Array {
                    elements,
                    span: Span::new(start, end),
                }
            }
            OpenBrace => self.parse_object_literal(),
            KAsync
                if self
                    .lookahead(|p| {
                        p.next();
                        if p.token() == Tok::KFunction && !p.line_break_before() {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some() =>
            {
                // `async function () {}` / `async function* () {}` expression
                let aspan = self.token_span();
                self.next(); // async
                let f = self.parse_function(
                    vec![Modifier {
                        kind: ModifierKind::Async,
                        span: aspan,
                    }],
                    FuncKind::Expression,
                );
                Expr::FunctionExpr(Box::new(f))
            }
            KFunction => {
                let f = self.parse_function(Vec::new(), FuncKind::Expression);
                Expr::FunctionExpr(Box::new(f))
            }
            KClass => {
                let c = self.parse_class(Vec::new());
                Expr::ClassExpr(Box::new(c))
            }
            At => {
                // `@dec class { … }` decorated class expression
                let decorators = self.parse_decorators();
                if self.token() == Tok::KClass {
                    let mut c = self.parse_class(Vec::new());
                    if let Some(first) = decorators.first() {
                        c.span.start = first.span.start;
                    }
                    c.decorators = decorators;
                    Expr::ClassExpr(Box::new(c))
                } else {
                    self.error_at_current(&gen::Expression_expected, &[]);
                    Expr::Missing {
                        span: Span::new(span.start as usize, self.prev_end()),
                    }
                }
            }
            KNew => self.parse_new_expr(),
            _ if self.is_ident_like() || self.token().is_strict_reserved_word() => {
                let id = self.parse_ident();
                Expr::Ident(id)
            }
            _ => {
                self.error_at_current(&gen::Expression_expected, &[]);
                Expr::Missing {
                    span: Span::new(span.start as usize, span.start as usize),
                }
            }
        }
    }

    fn parse_object_literal(&mut self) -> Expr {
        let start = self.start();
        self.expect(Tok::OpenBrace);
        let mut props = Vec::new();
        while self.token() != Tok::CloseBrace && self.token() != Tok::Eof {
            let pstart = self.start();
            if self.token() == Tok::DotDotDot {
                self.next();
                let e = self.parse_assignment_expr();
                let span = Span::new(pstart, e.span().end as usize);
                props.push(ObjectProp::Spread { expr: e, span });
            } else {
                // Object literals do not allow modifiers, but tsc parses (and
                // defers) a leading accessibility/static/readonly modifier
                // before a method or accessor (`{ public get foo() {} }`).
                while matches!(
                    self.token(),
                    Tok::KPublic
                        | Tok::KPrivate
                        | Tok::KProtected
                        | Tok::KStatic
                        | Tok::KReadonly
                        | Tok::KExport
                ) && self
                    .lookahead(|p| {
                        p.next();
                        if matches!(
                            p.token(),
                            Tok::Colon
                                | Tok::Comma
                                | Tok::CloseBrace
                                | Tok::Eq
                                | Tok::OpenParen
                                | Tok::Lt
                                | Tok::Question
                        ) {
                            None
                        } else {
                            Some(())
                        }
                    })
                    .is_some()
                {
                    self.next();
                }
                // method?
                let is_method = self
                    .lookahead(|p| {
                        let mods_async = p.token() == Tok::KAsync;
                        if mods_async {
                            p.next();
                        }
                        p.eat(Tok::Star);
                        if !(p.token() == Tok::Ident
                            || p.token().is_keyword()
                            || p.token() == Tok::StrLit
                            || p.token() == Tok::NumLit
                            || p.token() == Tok::OpenBracket
                            || p.token() == Tok::PrivateIdent)
                        {
                            return None;
                        }
                        let _name = p.parse_prop_name();
                        // optional/definite method `a?() {}` / `a!() {}` (invalid in
                        // an object literal, but parsed; the checker reports it)
                        p.eat(Tok::Question);
                        p.eat(Tok::Bang);
                        if p.token() == Tok::OpenParen || p.token() == Tok::Lt {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some();
                if is_method {
                    let mut modifiers = Vec::new();
                    if self.token() == Tok::KAsync {
                        modifiers.push(Modifier {
                            kind: ModifierKind::Async,
                            span: self.token_span(),
                        });
                        self.next();
                    }
                    let is_generator = self.eat(Tok::Star);
                    let name = self.parse_prop_name();
                    let _question = self.eat(Tok::Question);
                    let _definite = self.eat(Tok::Bang);
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
                        None
                    };
                    props.push(ObjectProp::Method(Box::new(FunctionLike {
                        decorators: Vec::new(),
                        kind: FuncKind::Method,
                        modifiers,
                        name: Some(name),
                        question: false,
                        type_params,
                        params,
                        return_type,
                        body,
                        is_generator,
                        span: Span::new(pstart, self.prev_end()),
                    })));
                } else if matches!(self.token(), Tok::KGet | Tok::KSet)
                    && self
                        .lookahead(|p| {
                            p.next();
                            if p.is_ident_like()
                                || matches!(
                                    p.token(),
                                    Tok::StrLit
                                        | Tok::NumLit
                                        | Tok::OpenBracket
                                        | Tok::PrivateIdent
                                )
                            {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                {
                    let is_get = self.token() == Tok::KGet;
                    self.next();
                    let name = self.parse_prop_name();
                    let params = self.parse_params();
                    let return_type = if self.eat(Tok::Colon) {
                        Some(self.parse_type_or_predicate())
                    } else {
                        None
                    };
                    let body = if self.token() == Tok::OpenBrace {
                        Some(FuncBody::Block(self.parse_block()))
                    } else {
                        None
                    };
                    props.push(ObjectProp::Method(Box::new(FunctionLike {
                        decorators: Vec::new(),
                        kind: if is_get {
                            FuncKind::Getter
                        } else {
                            FuncKind::Setter
                        },
                        modifiers: Vec::new(),
                        name: Some(name),
                        question: false,
                        type_params: None,
                        params,
                        return_type,
                        body,
                        is_generator: false,
                        span: Span::new(pstart, self.prev_end()),
                    })));
                } else if matches!(self.token(), Tok::Colon | Tok::Comma) {
                    // `{ : 1 }` — not a property at all
                    self.error_at_current(&gen::Property_assignment_expected, &[]);
                    self.next();
                    continue;
                } else {
                    let name = self.parse_prop_name();
                    let question_span = if self.token() == Tok::Question {
                        let s = self.token_span();
                        self.next();
                        Some(s)
                    } else {
                        None
                    };
                    if self.eat(Tok::Colon) {
                        let value = self.parse_assignment_expr();
                        let span = Span::new(pstart, value.span().end as usize);
                        props.push(ObjectProp::Property {
                            name,
                            value,
                            question_span,
                            span,
                        });
                    } else if let PropName::Ident(id) = name {
                        let span = id.span;
                        // `{ a! }` — a non-null assertion on a shorthand property
                        // is invalid but parsed (the checker reports the error).
                        let _ = self.eat(Tok::Bang);
                        // `{ a = 1 }` outside destructuring (1312)
                        let eq_span = if self.token() == Tok::Eq {
                            let s = self.token_span();
                            self.next();
                            let _ = self.parse_assignment_expr();
                            Some(s)
                        } else {
                            None
                        };
                        props.push(ObjectProp::Shorthand {
                            name: id,
                            eq_span,
                            span,
                        });
                    } else {
                        self.error_at_current(&gen::_0_expected, &[":".to_string()]);
                    }
                }
            }
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        let end = self.scanner.token_end();
        self.expect(Tok::CloseBrace);
        Expr::Object {
            props,
            span: Span::new(start, end),
        }
    }
}
