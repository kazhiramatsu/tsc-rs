//! Binder: walks every file eagerly, building scopes, symbols (with tsc-style
//! declaration merging) and export tables. Duplicate-declaration diagnostics
//! (2300/2451/2393) are emitted here; everything type-ish is lazy in the checker.

use crate::ast::*;
use crate::diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScopeId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FlowNodeId(pub u32);

/// A node in the control-flow graph built at bind time (Tier-2 flow engine,
/// Stage 0 — see the plan). Antecedents point *backward*, toward the function /
/// program start; `get_flow_type_of_reference` walks them to resolve a
/// reference's flow-narrowed type. Populated by `crate::flow_graph::build` after
/// `bind()` and before the `BindResult` is frozen in an `Arc`. During Stage 0
/// the graph is built but not yet consumed by any diagnostic, so program output
/// is unchanged.
pub enum FlowNode<'a> {
    /// Function/program entry: resolve to the declared type (no flow
    /// refinement). For function-expression / arrow / object-literal-or-
    /// class-expression-method containers, `outer` carries the enclosing
    /// container's flow at the function's position plus the function's span
    /// (tsc `FlowStart.node` + `container.flowNode`): the resolver resumes a
    /// bare `const` reference's walk at `outer` when the reference's symbol
    /// is declared outside the span (the checker's flow-container extension
    /// for constants in `checkIdentifier`).
    Start {
        outer: Option<(FlowNodeId, crate::ast::Span)>,
    },
    /// The flow after a `return`/`throw`/`break`/`continue`: no control path
    /// reaches here, so joins skip it entirely (it must NOT contribute the
    /// declared type to a `Branch` union — e.g. the fall-through edge out of
    /// a terminated switch clause).
    Unreachable,
    /// A control-flow join — resolve as the union over every antecedent. Also
    /// used as a loop label (back-edge antecedents are appended during build).
    Branch(Vec<FlowNodeId>),
    /// `cond` was evaluated with truthiness `sense` on this edge; the resolver
    /// applies `narrow_by_condition(cond, sense)` when the reference matches.
    /// `scope` is the scope in effect at the condition, so names in `cond`
    /// resolve correctly even when the resolver runs from a different scope.
    Cond {
        cond: &'a crate::ast::Expr,
        sense: bool,
        scope: ScopeId,
        ante: FlowNodeId,
    },
    /// A reference was assigned here: `x = rhs` (incl. compound / logical
    /// assignment operators) or `x++`/`x--`. `target` is the left-hand side /
    /// operand — the resolver matches its RefKey against the queried
    /// reference; on a match it recomputes the post-assignment type from
    /// `expr` (the whole assigning expression) and clears prior narrowings of
    /// the target.
    Assign {
        target: &'a crate::ast::Expr,
        expr: &'a crate::ast::Expr,
        scope: ScopeId,
        ante: FlowNodeId,
    },
    /// A declarator bound its name(s) here: `let x = init`, or the
    /// per-iteration binding of a `for (const x in/of e)` head (where `init`
    /// is `None`). The resolver matches the queried reference's symbol
    /// against the binding; on a match the flow type is the initial type.
    Init {
        decl: &'a crate::ast::VarDeclarator,
        scope: ScopeId,
        ante: FlowNodeId,
    },
    /// Switch discriminant narrowing: control entered clause `clause` of the
    /// switch on `disc`. A `default` clause (or `clause == cases.len()`, the
    /// implicit no-clause-matched path past the switch) narrows by the
    /// negation of every case label.
    Switch {
        disc: &'a crate::ast::Expr,
        cases: &'a [crate::ast::SwitchCase],
        clause: u32,
        scope: ScopeId,
        ante: FlowNodeId,
    },
    /// A call whose signature may assert (`asserts x is T` / `asserts x`).
    /// Whether it actually asserts is only known at check time, so every call
    /// gets a node; the resolver treats non-asserting calls as pass-through.
    Call {
        call: &'a crate::ast::Expr,
        scope: ScopeId,
        ante: FlowNodeId,
    },
    /// `expr` was tested for nullishness on this edge (`a ?? b`): sense=true
    /// is the non-nullish skip edge, sense=false the nullish edge into the
    /// RHS (tsc narrowTypeByOptionality: NEUndefinedOrNull / EQUndefinedOrNull
    /// facts). Truthiness `Cond` cannot express this — `""` survives the
    /// non-nullish edge.
    Nullish {
        expr: &'a crate::ast::Expr,
        sense: bool,
        scope: ScopeId,
        ante: FlowNodeId,
    },
}

pub mod flags {
    pub const FUNCTION_SCOPED_VARIABLE: u32 = 1 << 0; // var, param
    pub const BLOCK_SCOPED_VARIABLE: u32 = 1 << 1; // let, const, catch var
    pub const PROPERTY: u32 = 1 << 2;
    pub const FUNCTION: u32 = 1 << 3;
    pub const CLASS: u32 = 1 << 4;
    pub const INTERFACE: u32 = 1 << 5;
    pub const TYPE_ALIAS: u32 = 1 << 6;
    pub const TYPE_PARAM: u32 = 1 << 7;
    pub const METHOD: u32 = 1 << 8;
    pub const ALIAS: u32 = 1 << 9; // import binding
    pub const OPTIONAL: u32 = 1 << 10;
    pub const READONLY: u32 = 1 << 11;
    pub const CONST_VARIABLE: u32 = 1 << 12;
    pub const GET_ACCESSOR: u32 = 1 << 13;
    pub const SET_ACCESSOR: u32 = 1 << 14;
    pub const ABSTRACT: u32 = 1 << 15;
    pub const STATIC: u32 = 1 << 16;
    pub const PARAMETER: u32 = 1 << 17;
    pub const ENUM: u32 = 1 << 18;
    pub const ENUM_MEMBER: u32 = 1 << 19;
    pub const NAMESPACE: u32 = 1 << 20;
    /// Carried by a value declaration introduced with `declare` (ambient).
    /// Used by definite-assignment checks (TS2454) to skip a declaration
    /// that is treated as already-initialized by tsc.
    pub const AMBIENT: u32 = 1 << 21;

    pub const VALUE: u32 = FUNCTION_SCOPED_VARIABLE
        | BLOCK_SCOPED_VARIABLE
        | PROPERTY
        | FUNCTION
        | CLASS
        | METHOD
        | GET_ACCESSOR
        | SET_ACCESSOR
        | ENUM
        | ENUM_MEMBER
        | NAMESPACE;
    pub const TYPE: u32 = CLASS | INTERFACE | TYPE_ALIAS | TYPE_PARAM | ENUM;
}

