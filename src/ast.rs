//! AST: a plain Box/Vec tree with byte spans. Declaration nodes are referenced
//! by the binder/checker via `&'a` borrows; identity for side tables comes from
//! pointer addresses (the tree is immutable for the program's lifetime).

use crate::jsstr::JsString;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span {
            start: start as u32,
            end: end as u32,
        }
    }
    pub fn len(&self) -> u32 {
        self.end - self.start
    }
}

/// Stable identity for any AST node (pointer-based; valid while the tree lives).
pub fn node_key<T>(node: &T) -> usize {
    node as *const T as usize
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

// ── modifiers ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModifierKind {
    Export,
    Declare,
    Abstract,
    Public,
    Private,
    Protected,
    Static,
    Readonly,
    Async,
    Override,
    Default,
    Accessor,
    In,
    Out,
}

#[derive(Clone, Debug)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Decorator {
    /// span of the `@`
    pub at_span: Span,
    pub expr: Expr,
    pub span: Span,
}

pub type Modifiers = Vec<Modifier>;

pub fn has_modifier(mods: &Modifiers, kind: ModifierKind) -> bool {
    mods.iter().any(|m| m.kind == kind)
}

// ── types ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeywordTypeKind {
    Any,
    Unknown,
    String,
    Number,
    Boolean,
    Object,
    Symbol,
    Bigint,
    Void,
    Undefined,
    Never,
    Null,
    Intrinsic,
}

#[derive(Clone, Debug)]
pub struct EntityName {
    /// Dotted name a.b.c — at least one segment.
    pub parts: Vec<Ident>,
    pub span: Span,
}

impl EntityName {
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Clone, Debug)]
pub enum TypeNode {
    Keyword(KeywordTypeKind, Span),
    /// `this` type
    This(Span),
    Ref(TypeRef),
    Array {
        elem: Box<TypeNode>,
        span: Span,
    },
    Tuple {
        elems: Vec<TupleElem>,
        span: Span,
    },
    Union {
        members: Vec<TypeNode>,
        span: Span,
    },
    Intersection {
        members: Vec<TypeNode>,
        span: Span,
    },
    Function(Box<FunctionTypeNode>),
    Ctor(Box<FunctionTypeNode>),
    TypeLiteral {
        members: Vec<TypeMember>,
        span: Span,
    },
    LiteralString {
        value: JsString,
        span: Span,
    },
    LiteralNumber {
        value: f64,
        text: String,
        span: Span,
    },
    LiteralBigInt {
        text: String,
        span: Span,
    },
    LiteralBool {
        value: bool,
        span: Span,
    },
    Paren {
        inner: Box<TypeNode>,
        span: Span,
    },
    /// typeof entityName
    TypeQuery {
        name: EntityName,
        type_args: Option<Vec<TypeNode>>,
        span: Span,
    },
    /// keyof T
    Keyof {
        ty: Box<TypeNode>,
        span: Span,
    },
    /// readonly T (type operator, e.g. readonly string[])
    ReadonlyOp {
        ty: Box<TypeNode>,
        span: Span,
    },
    IndexedAccess {
        obj: Box<TypeNode>,
        index: Box<TypeNode>,
        span: Span,
    },
    Conditional(Box<ConditionalTypeNode>),
    /// `x is T` (checked for 2677, behaves as boolean otherwise)
    Predicate {
        param_name: Ident,
        asserts: bool,
        ty: Option<Box<TypeNode>>,
        span: Span,
    },
    /// `infer R` (within a conditional's extends type)
    Infer {
        name: Ident,
        constraint: Option<Box<TypeNode>>,
        span: Span,
    },
    Mapped(Box<MappedTypeNode>),
    /// `` `head${T}mid${U}tail` ``
    TemplateLit {
        head: String,
        parts: Vec<(TypeNode, String)>,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct ConditionalTypeNode {
    pub check: TypeNode,
    pub extends_ty: TypeNode,
    pub true_ty: TypeNode,
    pub false_ty: TypeNode,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MappedModifier {
    Add,
    Remove,
}

#[derive(Clone, Debug)]
pub struct MappedTypeNode {
    pub readonly_mod: Option<MappedModifier>,
    pub key: Ident,
    pub constraint: TypeNode,
    /// `as` key-remapping clause
    pub name_type: Option<TypeNode>,
    pub optional_mod: Option<MappedModifier>,
    pub value: Option<TypeNode>,
    pub span: Span,
}

impl TypeNode {
    pub fn span(&self) -> Span {
        match self {
            TypeNode::Keyword(_, s) | TypeNode::This(s) => *s,
            TypeNode::Ref(r) => r.span,
            TypeNode::Array { span, .. }
            | TypeNode::Tuple { span, .. }
            | TypeNode::Union { span, .. }
            | TypeNode::Intersection { span, .. }
            | TypeNode::TypeLiteral { span, .. }
            | TypeNode::LiteralString { span, .. }
            | TypeNode::LiteralNumber { span, .. }
            | TypeNode::LiteralBigInt { span, .. }
            | TypeNode::LiteralBool { span, .. }
            | TypeNode::Paren { span, .. }
            | TypeNode::TypeQuery { span, .. }
            | TypeNode::Keyof { span, .. }
            | TypeNode::ReadonlyOp { span, .. }
            | TypeNode::IndexedAccess { span, .. }
            | TypeNode::Infer { span, .. }
            | TypeNode::TemplateLit { span, .. } => *span,
            TypeNode::Conditional(c) => c.span,
            TypeNode::Predicate { span, .. } => *span,
            TypeNode::Mapped(m) => m.span,
            TypeNode::Function(f) | TypeNode::Ctor(f) => f.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TypeRef {
    pub name: EntityName,
    pub type_args: Option<Vec<TypeNode>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TupleElem {
    pub dotdotdot: bool,
    pub question: bool,
    pub ty: TypeNode,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FunctionTypeNode {
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub params: Vec<Param>,
    pub return_type: TypeNode,
    pub is_abstract: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeParamDecl {
    /// illegal modifier before the name (1273)
    pub illegal_modifier: Option<(ModifierKind, Span)>,
    /// `const` modifier span (1277 outside fn/method/class)
    pub const_span: Option<Span>,
    /// `in`/`out` variance modifier (1274 outside class/interface/alias)
    pub variance_span: Option<(String, Span)>,
    pub name: Ident,
    pub constraint: Option<TypeNode>,
    pub default: Option<TypeNode>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PropName {
    Ident(Ident),
    String {
        value: String,
        span: Span,
    },
    Number {
        value: f64,
        text: String,
        span: Span,
    },
    Computed {
        expr: Box<Expr>,
        span: Span,
    },
}

impl PropName {
    pub fn span(&self) -> Span {
        match self {
            PropName::Ident(i) => i.span,
            PropName::String { span, .. }
            | PropName::Number { span, .. }
            | PropName::Computed { span, .. } => *span,
        }
    }
    /// Property key text (numbers via JS number-to-string).
    pub fn text(&self) -> Option<String> {
        match self {
            PropName::Ident(i) => Some(i.name.clone()),
            PropName::String { value, .. } => Some(value.clone()),
            PropName::Number { value, .. } => Some(crate::js_num::to_js_string(*value)),
            PropName::Computed { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum TypeMember {
    Prop(PropSig),
    Method(MethodSig),
    Call(CallSig),
    Ctor(CallSig),
    Index(IndexSig),
}

#[derive(Clone, Debug)]
pub struct PropSig {
    /// visibility/static/etc. captured for TS1070 (illegal on type members)
    pub illegal_modifiers: Modifiers,
    pub readonly: bool,
    pub name: PropName,
    pub question: bool,
    pub ty: Option<TypeNode>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MethodSig {
    pub name: PropName,
    pub question: bool,
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CallSig {
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct IndexSig {
    /// `declare [k: string]: T` (1071)
    pub declare_span: Option<Span>,
    pub readonly: bool,
    /// `[...k: T]` (1017)
    pub rest_span: Option<Span>,
    /// `[public k: T]` (1018 + 2369)
    pub modifier_span: Option<Span>,
    /// `[k?: T]` (1019)
    pub question_span: Option<Span>,
    /// `[k: T];` without a value type (1021)
    pub missing_value: bool,
    pub param_name: Ident,
    pub key_type: TypeNode,
    pub value_type: TypeNode,
    pub span: Span,
}

// ── expressions ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnaryOp {
    Plus,
    Minus,
    Bang,
    Tilde,
    Typeof,
    Void,
    Delete,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    // arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
    // relational / equality
    Lt,
    Gt,
    LtEq,
    GtEq,
    EqEq,
    NotEq,
    EqEqEq,
    NotEqEq,
    In,
    Instanceof,
    // logical
    AmpAmp,
    BarBar,
    QuestionQuestion,
    // assignment
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    ExpAssign,
    ShlAssign,
    ShrAssign,
    UShrAssign,
    AmpAssign,
    BarAssign,
    CaretAssign,
    AmpAmpAssign,
    BarBarAssign,
    QuestionQuestionAssign,
    Comma,
}

impl BinOp {
    pub fn is_assignment(self) -> bool {
        use BinOp::*;
        matches!(
            self,
            Assign
                | AddAssign
                | SubAssign
                | MulAssign
                | DivAssign
                | ModAssign
                | ExpAssign
                | ShlAssign
                | ShrAssign
                | UShrAssign
                | AmpAssign
                | BarAssign
                | CaretAssign
                | AmpAmpAssign
                | BarBarAssign
                | QuestionQuestionAssign
        )
    }
    pub fn text(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => "%",
            Exp => "**",
            Shl => "<<",
            Shr => ">>",
            UShr => ">>>",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Lt => "<",
            Gt => ">",
            LtEq => "<=",
            GtEq => ">=",
            EqEq => "==",
            NotEq => "!=",
            EqEqEq => "===",
            NotEqEq => "!==",
            In => "in",
            Instanceof => "instanceof",
            AmpAmp => "&&",
            BarBar => "||",
            QuestionQuestion => "??",
            Assign => "=",
            AddAssign => "+=",
            SubAssign => "-=",
            MulAssign => "*=",
            DivAssign => "/=",
            ModAssign => "%=",
            ExpAssign => "**=",
            ShlAssign => "<<=",
            ShrAssign => ">>=",
            UShrAssign => ">>>=",
            AmpAssign => "&=",
            BarAssign => "|=",
            CaretAssign => "^=",
            AmpAmpAssign => "&&=",
            BarBarAssign => "||=",
            QuestionQuestionAssign => "??=",
            Comma => ",",
        }
    }
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    /// Cooked string chunk.
    Str(JsString),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub enum ObjectProp {
    /// `name: value`
    Property {
        name: PropName,
        value: Expr,
        question_span: Option<Span>,
        span: Span,
    },
    /// `{ name }`
    Shorthand {
        name: Ident,
        eq_span: Option<Span>,
        span: Span,
    },
    /// `m() { ... }`
    Method(Box<FunctionLike>),
    /// `...expr`
    Spread { expr: Expr, span: Span },
}

impl ObjectProp {
    pub fn span(&self) -> Span {
        match self {
            ObjectProp::Property { span, .. }
            | ObjectProp::Shorthand { span, .. }
            | ObjectProp::Spread { span, .. } => *span,
            ObjectProp::Method(m) => m.span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Ident(Ident),
    NumLit {
        value: f64,
        text: String,
        span: Span,
    },
    StrLit {
        value: JsString,
        span: Span,
    },
    BigIntLit {
        text: String,
        span: Span,
    },
    BoolLit {
        value: bool,
        span: Span,
    },
    NullLit {
        span: Span,
    },
    RegexLit {
        text: String,
        span: Span,
    },
    /// Synthetic first argument of a tagged template call.
    TemplateStringsArray {
        span: Span,
    },
    Template {
        parts: Vec<TemplatePart>,
        span: Span,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    Object {
        props: Vec<ObjectProp>,
        span: Span,
    },
    Arrow(Box<FunctionLike>),
    FunctionExpr(Box<FunctionLike>),
    ClassExpr(Box<ClassDecl>),
    Call {
        callee: Box<Expr>,
        question_dot: bool,
        type_args: Option<Vec<TypeNode>>,
        args: Vec<Expr>,
        span: Span,
    },
    New {
        callee: Box<Expr>,
        type_args: Option<Vec<TypeNode>>,
        args: Option<Vec<Expr>>,
        span: Span,
    },
    PropAccess {
        obj: Box<Expr>,
        question_dot: bool,
        name: Ident,
        span: Span,
    },
    ElemAccess {
        obj: Box<Expr>,
        question_dot: bool,
        index: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Update {
        op_plus: bool,
        prefix: bool,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        op_span: Span,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Cond {
        cond: Box<Expr>,
        when_true: Box<Expr>,
        when_false: Box<Expr>,
        span: Span,
    },
    Paren {
        inner: Box<Expr>,
        span: Span,
    },
    /// expr as T / expr satisfies T / <T>expr / expr as const
    Assertion {
        expr: Box<Expr>,
        ty: TypeNode,
        kind: AssertionKind,
        kw_span: Span,
        span: Span,
    },
    NonNull {
        expr: Box<Expr>,
        span: Span,
    },
    This {
        span: Span,
    },
    Super {
        span: Span,
    },
    Spread {
        expr: Box<Expr>,
        span: Span,
    },
    Await {
        expr: Box<Expr>,
        span: Span,
    },
    Yield {
        expr: Option<Box<Expr>>,
        delegate: bool,
        span: Span,
    },
    /// `import(specifier, ...)` — dynamic import expression.
    ImportCall {
        args: Vec<Expr>,
        span: Span,
    },
    /// `import.meta` (and similar `import.<name>` meta-properties).
    ImportMeta {
        span: Span,
    },
    JsxElement(Box<JsxElement>),
    /// produced by parse errors
    Missing {
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct JsxElement {
    /// None = fragment (`<>...</>`)
    pub tag: Option<Ident>,
    pub attrs: Vec<JsxAttr>,
    pub children: Vec<JsxChild>,
    /// span of the closing tag (`</name>` / `</>`), if any
    pub closing_span: Option<Span>,
    pub self_closing: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct JsxAttr {
    pub name: Ident,
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum JsxChild {
    Element(JsxElement),
    Expr(Expr),
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertionKind {
    As,
    Satisfies,
    Angle,
    /// `expr as const`
    ConstAssert,
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Ident(i) => i.span,
            Expr::NumLit { span, .. }
            | Expr::StrLit { span, .. }
            | Expr::BigIntLit { span, .. }
            | Expr::BoolLit { span, .. }
            | Expr::NullLit { span }
            | Expr::RegexLit { span, .. }
            | Expr::TemplateStringsArray { span }
            | Expr::Template { span, .. }
            | Expr::Array { span, .. }
            | Expr::Object { span, .. }
            | Expr::Call { span, .. }
            | Expr::New { span, .. }
            | Expr::PropAccess { span, .. }
            | Expr::ElemAccess { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Update { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Cond { span, .. }
            | Expr::Paren { span, .. }
            | Expr::Assertion { span, .. }
            | Expr::NonNull { span, .. }
            | Expr::This { span }
            | Expr::Super { span }
            | Expr::Spread { span, .. }
            | Expr::Await { span, .. }
            | Expr::Yield { span, .. }
            | Expr::ImportCall { span, .. }
            | Expr::ImportMeta { span }
            | Expr::Missing { span } => *span,
            Expr::Arrow(f) | Expr::FunctionExpr(f) => f.span,
            Expr::ClassExpr(c) => c.span,
            Expr::JsxElement(j) => j.span,
        }
    }
}

// ── functions / parameters ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuncKind {
    Declaration,
    Expression,
    Arrow,
    Method,
    Constructor,
    Getter,
    Setter,
}

#[derive(Clone, Debug)]
pub enum Binding {
    Ident(Ident),
    Object(ObjectPattern),
    Array(ArrayPattern),
}

#[derive(Clone, Debug)]
pub struct ObjectPattern {
    pub props: Vec<ObjectPatternProp>,
    pub rest: Option<Box<Binding>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ObjectPatternProp {
    pub key: PropName,
    pub binding: Box<Binding>,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ArrayPatternElem {
    pub binding: Box<Binding>,
    pub default: Option<Expr>,
    pub rest: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ArrayPattern {
    /// None = elision hole
    pub elements: Vec<Option<ArrayPatternElem>>,
    pub span: Span,
}

impl Binding {
    pub fn span(&self) -> Span {
        match self {
            Binding::Ident(i) => i.span,
            Binding::Object(p) => p.span,
            Binding::Array(p) => p.span,
        }
    }
    pub fn as_ident(&self) -> Option<&Ident> {
        match self {
            Binding::Ident(i) => Some(i),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub decorators: Vec<Decorator>,
    pub modifiers: Modifiers,
    pub dotdotdot: bool,
    pub dotdotdot_span: Option<Span>,
    pub name: Binding,
    pub question: bool,
    pub question_span: Option<Span>,
    pub ty: Option<TypeNode>,
    pub initializer: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum FuncBody {
    Block(Block),
    /// arrow concise body
    Expr(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct FunctionLike {
    pub decorators: Vec<Decorator>,
    pub kind: FuncKind,
    pub modifiers: Modifiers,
    pub name: Option<PropName>,
    pub question: bool,
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Option<FuncBody>,
    pub is_generator: bool,
    pub span: Span,
}

impl FunctionLike {
    pub fn name_ident(&self) -> Option<&Ident> {
        match &self.name {
            Some(PropName::Ident(i)) => Some(i),
            _ => None,
        }
    }
}

// ── statements / declarations ───────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarKind {
    Var,
    Let,
    Const,
}

impl VarKind {
    pub fn text(self) -> &'static str {
        match self {
            VarKind::Var => "var",
            VarKind::Let => "let",
            VarKind::Const => "const",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VarDeclarator {
    pub name: Binding,
    pub exclam: bool,
    pub exclam_span: Option<Span>,
    pub ty: Option<TypeNode>,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct VarStmt {
    pub modifiers: Modifiers,
    pub kind: VarKind,
    /// parsed from a `using` / `await using` declaration (kind is Const);
    /// tsc exempts underscore-named using declarations from the unused check
    pub is_using: bool,
    pub decls: Vec<VarDeclarator>,
    /// span of the `var`/`let`/`const` keyword (for 1155 etc.)
    pub kw_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct InterfaceDecl {
    pub modifiers: Modifiers,
    pub name: Ident,
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub extends: Vec<TypeRef>,
    pub members: Vec<TypeMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeAliasDecl {
    pub modifiers: Modifiers,
    pub name: Ident,
    pub type_params: Option<Vec<TypeParamDecl>>,
    pub ty: TypeNode,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub decorators: Vec<Decorator>,
    pub modifiers: Modifiers,
    pub name: Option<Ident>,
    pub type_params: Option<Vec<TypeParamDecl>>,
    /// extends clause: expression + optional type args
    pub extends: Option<HeritageClause>,
    pub implements: Vec<TypeRef>,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HeritageClause {
    pub expr: Expr,
    pub type_args: Option<Vec<TypeNode>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ClassMember {
    StaticBlock(Block),
    Property(PropertyDecl),
    Method(Box<FunctionLike>),
    Constructor(Box<FunctionLike>),
    Index(IndexSig),
}

#[derive(Clone, Debug)]
pub struct PropertyDecl {
    pub decorators: Vec<Decorator>,
    /// span of an illegal `const` keyword before the name (1248)
    pub const_span: Option<Span>,
    /// `accessor` keyword span (18045 at ES5)
    pub accessor_span: Option<Span>,
    pub modifiers: Modifiers,
    pub name: PropName,
    pub question: bool,
    pub question_span: Option<Span>,
    pub exclam: bool,
    pub exclam_span: Option<Span>,
    pub ty: Option<TypeNode>,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct NamespaceDecl {
    pub modifiers: Modifiers,
    pub name: Ident,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub modifiers: Modifiers,
    pub is_const: bool,
    pub name: Ident,
    pub members: Vec<EnumMemberDecl>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumMemberDecl {
    pub name: PropName,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImportSpec {
    /// `import { prop_name as name }` — prop_name is None for plain named import.
    pub prop_name: Option<Ident>,
    pub name: Ident,
    pub type_only: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub modifiers: Modifiers,
    pub type_only: bool,
    pub default_name: Option<Ident>,
    pub namespace_name: Option<Ident>,
    pub named: Option<Vec<ImportSpec>>,
    pub module: StrLitNode,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StrLitNode {
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExportNamedDecl {
    /// `export * from "m"`
    pub star: bool,
    pub specifiers: Vec<ImportSpec>,
    pub module: Option<StrLitNode>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CatchClause {
    pub param: Option<Param>,
    pub block: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SwitchCase {
    /// None = default clause
    pub test: Option<Expr>,
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ForInit {
    Var(VarStmt),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Var(VarStmt),
    Func(Box<FunctionLike>),
    Class(Box<ClassDecl>),
    Interface(Box<InterfaceDecl>),
    TypeAlias(Box<TypeAliasDecl>),
    Enum(Box<EnumDecl>),
    Namespace(Box<NamespaceDecl>),
    With {
        obj: Expr,
        body: Box<Stmt>,
        kw_span: Span,
        span: Span,
    },
    Return {
        expr: Option<Expr>,
        span: Span,
    },
    If {
        cond: Expr,
        then: Box<Stmt>,
        els: Option<Box<Stmt>>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
        span: Span,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
        span: Span,
    },
    For {
        init: Option<Box<ForInit>>,
        cond: Option<Expr>,
        incr: Option<Expr>,
        body: Box<Stmt>,
        span: Span,
    },
    ForIn {
        left: Box<ForInit>,
        expr: Expr,
        body: Box<Stmt>,
        init_span: Option<Span>,
        extra_decl_span: Option<Span>,
        span: Span,
    },
    ForOf {
        left: Box<ForInit>,
        expr: Expr,
        body: Box<Stmt>,
        await_span: Option<Span>,
        init_span: Option<Span>,
        extra_decl_span: Option<Span>,
        span: Span,
    },
    Block(Block),
    Expr {
        expr: Expr,
        span: Span,
    },
    Empty {
        span: Span,
    },
    Break {
        label: Option<Ident>,
        span: Span,
    },
    Continue {
        label: Option<Ident>,
        span: Span,
    },
    Throw {
        expr: Expr,
        span: Span,
    },
    Try {
        block: Block,
        catch: Option<CatchClause>,
        finally: Option<Block>,
        span: Span,
    },
    Switch {
        expr: Expr,
        cases: Vec<SwitchCase>,
        span: Span,
    },
    Labeled {
        label: Ident,
        stmt: Box<Stmt>,
        span: Span,
    },
    Import(Box<ImportDecl>),
    ExportNamed(Box<ExportNamedDecl>),
    ExportDefault {
        expr: Expr,
        span: Span,
    },
    /// `export = expr;`
    ExportAssign {
        expr: Expr,
        span: Span,
    },
    /// `import name = require("m");` / `import name = A.b;`
    ImportEquals {
        name: Ident,
        module: StrLitNode,
        /// `export import a = …` (ImportEquals carries no modifier list)
        exported: bool,
        /// external `require("m")` form (vs an entity-name reference)
        is_require: bool,
        span: Span,
    },
    Missing {
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Var(v) => v.span,
            Stmt::Func(f) => f.span,
            Stmt::Class(c) => c.span,
            Stmt::Interface(i) => i.span,
            Stmt::TypeAlias(t) => t.span,
            Stmt::Enum(e) => e.span,
            Stmt::Namespace(n) => n.span,
            Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::DoWhile { span, .. }
            | Stmt::For { span, .. }
            | Stmt::ForIn { span, .. }
            | Stmt::ForOf { span, .. }
            | Stmt::Expr { span, .. }
            | Stmt::Empty { span }
            | Stmt::Break { span, .. }
            | Stmt::Continue { span, .. }
            | Stmt::Throw { span, .. }
            | Stmt::Try { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Labeled { span, .. }
            | Stmt::Missing { span } => *span,
            Stmt::Block(b) => b.span,
            Stmt::Import(i) => i.span,
            Stmt::ExportNamed(e) => e.span,
            Stmt::ExportDefault { span, .. } => *span,
            Stmt::With { span, .. } => *span,
            Stmt::ExportAssign { span, .. } => *span,
            Stmt::ImportEquals { span, .. } => *span,
        }
    }
}

#[derive(Debug)]
pub struct SourceFileAst {
    /// (start, end, is_expect_error) — @ts-expect-error / @ts-ignore
    pub comment_directives: Vec<(u32, u32, bool)>,
    pub stmts: Vec<Stmt>,
    pub is_module: bool,
    pub span: Span,
}