/// A declaration site. Copy so symbol access never holds borrows.
#[derive(Clone, Copy, Debug)]
pub enum Decl<'a> {
    Var(&'a VarDeclarator, VarKind),
    Param(&'a Param),
    Func(&'a FunctionLike),
    Class(&'a ClassDecl),
    Interface(&'a InterfaceDecl),
    Alias(&'a TypeAliasDecl),
    PropSig(&'a PropSig),
    MethodSig(&'a MethodSig),
    PropertyDecl(&'a PropertyDecl),
    Method(&'a FunctionLike),
    TypeParam(&'a TypeParamDecl),
    Enum(&'a EnumDecl),
    EnumMember(&'a EnumMemberDecl),
    Namespace(&'a NamespaceDecl),
    DefaultExport,
    /// `import name = require("m")`
    ImportEquals(&'a Ident, &'a StrLitNode),
    /// a name bound inside a destructuring pattern
    PatternVar(&'a Ident, VarKind),
    PatternParam(&'a Ident),
    Import(&'a ImportSpec, &'a ImportDecl),
    ImportDefault(&'a ImportDecl),
    ImportNamespace(&'a ImportDecl),
    CatchVar(&'a Param),
}

impl<'a> Decl<'a> {
    pub fn name_span(&self) -> Span {
        match self {
            Decl::Var(d, _) => d.name.span(),
            Decl::Param(p) | Decl::CatchVar(p) => p.name.span(),
            Decl::Func(f) | Decl::Method(f) => f.name.as_ref().map(|n| n.span()).unwrap_or(f.span),
            Decl::Class(c) => c.name.as_ref().map(|n| n.span).unwrap_or(c.span),
            Decl::Interface(i) => i.name.span,
            Decl::Alias(a) => a.name.span,
            Decl::PropSig(p) => p.name.span(),
            Decl::MethodSig(m) => m.name.span(),
            Decl::PropertyDecl(p) => p.name.span(),
            Decl::TypeParam(t) => t.name.span,
            Decl::Enum(e) => e.name.span,
            Decl::EnumMember(m) => m.name.span(),
            Decl::Namespace(n) => n.name.span,
            Decl::DefaultExport => Span::new(0, 0),
            Decl::ImportEquals(n, _) => n.span,
            Decl::PatternVar(i, _) | Decl::PatternParam(i) => i.span,
            Decl::Import(s, _) => s.name.span,
            Decl::ImportDefault(i) => i.default_name.as_ref().unwrap().span,
            Decl::ImportNamespace(i) => i.namespace_name.as_ref().unwrap().span,
        }
    }
    pub fn pos(&self) -> u32 {
        self.name_span().start
    }
}

#[derive(Debug)]
pub struct Symbol<'a> {
    pub name: String,
    pub flags: u32,
    pub decls: Vec<Decl<'a>>,
    /// instance members for interface/class symbols (insertion-ordered)
    pub members: Table,
    /// static members for class symbols
    pub statics: Table,
    /// file the symbol was declared in
    pub file: usize,
    /// dup diagnostics already emitted for this symbol
    pub dup_reported: bool,
    /// owning class/interface for member symbols
    pub parent: Option<SymbolId>,
}

/// Insertion-ordered (name -> SymbolId) table.
#[derive(Default, Debug, Clone)]
pub struct Table(pub Vec<(String, SymbolId)>);

impl Table {
    pub fn get(&self, name: &str) -> Option<SymbolId> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, s)| *s)
    }
    pub fn insert(&mut self, name: String, id: SymbolId) {
        self.0.push((name, id));
    }
    pub fn iter(&self) -> impl Iterator<Item = &(String, SymbolId)> {
        self.0.iter()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    Global,
    Module,
    Function,
    Block,
    TypeParams,
}

#[derive(Debug)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub values: Table,
    pub types: Table,
}

pub struct BindResult<'a> {
    pub symbols: Vec<Symbol<'a>>,
    pub scopes: Vec<Scope>,
    pub global_scope: ScopeId,
    pub module_scope: HashMap<usize, ScopeId>, // file -> module scope
    /// container node (fn/block/class/etc. ptr) -> its scope
    pub node_scope: HashMap<usize, ScopeId>,
    /// Scopes owned by class static blocks. Direct declarations in these scopes
    /// are not surfaced by tsc's unused-locals pass; nested block/function
    /// scopes are checked normally.
    pub static_block_scopes: HashSet<ScopeId>,
    /// declaration node ptr -> symbol
    pub decl_symbol: HashMap<usize, SymbolId>,
    /// file -> exports table
    pub exports: HashMap<usize, Table>,
    /// decl node ptr -> enclosing function-like ptr (0 = top level of file)
    pub decl_container: HashMap<usize, usize>,
    /// decl node ptr -> scope it was declared in
    pub decl_scope: HashMap<usize, ScopeId>,
    /// function-like node ptr -> the node (for lazy return inference)
    pub fn_decls: HashMap<usize, &'a FunctionLike>,
    /// decl node ptr -> file index
    pub decl_file: HashMap<usize, usize>,
    /// file -> the `export =` symbol
    pub export_equals: HashMap<usize, SymbolId>,
    /// (file, pattern span, member symbols) per destructuring declarator
    pub pattern_groups: Vec<(usize, Span, Vec<SymbolId>)>,
    /// (file, stmt-head span, symbols) per multi-declarator var statement
    pub var_stmt_groups: Vec<(usize, Span, Vec<SymbolId>)>,
    /// Symbols declared inside an ambient namespace/module body. Unlike an
    /// explicit `declare` inside a non-ambient namespace, these are not
    /// surfaced as unused locals by tsc.
    pub ambient_context_symbols: HashSet<SymbolId>,
    /// Tier-2 control-flow graph (Stage 0): the flow-node arena, populated by
    /// `crate::flow_graph::build` after `bind()` (before the `Arc` freeze).
    /// Empty until then.
    pub flow_nodes: Vec<FlowNode<'a>>,
    /// reference / statement `node_key` -> the flow node in effect at that
    /// point (its antecedent). Built alongside `flow_nodes`.
    pub flow_node: HashMap<usize, FlowNodeId>,
    pub diags: Vec<Diagnostic>,
}

pub fn bind<'a>(files: &'a [(String, crate::text::SourceText, SourceFileAst)]) -> BindResult<'a> {
    let mut b = Binder {
        symbols: Vec::new(),
        scopes: vec![Scope {
            parent: None,
            kind: ScopeKind::Global,
            values: Table::default(),
            types: Table::default(),
        }],
        node_scope: HashMap::new(),
        static_block_scopes: HashSet::new(),
        decl_symbol: HashMap::new(),
        exports: HashMap::new(),
        default_exports: Vec::new(),
        export_assigns: Vec::new(),
        pattern_groups: Vec::new(),
        var_stmt_groups: Vec::new(),
        ambient_context_symbols: HashSet::new(),
        decl_container: HashMap::new(),
        decl_scope: HashMap::new(),
        decl_file: HashMap::new(),
        fn_decls: HashMap::new(),
        diags: Vec::new(),
        file: 0,
        ambient_context_depth: 0,
        current_fn: 0,
        module_scope: HashMap::new(),
    };
    let global = ScopeId(0);
    for (i, (_name, _text, ast)) in files.iter().enumerate() {
        b.file = i;
        b.current_fn = 0;
        let scope = if ast.is_module {
            let s = b.new_scope(Some(global), ScopeKind::Module);
            b.module_scope.insert(i, s);
            s
        } else {
            b.module_scope.insert(i, global);
            global
        };
        b.bind_statements(&ast.stmts, scope, scope);
        if ast.is_module {
            b.collect_exports(i, &ast.stmts, scope);
        }
    }
    // `export * from "m"`: merge the target module's exports (single pass
    // repeated until stable for chains)
    for _ in 0..4 {
        let mut additions: Vec<(usize, String, SymbolId)> = Vec::new();
        for (i, (_n, _t, ast)) in files.iter().enumerate() {
            for s in &ast.stmts {
                if let Stmt::ExportNamed(e) = s {
                    if e.star {
                        if let Some(m) = &e.module {
                            if let Some(target) = resolve_module_name(files, i, &m.value) {
                                if let Some(texp) = b.exports.get(&target) {
                                    for (name, sym) in texp.0.clone() {
                                        additions.push((i, name, sym));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut changed = false;
        for (f, name, sym) in additions {
            let table = b.exports.entry(f).or_default();
            if table.get(&name).is_none() {
                table.insert(name, sym);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // a module cannot have multiple default exports (2528, every site);
    let export_equals = {
        let mut m = HashMap::new();
        for (f, name) in &b.export_assigns {
            if let Some(&scope) = b.module_scope.get(f) {
                if let Some(sym) = b.scopes[scope.0 as usize].values.get(name) {
                    m.insert(*f, sym);
                }
            }
        }
        m
    };
    BindResult {
        symbols: b.symbols,
        scopes: b.scopes,
        global_scope: global,
        module_scope: b.module_scope,
        node_scope: b.node_scope,
        static_block_scopes: b.static_block_scopes,
        decl_symbol: b.decl_symbol,
        exports: b.exports,
        decl_container: b.decl_container,
        decl_scope: b.decl_scope,
        decl_file: b.decl_file,
        fn_decls: b.fn_decls,
        export_equals,
        pattern_groups: b.pattern_groups,
        var_stmt_groups: b.var_stmt_groups,
        ambient_context_symbols: b.ambient_context_symbols,
        flow_nodes: Vec::new(),
        flow_node: HashMap::new(),
        diags: b.diags,
    }
}

struct Binder<'a> {
    symbols: Vec<Symbol<'a>>,
    scopes: Vec<Scope>,
    node_scope: HashMap<usize, ScopeId>,
    static_block_scopes: HashSet<ScopeId>,
    decl_symbol: HashMap<usize, SymbolId>,
    exports: HashMap<usize, Table>,
    default_exports: Vec<(usize, Span)>,
    export_assigns: Vec<(usize, String)>,
    pattern_groups: Vec<(usize, Span, Vec<SymbolId>)>,
    var_stmt_groups: Vec<(usize, Span, Vec<SymbolId>)>,
    ambient_context_symbols: HashSet<SymbolId>,
    decl_container: HashMap<usize, usize>,
    decl_scope: HashMap<usize, ScopeId>,
    decl_file: HashMap<usize, usize>,
    fn_decls: HashMap<usize, &'a FunctionLike>,
    diags: Vec<Diagnostic>,
    file: usize,
    ambient_context_depth: u32,
    /// ptr of enclosing function-like (0 at top level)
    current_fn: usize,
    module_scope: HashMap<usize, ScopeId>,
}

impl<'a> Binder<'a> {
    fn new_scope(&mut self, parent: Option<ScopeId>, kind: ScopeKind) -> ScopeId {
        self.scopes.push(Scope {
            parent,
            kind,
            values: Table::default(),
            types: Table::default(),
        });
        ScopeId(self.scopes.len() as u32 - 1)
    }

    fn error(&mut self, span: Span, msg: &'static DiagnosticMessage, args: &[String]) {
        self.diags.push(Diagnostic {
            file: Some(self.file),
            start: span.start,
            length: span.len(),
            message: MessageChain::new(msg, args),
            related: Vec::new(),
        });
    }

    fn error_in_file(
        &mut self,
        file: usize,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
    ) {
        self.diags.push(Diagnostic {
            file: Some(file),
            start: span.start,
            length: span.len(),
            message: MessageChain::new(msg, args),
            related: Vec::new(),
        });
    }

    fn new_symbol(&mut self, name: &str, flags: u32, decl: Decl<'a>) -> SymbolId {
        self.symbols.push(Symbol {
            name: name.to_string(),
            flags,
            decls: vec![decl],
            members: Table::default(),
            statics: Table::default(),
            file: self.file,
            dup_reported: false,
            parent: None,
        });
        SymbolId(self.symbols.len() as u32 - 1)
    }

    /// Declare `name` in `scope`'s value and/or type space, applying tsc's
    /// merging rules; reports 2300/2451/2393 on conflicts.
    fn declare(
        &mut self,
        scope: ScopeId,
        name: &str,
        flags: u32,
        decl: Decl<'a>,
        decl_key: usize,
    ) -> SymbolId {
        let in_value = flags & flags::VALUE != 0 || flags & flags::ALIAS != 0;
        let in_type = flags & flags::TYPE != 0 || flags & flags::ALIAS != 0;

        // find existing in either space
        let existing_value = if in_value {
            self.scopes[scope.0 as usize].values.get(name)
        } else {
            None
        };
        let existing_type = if in_type {
            self.scopes[scope.0 as usize].types.get(name)
        } else {
            None
        };
        let existing = existing_value.or(existing_type);

        let id = if let Some(eid) = existing {
            let can_merge = self.can_merge(self.symbols[eid.0 as usize].flags, flags);
            if can_merge {
                self.symbols[eid.0 as usize].decls.push(decl);
                self.symbols[eid.0 as usize].flags |= flags;
                eid
            } else {
                self.report_duplicate(eid, name, flags, decl);
                // still record the new decl on the symbol so later phases see it
                self.symbols[eid.0 as usize].decls.push(decl);
                self.symbols[eid.0 as usize].flags |= flags;
                eid
            }
        } else {
            let id = self.new_symbol(name, flags, decl);
            if in_value && existing_value.is_none() {
                self.scopes[scope.0 as usize]
                    .values
                    .insert(name.to_string(), id);
            }
            if in_type && existing_type.is_none() {
                self.scopes[scope.0 as usize]
                    .types
                    .insert(name.to_string(), id);
            }
            // cross-space insert when symbol spans both spaces
            id
        };
        // ensure both spaces point at the symbol
        if in_value && self.scopes[scope.0 as usize].values.get(name).is_none() {
            self.scopes[scope.0 as usize]
                .values
                .insert(name.to_string(), id);
        }
        if in_type && self.scopes[scope.0 as usize].types.get(name).is_none() {
            self.scopes[scope.0 as usize]
                .types
                .insert(name.to_string(), id);
        }
        self.decl_symbol.insert(decl_key, id);
        self.decl_container.insert(decl_key, self.current_fn);
        self.decl_scope.insert(decl_key, scope);
        self.decl_file.insert(decl_key, self.file);
        id
    }

    fn can_merge(&self, existing: u32, new: u32) -> bool {
        use flags::*;
        // two parameters with one name collide (2300 both sites)
        if existing & PARAMETER != 0 && new & PARAMETER != 0 {
            return false;
        }
        // var + var
        if existing & FUNCTION_SCOPED_VARIABLE != 0 && new & FUNCTION_SCOPED_VARIABLE != 0 {
            return true;
        }
        // interface + interface
        if existing & INTERFACE != 0
            && new & INTERFACE != 0
            && existing & (CLASS | TYPE_ALIAS) == 0
            && new & (CLASS | TYPE_ALIAS) == 0
        {
            return true;
        }
        // class + interface (either order)
        if (existing & CLASS != 0 && new & INTERFACE != 0)
            || (existing & INTERFACE != 0 && new & CLASS != 0)
        {
            return true;
        }
        // function overloads: function + function merge (duplicate *bodies*
        // are flagged separately as TS2393 in the overload-group check).
        if existing & FUNCTION != 0 && new & FUNCTION != 0 {
            return true;
        }
        // enums merge with enums
        if existing & ENUM != 0 && new & ENUM != 0 {
            return true;
        }
        // namespace merging with functions/classes/enums (and namespaces)
        if (existing | new) & NAMESPACE != 0 {
            let other = if existing & NAMESPACE != 0 {
                new
            } else {
                existing
            };
            if other & (FUNCTION | CLASS | ENUM | NAMESPACE) != 0 {
                return true;
            }
        }
        false
    }

    fn report_duplicate(
        &mut self,
        existing: SymbolId,
        name: &str,
        new_flags: u32,
        new_decl: Decl<'a>,
    ) {
        use flags::*;
        let ex_flags = self.symbols[existing.0 as usize].flags;
        // duplicate type parameters report only at the later site
        if ex_flags & TYPE_PARAM != 0 && new_flags & TYPE_PARAM != 0 {
            self.error(
                new_decl.name_span(),
                &gen::Duplicate_identifier_0,
                &[name.to_string()],
            );
            return;
        }
        // redeclaring a catch-clause variable → 2492 at the redeclaration
        if matches!(
            self.symbols[existing.0 as usize].decls.first(),
            Some(Decl::CatchVar(_))
        ) {
            let span = new_decl.name_span();
            self.error(
                span,
                &gen::Cannot_redeclare_identifier_0_in_catch_clause,
                &[name.to_string()],
            );
            return;
        }
        // an import colliding with a local declaration → 2440 on the import
        if (ex_flags | new_flags) & ALIAS != 0 && (ex_flags & ALIAS == 0 || new_flags & ALIAS == 0)
        {
            let import_span = if ex_flags & ALIAS != 0 {
                let f = self.symbols[existing.0 as usize].file;
                let span = self.symbols[existing.0 as usize]
                    .decls
                    .first()
                    .map(|d| d.name_span())
                    .unwrap_or(new_decl.name_span());
                (f, span)
            } else {
                (self.file, new_decl.name_span())
            };
            if !self.symbols[existing.0 as usize].dup_reported {
                self.symbols[existing.0 as usize].dup_reported = true;
                self.error_in_file(
                    import_span.0,
                    import_span.1,
                    &gen::Import_declaration_conflicts_with_local_declaration_of_0,
                    &[name.to_string()],
                );
            }
            return;
        }
        let block_scoped = (ex_flags | new_flags) & BLOCK_SCOPED_VARIABLE != 0;
        let msg: (&'static DiagnosticMessage, Vec<String>) = if block_scoped {
            (
                &gen::Cannot_redeclare_block_scoped_variable_0,
                vec![name.to_string()],
            )
        } else {
            (&gen::Duplicate_identifier_0, vec![name.to_string()])
        };
        // report at every declaration site (existing ones once, then new)
        if !self.symbols[existing.0 as usize].dup_reported {
            self.symbols[existing.0 as usize].dup_reported = true;
            let prior: Vec<(usize, Span)> = self.symbols[existing.0 as usize]
                .decls
                .iter()
                .map(|d| (self.symbols[existing.0 as usize].file, d.name_span()))
                .collect();
            for (f, span) in prior {
                self.error_in_file(f, span, msg.0, &msg.1);
            }
        }
        self.error(new_decl.name_span(), msg.0, &msg.1);
    }

    // ── walking ────────────────────────────────────────────────────────────

    fn hoist_target(&self, mut scope: ScopeId) -> ScopeId {
        loop {
            match self.scopes[scope.0 as usize].kind {
                ScopeKind::Block | ScopeKind::TypeParams => {
                    scope = self.scopes[scope.0 as usize].parent.unwrap();
                }
                _ => return scope,
            }
        }
    }

    fn bind_statements(&mut self, stmts: &'a [Stmt], scope: ScopeId, _fn_scope: ScopeId) {
        for stmt in stmts {
            self.bind_statement(stmt, scope);
        }
    }

    fn bind_statement(&mut self, stmt: &'a Stmt, scope: ScopeId) {
        match stmt {
            Stmt::With { obj, body, .. } => {
                self.bind_expr(obj, scope);
                self.bind_statement(body, scope);
            }
            Stmt::ExportAssign { expr, .. } => {
                self.bind_expr(expr, scope);
                if let Expr::Ident(id) = expr {
                    self.export_assigns.push((self.file, id.name.clone()));
                }
            }
            Stmt::ImportEquals { name, module, .. } => {
                self.declare(
                    scope,
                    &name.name,
                    flags::ALIAS,
                    Decl::ImportEquals(name, module),
                    node_key(name),
                );
            }
            Stmt::ExportDefault { expr, span } => {
                self.bind_expr(expr, scope);
                let report_span = match expr {
                    Expr::Ident(id) => id.span,
                    _ => *span,
                };
                self.default_exports.push((self.file, report_span));
            }
            Stmt::Var(v) => self.bind_var_stmt(v, scope),
            Stmt::Func(f) => {
                if let Some(name) = f.name_ident() {
                    let target = self.hoist_target(scope);
                    let sid = self.declare(
                        target,
                        &name.name,
                        flags::FUNCTION,
                        Decl::Func(f),
                        node_key(&**f),
                    );
                    if self.ambient_context_depth > 0 {
                        self.ambient_context_symbols.insert(sid);
                    }
                }
                self.bind_function_like(f, scope);
            }
            Stmt::Class(c) => {
                if let Some(name) = &c.name {
                    let sid = self.declare(
                        scope,
                        &name.name,
                        flags::CLASS,
                        Decl::Class(c),
                        node_key(&**c),
                    );
                    if self.ambient_context_depth > 0 {
                        self.ambient_context_symbols.insert(sid);
                    }
                }
                self.bind_class(c, scope);
            }
            Stmt::Interface(i) => {
                let id = self.declare(
                    scope,
                    &i.name.name,
                    flags::INTERFACE,
                    Decl::Interface(i),
                    node_key(&**i),
                );
                if self.ambient_context_depth > 0 {
                    self.ambient_context_symbols.insert(id);
                }
                if i.type_params.is_none() {
                    self.node_scope.insert(node_key(&**i), scope);
                }
                self.bind_interface_members(id, i, scope);
            }
            Stmt::Namespace(n) => {
                let id = self.declare(
                    scope,
                    &n.name.name,
                    flags::NAMESPACE,
                    Decl::Namespace(n),
                    node_key(&**n),
                );
                if self.ambient_context_depth > 0 {
                    self.ambient_context_symbols.insert(id);
                }
                let ns_scope = self.new_scope(Some(scope), ScopeKind::Module);
                self.node_scope.insert(node_key(&**n), ns_scope);
                let pushed_ambient = has_modifier(&n.modifiers, ModifierKind::Declare);
                if pushed_ambient {
                    self.ambient_context_depth += 1;
                }
                self.bind_statements(&n.body, ns_scope, ns_scope);
                if pushed_ambient {
                    self.ambient_context_depth -= 1;
                }
                // exports: values into members, types into statics
                for s in &n.body {
                    let (vname, tname): (Option<&str>, Option<&str>) = match s {
                        Stmt::Var(v) if has_modifier(&v.modifiers, ModifierKind::Export) => {
                            for d in &v.decls {
                                if let Some(idn) = d.name.as_ident() {
                                    if let Some(ms) =
                                        self.scopes[ns_scope.0 as usize].values.get(&idn.name)
                                    {
                                        if self.symbols[id.0 as usize]
                                            .members
                                            .get(&idn.name)
                                            .is_none()
                                        {
                                            self.symbols[id.0 as usize]
                                                .members
                                                .insert(idn.name.clone(), ms);
                                        }
                                    }
                                }
                            }
                            (None, None)
                        }
                        Stmt::Func(f) if has_modifier(&f.modifiers, ModifierKind::Export) => {
                            (f.name_ident().map(|i| i.name.as_str()), None)
                        }
                        Stmt::Class(c) if has_modifier(&c.modifiers, ModifierKind::Export) => {
                            let n2 = c.name.as_ref().map(|i| i.name.as_str());
                            (n2, n2)
                        }
                        Stmt::Interface(i) if has_modifier(&i.modifiers, ModifierKind::Export) => {
                            (None, Some(i.name.name.as_str()))
                        }
                        Stmt::TypeAlias(t) if has_modifier(&t.modifiers, ModifierKind::Export) => {
                            (None, Some(t.name.name.as_str()))
                        }
                        Stmt::Enum(en) if has_modifier(&en.modifiers, ModifierKind::Export) => {
                            let n2 = Some(en.name.name.as_str());
                            (n2, n2)
                        }
                        // a nested `export namespace B` is a value (and type)
                        // member of the enclosing namespace, so `A.B.y` resolves.
                        Stmt::Namespace(ns)
                            if has_modifier(&ns.modifiers, ModifierKind::Export) =>
                        {
                            let n2 = Some(ns.name.name.as_str());
                            (n2, n2)
                        }
                        _ => (None, None),
                    };
                    if let Some(vn) = vname {
                        if let Some(ms) = self.scopes[ns_scope.0 as usize].values.get(vn) {
                            if self.symbols[id.0 as usize].members.get(vn).is_none() {
                                self.symbols[id.0 as usize]
                                    .members
                                    .insert(vn.to_string(), ms);
                            }
                        }
                    }
                    if let Some(tn) = tname {
                        if let Some(ms) = self.scopes[ns_scope.0 as usize].types.get(tn) {
                            if self.symbols[id.0 as usize].statics.get(tn).is_none() {
                                self.symbols[id.0 as usize]
                                    .statics
                                    .insert(tn.to_string(), ms);
                            }
                        }
                    }
                }
            }
            Stmt::Enum(e) => {
                let id = self.declare(
                    scope,
                    &e.name.name,
                    flags::ENUM,
                    Decl::Enum(e),
                    node_key(&**e),
                );
                if self.ambient_context_depth > 0 {
                    self.ambient_context_symbols.insert(id);
                }
                self.node_scope.insert(node_key(&**e), scope);
                for m in &e.members {
                    if let Some(init) = &m.init {
                        self.bind_expr(init, scope);
                    }
                    let Some(name) = m.name.text() else { continue };
                    if self.symbols[id.0 as usize].members.get(&name).is_none() {
                        let mid = self.new_symbol(&name, flags::ENUM_MEMBER, Decl::EnumMember(m));
                        self.symbols[mid.0 as usize].parent = Some(id);
                        self.decl_symbol.insert(node_key(m), mid);
                        // Register the member on the enum so `E.Member` resolves
                        // (EnumObject builds its shape from this table).
                        self.symbols[id.0 as usize]
                            .members
                            .insert(name.to_string(), mid);
                    }
                }
            }
            Stmt::TypeAlias(t) => {
                let sid = self.declare(
                    scope,
                    &t.name.name,
                    flags::TYPE_ALIAS,
                    Decl::Alias(t),
                    node_key(&**t),
                );
                if self.ambient_context_depth > 0 {
                    self.ambient_context_symbols.insert(sid);
                }
                if let Some(tps) = &t.type_params {
                    let tscope = self.new_scope(Some(scope), ScopeKind::TypeParams);
                    self.node_scope.insert(node_key(&**t), tscope);
                    self.bind_type_params(tps, tscope);
                } else {
                    self.node_scope.insert(node_key(&**t), scope);
                }
            }
            Stmt::Block(b) => {
                let s = self.new_scope(Some(scope), ScopeKind::Block);
                self.node_scope.insert(node_key(b), s);
                self.bind_statements(&b.stmts, s, s);
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.bind_expr(cond, scope);
                self.bind_statement(then, scope);
                if let Some(e) = els {
                    self.bind_statement(e, scope);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.bind_expr(cond, scope);
                self.bind_statement(body, scope);
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.bind_statement(body, scope);
                self.bind_expr(cond, scope);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                span,
            } => {
                let s = self.new_scope(Some(scope), ScopeKind::Block);
                let _ = span;
                self.node_scope.insert(node_key(stmt), s);
                if let Some(init) = init {
                    match &**init {
                        ForInit::Var(v) => self.bind_var_stmt(v, s),
                        ForInit::Expr(e) => self.bind_expr(e, s),
                    }
                }
                if let Some(c) = cond {
                    self.bind_expr(c, s);
                }
                if let Some(i) = incr {
                    self.bind_expr(i, s);
                }
                self.bind_statement(body, s);
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                let s = self.new_scope(Some(scope), ScopeKind::Block);
                self.node_scope.insert(node_key(stmt), s);
                match &**left {
                    ForInit::Var(v) => self.bind_var_stmt(v, s),
                    ForInit::Expr(e) => self.bind_expr(e, s),
                }
                self.bind_expr(expr, s);
                self.bind_statement(body, s);
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.bind_expr(e, scope);
                }
            }
            Stmt::Expr { expr, .. } => self.bind_expr(expr, scope),
            Stmt::Throw { expr, .. } => self.bind_expr(expr, scope),
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                let s = self.new_scope(Some(scope), ScopeKind::Block);
                self.node_scope.insert(node_key(block), s);
                self.bind_statements(&block.stmts, s, s);
                if let Some(c) = catch {
                    let cs = self.new_scope(Some(scope), ScopeKind::Block);
                    self.node_scope.insert(node_key(c), cs);
                    if let Some(p) = &c.param {
                        if let Some(id) = p.name.as_ident() {
                            self.declare(
                                cs,
                                &id.name,
                                flags::BLOCK_SCOPED_VARIABLE,
                                Decl::CatchVar(p),
                                node_key(p),
                            );
                        }
                    }
                    // block locals share the clause scope so redeclaring the
                    // catch variable collides (2492)
                    self.node_scope.insert(node_key(&c.block), cs);
                    self.bind_statements(&c.block.stmts, cs, cs);
                }
                if let Some(f) = finally {
                    let fs = self.new_scope(Some(scope), ScopeKind::Block);
                    self.node_scope.insert(node_key(f), fs);
                    self.bind_statements(&f.stmts, fs, fs);
                }
            }
            Stmt::Switch { expr, cases, .. } => {
                self.bind_expr(expr, scope);
                let s = self.new_scope(Some(scope), ScopeKind::Block);
                self.node_scope.insert(node_key(stmt), s);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.bind_expr(t, s);
                    }
                    self.bind_statements(&c.stmts, s, s);
                }
            }
            Stmt::Labeled { stmt: inner, .. } => self.bind_statement(inner, scope),
            Stmt::Import(i) => self.bind_import(i, scope),
            Stmt::ExportNamed(_) => {}
            Stmt::Empty { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Missing { .. } => {}
        }
    }

    fn bind_var_stmt(&mut self, v: &'a VarStmt, scope: ScopeId) {
        let mut stmt_syms: Vec<SymbolId> = Vec::new();
        let is_ambient_context = self.ambient_context_depth > 0;
        let is_ambient = has_modifier(&v.modifiers, ModifierKind::Declare) || is_ambient_context;
        for d in &v.decls {
            let (target, fl) = match v.kind {
                VarKind::Var => (self.hoist_target(scope), flags::FUNCTION_SCOPED_VARIABLE),
                VarKind::Let => (scope, flags::BLOCK_SCOPED_VARIABLE),
                VarKind::Const => (scope, flags::BLOCK_SCOPED_VARIABLE | flags::CONST_VARIABLE),
            };
            let fl = if is_ambient { fl | flags::AMBIENT } else { fl };
            match &d.name {
                Binding::Ident(_) => {
                    if let Some(id) = d.name.as_ident() {
                        let sid =
                            self.declare(target, &id.name, fl, Decl::Var(d, v.kind), node_key(d));
                        if is_ambient_context {
                            self.ambient_context_symbols.insert(sid);
                        }
                        stmt_syms.push(sid);
                    }
                }
                pattern => {
                    let before = self.symbols.len();
                    self.bind_pattern_names(pattern, target, fl, v.kind, scope);
                    let members: Vec<SymbolId> = (before..self.symbols.len())
                        .map(|i| SymbolId(i as u32))
                        .collect();
                    if is_ambient_context {
                        for sym in &members {
                            self.ambient_context_symbols.insert(*sym);
                        }
                    }
                    self.pattern_groups
                        .push((self.file, pattern.span(), members));
                }
            }
            // Bind the initializer so nested function-likes (arrows, function
            // expressions, object methods) get their own scopes and parameter
            // bindings. Without this, `const f = (x) => x` leaves `x` unbound.
            if let Some(init) = &d.init {
                self.bind_expr(init, scope);
            }
        }
        if v.decls.len() > 1 && v.decls.iter().all(|d| d.name.as_ident().is_some()) {
            self.var_stmt_groups.push((self.file, v.span, stmt_syms));
        }
    }

    fn bind_pattern_names(
        &mut self,
        b: &'a Binding,
        target: ScopeId,
        fl: u32,
        kind: VarKind,
        expr_scope: ScopeId,
    ) {
        match b {
            Binding::Ident(id) => {
                self.declare(
                    target,
                    &id.name,
                    fl,
                    Decl::PatternVar(id, kind),
                    node_key(id),
                );
            }
            Binding::Object(p) => {
                for prop in &p.props {
                    if let Some(dflt) = &prop.default {
                        self.bind_expr(dflt, expr_scope);
                    }
                    self.bind_pattern_names(&prop.binding, target, fl, kind, expr_scope);
                }
                if let Some(rest) = &p.rest {
                    self.bind_pattern_names(rest, target, fl, kind, expr_scope);
                }
            }
            Binding::Array(p) => {
                for el in p.elements.iter().flatten() {
                    if let Some(dflt) = &el.default {
                        self.bind_expr(dflt, expr_scope);
                    }
                    self.bind_pattern_names(&el.binding, target, fl, kind, expr_scope);
                }
            }
        }
    }

    fn bind_param_pattern(&mut self, b: &'a Binding, fn_scope: ScopeId) {
        match b {
            Binding::Ident(id) => {
                self.declare(
                    fn_scope,
                    &id.name,
                    flags::FUNCTION_SCOPED_VARIABLE | flags::PARAMETER,
                    Decl::PatternParam(id),
                    node_key(id),
                );
            }
            Binding::Object(p) => {
                for prop in &p.props {
                    if let Some(dflt) = &prop.default {
                        self.bind_expr(dflt, fn_scope);
                    }
                    self.bind_param_pattern(&prop.binding, fn_scope);
                }
                if let Some(rest) = &p.rest {
                    self.bind_param_pattern(rest, fn_scope);
                }
            }
            Binding::Array(p) => {
                for el in p.elements.iter().flatten() {
                    if let Some(dflt) = &el.default {
                        self.bind_expr(dflt, fn_scope);
                    }
                    self.bind_param_pattern(&el.binding, fn_scope);
                }
            }
        }
    }

    fn bind_type_params(&mut self, tps: &'a [TypeParamDecl], scope: ScopeId) {
        for tp in tps {
            self.declare(
                scope,
                &tp.name.name,
                flags::TYPE_PARAM,
                Decl::TypeParam(tp),
                node_key(tp),
            );
        }
    }

    fn bind_function_like(&mut self, f: &'a FunctionLike, enclosing: ScopeId) {
        let mut scope = enclosing;
        if let Some(tps) = &f.type_params {
            scope = self.new_scope(Some(scope), ScopeKind::TypeParams);
            self.bind_type_params(tps, scope);
        }
        let fn_scope = self.new_scope(Some(scope), ScopeKind::Function);
        self.node_scope.insert(node_key(f), fn_scope);
        self.fn_decls.insert(node_key(f), f);
        let prev_fn = self.current_fn;
        self.current_fn = node_key(f);
        for p in &f.params {
            if let Some(init) = &p.initializer {
                self.bind_expr(init, fn_scope);
            }
            match &p.name {
                Binding::Ident(id) => {
                    if id.name != "this" {
                        self.declare(
                            fn_scope,
                            &id.name,
                            flags::FUNCTION_SCOPED_VARIABLE | flags::PARAMETER,
                            Decl::Param(p),
                            node_key(p),
                        );
                    }
                }
                pattern => {
                    let before = self.symbols.len();
                    self.bind_param_pattern(pattern, fn_scope);
                    // a fully-unused destructuring parameter pattern is reported
                    // as TS6198; group its names so check_unused_groups can see
                    // them. Only meaningful when the function has a body (tsc
                    // does not unused-check overload/ambient parameters).
                    if f.body.is_some() {
                        let members: Vec<SymbolId> = (before..self.symbols.len())
                            .map(|i| SymbolId(i as u32))
                            .collect();
                        if !members.is_empty() {
                            self.pattern_groups
                                .push((self.file, pattern.span(), members));
                        }
                    }
                }
            }
        }
        match &f.body {
            Some(FuncBody::Block(b)) => {
                self.node_scope.insert(node_key(b), fn_scope);
                self.bind_statements(&b.stmts, fn_scope, fn_scope);
            }
            Some(FuncBody::Expr(e)) => self.bind_expr(e, fn_scope),
            None => {}
        }
        self.current_fn = prev_fn;
    }

    fn bind_jsx(&mut self, j: &'a JsxElement, scope: ScopeId) {
        for a in &j.attrs {
            if let Some(v) = &a.value {
                self.bind_expr(v, scope);
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(e) => self.bind_jsx(e, scope),
                JsxChild::Expr(e) => self.bind_expr(e, scope),
                JsxChild::Text => {}
            }
        }
    }

    fn bind_class_expr(&mut self, c: &'a ClassDecl, scope: ScopeId, name_hint: Option<&str>) {
        let display = c
            .name
            .as_ref()
            .map(|n| n.name.clone())
            .or_else(|| name_hint.map(|s| s.to_string()))
            .unwrap_or_else(|| "(Anonymous class)".to_string());
        let id = self.new_symbol(&display, flags::CLASS, Decl::Class(c));
        self.decl_symbol.insert(node_key(c), id);
        self.decl_scope.insert(node_key(c), scope);
        self.decl_file.insert(node_key(c), self.file);
        self.bind_class(c, scope);
    }

    fn bind_static_block(&mut self, b: &'a Block, scope: ScopeId) {
        let s = self.new_scope(Some(scope), ScopeKind::Function);
        self.static_block_scopes.insert(s);
        self.bind_statements(&b.stmts, s, s);
        self.node_scope.insert(node_key(b), s);
    }

    fn bind_class(&mut self, c: &'a ClassDecl, enclosing: ScopeId) {
        let mut scope = enclosing;
        if let Some(tps) = &c.type_params {
            scope = self.new_scope(Some(scope), ScopeKind::TypeParams);
            self.bind_type_params(tps, scope);
        }
        self.node_scope.insert(node_key(c), scope);
        if let Some(h) = &c.extends {
            self.bind_expr(&h.expr, scope);
        }
        if let Some(h) = &c.extends {
            self.bind_expr(&h.expr, scope);
        }
        // member symbols onto the class symbol's tables
        let class_sym = self.decl_symbol.get(&node_key(c)).copied();
        for m in &c.members {
            match m {
                ClassMember::StaticBlock(b) => {
                    self.bind_static_block(b, scope);
                }
                ClassMember::Property(p) => {
                    if let Some(init) = &p.init {
                        self.bind_expr(init, scope);
                    }
                    if let (Some(sym), Some(name)) = (class_sym, p.name.text()) {
                        let is_static = has_modifier(&p.modifiers, ModifierKind::Static);
                        let mut fl = flags::PROPERTY;
                        if p.question {
                            fl |= flags::OPTIONAL;
                        }
                        if has_modifier(&p.modifiers, ModifierKind::Readonly) {
                            fl |= flags::READONLY;
                        }
                        if has_modifier(&p.modifiers, ModifierKind::Abstract) {
                            fl |= flags::ABSTRACT;
                        }
                        let mid = self.new_symbol(&name, fl, Decl::PropertyDecl(p));
                        self.symbols[mid.0 as usize].parent = class_sym;
                        self.decl_symbol.insert(node_key(p), mid);
                        self.decl_scope.insert(node_key(p), scope);
                        let table = if is_static {
                            &mut self.symbols[sym.0 as usize].statics
                        } else {
                            &mut self.symbols[sym.0 as usize].members
                        };
                        if table.get(&name).is_none() {
                            table.insert(name, mid);
                        }
                    }
                }
                ClassMember::Method(f) | ClassMember::Constructor(f) => {
                    if let (Some(sym), Some(name)) =
                        (class_sym, f.name.as_ref().and_then(|n| n.text()))
                    {
                        if !matches!(m, ClassMember::Constructor(_)) {
                            let is_static = has_modifier(&f.modifiers, ModifierKind::Static);
                            let mut fl = flags::METHOD;
                            if has_modifier(&f.modifiers, ModifierKind::Abstract) {
                                fl |= flags::ABSTRACT;
                            }
                            if f.kind == FuncKind::Getter {
                                fl = flags::GET_ACCESSOR | flags::PROPERTY;
                            } else if f.kind == FuncKind::Setter {
                                fl = flags::SET_ACCESSOR | flags::PROPERTY;
                            }
                            if f.question {
                                fl |= flags::OPTIONAL;
                            }
                            let mid = self.new_symbol(&name, fl, Decl::Method(f));
                            self.symbols[mid.0 as usize].parent = class_sym;
                            self.decl_symbol.insert(node_key(&**f), mid);
                            let table = if is_static {
                                &mut self.symbols[sym.0 as usize].statics
                            } else {
                                &mut self.symbols[sym.0 as usize].members
                            };
                            if let Some(existing) = table.get(&name) {
                                // merge a complementary accessor (get + set) into
                                // the existing member symbol so it carries both
                                // GET_ACCESSOR and SET_ACCESSOR — a property is
                                // read-only only when a getter has no setter.
                                let ef = self.symbols[existing.0 as usize].flags;
                                if (ef & flags::GET_ACCESSOR != 0 || ef & flags::SET_ACCESSOR != 0)
                                    && (fl & flags::GET_ACCESSOR != 0
                                        || fl & flags::SET_ACCESSOR != 0)
                                {
                                    self.symbols[existing.0 as usize].flags |= fl;
                                    self.symbols[existing.0 as usize]
                                        .decls
                                        .push(Decl::Method(f));
                                }
                            } else {
                                table.insert(name, mid);
                            }
                        }
                    }
                    self.bind_function_like(f, scope);
                    // parameter properties: declare on instance side
                    if matches!(m, ClassMember::Constructor(_)) {
                        if let Some(sym) = class_sym {
                            for p in &f.params {
                                let is_param_prop = p.modifiers.iter().any(|mm| {
                                    matches!(
                                        mm.kind,
                                        ModifierKind::Public
                                            | ModifierKind::Private
                                            | ModifierKind::Protected
                                            | ModifierKind::Readonly
                                    )
                                });
                                if is_param_prop {
                                    if let Some(id) = p.name.as_ident() {
                                        let mut fl = flags::PROPERTY;
                                        if has_modifier(&p.modifiers, ModifierKind::Readonly) {
                                            fl |= flags::READONLY;
                                        }
                                        let mid = self.new_symbol(&id.name, fl, Decl::Param(p));
                                        self.symbols[mid.0 as usize].parent = Some(sym);
                                        if self.symbols[sym.0 as usize]
                                            .members
                                            .get(&id.name)
                                            .is_none()
                                        {
                                            self.symbols[sym.0 as usize]
                                                .members
                                                .insert(id.name.clone(), mid);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ClassMember::Index(_) => {}
            }
        }
    }

    fn bind_interface_members(&mut self, sym: SymbolId, i: &'a InterfaceDecl, enclosing: ScopeId) {
        let mut member_scope = enclosing;
        if let Some(tps) = &i.type_params {
            let scope = self.new_scope(Some(enclosing), ScopeKind::TypeParams);
            self.node_scope.insert(node_key(i), scope);
            self.bind_type_params(tps, scope);
            member_scope = scope;
        }
        for m in &i.members {
            let (name, fl, decl) = match m {
                TypeMember::Prop(p) => {
                    let Some(name) = p.name.text() else { continue };
                    let mut fl = flags::PROPERTY;
                    if p.question {
                        fl |= flags::OPTIONAL;
                    }
                    if p.readonly {
                        fl |= flags::READONLY;
                    }
                    (name, fl, Decl::PropSig(p))
                }
                TypeMember::Method(ms) => {
                    let Some(name) = ms.name.text() else { continue };
                    let mut fl = flags::METHOD;
                    if ms.question {
                        fl |= flags::OPTIONAL;
                    }
                    (name, fl, Decl::MethodSig(ms))
                }
                _ => continue,
            };
            let existing = self.symbols[sym.0 as usize].members.get(&name);
            match existing {
                Some(mid)
                    if self.symbols[mid.0 as usize].flags & flags::METHOD != 0
                        && fl & flags::METHOD != 0 =>
                {
                    // method overloads merge
                    self.symbols[mid.0 as usize].decls.push(decl);
                    if let Decl::MethodSig(ms) = decl {
                        self.decl_symbol.insert(node_key(ms), mid);
                        // every overload signature must resolve against the same
                        // member scope; without this the 2nd+ overload falls back
                        // to the global scope and loses the interface's type
                        // parameters (spurious "Cannot find name 'T'").
                        self.decl_scope.insert(node_key(ms), member_scope);
                    }
                }
                Some(_mid) => {
                    // duplicate property in same/merged interface — checker (2717/2300 stretch)
                }
                None => {
                    let mid = self.new_symbol(&name, fl, decl);
                    self.symbols[mid.0 as usize].parent = Some(sym);
                    match decl {
                        Decl::PropSig(p) => {
                            self.decl_symbol.insert(node_key(p), mid);
                            self.decl_scope.insert(node_key(p), member_scope);
                        }
                        Decl::MethodSig(ms) => {
                            self.decl_symbol.insert(node_key(ms), mid);
                            self.decl_scope.insert(node_key(ms), member_scope);
                        }
                        _ => {}
                    }
                    self.symbols[sym.0 as usize].members.insert(name, mid);
                }
            }
        }
    }

    /// Walk expressions to bind nested function-likes (arrows, function
    /// expressions, object-literal methods) and record their scopes.
    fn bind_expr(&mut self, e: &'a Expr, scope: ScopeId) {
        match e {
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                if let (Expr::FunctionExpr(_), Some(name)) = (e, f.name_ident()) {
                    // named function expressions bind their own name inside
                    let s = self.new_scope(Some(scope), ScopeKind::Block);
                    self.declare(
                        s,
                        &name.name,
                        flags::FUNCTION,
                        Decl::Func(f),
                        node_key(&**f),
                    );
                    self.bind_function_like(f, s);
                } else {
                    self.bind_function_like(f, scope);
                }
            }
            Expr::Array { elements, .. } => {
                for el in elements {
                    self.bind_expr(el, scope);
                }
            }
            Expr::Object { props, .. } => {
                for p in props {
                    match p {
                        ObjectProp::Property { value, .. } => self.bind_expr(value, scope),
                        ObjectProp::Method(f) => self.bind_function_like(f, scope),
                        ObjectProp::Spread { expr, .. } => self.bind_expr(expr, scope),
                        ObjectProp::Shorthand { .. } => {}
                    }
                }
            }
            Expr::Call { callee, args, .. } => {
                self.bind_expr(callee, scope);
                for a in args {
                    self.bind_expr(a, scope);
                }
            }
            Expr::New { callee, args, .. } => {
                self.bind_expr(callee, scope);
                if let Some(args) = args {
                    for a in args {
                        self.bind_expr(a, scope);
                    }
                }
            }
            Expr::PropAccess { obj, .. } => self.bind_expr(obj, scope),
            Expr::ElemAccess { obj, index, .. } => {
                self.bind_expr(obj, scope);
                self.bind_expr(index, scope);
            }
            Expr::Unary { operand, .. } => self.bind_expr(operand, scope),
            Expr::Update { operand, .. } => self.bind_expr(operand, scope),
            Expr::Binary { left, right, .. } => {
                self.bind_expr(left, scope);
                self.bind_expr(right, scope);
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                self.bind_expr(cond, scope);
                self.bind_expr(when_true, scope);
                self.bind_expr(when_false, scope);
            }
            Expr::Paren { inner, .. } => self.bind_expr(inner, scope),
            Expr::Assertion { expr, .. } => self.bind_expr(expr, scope),
            Expr::NonNull { expr, .. } => self.bind_expr(expr, scope),
            Expr::Spread { expr, .. } => self.bind_expr(expr, scope),
            Expr::Await { expr, .. } => self.bind_expr(expr, scope),
            Expr::Yield { expr, .. } => {
                if let Some(e) = expr {
                    self.bind_expr(e, scope);
                }
            }
            Expr::Template { parts, .. } => {
                for p in parts {
                    if let TemplatePart::Expr(e) = p {
                        self.bind_expr(e, scope);
                    }
                }
            }
            Expr::ImportCall { args, .. } => {
                for a in args {
                    self.bind_expr(a, scope);
                }
            }
            Expr::ImportMeta { .. } => {}
            Expr::ClassExpr(c) => self.bind_class_expr(c, scope, None),
            Expr::JsxElement(j) => self.bind_jsx(j, scope),
            Expr::Ident(_)
            | Expr::NumLit { .. }
            | Expr::StrLit { .. }
            | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. }
            | Expr::NullLit { .. }
            | Expr::RegexLit { .. }
            | Expr::This { .. }
            | Expr::Super { .. }
            | Expr::Missing { .. } => {}
        }
    }

    fn bind_import(&mut self, i: &'a ImportDecl, scope: ScopeId) {
        if let Some(d) = &i.default_name {
            self.declare(
                scope,
                &d.name,
                flags::ALIAS,
                Decl::ImportDefault(i),
                node_key(d),
            );
        }
        if let Some(ns) = &i.namespace_name {
            self.declare(
                scope,
                &ns.name,
                flags::ALIAS,
                Decl::ImportNamespace(i),
                node_key(ns),
            );
        }
        if let Some(named) = &i.named {
            for spec in named {
                self.declare(
                    scope,
                    &spec.name.name,
                    flags::ALIAS,
                    Decl::Import(spec, i),
                    node_key(spec),
                );
            }
        }
    }

    fn collect_exports(&mut self, file: usize, stmts: &'a [Stmt], scope: ScopeId) {
        let mut table = Table::default();
        for stmt in stmts {
            match stmt {
                Stmt::Var(v) if has_modifier(&v.modifiers, ModifierKind::Export) => {
                    for d in &v.decls {
                        if let Some(id) = d.name.as_ident() {
                            if let Some(sym) = self.scopes[scope.0 as usize].values.get(&id.name) {
                                table.insert(id.name.clone(), sym);
                            }
                        }
                    }
                }
                Stmt::Func(f) if has_modifier(&f.modifiers, ModifierKind::Export) => {
                    if let Some(n) = f.name_ident() {
                        if let Some(sym) = self.scopes[scope.0 as usize].values.get(&n.name) {
                            table.insert(n.name.clone(), sym);
                        }
                    }
                }
                Stmt::Class(c) if has_modifier(&c.modifiers, ModifierKind::Export) => {
                    if let Some(n) = &c.name {
                        if let Some(sym) = self.scopes[scope.0 as usize]
                            .values
                            .get(&n.name)
                            .or(self.scopes[scope.0 as usize].types.get(&n.name))
                        {
                            table.insert(n.name.clone(), sym);
                        }
                    }
                }
                Stmt::Interface(idecl) if has_modifier(&idecl.modifiers, ModifierKind::Export) => {
                    if let Some(sym) = self.scopes[scope.0 as usize].types.get(&idecl.name.name) {
                        table.insert(idecl.name.name.clone(), sym);
                    }
                }
                Stmt::TypeAlias(t) if has_modifier(&t.modifiers, ModifierKind::Export) => {
                    if let Some(sym) = self.scopes[scope.0 as usize].types.get(&t.name.name) {
                        table.insert(t.name.name.clone(), sym);
                    }
                }
                Stmt::Enum(e) if has_modifier(&e.modifiers, ModifierKind::Export) => {
                    if let Some(sym) = self.scopes[scope.0 as usize].values.get(&e.name.name) {
                        table.insert(e.name.name.clone(), sym);
                    }
                }
                Stmt::ExportDefault { .. } => {
                    let id = self.new_symbol(
                        "default",
                        flags::FUNCTION_SCOPED_VARIABLE,
                        Decl::DefaultExport,
                    );
                    table.insert("default".to_string(), id);
                }
                Stmt::Func(f)
                    if has_modifier(&f.modifiers, ModifierKind::Export)
                        && has_modifier(&f.modifiers, ModifierKind::Default) =>
                {
                    if let Some(n) = f.name_ident() {
                        if let Some(sym) = self.scopes[scope.0 as usize].values.get(&n.name) {
                            table.insert("default".to_string(), sym);
                        }
                    }
                }
                Stmt::Class(c)
                    if has_modifier(&c.modifiers, ModifierKind::Export)
                        && has_modifier(&c.modifiers, ModifierKind::Default) =>
                {
                    if let Some(n) = &c.name {
                        if let Some(sym) = self.scopes[scope.0 as usize].values.get(&n.name) {
                            table.insert("default".to_string(), sym);
                        }
                    }
                }
                Stmt::ExportNamed(e) if e.module.is_none() => {
                    for spec in &e.specifiers {
                        let local = spec.prop_name.as_ref().unwrap_or(&spec.name);
                        let exported = &spec.name;
                        if let Some(sym) = self.scopes[scope.0 as usize]
                            .values
                            .get(&local.name)
                            .or(self.scopes[scope.0 as usize].types.get(&local.name))
                        {
                            table.insert(exported.name.clone(), sym);
                        }
                    }
                }
                _ => {}
            }
        }
        self.exports.insert(file, table);
    }
}

pub fn run_function_impl_checks(b: &mut BindResult) {
    let mut diags = Vec::new();
    for s in &mut b.symbols {
        if s.flags & flags::FUNCTION == 0 {
            continue;
        }
        let impls: Vec<Span> = s
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Func(f) if f.body.is_some() => Some(d.name_span()),
                _ => None,
            })
            .collect();
        if impls.len() > 1 {
            for span in impls {
                diags.push(Diagnostic {
                    file: Some(s.file),
                    start: span.start,
                    length: span.len(),
                    message: MessageChain::new(&gen::Duplicate_function_implementation, &[]),
                    related: Vec::new(),
                });
            }
        }
    }
    b.diags.append(&mut diags);
}

/// module specifier resolution shared with the checker (name-join only)
fn resolve_module_name(
    files: &[(String, crate::text::SourceText, SourceFileAst)],
    from_file: usize,
    spec: &str,
) -> Option<usize> {
    if !spec.starts_with("./") && !spec.starts_with("../") {
        return None;
    }
    let joined = resolve_relative_module_base(&files[from_file].0, spec);
    for cand in [
        format!("{joined}.ts"),
        format!("{joined}.tsx"),
        format!("{joined}.d.ts"),
        format!("{joined}/index.ts"),
    ] {
        if let Some(idx) = files.iter().position(|(n, _, _)| *n == cand) {
            return Some(idx);
        }
    }
    None
}

pub(crate) fn resolve_relative_module_base(from: &str, spec: &str) -> String {
    let absolute = from.starts_with('/');
    let dir = match from.rfind('/') {
        Some(0) if absolute => "/",
        Some(i) => &from[..i],
        None => "",
    };
    let mut parts: Vec<&str> = if dir.is_empty() || dir == "/" {
        Vec::new()
    } else {
        dir.trim_start_matches('/').split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}
