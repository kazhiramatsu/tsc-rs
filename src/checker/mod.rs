//! The checker: scope-driven type checking with tsc-fidelity diagnostics.
//! Split across submodules; all share `impl<'a> Checker<'a>`.

pub mod access;
pub mod aliases;
pub mod apparent;
pub mod calls;
pub mod classes;
pub mod conditional;
pub mod display;
pub mod exprs;
pub mod flow;
pub mod functions;
pub mod infer;
pub mod instantiate;
pub mod operators;
pub mod relation_errors;
pub mod relations;
pub mod shapes;
pub mod stmts;
pub mod symbols;

use crate::ast::*;
use crate::binder::{flags, BindResult, ScopeId, SymbolId};
use crate::diagnostics::{gen, Category, Diagnostic, DiagnosticMessage, MessageChain, RelatedInfo};
use crate::options::CompilerOptions;
use crate::text::SourceText;
use crate::types::{TypeId, TypeTable};
use std::collections::{HashMap, HashSet};

pub type Files = [(String, SourceText, SourceFileAst)];

/// What a symbol resolution slot is computing (cycle detection).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    ValueType,
    AliasTarget,
    BaseType,
    StaticBaseType,
    Constraint,
    ReturnType(usize), // keyed by decl ptr
}

pub struct FnCtx {
    /// declared return type (None = inferred)
    pub return_type: Option<TypeId>,
    pub is_async: bool,
    pub is_generator: bool,
    pub kind: crate::ast::FuncKind,
    pub fn_key: usize,
    /// type of an explicit `this` parameter (`function f(this: T)`); None when
    /// the function has no `this` annotation. Used to type the `this` expression
    /// inside the body instead of falling back to implicit-any.
    pub this_ty: Option<TypeId>,
}

/// Single-report dedup guards: ensure a given diagnostic is emitted at most once
/// even when its node/symbol is reached from several code paths. (The final
/// `sort_and_dedupe` only collapses byte-identical diagnostics; these guards
/// also avoid rebuilding the diagnostic and pin a single emission.) The two
/// generic sets replace what used to be one named `HashSet` per code — keyed by
/// `(code, …)` so distinct codes never collide. Use the `report_once_*` helpers.
#[derive(Default)]
pub struct ReportGuards {
    /// "emit once per (diagnostic code, AST node key)" — covers 2717, 2368,
    /// 7023, the 7032/7033 accessor implicit-any, and 7039.
    pub reported_once_node: HashSet<(u32, usize)>,
    /// "emit once per (diagnostic code, symbol)" — covers 2395, 2428, 2567.
    pub reported_once_sym: HashSet<(u32, SymbolId)>,
    /// file-less "cannot find global type" (2318), deduped by type name.
    pub reported_missing_globals: HashSet<String>,
    /// TS2454 "used before assigned", keyed by (file, read-span start): dedups
    /// the forward DA pass against the per-use check at the same read position.
    pub reported_2454: HashSet<(usize, usize)>,
}

/// Control-flow state: the narrowing fact stack (one frame per active scope),
/// loop/switch nesting depth, active labels, auto-variable TS7034 records,
/// constructor property-assignment flow, the lazy-return cycle stack, and the
/// set of exhaustively-covered switches.
#[derive(Default)]
pub struct FlowState {
    pub facts: Vec<HashMap<RefKey, TypeId>>,
    pub loop_depth: u32,
    pub switch_depth: u32,
    pub label_stack: Vec<String>,
    /// auto (control-flow-typed) variables whose capture reads fired TS7005
    /// → TS7034 at the declaration name (file, span) in `check_unused`
    pub auto_fired: HashMap<SymbolId, (usize, Span)>,
    /// TS7027 state: suppress reports inside an already-reported
    /// unreachable statement (tsc withinUnreachableCode), and remember
    /// range-consumed statements so the per-statement hook skips them.
    pub within_unreachable_code: bool,
    pub reported_unreachable: HashSet<usize>,
    pub return_stack: Vec<usize>,
    pub exhaustive_switches: std::collections::HashSet<usize>,
}

/// Tier-2 flow-graph resolver state (Stage 0 dark launch): per-top-level-query
/// memoization and in-progress guard (cycle detection across loop back-edges)
/// for `get_flow_type_of_reference`. See `src/checker/flow/resolver.rs`.
#[derive(Default)]
pub struct FlowResolve {
    /// (ref, flow) → resolved type (`None` = unresolvable); cleared per query.
    pub memo: HashMap<(RefKey, crate::binder::FlowNodeId), Option<TypeId>>,
    /// (ref, flow) pairs on the active resolution stack.
    pub in_progress: HashSet<(RefKey, crate::binder::FlowNodeId)>,
    /// >0 while a resolver scaffold re-runs checker code exploratorily
    /// (its diagnostics are rolled back afterwards). Exploratory runs must
    /// not populate `expr_type_cache` or consume the once-per-node/symbol
    /// report guards: either would silently eat the diagnostic when the
    /// real check reaches the same node later.
    pub quiet: u32,
    /// Definite-assignment seed for the current query (tsc's `initialType`):
    /// when the walk for THIS reference reaches a container `Start` without
    /// meeting an assignment, it reads the seeded type (typically
    /// `declared | undefined`) instead of the declared type. Fields:
    /// (reference, seed type, declarator span, never_initialized). The seed
    /// is consumed only at the Start of the container CONTAINING the
    /// declarator — at a foreign entry the variable is outer and tsc
    /// assumes it initialized — EXCEPT for never-initialized variables
    /// (annotated, never-assigned `let`s), which tsc checks even across
    /// closures. Keyed so sub-walks of other references are unaffected.
    pub initial: Option<(RefKey, TypeId, crate::ast::Span, bool)>,
    /// Reachability memos (Branch nodes only — tsc caches its Shared join
    /// points). Lazy = never-calls/exhaustive-switches terminate flow;
    /// structural = the binder's type-blind view. Order-dependent exactly
    /// like tsc: exhaustive_switches grows during checking, and all
    /// reachability queries run after the switch they cross was checked.
    pub reach_lazy: HashMap<crate::binder::FlowNodeId, bool>,
    pub reach_structural: HashMap<crate::binder::FlowNodeId, bool>,
    /// The class's `this` parameter symbol for the current SEEDED query:
    /// `ref_key_in_scope` keys `Expr::This` as this root only while set, so
    /// `this.x = v` Assign targets match the 2564/2565 queries without
    /// changing Stage-1 narrowing (whose read seams never set it).
    pub this_sym: Option<crate::binder::SymbolId>,
    /// The reference of the current AUTO query (tsc autoType CFA — an
    /// unannotated, nullish-or-un-initialized, noImplicitAny let/var read):
    /// the Init arm yields the initializer's nullish type, a foreign
    /// container Start yields the auto marker, and Branch joins with a
    /// marker antecedent stay marker (tsc: auto is infectious at joins).
    pub auto: Option<RefKey>,
    /// The non-interned `Any` clone standing in for tsc's autoType. Kind ==
    /// Any, so every narrowing helper / union / relation treats it as any
    /// (filters that return their input preserve it; producing filters —
    /// typeof — kill it, which is what cancels TS7005). Distinct TypeId, so
    /// a result that IS the marker means "every path reached a foreign
    /// Start unassigned" ⇒ TS7005/7034.
    pub auto_marker: Option<TypeId>,
}

/// Resolution memoization caches: symbol→type, member-shape, signature return &
/// `this` types, alias targets, and per-node / per-expression type caches.
#[derive(Default)]
pub struct ResolutionCaches {
    pub sym_type: HashMap<SymbolId, TypeId>,
    pub members_cache: HashMap<TypeId, crate::types::ShapeId>,
    pub sig_ret_cache: HashMap<usize, TypeId>,
    pub sig_this_ty: HashMap<crate::types::SigId, TypeId>,
    pub alias_type_cache: HashMap<SymbolId, TypeId>,
    pub node_type_cache: HashMap<usize, TypeId>,
    pub expr_type_cache: HashMap<usize, TypeId>,
    /// contextual parameter types resolved per call node
    pub param_ctx_types: HashMap<usize, TypeId>,
    /// fresh object-literal property spans (excess-property reporting)
    pub fresh_obj_props: HashMap<TypeId, Vec<(String, Span)>>,
    /// overload implementation signature per overloaded shape
    pub overload_impl: HashMap<crate::types::ShapeId, (crate::types::SigId, u32, u32, usize)>,
}

/// Deferred / lazy type-evaluation state: type-literal, conditional, and mapped
/// type nodes whose evaluation is postponed until their members are demanded,
/// plus synthetic type-parameter symbols and aliases pending resolution.
#[derive(Default)]
pub struct DeferredState<'a> {
    /// deferred type-literal members (node key -> members + scope)
    pub deferred_literals: HashMap<usize, (&'a [TypeMember], ScopeId, Vec<(String, SymbolId)>)>,
    /// deferred conditional type nodes (node key -> node + scope + file)
    pub deferred_conds: HashMap<usize, (&'a ConditionalTypeNode, ScopeId, usize)>,
    /// deferred mapped type nodes
    pub deferred_mappeds: HashMap<usize, (&'a MappedTypeNode, ScopeId, usize)>,
    /// synthetic type-param symbols for mapped keys / infer declarations
    pub synthetic_type_params: HashMap<usize, SymbolId>,
    /// Polymorphic `this` type parameters, one per owning class/interface
    /// (non-generic owners only — generic owners use their own type params).
    /// `this_params`: owner symbol → the synthetic `this`-type parameter.
    /// `this_param_owner`: the reverse map, used by `constraint_of_type_param`
    /// and by member access to recover the owner without scanning.
    pub this_params: HashMap<SymbolId, SymbolId>,
    pub this_param_owner: HashMap<SymbolId, SymbolId>,
    /// alias currently being resolved + its body node key (outer alias owns
    /// the display name only for the body's own type reference)
    pub pending_alias: Vec<(SymbolId, usize)>,
}

/// Recursion / DoS guards: bounded depth counters and an instantiation budget
/// that keep pathological types from hanging or blowing the stack (TS2589 etc.).
#[derive(Default)]
pub struct RecursionGuards {
    pub eval_depth: u32,
    pub inst_depth: u32,
    /// cumulative count of type instantiations (tsc instantiationCount); a
    /// runaway count signals an infinitely-expanding type (TS2589).
    pub inst_count: u64,
    /// recursion guard for structural inference (infer_from_shapes).
    pub infer_depth: u32,
    /// nesting depth inside a deferring type wrapper (array/tuple/function
    /// element) while resolving an alias body. A self-reference reached at
    /// depth > 0 is a legal recursive type (broken permissively) rather than a
    /// circular alias (TS2456).
    pub alias_wrapper_depth: u32,
}

/// Enum checking state: computed member values and whether a const-enum member
/// reference is currently in a position where it is allowed.
#[derive(Default)]
pub struct EnumState {
    pub enum_member_values: HashMap<SymbolId, EnumValue>,
    pub const_enum_ident_ok: bool,
}

/// Identifies *what kind* of declaration body we are currently checking, for
/// the purpose of resolving a `this` reference inside it. See `ThisContainer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    /// A class body itself (between the `{`...`}` of `class C { ... }`),
    /// covering decorators, the heritage clause, and non-static field
    /// initializers. `this` here is the class instance type.
    ClassBody,
    /// A non-arrow class method body (regular, getter, setter, constructor)
    /// or static block.
    Method,
    /// A non-arrow function declaration or expression *outside* a class
    /// context — or one that crosses a class boundary. Cuts the lexical
    /// `this`: `Expr::This` here uses the explicit annotation if present,
    /// otherwise `any`.
    NonArrowFn,
    /// An arrow function: transparent. The `this` walk passes through.
    Arrow,
    /// An interface body — used only by `TypeNode::This`.
    InterfaceBody,
}

/// The `this`-resolving context of a single body currently being checked.
/// Pushed onto `this_container_stack` at every entry point that evaluates the
/// body or initializer of a declaration — both direct traversals
/// (`check_class` → `check_function_body`) and *lazy* ones (`sig_return` →
/// `infer_return_from_body`, `type_of_symbol(PropertyDecl)` evaluating an
/// initializer, etc.). This is the single source of truth for `Expr::This`
/// and `TypeNode::This`; it does not depend on `class_stack`/`fn_stack`,
/// which keep their original roles of tracking *lexical* class/function
/// scope for things like private-access checks.
#[derive(Clone, Copy, Debug)]
pub struct ThisContainer {
    /// The class or interface symbol that this body belongs to, if any. For a
    /// top-level function or a function expression outside any class, this is
    /// `None` (so `this` walk past this entry continues outward).
    pub class_owner: Option<SymbolId>,
    /// `true` when this body is on the static side of `class_owner`
    /// (static field initializer, static method body, or static block).
    pub is_static: bool,
    pub kind: ContainerKind,
    /// An explicit `this:` parameter annotation, or a contextual `ThisType<T>`
    /// stage. When `Some`, it overrides the implicit class-derived `this`.
    pub explicit_this: Option<TypeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtorFieldContextKind {
    Initializer,
    TypeAnnotation,
}

#[derive(Clone, Debug)]
pub struct CtorFieldContext {
    pub field_name: String,
    pub blocked_names: HashSet<String>,
    pub kind: CtorFieldContextKind,
}

/// Traversal context stacks: the active function, class, and `this`-type nesting
/// pushed and popped as declarations are entered and left.
#[derive(Default)]
pub struct TraversalStacks {
    pub fn_stack: Vec<FnCtx>,
    pub class_stack: Vec<SymbolId>,
    pub this_type_stack: Vec<SymbolId>,
    /// Single source of truth for resolving `this`. See `ThisContainer`.
    pub this_container_stack: Vec<ThisContainer>,
}

/// In-progress type-resolution state: the active resolution stack (cycle guard)
/// and the set of (symbol, slot) resolutions already known to have failed.
#[derive(Default)]
pub struct ResolutionState {
    pub resolving: Vec<(SymbolId, Slot)>,
    pub resolution_failed: HashSet<(SymbolId, Slot)>,
}

/// Type-parameter resolution context: the transient `infer` / mapped-key
/// name→symbol environment — synthetic params minted during conditional- and
/// mapped-type evaluation, which have no lexical scope — plus scratch space for
/// a rest type parameter being matched. Real signature type params resolve by
/// ordinary lexical lookup via `push_tp_scope`, not through this environment.
#[derive(Default)]
pub struct TypeParamCtx {
    pub infer_mapped_env: Vec<(String, SymbolId)>,
    pub rest_tp_scratch: Option<crate::binder::SymbolId>,
}

/// Transient checking-mode flags: inside a `const` assertion, skipping context-
/// sensitive inference, widening object-literal inferences, inside a constructor
/// parameter initializer, and current namespace nesting depth.
#[derive(Default)]
pub struct CheckFlags {
    pub in_const_assertion: bool,
    pub skip_ctx_sensitive: bool,
    pub infer_widen_objlit: bool,
    pub in_ctor_param_init: bool,
    /// node key of an identifier directly wrapped by `!` (NonNull) — that
    /// read is exempt from definite-assignment checking (tsc's
    /// NonNullExpression-parent disjunct). Keyed by node, so no reset is
    /// needed; parens (`(x)!`) intentionally do not set it.
    pub nonnull_ident: usize,
    /// node key of the prop-access currently being checked AS AN ASSIGNMENT
    /// TARGET: the read seam must not flow-narrow the target reference
    /// itself (tsc AssignmentKind.Definite reads the declared type), while
    /// its RECEIVER sub-reads narrow normally (`control[key] = value` inside
    /// an `if (control !== undefined)` guard sees the narrowed receiver).
    pub assign_target: usize,
    /// >0 while checking a destructuring assignment TARGET pattern (or a
    /// parenthesized identifier target): every identifier inside is a write
    /// position — the read seam, definite-assignment and auto hooks all
    /// yield the declared type (tsc AssignmentKind.Definite for each leaf).
    pub pattern_target: u32,
    /// Nesting depth while checking a class static block. Some names such as
    /// `await` are parse-time invalid in arrow parameter lists there; when the
    /// parser recovers them as an arrow, semantic implicit-any suggestions must
    /// not be layered on top of that grammar failure.
    pub in_class_static_block: u32,
    /// Nesting depth while checking the expression of a return statement that
    /// is itself outside any function body. tsc still parses into the subtree,
    /// but suggestion diagnostics from nested function bodies are not reliably
    /// produced, so semantic implicit-any reporting is suppressed there.
    pub invalid_return_expr_depth: u32,
    /// While a generator function argument is being checked during inference
    /// without its contextual return type, yielded function expressions would
    /// otherwise report implicit-any parameters even though their contextual
    /// type comes from the generator's contextual `Iterator<Y>` yield type.
    pub suppress_yield_function_implicit_any_params: u32,
    /// Nesting depth while evaluating a class heritage expression. Property
    /// lookup through the current class's `this` type can re-enter its
    /// instance-shape computation before the base type is known; that is not a
    /// real circular base reference by itself.
    pub in_heritage_expr: u32,
    pub namespace_depth: u32,
    pub ambient_context_depth: u32,
    /// Stack depths at each namespace/module body entry. A `this` expression is
    /// a namespace-body `this` only while function/class stack depths still
    /// match the innermost namespace entry; nested functions and classes own
    /// their own `this` rules.
    pub namespace_stack: Vec<NamespaceContext>,
    /// Set by call checking while typing an immediately-invoked function
    /// expression. The next FunctionLike consumes one slot and suppresses only
    /// its own implicit-any parameter diagnostics; nested callbacks in the body
    /// still report normally.
    pub suppress_next_function_implicit_any_params: u32,
    /// Same one-shot mechanism for signature-only return implicit-any
    /// diagnostics. Used for declaration forms where tsc still checks
    /// parameters/grammar but omits the return suggestion.
    pub suppress_next_function_implicit_any_return: u32,
    /// Classic class-field semantics check instance property initializers in
    /// constructor scope but reject references to constructor parameters/locals.
    pub ctor_field_stack: Vec<CtorFieldContext>,
    /// The contextual `this` type extracted from an object literal's contextual
    /// type (its `ThisType<T>` constituent), staged immediately before checking
    /// a non-arrow method/function-expression property and consumed once by
    /// that function's body so `this` resolves to `T`.
    pub pending_objlit_this: Option<TypeId>,
}

#[derive(Clone, Copy, Debug)]
pub struct NamespaceContext {
    pub fn_depth: usize,
    pub class_depth: usize,
    pub this_container_depth: usize,
}

/// Symbol-usage tracking for unused/assigned diagnostics (TS6133 etc.): symbols
/// referenced and symbols assigned.
#[derive(Default)]
pub struct SymbolUsage {
    pub used_symbols: HashSet<SymbolId>,
    pub assigned_symbols: HashSet<SymbolId>,
}

struct TypeParamUsageGroup<'a> {
    file: usize,
    list_span: Span,
    params: Vec<TypeParamUsageEntry<'a>>,
    refs: Vec<String>,
    refs_only: bool,
}

struct TypeParamUsageEntry<'a> {
    sym: Option<SymbolId>,
    decl: &'a TypeParamDecl,
    name: String,
    span: Span,
    exempt: bool,
}

/// Type-parameter symbols minted by the checker (signature-level params, `infer`
/// bindings, mapped-type keys). These are kept here rather than pushed into the
/// shared `bind.symbols`, so the immutable `BindResult` can be shared across
/// parallel checker threads. Ids are offset by `base` (= `bind.symbols.len()` at
/// construction): a `SymbolId` < base indexes `bind.symbols`; >= base indexes
/// `symbols` here. `symbol()` resolves both tiers transparently.
#[derive(Default)]
pub struct SynthSymbols<'a> {
    pub symbols: Vec<crate::binder::Symbol<'a>>,
    pub base: u32,
    /// node-key -> id for lazily-created signature type params (idempotency),
    /// replacing the `bind.decl_symbol` writes `ensure_type_param_symbol` did.
    pub decl_symbol: std::collections::HashMap<usize, SymbolId>,
    /// transient `TypeParams` scopes minted for signature-level type parameters
    /// (method / call / construct sigs, function-type nodes). Like `symbols`,
    /// kept here so `bind` stays immutable. A `ScopeId` < `scope_base` indexes
    /// `bind.scopes`; >= `scope_base` indexes `scopes` here. `scope_at()`
    /// resolves both tiers; resolving a signature's types under its own scope is
    /// what lets signature type params resolve by ordinary lexical lookup.
    pub scopes: Vec<crate::binder::Scope>,
    pub scope_base: u32,
    /// node-key(type-param decl) -> its transient `TypeParams` scope, so a
    /// signature type param's constraint/default resolves in the scope where it
    /// is declared (outer params visible via the parent chain). Consulted by
    /// `scope_of_decl` ahead of the binder's `decl_scope`.
    pub decl_scope: std::collections::HashMap<usize, ScopeId>,
}

/// Type-relation checking state: the assignability/comparability result cache,
/// the in-progress relation stack (recursion/cycle guard), the depth-overflow
/// flag, and the flag that keeps an error head above missing-property reports.
#[derive(Default)]
pub struct RelationState {
    pub relation_cache: HashMap<(TypeId, TypeId), bool>,
    pub relation_stack: Vec<(TypeId, TypeId)>,
    pub relation_depth_overflow: bool,
    pub keep_head_for_missing: bool,
}

pub struct Checker<'a> {
    pub files: &'a Files,
    pub options: &'a CompilerOptions,
    /// the immutable binder result, shared (`Arc`) across parallel checker
    /// workers; never mutated during checking (Phase 3a).
    pub bind: std::sync::Arc<BindResult<'a>>,
    pub types: TypeTable,
    pub diags: Vec<Diagnostic>,
    pub current_file: usize,

    /// resolution memoization caches (see `ResolutionCaches`)
    pub caches: ResolutionCaches,
    /// res (see `ResolutionState`)
    pub res: ResolutionState,

    /// stacks (see `TraversalStacks`)
    pub stacks: TraversalStacks,

    /// control-flow state (see `FlowState`)
    pub flow: FlowState,

    /// flow-graph resolver state (see `FlowResolve`)
    pub fresolve: FlowResolve,
    /// checked declarations (idempotence for lazy double-checks)
    pub checked_decls: HashSet<usize>,

    /// lib file index (suggestions ordering: lib globals come last)
    pub lib_file: usize,

    /// tp (see `TypeParamCtx`)
    pub tp: TypeParamCtx,

    /// relation machinery (see `RelationState`)
    pub rel: RelationState,
    pub current_scope: ScopeId,
    /// enums (see `EnumState`)
    pub enums: EnumState,
    /// cflags (see `CheckFlags`)
    pub cflags: CheckFlags,
    /// single-report dedup guards (see `ReportGuards`)
    pub reported: ReportGuards,
    /// yield expressions in statement position (no 7057)
    pub yield_statement_positions: HashSet<usize>,
    /// start offset of the property whose initializer is being checked (2729)
    pub prop_init_pos: Option<usize>,
    /// deferred / lazy type-evaluation state (see `DeferredState`)
    pub deferred: DeferredState<'a>,
    /// recursion / DoS guards (see `RecursionGuards`)
    pub guards: RecursionGuards,
    /// `const c = <type-guard expr>`: maps the constant to the boolean-producing
    /// initializer so that `if (c)` narrows the same references the expression
    /// would (tsc's aliased conditional narrowing).
    pub cond_aliases: std::collections::HashMap<crate::binder::SymbolId, &'a Expr>,
    /// symuse (see `SymbolUsage`)
    pub symuse: SymbolUsage,
    /// checker-minted type-param symbols (see `SynthSymbols`); keeps `bind` immutable
    pub synth: SynthSymbols<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EnumValue {
    Number(f64),
    Str(String),
    Computed,
}

/// A narrowable reference: root symbol + property path.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct RefKey(pub SymbolId, pub Vec<String>);

pub fn check<'a>(
    files: &'a Files,
    options: &'a CompilerOptions,
    mut bind: BindResult<'a>,
) -> Vec<Diagnostic> {
    // The binder result is immutable during checking (Phase 3a), so the files of
    // a single program can be checked in parallel: each worker owns a Checker
    // (its own types / caches / synth / diagnostics) but shares one `Arc<bind>`.
    // The only cross-file state the final unused pass needs — used/assigned
    // symbols and evolving-null flow — is accumulated per worker and then
    // unioned. Diagnostics are merged in arbitrary order and later sorted +
    // deduped by the caller, and every per-symbol / file-less diagnostic is
    // emitted at a canonical position, so neither ordering nor identical
    // cross-worker duplicates affect the result. Determinism therefore does not
    // depend on how files are distributed across workers.
    // Tier-2 Stage 0: build the control-flow graph into `bind` while it is still
    // owned/mutable (before the Arc freeze below). Syntax-only; not yet consumed
    // by diagnostics, so output is unchanged. See src/flow_graph.rs.
    crate::flow_graph::build(&mut bind, files);
    let binder_diags = std::mem::take(&mut bind.diags);
    let lib_file = (0..files.len())
        .find(|&i| files[i].0 == crate::LIB_NAME || files[i].0.ends_with("/lib.tsrs.d.ts"))
        .unwrap_or(0);
    let bind = std::sync::Arc::new(bind);
    let n = files.len();

    // Worker count. Intra-fixture parallelism is OPT-IN (TSRS_JOBS>1) and
    // defaults to 1 (serial) so a program's diagnostics are DETERMINISTIC:
    // a single worker owns one type table and interns cross-file types in one
    // order, whereas parallel workers with independent tables intern them in a
    // worker-layout-dependent order — union members are stored sorted by
    // TypeId, so that shifts error-elaboration anchors and some resolution-side-
    // effect diagnostics (see docs/determinism-design.md). Throughput comes from
    // fixture-level batch parallelism (`--check-batch --jobs`), which buffers
    // per-file output in input order and is deterministic. Setting TSRS_JOBS>1
    // opts into intra-fixture parallelism, which is not yet deterministic.
    let jobs = std::env::var("TSRS_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
        .min(n.max(1));

    // Single-worker fast path: check every file with one Checker and run the
    // unused pass on it directly — no thread spawn, no merge. Behaviourally
    // identical to the pre-parallel implementation (and to the parallel path).
    if jobs == 1 {
        let mut c = new_checker(files, options, std::sync::Arc::clone(&bind), lib_file);
        for (i, (_n, _t, ast)) in files.iter().enumerate() {
            c.current_file = i;
            let scope = c.bind.module_scope[&i];
            c.prime_declarator_annotations(&ast.stmts, scope);
            c.check_statements(&ast.stmts, scope);
        }
        c.check_unused();
        let mut diags = std::mem::take(&mut c.diags);
        diags.extend(binder_diags);
        return diags;
    }

    type WorkerOut = (
        Vec<Diagnostic>,
        HashSet<SymbolId>,
        HashSet<SymbolId>,
        HashMap<SymbolId, (usize, Span)>,
    );
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<WorkerOut>>> =
        (0..jobs).map(|_| std::sync::Mutex::new(None)).collect();
    let next_ref = &next;
    let results_ref = &results;
    let bind_ref = &bind;

    std::thread::scope(|s| {
        for w in 0..jobs {
            let bind_w = std::sync::Arc::clone(bind_ref);
            s.spawn(move || {
                let mut c = new_checker(files, options, bind_w, lib_file);
                // Deterministic seed (Stage 1, docs/determinism-design.md): every
                // worker checks the lib file first, so lib globals are interned in
                // the same order — and thus get the same TypeIds — in every worker,
                // regardless of which program files this worker is later assigned.
                // Without it, a worker that checks `main` but not `lib` interns lib
                // types on demand in a different order; union members are stored
                // sorted by TypeId, so error-elaboration anchors shift and the
                // output stops being worker-count-independent. This matches the
                // jobs==1 order (lib is file 0 there). Duplicate lib diagnostics
                // across workers are folded by sort_and_dedupe. (The redundant
                // per-worker lib work is removed in Stage 3 by sharing a frozen
                // resolved base.)
                if let Some(lib_scope) = c.bind.module_scope.get(&lib_file).copied() {
                    c.current_file = lib_file;
                    let lib_ast = &files[lib_file].2;
                    c.prime_declarator_annotations(&lib_ast.stmts, lib_scope);
                    c.check_statements(&lib_ast.stmts, lib_scope);
                }
                loop {
                    let i = next_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    if i == lib_file {
                        continue; // already checked as the deterministic seed
                    }
                    let (_n, _t, ast) = &files[i];
                    c.current_file = i;
                    let scope = c.bind.module_scope[&i];
                    c.prime_declarator_annotations(&ast.stmts, scope);
                    c.check_statements(&ast.stmts, scope);
                }
                *results_ref[w].lock().unwrap() = Some((
                    std::mem::take(&mut c.diags),
                    std::mem::take(&mut c.symuse.used_symbols),
                    std::mem::take(&mut c.symuse.assigned_symbols),
                    std::mem::take(&mut c.flow.auto_fired),
                ));
            });
        }
    });

    // merge per-worker diagnostics and the cross-file usage state
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut used: HashSet<SymbolId> = HashSet::new();
    let mut assigned: HashSet<SymbolId> = HashSet::new();
    let mut autos: HashMap<SymbolId, (usize, Span)> = HashMap::new();
    let bind_symbol_count = bind.symbols.len() as u32;
    for r in &results {
        if let Some((d, u, a, af)) = r.lock().unwrap().take() {
            diags.extend(d);
            used.extend(u.into_iter().filter(|sym| sym.0 < bind_symbol_count));
            assigned.extend(a.into_iter().filter(|sym| sym.0 < bind_symbol_count));
            autos.extend(af);
        }
    }

    // single sequential unused pass over the whole program, with the merged
    // cross-file usage state (a symbol used in one file must not be reported
    // unused because another worker checked its declaration).
    let mut fc = new_checker(files, options, std::sync::Arc::clone(&bind), lib_file);
    fc.symuse.used_symbols = used;
    fc.symuse.assigned_symbols = assigned;
    fc.flow.auto_fired = autos;
    fc.check_unused();
    diags.extend(std::mem::take(&mut fc.diags));

    diags.extend(binder_diags);
    diags
}

/// Build a fresh `Checker` sharing the immutable `bind`. All mutable state is
/// per-Checker so multiple of these can run on disjoint files in parallel.
fn new_checker<'a>(
    files: &'a Files,
    options: &'a CompilerOptions,
    bind: std::sync::Arc<BindResult<'a>>,
    lib_file: usize,
) -> Checker<'a> {
    let synth_base = bind.symbols.len() as u32;
    let synth_scope_base = bind.scopes.len() as u32;
    Checker {
        files,
        options,
        bind,
        types: TypeTable::new(),
        diags: Vec::new(),
        current_file: 0,
        caches: ResolutionCaches::default(),
        res: ResolutionState::default(),
        stacks: TraversalStacks::default(),
        flow: FlowState {
            facts: vec![HashMap::new()],
            ..Default::default()
        },
        fresolve: FlowResolve::default(),
        checked_decls: HashSet::new(),
        lib_file,
        tp: TypeParamCtx::default(),
        rel: RelationState::default(),
        current_scope: ScopeId(0),
        enums: EnumState::default(),
        cflags: CheckFlags::default(),
        reported: ReportGuards::default(),
        yield_statement_positions: HashSet::new(),
        prop_init_pos: None,
        deferred: DeferredState::default(),
        guards: RecursionGuards::default(),
        cond_aliases: std::collections::HashMap::new(),
        symuse: SymbolUsage::default(),
        synth: SynthSymbols {
            base: synth_base,
            scope_base: synth_scope_base,
            ..Default::default()
        },
    }
}

impl<'a> Checker<'a> {
    fn check_unused(&mut self) {
        // 7034 heads for auto (CFA-typed) variables whose capture reads
        // produced 7005
        let mut auto_fired: Vec<(SymbolId, (usize, Span))> =
            self.flow.auto_fired.iter().map(|(s, l)| (*s, *l)).collect();
        auto_fired.sort_by_key(|(s, _)| s.0);
        for (sym, (file, span)) in auto_fired {
            let name = self.symbol(sym).name.clone();
            let prev = self.current_file;
            self.current_file = file;
            self.error_at(
                span,
                &gen::Variable_0_implicitly_has_type_1_in_some_locations_where_its_type_cannot_be_determined,
                &[name, "any".to_string()],
            );
            self.current_file = prev;
        }
        // Unused checks always run: emitted as errors when the corresponding
        // flag is set, otherwise as suggestions (tsc getSuggestionDiagnostics).
        self.check_unused_imports();
        self.check_unused_private_members();
        self.check_unused_type_params();
        self.check_unused_infer_params();
        self.check_unused_groups();
        let syntactic_class_uses = self.collect_syntactic_class_type_uses();
        self.symuse.used_symbols.extend(syntactic_class_uses);
        let no_locals = self.options.no_unused_locals;
        let no_params = self.options.no_unused_parameters;
        for i in 0..self.bind.symbols.len() {
            let sym = SymbolId(i as u32);
            let s = &self.bind.symbols[i];
            if s.file == self.lib_file || self.symuse.used_symbols.contains(&sym) {
                continue;
            }
            if self.files[s.file].0.ends_with(".d.ts") {
                continue;
            }
            if self.bind.ambient_context_symbols.contains(&sym) {
                continue;
            }
            if self.is_exported_namespace_member(sym) || self.has_exported_module_declaration(s) {
                continue;
            }
            let is_param = s.flags & flags::PARAMETER != 0;
            let is_local_var =
                s.flags & (flags::FUNCTION_SCOPED_VARIABLE | flags::BLOCK_SCOPED_VARIABLE) != 0
                    && !is_param;
            let is_type_decl = s.flags & (flags::INTERFACE | flags::TYPE_ALIAS) != 0
                || self.has_unused_class_declaration(s);
            let is_function = s.flags & flags::FUNCTION != 0;
            if !(is_param || is_local_var || is_type_decl || is_function) {
                continue;
            }
            let as_error = if is_param { no_params } else { no_locals };
            // tsc exempts leading-underscore names from the unused check for
            // *parameters* only; `_`-prefixed locals/types are still reported.
            if s.name == "this" {
                continue;
            }
            if is_param && s.name.starts_with('_') {
                continue;
            }
            let Some(decl) = s.decls.first().copied() else {
                continue;
            };
            // only function-contained locals (top-level script vars are global)
            let decl_key = match decl {
                crate::binder::Decl::Var(d, _) => crate::ast::node_key(d),
                crate::binder::Decl::Param(p) => crate::ast::node_key(p),
                crate::binder::Decl::Interface(i) => crate::ast::node_key(i),
                crate::binder::Decl::Alias(a) => crate::ast::node_key(a),
                crate::binder::Decl::Func(fl) => crate::ast::node_key(fl),
                crate::binder::Decl::Class(c) if self.is_reportable_unused_class_declaration(c) => {
                    crate::ast::node_key(c)
                }
                _ => continue,
            };
            if self.is_direct_static_block_declaration(decl_key) {
                continue;
            }
            let in_fn = self
                .bind
                .decl_container
                .get(&decl_key)
                .copied()
                .unwrap_or(0)
                != 0;
            // Parameters of a function with no body (overload signatures, ambient
            // declarations, function-type members) are declarations only; tsc
            // does not unused-check them.
            if is_param {
                // a parameter property (`constructor(public x: number)`) declares
                // a class member, not just a parameter; tsc checks it as a member
                // (via the private-member pass), never as an unused parameter.
                if let crate::binder::Decl::Param(p) = decl {
                    use crate::ast::{has_modifier, ModifierKind as MK};
                    if has_modifier(&p.modifiers, MK::Public)
                        || has_modifier(&p.modifiers, MK::Private)
                        || has_modifier(&p.modifiers, MK::Protected)
                        || has_modifier(&p.modifiers, MK::Readonly)
                    {
                        continue;
                    }
                }
                let fn_key = self
                    .bind
                    .decl_container
                    .get(&decl_key)
                    .copied()
                    .unwrap_or(0);
                if let Some(f) = self.bind.fn_decls.get(&fn_key) {
                    if f.body.is_none() {
                        continue;
                    }
                }
            }
            let file_is_module = self.files[s.file].2.is_module;
            // tsc exempts from the unused check only declarations directly in the
            // global scope of a *script* file (they become globals). Anything
            // nested inside a block or function is a local and is checked even in
            // a script.
            let in_global_top = {
                let ds = self.bind.decl_scope.get(&decl_key).copied();
                let ms = self.bind.module_scope.get(&s.file).copied();
                ds.is_some() && ds == ms
            };
            let _ = in_fn;
            if in_global_top && !file_is_module {
                continue;
            }
            // exported?
            if let Some(exports) = self.bind.exports.get(&s.file) {
                if exports.iter().any(|(_, e)| *e == sym) {
                    continue;
                }
            }
            let name = s.name.clone();
            let file = s.file;
            // tsc reports an unused function on *each* of its declarations
            // (every overload signature plus the implementation).
            let spans: Vec<crate::ast::Span> = if is_function {
                s.decls
                    .iter()
                    .filter(|d| matches!(d, crate::binder::Decl::Func(_)))
                    .map(|d| d.name_span())
                    .collect()
            } else if is_type_decl {
                let spans = self.unused_type_decl_spans(s);
                if spans.is_empty() {
                    continue;
                }
                spans
            } else {
                vec![decl.name_span()]
            };
            let prev = self.current_file;
            self.current_file = file;
            for span in spans {
                if is_type_decl {
                    self.unused_diag(
                        span,
                        &gen::_0_is_declared_but_never_used,
                        &[name.clone()],
                        as_error,
                    );
                } else {
                    self.unused_diag(
                        span,
                        &gen::_0_is_declared_but_its_value_is_never_read,
                        &[name.clone()],
                        as_error,
                    );
                }
            }
            self.current_file = prev;
        }
    }

    fn is_exported_namespace_member(&self, sym: SymbolId) -> bool {
        self.bind.symbols.iter().any(|ns| {
            ns.flags & flags::NAMESPACE != 0
                && (ns.members.iter().any(|(_, member)| *member == sym)
                    || ns.statics.iter().any(|(_, member)| *member == sym))
        })
    }

    fn collect_syntactic_class_type_uses(&self) -> HashSet<SymbolId> {
        let mut out = HashSet::new();
        for (file, (_, _, ast)) in self.files.iter().enumerate() {
            let scope = self
                .bind
                .module_scope
                .get(&file)
                .copied()
                .unwrap_or(self.bind.global_scope);
            self.collect_class_type_uses_in_stmts(&ast.stmts, scope, &mut out);
        }
        out
    }

    fn collect_class_type_uses_in_stmts(
        &self,
        stmts: &'a [Stmt],
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        for stmt in stmts {
            self.collect_class_type_uses_in_stmt(stmt, scope, out);
        }
    }

    fn collect_class_type_uses_in_stmt(
        &self,
        stmt: &'a Stmt,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        match stmt {
            Stmt::Var(v) => {
                for decl in &v.decls {
                    if let Some(ty) = &decl.ty {
                        self.collect_class_type_uses_in_type(ty, scope, out);
                    }
                    if let Some(init) = &decl.init {
                        self.collect_class_type_uses_in_expr(init, scope, out);
                    }
                }
            }
            Stmt::Func(f) => self.collect_class_type_uses_in_function_like(f, scope, out),
            Stmt::Class(c) => self.collect_class_type_uses_in_class(c, scope, out),
            Stmt::Interface(i) => {
                let member_scope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**i))
                    .copied()
                    .unwrap_or(scope);
                if let Some(tps) = &i.type_params {
                    for tp in tps {
                        if let Some(constraint) = &tp.constraint {
                            self.collect_class_type_uses_in_type(constraint, member_scope, out);
                        }
                        if let Some(default) = &tp.default {
                            self.collect_class_type_uses_in_type(default, member_scope, out);
                        }
                    }
                }
                for ext in &i.extends {
                    self.collect_class_type_uses_in_type_ref(ext, member_scope, out);
                }
                for member in &i.members {
                    self.collect_class_type_uses_in_type_member(member, member_scope, out);
                }
            }
            Stmt::TypeAlias(t) => {
                let alias_scope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**t))
                    .copied()
                    .unwrap_or(scope);
                if let Some(tps) = &t.type_params {
                    for tp in tps {
                        if let Some(constraint) = &tp.constraint {
                            self.collect_class_type_uses_in_type(constraint, alias_scope, out);
                        }
                        if let Some(default) = &tp.default {
                            self.collect_class_type_uses_in_type(default, alias_scope, out);
                        }
                    }
                }
                self.collect_class_type_uses_in_type(&t.ty, alias_scope, out);
            }
            Stmt::Namespace(n) => {
                let ns_scope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**n))
                    .copied()
                    .unwrap_or(scope);
                self.collect_class_type_uses_in_stmts(&n.body, ns_scope, out);
            }
            Stmt::Block(b) => {
                let block_scope = self
                    .bind
                    .node_scope
                    .get(&node_key(b))
                    .copied()
                    .unwrap_or(scope);
                self.collect_class_type_uses_in_stmts(&b.stmts, block_scope, out);
            }
            Stmt::Expr { expr, .. } | Stmt::Throw { expr, .. } => {
                self.collect_class_type_uses_in_expr(expr, scope, out);
            }
            Stmt::Return {
                expr: Some(expr), ..
            } => {
                self.collect_class_type_uses_in_expr(expr, scope, out);
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.collect_class_type_uses_in_expr(cond, scope, out);
                self.collect_class_type_uses_in_stmt(then, scope, out);
                if let Some(els) = els {
                    self.collect_class_type_uses_in_stmt(els, scope, out);
                }
            }
            Stmt::While { cond, body, .. } | Stmt::DoWhile { cond, body, .. } => {
                self.collect_class_type_uses_in_expr(cond, scope, out);
                self.collect_class_type_uses_in_stmt(body, scope, out);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                if let Some(init) = init {
                    match &**init {
                        ForInit::Var(v) => {
                            for decl in &v.decls {
                                if let Some(ty) = &decl.ty {
                                    self.collect_class_type_uses_in_type(ty, scope, out);
                                }
                                if let Some(init) = &decl.init {
                                    self.collect_class_type_uses_in_expr(init, scope, out);
                                }
                            }
                        }
                        ForInit::Expr(expr) => {
                            self.collect_class_type_uses_in_expr(expr, scope, out)
                        }
                    }
                }
                if let Some(cond) = cond {
                    self.collect_class_type_uses_in_expr(cond, scope, out);
                }
                if let Some(incr) = incr {
                    self.collect_class_type_uses_in_expr(incr, scope, out);
                }
                self.collect_class_type_uses_in_stmt(body, scope, out);
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                if let ForInit::Var(v) = &**left {
                    for decl in &v.decls {
                        if let Some(ty) = &decl.ty {
                            self.collect_class_type_uses_in_type(ty, scope, out);
                        }
                    }
                }
                self.collect_class_type_uses_in_expr(expr, scope, out);
                self.collect_class_type_uses_in_stmt(body, scope, out);
            }
            Stmt::With { obj, body, .. } => {
                self.collect_class_type_uses_in_expr(obj, scope, out);
                self.collect_class_type_uses_in_stmt(body, scope, out);
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                self.collect_class_type_uses_in_stmts(&block.stmts, scope, out);
                if let Some(catch) = catch {
                    self.collect_class_type_uses_in_stmts(&catch.block.stmts, scope, out);
                }
                if let Some(finally) = finally {
                    self.collect_class_type_uses_in_stmts(&finally.stmts, scope, out);
                }
            }
            Stmt::Switch { expr, cases, .. } => {
                self.collect_class_type_uses_in_expr(expr, scope, out);
                for case in cases {
                    self.collect_class_type_uses_in_stmts(&case.stmts, scope, out);
                }
            }
            Stmt::Labeled { stmt, .. } => self.collect_class_type_uses_in_stmt(stmt, scope, out),
            Stmt::Enum(_)
            | Stmt::Empty { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Return { expr: None, .. }
            | Stmt::Import(_)
            | Stmt::ExportNamed(_)
            | Stmt::ExportDefault { .. }
            | Stmt::ExportAssign { .. }
            | Stmt::ImportEquals { .. }
            | Stmt::Missing { .. } => {}
        }
    }

    fn collect_class_type_uses_in_class(
        &self,
        class: &'a ClassDecl,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        let class_scope = self
            .bind
            .node_scope
            .get(&node_key(class))
            .copied()
            .unwrap_or(scope);
        if let Some(tps) = &class.type_params {
            for tp in tps {
                if let Some(constraint) = &tp.constraint {
                    self.collect_class_type_uses_in_type(constraint, class_scope, out);
                }
                if let Some(default) = &tp.default {
                    self.collect_class_type_uses_in_type(default, class_scope, out);
                }
            }
        }
        if let Some(ext) = &class.extends {
            self.collect_class_type_uses_in_expr(&ext.expr, class_scope, out);
            if let Some(args) = &ext.type_args {
                for arg in args {
                    self.collect_class_type_uses_in_type(arg, class_scope, out);
                }
            }
        }
        for imp in &class.implements {
            self.collect_class_type_uses_in_type_ref(imp, class_scope, out);
        }
        for member in &class.members {
            self.collect_class_type_uses_in_class_member(member, class_scope, out);
        }
    }

    fn collect_class_type_uses_in_function_like(
        &self,
        f: &'a FunctionLike,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        let fn_scope = self
            .bind
            .node_scope
            .get(&node_key(f))
            .copied()
            .unwrap_or(scope);
        if let Some(tps) = &f.type_params {
            for tp in tps {
                if let Some(constraint) = &tp.constraint {
                    self.collect_class_type_uses_in_type(constraint, fn_scope, out);
                }
                if let Some(default) = &tp.default {
                    self.collect_class_type_uses_in_type(default, fn_scope, out);
                }
            }
        }
        for param in &f.params {
            if let Some(ty) = &param.ty {
                self.collect_class_type_uses_in_type(ty, fn_scope, out);
            }
            if let Some(init) = &param.initializer {
                self.collect_class_type_uses_in_expr(init, fn_scope, out);
            }
        }
        if let Some(ret) = &f.return_type {
            self.collect_class_type_uses_in_type(ret, fn_scope, out);
        }
        if let Some(body) = &f.body {
            match body {
                FuncBody::Block(block) => {
                    self.collect_class_type_uses_in_stmts(&block.stmts, fn_scope, out)
                }
                FuncBody::Expr(expr) => self.collect_class_type_uses_in_expr(expr, fn_scope, out),
            }
        }
    }

    fn collect_class_type_uses_in_signature_parts(
        &self,
        type_params: Option<&'a Vec<TypeParamDecl>>,
        params: &'a [Param],
        return_type: Option<&'a TypeNode>,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        if let Some(tps) = type_params {
            for tp in tps {
                if let Some(constraint) = &tp.constraint {
                    self.collect_class_type_uses_in_type(constraint, scope, out);
                }
                if let Some(default) = &tp.default {
                    self.collect_class_type_uses_in_type(default, scope, out);
                }
            }
        }
        for param in params {
            if let Some(ty) = &param.ty {
                self.collect_class_type_uses_in_type(ty, scope, out);
            }
        }
        if let Some(return_type) = return_type {
            self.collect_class_type_uses_in_type(return_type, scope, out);
        }
    }

    fn collect_class_type_uses_in_class_member(
        &self,
        member: &'a ClassMember,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        match member {
            ClassMember::Property(p) => {
                if let Some(ty) = &p.ty {
                    self.collect_class_type_uses_in_type(ty, scope, out);
                }
                if let Some(init) = &p.init {
                    self.collect_class_type_uses_in_expr(init, scope, out);
                }
            }
            ClassMember::Method(f) | ClassMember::Constructor(f) => {
                self.collect_class_type_uses_in_function_like(f, scope, out);
            }
            ClassMember::Index(ix) => {
                self.collect_class_type_uses_in_type(&ix.key_type, scope, out);
                self.collect_class_type_uses_in_type(&ix.value_type, scope, out);
            }
            ClassMember::StaticBlock(block) => {
                let block_scope = self
                    .bind
                    .node_scope
                    .get(&node_key(block))
                    .copied()
                    .unwrap_or(scope);
                self.collect_class_type_uses_in_stmts(&block.stmts, block_scope, out);
            }
        }
    }

    fn collect_class_type_uses_in_type_member(
        &self,
        member: &'a TypeMember,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        match member {
            TypeMember::Prop(p) => {
                if let Some(ty) = &p.ty {
                    self.collect_class_type_uses_in_type(ty, scope, out);
                }
            }
            TypeMember::Method(m) => {
                self.collect_class_type_uses_in_signature_parts(
                    m.type_params.as_ref(),
                    &m.params,
                    m.return_type.as_ref(),
                    scope,
                    out,
                );
            }
            TypeMember::Call(f) | TypeMember::Ctor(f) => {
                self.collect_class_type_uses_in_signature_parts(
                    f.type_params.as_ref(),
                    &f.params,
                    f.return_type.as_ref(),
                    scope,
                    out,
                );
            }
            TypeMember::Index(ix) => {
                self.collect_class_type_uses_in_type(&ix.key_type, scope, out);
                self.collect_class_type_uses_in_type(&ix.value_type, scope, out);
            }
        }
    }

    fn collect_class_type_uses_in_expr(
        &self,
        expr: &'a Expr,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        match expr {
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.collect_class_type_uses_in_function_like(f, scope, out);
            }
            Expr::ClassExpr(c) => self.collect_class_type_uses_in_class(c, scope, out),
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                self.collect_class_type_uses_in_expr(callee, scope, out);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_class_type_uses_in_type(arg, scope, out);
                    }
                }
                for arg in args {
                    self.collect_class_type_uses_in_expr(arg, scope, out);
                }
            }
            Expr::New {
                callee,
                type_args,
                args,
                ..
            } => {
                self.collect_class_type_uses_in_expr(callee, scope, out);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_class_type_uses_in_type(arg, scope, out);
                    }
                }
                if let Some(args) = args {
                    for arg in args {
                        self.collect_class_type_uses_in_expr(arg, scope, out);
                    }
                }
            }
            Expr::Assertion { expr, ty, .. } => {
                self.collect_class_type_uses_in_expr(expr, scope, out);
                self.collect_class_type_uses_in_type(ty, scope, out);
            }
            Expr::Array { elements, .. } => {
                for elem in elements {
                    self.collect_class_type_uses_in_expr(elem, scope, out);
                }
            }
            Expr::Object { props, .. } => {
                for prop in props {
                    match prop {
                        ObjectProp::Property { value, .. } => {
                            self.collect_class_type_uses_in_expr(value, scope, out)
                        }
                        ObjectProp::Spread { expr, .. } => {
                            self.collect_class_type_uses_in_expr(expr, scope, out)
                        }
                        ObjectProp::Method(f) => {
                            self.collect_class_type_uses_in_function_like(f, scope, out)
                        }
                        ObjectProp::Shorthand { .. } => {}
                    }
                }
            }
            Expr::Template { parts, .. } => {
                for part in parts {
                    if let TemplatePart::Expr(expr) = part {
                        self.collect_class_type_uses_in_expr(expr, scope, out);
                    }
                }
            }
            Expr::PropAccess { obj, .. }
            | Expr::Unary { operand: obj, .. }
            | Expr::Update { operand: obj, .. }
            | Expr::Paren { inner: obj, .. }
            | Expr::NonNull { expr: obj, .. }
            | Expr::Spread { expr: obj, .. }
            | Expr::Await { expr: obj, .. } => {
                self.collect_class_type_uses_in_expr(obj, scope, out);
            }
            Expr::ElemAccess { obj, index, .. } => {
                self.collect_class_type_uses_in_expr(obj, scope, out);
                self.collect_class_type_uses_in_expr(index, scope, out);
            }
            Expr::Binary { left, right, .. } => {
                self.collect_class_type_uses_in_expr(left, scope, out);
                self.collect_class_type_uses_in_expr(right, scope, out);
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                self.collect_class_type_uses_in_expr(cond, scope, out);
                self.collect_class_type_uses_in_expr(when_true, scope, out);
                self.collect_class_type_uses_in_expr(when_false, scope, out);
            }
            Expr::Yield {
                expr: Some(expr), ..
            } => self.collect_class_type_uses_in_expr(expr, scope, out),
            Expr::ImportCall { args, .. } => {
                for arg in args {
                    self.collect_class_type_uses_in_expr(arg, scope, out);
                }
            }
            Expr::JsxElement(_)
            | Expr::Ident(_)
            | Expr::NumLit { .. }
            | Expr::StrLit { .. }
            | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. }
            | Expr::NullLit { .. }
            | Expr::RegexLit { .. }
            | Expr::This { .. }
            | Expr::Super { .. }
            | Expr::Yield { expr: None, .. }
            | Expr::ImportMeta { .. }
            | Expr::Missing { .. } => {}
        }
    }

    fn collect_class_type_uses_in_type(
        &self,
        ty: &'a TypeNode,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        match ty {
            TypeNode::Ref(r) => self.collect_class_type_uses_in_type_ref(r, scope, out),
            TypeNode::TypeQuery {
                name, type_args, ..
            } => {
                if name.parts.len() == 1 {
                    if let Some(sym) = self.lookup_value(scope, &name.parts[0].name) {
                        self.mark_if_class(sym, out);
                    }
                }
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_class_type_uses_in_type(arg, scope, out);
                    }
                }
            }
            TypeNode::Array { elem, .. }
            | TypeNode::Paren { inner: elem, .. }
            | TypeNode::Keyof { ty: elem, .. }
            | TypeNode::ReadonlyOp { ty: elem, .. } => {
                self.collect_class_type_uses_in_type(elem, scope, out);
            }
            TypeNode::Tuple { elems, .. } => {
                for elem in elems {
                    self.collect_class_type_uses_in_type(&elem.ty, scope, out);
                }
            }
            TypeNode::Union { members, .. } | TypeNode::Intersection { members, .. } => {
                for member in members {
                    self.collect_class_type_uses_in_type(member, scope, out);
                }
            }
            TypeNode::Function(f) | TypeNode::Ctor(f) => {
                self.collect_class_type_uses_in_signature_parts(
                    f.type_params.as_ref(),
                    &f.params,
                    Some(&f.return_type),
                    scope,
                    out,
                );
            }
            TypeNode::TypeLiteral { members, .. } => {
                for member in members {
                    self.collect_class_type_uses_in_type_member(member, scope, out);
                }
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                self.collect_class_type_uses_in_type(obj, scope, out);
                self.collect_class_type_uses_in_type(index, scope, out);
            }
            TypeNode::Conditional(c) => {
                self.collect_class_type_uses_in_type(&c.check, scope, out);
                self.collect_class_type_uses_in_type(&c.extends_ty, scope, out);
                self.collect_class_type_uses_in_type(&c.true_ty, scope, out);
                self.collect_class_type_uses_in_type(&c.false_ty, scope, out);
            }
            TypeNode::Predicate { ty: Some(ty), .. } => {
                self.collect_class_type_uses_in_type(ty, scope, out);
            }
            TypeNode::Mapped(m) => {
                self.collect_class_type_uses_in_type(&m.constraint, scope, out);
                if let Some(name_type) = &m.name_type {
                    self.collect_class_type_uses_in_type(name_type, scope, out);
                }
                if let Some(value) = &m.value {
                    self.collect_class_type_uses_in_type(value, scope, out);
                }
            }
            TypeNode::TemplateLit { parts, .. } => {
                for (part, _) in parts {
                    self.collect_class_type_uses_in_type(part, scope, out);
                }
            }
            TypeNode::Infer {
                constraint: Some(constraint),
                ..
            } => self.collect_class_type_uses_in_type(constraint, scope, out),
            TypeNode::Keyword(..)
            | TypeNode::This(_)
            | TypeNode::LiteralString { .. }
            | TypeNode::LiteralNumber { .. }
            | TypeNode::LiteralBigInt { .. }
            | TypeNode::LiteralBool { .. }
            | TypeNode::Predicate { ty: None, .. }
            | TypeNode::Infer {
                constraint: None, ..
            } => {}
        }
    }

    fn collect_class_type_uses_in_type_ref(
        &self,
        r: &'a TypeRef,
        scope: ScopeId,
        out: &mut HashSet<SymbolId>,
    ) {
        if r.name.parts.len() == 1 {
            if let Some(sym) = self.lookup_type(scope, &r.name.parts[0].name) {
                self.mark_if_class(sym, out);
            }
        } else if let Some(ns) = self
            .lookup_type(scope, &r.name.parts[0].name)
            .or_else(|| self.lookup_value(scope, &r.name.parts[0].name))
        {
            if self.bind.symbols[ns.0 as usize].flags & flags::NAMESPACE != 0 {
                if let Some(member) = self.bind.symbols[ns.0 as usize]
                    .statics
                    .get(&r.name.parts[1].name)
                {
                    self.mark_if_class(member, out);
                }
            }
        }
        if let Some(args) = &r.type_args {
            for arg in args {
                self.collect_class_type_uses_in_type(arg, scope, out);
            }
        }
    }

    fn mark_if_class(&self, sym: SymbolId, out: &mut HashSet<SymbolId>) {
        if self.bind.symbols[sym.0 as usize].flags & flags::CLASS != 0 {
            out.insert(sym);
        }
    }

    fn has_exported_module_declaration(&self, symbol: &crate::binder::Symbol<'a>) -> bool {
        symbol.decls.iter().any(|decl| match decl {
            crate::binder::Decl::Class(c) if has_modifier(&c.modifiers, ModifierKind::Export) => {
                self.decl_scope_is_module(Self::decl_key(decl))
            }
            crate::binder::Decl::Interface(i)
                if has_modifier(&i.modifiers, ModifierKind::Export) =>
            {
                self.decl_scope_is_module(Self::decl_key(decl))
            }
            crate::binder::Decl::Alias(a) if has_modifier(&a.modifiers, ModifierKind::Export) => {
                self.decl_scope_is_module(Self::decl_key(decl))
            }
            crate::binder::Decl::Enum(e) if has_modifier(&e.modifiers, ModifierKind::Export) => {
                self.decl_scope_is_module(Self::decl_key(decl))
            }
            crate::binder::Decl::Namespace(n)
                if has_modifier(&n.modifiers, ModifierKind::Export) =>
            {
                self.decl_scope_is_module(Self::decl_key(decl))
            }
            _ => false,
        })
    }

    fn decl_scope_is_module(&self, key: Option<usize>) -> bool {
        key.and_then(|key| self.bind.decl_scope.get(&key).copied())
            .is_some_and(|scope| {
                matches!(
                    self.bind.scopes[scope.0 as usize].kind,
                    crate::binder::ScopeKind::Module
                )
            })
    }

    fn is_statement_class_declaration(&self, class: &'a crate::ast::ClassDecl) -> bool {
        self.bind
            .decl_container
            .contains_key(&crate::ast::node_key(class))
    }

    fn is_reportable_unused_class_declaration(&self, class: &'a crate::ast::ClassDecl) -> bool {
        self.is_statement_class_declaration(class) && !self.class_suppresses_unused(class)
    }

    fn class_suppresses_unused(&self, class: &'a crate::ast::ClassDecl) -> bool {
        class
            .decorators
            .iter()
            .any(|decorator| decorator.span.end > decorator.expr.span().end)
            || self.class_source_contains_super_type_args(class)
            || class.members.iter().any(|member| match member {
                crate::ast::ClassMember::StaticBlock(block) => {
                    Self::block_has_operandless_await(block)
                }
                crate::ast::ClassMember::Method(f) | crate::ast::ClassMember::Constructor(f) => {
                    Self::function_has_super_call_type_args(f)
                }
                crate::ast::ClassMember::Property(p) => p
                    .init
                    .as_ref()
                    .is_some_and(Self::expr_has_super_call_type_args),
                _ => false,
            })
    }

    fn class_source_contains_super_type_args(&self, class: &'a crate::ast::ClassDecl) -> bool {
        let Some(file) = self.bind.decl_file.get(&node_key(class)).copied() else {
            return false;
        };
        let text = &self.files[file].1.text;
        let start = class.span.start as usize;
        let end = (class.span.end as usize).min(text.len());
        text.get(start..end)
            .is_some_and(|slice| slice.contains("super<"))
    }

    fn function_has_super_call_type_args(f: &crate::ast::FunctionLike) -> bool {
        match &f.body {
            Some(crate::ast::FuncBody::Block(block)) => Self::block_has_super_call_type_args(block),
            Some(crate::ast::FuncBody::Expr(expr)) => Self::expr_has_super_call_type_args(expr),
            None => false,
        }
    }

    fn block_has_super_call_type_args(block: &crate::ast::Block) -> bool {
        block.stmts.iter().any(Self::stmt_has_super_call_type_args)
    }

    fn stmt_has_super_call_type_args(stmt: &crate::ast::Stmt) -> bool {
        match stmt {
            crate::ast::Stmt::Expr { expr, .. }
            | crate::ast::Stmt::Throw { expr, .. }
            | crate::ast::Stmt::ExportDefault { expr, .. }
            | crate::ast::Stmt::ExportAssign { expr, .. } => {
                Self::expr_has_super_call_type_args(expr)
            }
            crate::ast::Stmt::Return {
                expr: Some(expr), ..
            } => Self::expr_has_super_call_type_args(expr),
            crate::ast::Stmt::Var(v) => v.decls.iter().any(|d| {
                d.init
                    .as_ref()
                    .is_some_and(Self::expr_has_super_call_type_args)
            }),
            crate::ast::Stmt::Block(block) => Self::block_has_super_call_type_args(block),
            crate::ast::Stmt::If {
                cond, then, els, ..
            } => {
                Self::expr_has_super_call_type_args(cond)
                    || Self::stmt_has_super_call_type_args(then)
                    || els
                        .as_deref()
                        .is_some_and(Self::stmt_has_super_call_type_args)
            }
            crate::ast::Stmt::While { cond, body, .. }
            | crate::ast::Stmt::DoWhile { cond, body, .. } => {
                Self::expr_has_super_call_type_args(cond)
                    || Self::stmt_has_super_call_type_args(body)
            }
            crate::ast::Stmt::For {
                cond, incr, body, ..
            } => {
                cond.as_ref()
                    .is_some_and(Self::expr_has_super_call_type_args)
                    || incr
                        .as_ref()
                        .is_some_and(Self::expr_has_super_call_type_args)
                    || Self::stmt_has_super_call_type_args(body)
            }
            crate::ast::Stmt::ForIn { expr, body, .. }
            | crate::ast::Stmt::ForOf { expr, body, .. }
            | crate::ast::Stmt::With {
                obj: expr, body, ..
            } => {
                Self::expr_has_super_call_type_args(expr)
                    || Self::stmt_has_super_call_type_args(body)
            }
            crate::ast::Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                Self::block_has_super_call_type_args(block)
                    || catch
                        .as_ref()
                        .is_some_and(|c| Self::block_has_super_call_type_args(&c.block))
                    || finally
                        .as_ref()
                        .is_some_and(Self::block_has_super_call_type_args)
            }
            crate::ast::Stmt::Switch { expr, cases, .. } => {
                Self::expr_has_super_call_type_args(expr)
                    || cases
                        .iter()
                        .any(|case| case.stmts.iter().any(Self::stmt_has_super_call_type_args))
            }
            crate::ast::Stmt::Labeled { stmt, .. } => Self::stmt_has_super_call_type_args(stmt),
            _ => false,
        }
    }

    fn expr_has_super_call_type_args(expr: &crate::ast::Expr) -> bool {
        match expr {
            crate::ast::Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                (type_args.is_some() && matches!(&**callee, crate::ast::Expr::Super { .. }))
                    || Self::expr_has_super_call_type_args(callee)
                    || args.iter().any(Self::expr_has_super_call_type_args)
            }
            crate::ast::Expr::New {
                callee,
                type_args: _,
                args,
                ..
            } => {
                Self::expr_has_super_call_type_args(callee)
                    || args
                        .as_ref()
                        .is_some_and(|args| args.iter().any(Self::expr_has_super_call_type_args))
            }
            crate::ast::Expr::Array { elements, .. } => {
                elements.iter().any(Self::expr_has_super_call_type_args)
            }
            crate::ast::Expr::Object { props, .. } => props.iter().any(|prop| match prop {
                crate::ast::ObjectProp::Property { value, .. } => {
                    Self::expr_has_super_call_type_args(value)
                }
                crate::ast::ObjectProp::Spread { expr, .. } => {
                    Self::expr_has_super_call_type_args(expr)
                }
                crate::ast::ObjectProp::Method(f) => Self::function_has_super_call_type_args(f),
                crate::ast::ObjectProp::Shorthand { .. } => false,
            }),
            crate::ast::Expr::Arrow(f) | crate::ast::Expr::FunctionExpr(f) => {
                Self::function_has_super_call_type_args(f)
            }
            crate::ast::Expr::ClassExpr(c) => c.members.iter().any(|m| match m {
                crate::ast::ClassMember::Method(f) | crate::ast::ClassMember::Constructor(f) => {
                    Self::function_has_super_call_type_args(f)
                }
                crate::ast::ClassMember::Property(p) => p
                    .init
                    .as_ref()
                    .is_some_and(Self::expr_has_super_call_type_args),
                crate::ast::ClassMember::StaticBlock(b) => Self::block_has_super_call_type_args(b),
                _ => false,
            }),
            crate::ast::Expr::PropAccess { obj, .. }
            | crate::ast::Expr::Unary { operand: obj, .. }
            | crate::ast::Expr::Update { operand: obj, .. }
            | crate::ast::Expr::Paren { inner: obj, .. }
            | crate::ast::Expr::Assertion { expr: obj, .. }
            | crate::ast::Expr::NonNull { expr: obj, .. }
            | crate::ast::Expr::Spread { expr: obj, .. }
            | crate::ast::Expr::Await { expr: obj, .. } => Self::expr_has_super_call_type_args(obj),
            crate::ast::Expr::ElemAccess { obj, index, .. } => {
                Self::expr_has_super_call_type_args(obj)
                    || Self::expr_has_super_call_type_args(index)
            }
            crate::ast::Expr::Binary { left, right, .. } => {
                Self::expr_has_super_call_type_args(left)
                    || Self::expr_has_super_call_type_args(right)
            }
            crate::ast::Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                Self::expr_has_super_call_type_args(cond)
                    || Self::expr_has_super_call_type_args(when_true)
                    || Self::expr_has_super_call_type_args(when_false)
            }
            crate::ast::Expr::Template { parts, .. } => parts.iter().any(|part| match part {
                crate::ast::TemplatePart::Expr(expr) => Self::expr_has_super_call_type_args(expr),
                crate::ast::TemplatePart::Str(_) => false,
            }),
            crate::ast::Expr::Yield {
                expr: Some(expr), ..
            } => Self::expr_has_super_call_type_args(expr),
            crate::ast::Expr::ImportCall { args, .. } => {
                args.iter().any(Self::expr_has_super_call_type_args)
            }
            _ => false,
        }
    }

    fn block_has_operandless_await(block: &crate::ast::Block) -> bool {
        block.stmts.iter().any(Self::stmt_has_operandless_await)
    }

    fn stmt_has_operandless_await(stmt: &crate::ast::Stmt) -> bool {
        match stmt {
            crate::ast::Stmt::Expr {
                expr: crate::ast::Expr::Ident(id),
                ..
            }
            | crate::ast::Stmt::Labeled { label: id, .. }
                if id.name == "await" =>
            {
                true
            }
            crate::ast::Stmt::Block(block) => Self::block_has_operandless_await(block),
            crate::ast::Stmt::If { then, els, .. } => {
                Self::stmt_has_operandless_await(then)
                    || els.as_deref().is_some_and(Self::stmt_has_operandless_await)
            }
            crate::ast::Stmt::While { body, .. }
            | crate::ast::Stmt::DoWhile { body, .. }
            | crate::ast::Stmt::For { body, .. }
            | crate::ast::Stmt::ForIn { body, .. }
            | crate::ast::Stmt::ForOf { body, .. }
            | crate::ast::Stmt::With { body, .. }
            | crate::ast::Stmt::Labeled { stmt: body, .. } => {
                Self::stmt_has_operandless_await(body)
            }
            crate::ast::Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                Self::block_has_operandless_await(block)
                    || catch
                        .as_ref()
                        .is_some_and(|c| Self::block_has_operandless_await(&c.block))
                    || finally
                        .as_ref()
                        .is_some_and(Self::block_has_operandless_await)
            }
            crate::ast::Stmt::Switch { cases, .. } => cases
                .iter()
                .any(|case| case.stmts.iter().any(Self::stmt_has_operandless_await)),
            _ => false,
        }
    }

    fn is_direct_static_block_declaration(&self, decl_key: usize) -> bool {
        self.bind
            .decl_scope
            .get(&decl_key)
            .is_some_and(|scope| self.bind.static_block_scopes.contains(scope))
    }

    fn has_unused_class_declaration(&self, symbol: &crate::binder::Symbol<'a>) -> bool {
        symbol.flags & flags::CLASS != 0
            && symbol.decls.iter().any(|decl| {
                matches!(
                    decl,
                    crate::binder::Decl::Class(class)
                        if self.is_reportable_unused_class_declaration(class)
                )
            })
    }

    fn unused_type_decl_spans(&self, symbol: &crate::binder::Symbol<'a>) -> Vec<crate::ast::Span> {
        if symbol.dup_reported {
            return symbol
                .decls
                .first()
                .map(|decl| vec![decl.name_span()])
                .unwrap_or_default();
        }

        let mut spans = Vec::new();
        for decl in &symbol.decls {
            match decl {
                crate::binder::Decl::Interface(_) => spans.push(decl.name_span()),
                crate::binder::Decl::Class(class)
                    if self.is_reportable_unused_class_declaration(class) =>
                {
                    spans.push(decl.name_span());
                }
                crate::binder::Decl::Alias(_) => spans.push(decl.name_span()),
                _ => {}
            }
        }
        spans
    }

    fn check_unused_type_params(&mut self) {
        let mut groups = self.collect_type_param_usage_groups();
        for group in &mut groups {
            for param in &mut group.params {
                if param.sym.is_none() {
                    param.sym = Some(self.ensure_type_param_symbol(param.decl));
                }
                if group.refs_only {
                    if let Some(sym) = param.sym {
                        self.symuse.used_symbols.remove(&sym);
                    }
                }
            }
        }
        self.mark_type_param_group_internal_usage(&groups);
        self.mark_type_param_constraint_default_usage();
        let as_error = self.options.no_unused_locals;
        let mut grouped_reports = Vec::new();
        let mut per_param_reports = Vec::new();
        for group in groups {
            if group.file == self.lib_file {
                continue;
            }
            let reportable: Vec<&TypeParamUsageEntry> =
                group.params.iter().filter(|p| !p.exempt).collect();
            if reportable.is_empty() {
                continue;
            }
            let unused: Vec<&TypeParamUsageEntry> = reportable
                .iter()
                .copied()
                .filter(|p| {
                    p.sym
                        .is_some_and(|sym| !self.symuse.used_symbols.contains(&sym))
                })
                .collect();
            if unused.is_empty() {
                continue;
            }
            if reportable.len() > 1 && unused.len() == reportable.len() {
                grouped_reports.push((group.file, group.list_span));
            } else {
                for p in unused {
                    per_param_reports.push((group.file, p.span, p.name.clone()));
                }
            }
        }
        for (file, span) in grouped_reports {
            let prev = self.current_file;
            self.current_file = file;
            self.unused_diag(span, &gen::All_type_parameters_are_unused, &[], as_error);
            self.current_file = prev;
        }
        for (file, span, name) in per_param_reports {
            let prev = self.current_file;
            self.current_file = file;
            self.unused_diag(
                span,
                &gen::_0_is_declared_but_its_value_is_never_read,
                &[name],
                as_error,
            );
            self.current_file = prev;
        }
    }

    fn mark_type_param_constraint_default_usage(&mut self) {
        let params = self.all_type_param_symbols();
        for sym in params {
            let Some(crate::binder::Decl::TypeParam(tp)) = self.symbol(sym).decls.first().copied()
            else {
                continue;
            };
            let file = self.symbol(sym).file;
            let scope = self.scope_of_decl(crate::ast::node_key(tp));
            let prev = self.current_file;
            self.current_file = file;
            let diag_len = self.diags.len();
            if let Some(constraint) = &tp.constraint {
                self.resolve_type_cached(constraint, scope);
            }
            if let Some(default) = &tp.default {
                self.resolve_type_cached(default, scope);
            }
            self.diags.truncate(diag_len);
            self.current_file = prev;
        }
    }

    fn mark_type_param_group_internal_usage(&mut self, groups: &[TypeParamUsageGroup<'a>]) {
        for group in groups {
            let names: std::collections::HashMap<&str, SymbolId> = group
                .params
                .iter()
                .filter_map(|p| p.sym.map(|sym| (p.name.as_str(), sym)))
                .collect();
            for name in &group.refs {
                if let Some(&sym) = names.get(name.as_str()) {
                    self.symuse.used_symbols.insert(sym);
                }
            }
            let group_names: Vec<String> = group.params.iter().map(|p| p.name.clone()).collect();
            for param in &group.params {
                for ty in param
                    .decl
                    .constraint
                    .iter()
                    .chain(param.decl.default.iter())
                {
                    let mut refs = Vec::new();
                    let mut shadowed = Vec::new();
                    self.collect_type_param_refs_in_type(
                        &group_names,
                        &mut shadowed,
                        ty,
                        &mut refs,
                    );
                    for name in refs {
                        if let Some(&sym) = names.get(name.as_str()) {
                            self.symuse.used_symbols.insert(sym);
                        }
                    }
                }
            }
        }
    }

    fn all_type_param_symbols(&self) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for (i, s) in self.bind.symbols.iter().enumerate() {
            if s.flags & flags::TYPE_PARAM != 0 {
                out.push(SymbolId(i as u32));
            }
        }
        for (i, s) in self.synth.symbols.iter().enumerate() {
            if s.flags & flags::TYPE_PARAM != 0 {
                out.push(SymbolId(self.synth.base + i as u32));
            }
        }
        out
    }

    fn collect_type_param_usage_groups(&self) -> Vec<TypeParamUsageGroup<'a>> {
        let mut groups = Vec::new();
        for (file, (_, _, ast)) in self.files.iter().enumerate() {
            self.collect_type_param_groups_in_stmts(file, &ast.stmts, &mut groups);
        }
        groups
    }

    fn collect_type_param_group(
        &self,
        file: usize,
        tps: Option<&'a Vec<TypeParamDecl>>,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) -> Option<usize> {
        let Some(tps) = tps else { return None };
        if tps.is_empty() {
            return None;
        }
        let mut params = Vec::new();
        for (idx, tp) in tps.iter().enumerate() {
            let key = crate::ast::node_key(tp);
            let sym = self
                .bind
                .decl_symbol
                .get(&key)
                .copied()
                .or_else(|| self.synth.decl_symbol.get(&key).copied());
            let span = if tps.len() == 1 {
                self.type_param_list_span(file, tps)
            } else {
                self.type_param_individual_span(tp, idx)
            };
            params.push(TypeParamUsageEntry {
                sym,
                decl: tp,
                name: tp.name.name.clone(),
                span,
                exempt: tp.name.name.starts_with('_'),
            });
        }
        if !params.is_empty() {
            let idx = groups.len();
            groups.push(TypeParamUsageGroup {
                file,
                list_span: self.type_param_list_span(file, tps),
                params,
                refs: Vec::new(),
                refs_only: false,
            });
            Some(idx)
        } else {
            None
        }
    }

    fn interface_symbol(&self, i: &'a InterfaceDecl) -> Option<SymbolId> {
        let key = crate::ast::node_key(i);
        self.bind.decl_symbol.get(&key).copied().or_else(|| {
            self.bind.symbols.iter().enumerate().find_map(|(idx, sym)| {
                if sym.decls.iter().any(|decl| match decl {
                    crate::binder::Decl::Interface(other) => crate::ast::node_key(*other) == key,
                    _ => false,
                }) {
                    Some(SymbolId(idx as u32))
                } else {
                    None
                }
            })
        })
    }

    fn is_last_interface_declaration(&self, i: &'a InterfaceDecl) -> bool {
        let key = crate::ast::node_key(i);
        let sym = self.interface_symbol(i);
        let Some(sym) = sym else { return true };
        let Some(last) = self
            .symbol(sym)
            .decls
            .iter()
            .rev()
            .find_map(|decl| match decl {
                crate::binder::Decl::Interface(other) => Some(*other),
                _ => None,
            })
        else {
            return true;
        };
        crate::ast::node_key(last) == key
    }

    fn is_exported_nested_interface(&self, file: usize, i: &'a InterfaceDecl) -> bool {
        if !has_modifier(&i.modifiers, ModifierKind::Export) {
            return false;
        }
        let key = crate::ast::node_key(i);
        let decl_scope = self.bind.decl_scope.get(&key).copied();
        let module_scope = self.bind.module_scope.get(&file).copied();
        decl_scope.is_some() && module_scope.is_some() && decl_scope != module_scope
    }

    fn interface_has_separate_value_declaration(&self, i: &'a InterfaceDecl) -> bool {
        let key = crate::ast::node_key(i);
        let Some(scope) = self.bind.decl_scope.get(&key).copied() else {
            return false;
        };
        let Some(value_sym) = self.scope_at(scope).values.get(&i.name.name) else {
            return false;
        };
        self.interface_symbol(i) != Some(value_sym)
    }

    fn collect_type_param_refs_in_interface_decl(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        i: &'a InterfaceDecl,
        out: &mut Vec<String>,
    ) {
        for ext in &i.extends {
            if let Some(args) = &ext.type_args {
                for arg in args {
                    self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                }
            }
        }
        for member in &i.members {
            self.collect_type_param_refs_in_type_member(group_names, shadowed, member, out);
        }
    }

    fn function_like_symbol(&self, f: &'a FunctionLike) -> Option<SymbolId> {
        let key = crate::ast::node_key(f);
        self.bind.decl_symbol.get(&key).copied().or_else(|| {
            self.bind.symbols.iter().enumerate().find_map(|(idx, sym)| {
                if sym.decls.iter().any(|decl| match decl {
                    crate::binder::Decl::Func(other) | crate::binder::Decl::Method(other) => {
                        crate::ast::node_key(*other) == key
                    }
                    _ => false,
                }) {
                    Some(SymbolId(idx as u32))
                } else {
                    None
                }
            })
        })
    }

    fn should_check_function_like_type_params(&self, f: &'a FunctionLike) -> bool {
        let key = crate::ast::node_key(f);
        let Some(sym) = self.function_like_symbol(f) else {
            return true;
        };
        let decls: Vec<&FunctionLike> = self
            .symbol(sym)
            .decls
            .iter()
            .filter_map(|decl| match decl {
                crate::binder::Decl::Func(other) | crate::binder::Decl::Method(other) => {
                    Some(*other)
                }
                _ => None,
            })
            .collect();
        if decls.len() <= 1 {
            return true;
        }
        if decls.iter().any(|decl| decl.body.is_some()) {
            return f.body.is_some();
        }
        decls
            .last()
            .map(|last| crate::ast::node_key(*last) == key)
            .unwrap_or(true)
    }

    fn collect_type_param_refs_for_group_signature(
        &self,
        groups: &mut [TypeParamUsageGroup<'a>],
        group_idx: Option<usize>,
        params: &'a [Param],
        return_type: Option<&'a TypeNode>,
    ) {
        let Some(group_idx) = group_idx else { return };
        let group_names: Vec<String> = groups[group_idx]
            .params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut refs = Vec::new();
        let mut shadowed = Vec::new();
        for param in params {
            self.collect_type_param_refs_in_param(&group_names, &mut shadowed, param, &mut refs);
        }
        if let Some(return_type) = return_type {
            self.collect_type_param_refs_in_type(
                &group_names,
                &mut shadowed,
                return_type,
                &mut refs,
            );
        }
        groups[group_idx].refs.extend(refs);
    }

    fn collect_type_param_refs_for_group_function_body(
        &self,
        groups: &mut [TypeParamUsageGroup<'a>],
        group_idx: Option<usize>,
        f: &'a FunctionLike,
    ) {
        let Some(group_idx) = group_idx else { return };
        let group_names: Vec<String> = groups[group_idx]
            .params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut refs = Vec::new();
        let mut shadowed = Vec::new();
        for param in &f.params {
            if let Some(init) = &param.initializer {
                self.collect_type_param_refs_in_expr(&group_names, &mut shadowed, init, &mut refs);
            }
        }
        if let Some(body) = &f.body {
            match body {
                FuncBody::Block(block) => {
                    self.collect_type_param_refs_in_stmts(
                        &group_names,
                        &mut shadowed,
                        &block.stmts,
                        &mut refs,
                    );
                }
                FuncBody::Expr(expr) => {
                    self.collect_type_param_refs_in_expr(
                        &group_names,
                        &mut shadowed,
                        expr,
                        &mut refs,
                    );
                }
            }
        }
        groups[group_idx].refs.extend(refs);
    }

    fn collect_type_param_refs_in_param(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        param: &'a Param,
        out: &mut Vec<String>,
    ) {
        if let Some(ty) = &param.ty {
            self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
        }
    }

    fn collect_type_param_refs_in_type_member(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        member: &'a TypeMember,
        out: &mut Vec<String>,
    ) {
        match member {
            TypeMember::Prop(p) => {
                if let Some(ty) = &p.ty {
                    self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
                }
            }
            TypeMember::Method(m) => {
                self.collect_type_param_refs_in_nested_signature(
                    group_names,
                    shadowed,
                    m.type_params.as_ref(),
                    &m.params,
                    m.return_type.as_ref(),
                    out,
                );
            }
            TypeMember::Call(s) | TypeMember::Ctor(s) => {
                self.collect_type_param_refs_in_nested_signature(
                    group_names,
                    shadowed,
                    s.type_params.as_ref(),
                    &s.params,
                    s.return_type.as_ref(),
                    out,
                );
            }
            TypeMember::Index(ix) => {
                self.collect_type_param_refs_in_type(group_names, shadowed, &ix.key_type, out);
                self.collect_type_param_refs_in_type(group_names, shadowed, &ix.value_type, out);
            }
        }
    }

    fn collect_type_param_refs_in_class_member(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        member: &'a ClassMember,
        out: &mut Vec<String>,
    ) {
        match member {
            ClassMember::Property(p) => {
                if has_modifier(&p.modifiers, ModifierKind::Static) {
                    return;
                }
                if let Some(ty) = &p.ty {
                    self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
                }
                if let Some(init) = &p.init {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, init, out);
                }
            }
            ClassMember::Method(f) | ClassMember::Constructor(f) => {
                if has_modifier(&f.modifiers, ModifierKind::Static) {
                    return;
                }
                self.collect_type_param_refs_in_function_like(group_names, shadowed, f, out);
            }
            ClassMember::Index(ix) => {
                self.collect_type_param_refs_in_type(group_names, shadowed, &ix.key_type, out);
                self.collect_type_param_refs_in_type(group_names, shadowed, &ix.value_type, out);
            }
            ClassMember::StaticBlock(_) => {}
        }
    }

    fn collect_type_param_refs_in_function_like(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        f: &'a FunctionLike,
        out: &mut Vec<String>,
    ) {
        let pushed = self.push_shadowing_type_params(group_names, shadowed, f.type_params.as_ref());
        if let Some(type_params) = &f.type_params {
            for tp in type_params {
                if let Some(constraint) = &tp.constraint {
                    self.collect_type_param_refs_in_type(group_names, shadowed, constraint, out);
                }
                if let Some(default) = &tp.default {
                    self.collect_type_param_refs_in_type(group_names, shadowed, default, out);
                }
            }
        }
        for param in &f.params {
            self.collect_type_param_refs_in_param(group_names, shadowed, param, out);
            if let Some(init) = &param.initializer {
                self.collect_type_param_refs_in_expr(group_names, shadowed, init, out);
            }
        }
        if let Some(return_type) = &f.return_type {
            self.collect_type_param_refs_in_type(group_names, shadowed, return_type, out);
        }
        if let Some(body) = &f.body {
            match body {
                FuncBody::Block(block) => {
                    self.collect_type_param_refs_in_stmts(group_names, shadowed, &block.stmts, out);
                }
                FuncBody::Expr(expr) => {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                }
            }
        }
        Self::pop_shadowed(shadowed, pushed);
    }

    fn collect_type_param_refs_in_stmts(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        stmts: &'a [Stmt],
        out: &mut Vec<String>,
    ) {
        for stmt in stmts {
            self.collect_type_param_refs_in_stmt(group_names, shadowed, stmt, out);
        }
    }

    fn collect_type_param_refs_in_stmt(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        stmt: &'a Stmt,
        out: &mut Vec<String>,
    ) {
        match stmt {
            Stmt::Var(v) => {
                for decl in &v.decls {
                    if let Some(ty) = &decl.ty {
                        self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
                    }
                    if let Some(init) = &decl.init {
                        self.collect_type_param_refs_in_expr(group_names, shadowed, init, out);
                    }
                }
            }
            Stmt::Func(f) => {
                self.collect_type_param_refs_in_function_like(group_names, shadowed, f, out)
            }
            Stmt::Class(c) => {
                let pushed =
                    self.push_shadowing_type_params(group_names, shadowed, c.type_params.as_ref());
                if let Some(ext) = &c.extends {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, &ext.expr, out);
                    if let Some(args) = &ext.type_args {
                        for arg in args {
                            self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                        }
                    }
                }
                for imp in &c.implements {
                    if let Some(args) = &imp.type_args {
                        for arg in args {
                            self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                        }
                    }
                }
                for member in &c.members {
                    self.collect_type_param_refs_in_class_member(
                        group_names,
                        shadowed,
                        member,
                        out,
                    );
                }
                Self::pop_shadowed(shadowed, pushed);
            }
            Stmt::TypeAlias(t) => {
                let pushed =
                    self.push_shadowing_type_params(group_names, shadowed, t.type_params.as_ref());
                self.collect_type_param_refs_in_type(group_names, shadowed, &t.ty, out);
                Self::pop_shadowed(shadowed, pushed);
            }
            Stmt::Interface(i) => {
                let pushed =
                    self.push_shadowing_type_params(group_names, shadowed, i.type_params.as_ref());
                self.collect_type_param_refs_in_interface_decl(group_names, shadowed, i, out);
                Self::pop_shadowed(shadowed, pushed);
            }
            Stmt::Namespace(n) => {
                self.collect_type_param_refs_in_stmts(group_names, shadowed, &n.body, out);
            }
            Stmt::Block(b) => {
                self.collect_type_param_refs_in_stmts(group_names, shadowed, &b.stmts, out);
            }
            Stmt::Expr { expr, .. } | Stmt::Throw { expr, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
            }
            Stmt::Return {
                expr: Some(expr), ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, cond, out);
                self.collect_type_param_refs_in_stmt(group_names, shadowed, then, out);
                if let Some(els) = els {
                    self.collect_type_param_refs_in_stmt(group_names, shadowed, els, out);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, cond, out);
                self.collect_type_param_refs_in_stmt(group_names, shadowed, body, out);
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.collect_type_param_refs_in_stmt(group_names, shadowed, body, out);
                self.collect_type_param_refs_in_expr(group_names, shadowed, cond, out);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                if let Some(init) = init {
                    match &**init {
                        ForInit::Var(v) => {
                            for decl in &v.decls {
                                if let Some(ty) = &decl.ty {
                                    self.collect_type_param_refs_in_type(
                                        group_names,
                                        shadowed,
                                        ty,
                                        out,
                                    );
                                }
                                if let Some(init) = &decl.init {
                                    self.collect_type_param_refs_in_expr(
                                        group_names,
                                        shadowed,
                                        init,
                                        out,
                                    );
                                }
                            }
                        }
                        ForInit::Expr(expr) => {
                            self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                        }
                    }
                }
                if let Some(cond) = cond {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, cond, out);
                }
                if let Some(incr) = incr {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, incr, out);
                }
                self.collect_type_param_refs_in_stmt(group_names, shadowed, body, out);
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                match &**left {
                    ForInit::Var(v) => {
                        for decl in &v.decls {
                            if let Some(ty) = &decl.ty {
                                self.collect_type_param_refs_in_type(
                                    group_names,
                                    shadowed,
                                    ty,
                                    out,
                                );
                            }
                        }
                    }
                    ForInit::Expr(expr) => {
                        self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                    }
                }
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                self.collect_type_param_refs_in_stmt(group_names, shadowed, body, out);
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                self.collect_type_param_refs_in_stmts(group_names, shadowed, &block.stmts, out);
                if let Some(catch) = catch {
                    if let Some(param) = &catch.param {
                        self.collect_type_param_refs_in_param(group_names, shadowed, param, out);
                    }
                    self.collect_type_param_refs_in_stmts(
                        group_names,
                        shadowed,
                        &catch.block.stmts,
                        out,
                    );
                }
                if let Some(finally) = finally {
                    self.collect_type_param_refs_in_stmts(
                        group_names,
                        shadowed,
                        &finally.stmts,
                        out,
                    );
                }
            }
            Stmt::Switch { expr, cases, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.collect_type_param_refs_in_expr(group_names, shadowed, test, out);
                    }
                    self.collect_type_param_refs_in_stmts(group_names, shadowed, &case.stmts, out);
                }
            }
            Stmt::Labeled { stmt, .. } => {
                self.collect_type_param_refs_in_stmt(group_names, shadowed, stmt, out);
            }
            Stmt::ExportDefault { expr, .. } | Stmt::ExportAssign { expr, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
            }
            Stmt::Return { expr: None, .. }
            | Stmt::Empty { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Import(_)
            | Stmt::ExportNamed(_)
            | Stmt::ImportEquals { .. }
            | Stmt::Enum(_)
            | Stmt::With { .. }
            | Stmt::Missing { .. } => {}
        }
    }

    fn collect_type_param_refs_in_expr(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        expr: &'a Expr,
        out: &mut Vec<String>,
    ) {
        match expr {
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            }
            | Expr::New {
                callee,
                type_args,
                args: Some(args),
                ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, callee, out);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                    }
                }
                for arg in args {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, arg, out);
                }
            }
            Expr::New {
                callee,
                type_args,
                args: None,
                ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, callee, out);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                    }
                }
            }
            Expr::Object { props, .. } => {
                for prop in props {
                    match prop {
                        ObjectProp::Property { name, value, .. } => {
                            self.collect_type_param_refs_in_prop_name(
                                group_names,
                                shadowed,
                                name,
                                out,
                            );
                            self.collect_type_param_refs_in_expr(group_names, shadowed, value, out);
                        }
                        ObjectProp::Method(f) => {
                            if let Some(name) = &f.name {
                                self.collect_type_param_refs_in_prop_name(
                                    group_names,
                                    shadowed,
                                    name,
                                    out,
                                );
                            }
                            self.collect_type_param_refs_in_function_like(
                                group_names,
                                shadowed,
                                f,
                                out,
                            );
                        }
                        ObjectProp::Spread { expr, .. } => {
                            self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                        }
                        ObjectProp::Shorthand { .. } => {}
                    }
                }
            }
            Expr::Array { elements, .. } => {
                for elem in elements {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, elem, out);
                }
            }
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.collect_type_param_refs_in_function_like(group_names, shadowed, f, out);
            }
            Expr::ClassExpr(c) => {
                let pushed =
                    self.push_shadowing_type_params(group_names, shadowed, c.type_params.as_ref());
                if let Some(ext) = &c.extends {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, &ext.expr, out);
                    if let Some(args) = &ext.type_args {
                        for arg in args {
                            self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                        }
                    }
                }
                for imp in &c.implements {
                    if let Some(args) = &imp.type_args {
                        for arg in args {
                            self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                        }
                    }
                }
                for member in &c.members {
                    self.collect_type_param_refs_in_class_member(
                        group_names,
                        shadowed,
                        member,
                        out,
                    );
                }
                Self::pop_shadowed(shadowed, pushed);
            }
            Expr::PropAccess { obj, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, obj, out);
            }
            Expr::ElemAccess { obj, index, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, obj, out);
                self.collect_type_param_refs_in_expr(group_names, shadowed, index, out);
            }
            Expr::Unary { operand, .. }
            | Expr::Update { operand, .. }
            | Expr::Paren { inner: operand, .. }
            | Expr::NonNull { expr: operand, .. }
            | Expr::Spread { expr: operand, .. }
            | Expr::Await { expr: operand, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, operand, out);
            }
            Expr::Binary { left, right, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, left, out);
                self.collect_type_param_refs_in_expr(group_names, shadowed, right, out);
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, cond, out);
                self.collect_type_param_refs_in_expr(group_names, shadowed, when_true, out);
                self.collect_type_param_refs_in_expr(group_names, shadowed, when_false, out);
            }
            Expr::Assertion { expr, ty, .. } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
            }
            Expr::Yield {
                expr: Some(expr), ..
            } => {
                self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
            }
            Expr::ImportCall { args, .. } => {
                for arg in args {
                    self.collect_type_param_refs_in_expr(group_names, shadowed, arg, out);
                }
            }
            Expr::JsxElement(jsx) => {
                for attr in &jsx.attrs {
                    if let Some(value) = &attr.value {
                        self.collect_type_param_refs_in_expr(group_names, shadowed, value, out);
                    }
                }
                for child in &jsx.children {
                    match child {
                        JsxChild::Element(elem) => {
                            for attr in &elem.attrs {
                                if let Some(value) = &attr.value {
                                    self.collect_type_param_refs_in_expr(
                                        group_names,
                                        shadowed,
                                        value,
                                        out,
                                    );
                                }
                            }
                        }
                        JsxChild::Expr(expr) => {
                            self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
                        }
                        JsxChild::Text => {}
                    }
                }
            }
            Expr::Ident(_)
            | Expr::NumLit { .. }
            | Expr::StrLit { .. }
            | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. }
            | Expr::NullLit { .. }
            | Expr::RegexLit { .. }
            | Expr::Template { .. }
            | Expr::This { .. }
            | Expr::Super { .. }
            | Expr::Yield { expr: None, .. }
            | Expr::ImportMeta { .. }
            | Expr::Missing { .. } => {}
        }
    }

    fn collect_type_param_refs_in_prop_name(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        name: &'a PropName,
        out: &mut Vec<String>,
    ) {
        if let PropName::Computed { expr, .. } = name {
            self.collect_type_param_refs_in_expr(group_names, shadowed, expr, out);
        }
    }

    fn collect_type_param_refs_in_nested_signature(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        type_params: Option<&'a Vec<TypeParamDecl>>,
        params: &'a [Param],
        return_type: Option<&'a TypeNode>,
        out: &mut Vec<String>,
    ) {
        let pushed = self.push_shadowing_type_params(group_names, shadowed, type_params);
        if let Some(type_params) = type_params {
            for tp in type_params {
                if let Some(constraint) = &tp.constraint {
                    self.collect_type_param_refs_in_type(group_names, shadowed, constraint, out);
                }
                if let Some(default) = &tp.default {
                    self.collect_type_param_refs_in_type(group_names, shadowed, default, out);
                }
            }
        }
        for param in params {
            self.collect_type_param_refs_in_param(group_names, shadowed, param, out);
        }
        if let Some(return_type) = return_type {
            self.collect_type_param_refs_in_type(group_names, shadowed, return_type, out);
        }
        Self::pop_shadowed(shadowed, pushed);
    }

    fn collect_type_param_refs_in_type(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        ty: &'a TypeNode,
        out: &mut Vec<String>,
    ) {
        match ty {
            TypeNode::Ref(r) => {
                if r.name.parts.len() == 1 {
                    let name = &r.name.parts[0].name;
                    if Self::name_in_type_param_group(group_names, name)
                        && !Self::name_is_shadowed(shadowed, name)
                    {
                        out.push(name.clone());
                    }
                }
                if let Some(args) = &r.type_args {
                    for arg in args {
                        self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                    }
                }
            }
            TypeNode::Array { elem, .. }
            | TypeNode::Paren { inner: elem, .. }
            | TypeNode::Keyof { ty: elem, .. }
            | TypeNode::ReadonlyOp { ty: elem, .. } => {
                self.collect_type_param_refs_in_type(group_names, shadowed, elem, out);
            }
            TypeNode::Tuple { elems, .. } => {
                for elem in elems {
                    self.collect_type_param_refs_in_type(group_names, shadowed, &elem.ty, out);
                }
            }
            TypeNode::Union { members, .. } | TypeNode::Intersection { members, .. } => {
                for member in members {
                    self.collect_type_param_refs_in_type(group_names, shadowed, member, out);
                }
            }
            TypeNode::Function(f) | TypeNode::Ctor(f) => {
                self.collect_type_param_refs_in_nested_signature(
                    group_names,
                    shadowed,
                    f.type_params.as_ref(),
                    &f.params,
                    Some(&f.return_type),
                    out,
                );
            }
            TypeNode::TypeLiteral { members, .. } => {
                for member in members {
                    self.collect_type_param_refs_in_type_member(group_names, shadowed, member, out);
                }
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                self.collect_type_param_refs_in_type(group_names, shadowed, obj, out);
                self.collect_type_param_refs_in_type(group_names, shadowed, index, out);
            }
            TypeNode::Conditional(c) => {
                self.collect_type_param_refs_in_type(group_names, shadowed, &c.check, out);
                self.collect_type_param_refs_in_type(group_names, shadowed, &c.extends_ty, out);
                let mut infer_names = Vec::new();
                Self::collect_infer_type_names(&c.extends_ty, &mut infer_names);
                let pushed = self.push_shadowing_names(group_names, shadowed, &infer_names);
                self.collect_type_param_refs_in_type(group_names, shadowed, &c.true_ty, out);
                Self::pop_shadowed(shadowed, pushed);
                self.collect_type_param_refs_in_type(group_names, shadowed, &c.false_ty, out);
            }
            TypeNode::Predicate { ty: Some(ty), .. } => {
                self.collect_type_param_refs_in_type(group_names, shadowed, ty, out);
            }
            TypeNode::Mapped(m) => {
                let pushed = self.push_shadowing_names(
                    group_names,
                    shadowed,
                    std::slice::from_ref(&m.key.name),
                );
                self.collect_type_param_refs_in_type(group_names, shadowed, &m.constraint, out);
                if let Some(name_type) = &m.name_type {
                    self.collect_type_param_refs_in_type(group_names, shadowed, name_type, out);
                }
                if let Some(value) = &m.value {
                    self.collect_type_param_refs_in_type(group_names, shadowed, value, out);
                }
                Self::pop_shadowed(shadowed, pushed);
            }
            TypeNode::TemplateLit { parts, .. } => {
                for (part, _) in parts {
                    self.collect_type_param_refs_in_type(group_names, shadowed, part, out);
                }
            }
            TypeNode::Infer {
                constraint: Some(constraint),
                ..
            } => {
                self.collect_type_param_refs_in_type(group_names, shadowed, constraint, out);
            }
            TypeNode::TypeQuery { type_args, .. } => {
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_refs_in_type(group_names, shadowed, arg, out);
                    }
                }
            }
            TypeNode::Keyword(..)
            | TypeNode::This(_)
            | TypeNode::LiteralString { .. }
            | TypeNode::LiteralNumber { .. }
            | TypeNode::LiteralBigInt { .. }
            | TypeNode::LiteralBool { .. }
            | TypeNode::Predicate { ty: None, .. }
            | TypeNode::Infer {
                constraint: None, ..
            } => {}
        }
    }

    fn push_shadowing_type_params(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        type_params: Option<&'a Vec<TypeParamDecl>>,
    ) -> usize {
        let before = shadowed.len();
        if let Some(type_params) = type_params {
            for tp in type_params {
                if Self::name_in_type_param_group(group_names, &tp.name.name) {
                    shadowed.push(tp.name.name.clone());
                }
            }
        }
        shadowed.len() - before
    }

    fn push_shadowing_names(
        &self,
        group_names: &[String],
        shadowed: &mut Vec<String>,
        names: &[String],
    ) -> usize {
        let before = shadowed.len();
        for name in names {
            if Self::name_in_type_param_group(group_names, name) {
                shadowed.push(name.clone());
            }
        }
        shadowed.len() - before
    }

    fn pop_shadowed(shadowed: &mut Vec<String>, count: usize) {
        for _ in 0..count {
            shadowed.pop();
        }
    }

    fn name_in_type_param_group(group_names: &[String], name: &str) -> bool {
        group_names.iter().any(|n| n == name)
    }

    fn name_is_shadowed(shadowed: &[String], name: &str) -> bool {
        shadowed.iter().rev().any(|n| n == name)
    }

    fn collect_infer_type_names(ty: &'a TypeNode, out: &mut Vec<String>) {
        match ty {
            TypeNode::Infer {
                name, constraint, ..
            } => {
                out.push(name.name.clone());
                if let Some(constraint) = constraint {
                    Self::collect_infer_type_names(constraint, out);
                }
            }
            TypeNode::Ref(r) => {
                if let Some(args) = &r.type_args {
                    for arg in args {
                        Self::collect_infer_type_names(arg, out);
                    }
                }
            }
            TypeNode::Array { elem, .. }
            | TypeNode::Paren { inner: elem, .. }
            | TypeNode::Keyof { ty: elem, .. }
            | TypeNode::ReadonlyOp { ty: elem, .. } => {
                Self::collect_infer_type_names(elem, out);
            }
            TypeNode::Tuple { elems, .. } => {
                for elem in elems {
                    Self::collect_infer_type_names(&elem.ty, out);
                }
            }
            TypeNode::Union { members, .. } | TypeNode::Intersection { members, .. } => {
                for member in members {
                    Self::collect_infer_type_names(member, out);
                }
            }
            TypeNode::Function(f) | TypeNode::Ctor(f) => {
                if let Some(type_params) = &f.type_params {
                    for tp in type_params {
                        if let Some(constraint) = &tp.constraint {
                            Self::collect_infer_type_names(constraint, out);
                        }
                        if let Some(default) = &tp.default {
                            Self::collect_infer_type_names(default, out);
                        }
                    }
                }
                for param in &f.params {
                    if let Some(ty) = &param.ty {
                        Self::collect_infer_type_names(ty, out);
                    }
                }
                Self::collect_infer_type_names(&f.return_type, out);
            }
            TypeNode::TypeLiteral { members, .. } => {
                for member in members {
                    match member {
                        TypeMember::Prop(p) => {
                            if let Some(ty) = &p.ty {
                                Self::collect_infer_type_names(ty, out);
                            }
                        }
                        TypeMember::Method(m) => {
                            if let Some(type_params) = &m.type_params {
                                for tp in type_params {
                                    if let Some(constraint) = &tp.constraint {
                                        Self::collect_infer_type_names(constraint, out);
                                    }
                                    if let Some(default) = &tp.default {
                                        Self::collect_infer_type_names(default, out);
                                    }
                                }
                            }
                            for param in &m.params {
                                if let Some(ty) = &param.ty {
                                    Self::collect_infer_type_names(ty, out);
                                }
                            }
                            if let Some(rt) = &m.return_type {
                                Self::collect_infer_type_names(rt, out);
                            }
                        }
                        TypeMember::Call(s) | TypeMember::Ctor(s) => {
                            if let Some(type_params) = &s.type_params {
                                for tp in type_params {
                                    if let Some(constraint) = &tp.constraint {
                                        Self::collect_infer_type_names(constraint, out);
                                    }
                                    if let Some(default) = &tp.default {
                                        Self::collect_infer_type_names(default, out);
                                    }
                                }
                            }
                            for param in &s.params {
                                if let Some(ty) = &param.ty {
                                    Self::collect_infer_type_names(ty, out);
                                }
                            }
                            if let Some(rt) = &s.return_type {
                                Self::collect_infer_type_names(rt, out);
                            }
                        }
                        TypeMember::Index(ix) => {
                            Self::collect_infer_type_names(&ix.key_type, out);
                            Self::collect_infer_type_names(&ix.value_type, out);
                        }
                    }
                }
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                Self::collect_infer_type_names(obj, out);
                Self::collect_infer_type_names(index, out);
            }
            TypeNode::Conditional(c) => {
                Self::collect_infer_type_names(&c.check, out);
                Self::collect_infer_type_names(&c.extends_ty, out);
                Self::collect_infer_type_names(&c.true_ty, out);
                Self::collect_infer_type_names(&c.false_ty, out);
            }
            TypeNode::Predicate { ty: Some(ty), .. } => {
                Self::collect_infer_type_names(ty, out);
            }
            TypeNode::Mapped(m) => {
                Self::collect_infer_type_names(&m.constraint, out);
                if let Some(name_type) = &m.name_type {
                    Self::collect_infer_type_names(name_type, out);
                }
                if let Some(value) = &m.value {
                    Self::collect_infer_type_names(value, out);
                }
            }
            TypeNode::TemplateLit { parts, .. } => {
                for (part, _) in parts {
                    Self::collect_infer_type_names(part, out);
                }
            }
            TypeNode::TypeQuery { type_args, .. } => {
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        Self::collect_infer_type_names(arg, out);
                    }
                }
            }
            TypeNode::Keyword(..)
            | TypeNode::This(_)
            | TypeNode::LiteralString { .. }
            | TypeNode::LiteralNumber { .. }
            | TypeNode::LiteralBigInt { .. }
            | TypeNode::LiteralBool { .. }
            | TypeNode::Predicate { ty: None, .. } => {}
        }
    }

    fn type_param_individual_span(&self, tp: &'a TypeParamDecl, _idx: usize) -> Span {
        tp.span
    }

    fn is_intrinsic_type_alias_body(ty: &TypeNode) -> bool {
        match ty {
            TypeNode::Keyword(KeywordTypeKind::Intrinsic, _) => true,
            TypeNode::Ref(r) => {
                r.type_args.is_none()
                    && r.name.parts.len() == 1
                    && r.name.parts[0].name == "intrinsic"
            }
            _ => false,
        }
    }

    fn type_param_list_span(&self, file: usize, tps: &'a [TypeParamDecl]) -> Span {
        let Some(first) = tps.first() else {
            return Span::new(0, 0);
        };
        let last = tps.last().unwrap();
        let text = &self.files[file].1.text;
        let bytes = text.as_bytes();
        let mut start = first.span.start as usize;
        while start > 0 && bytes[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        if start > 0 && bytes[start - 1] == b'<' {
            start -= 1;
        }
        let mut end = last.span.end as usize;
        while end < bytes.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end < bytes.len() && bytes[end] == b'>' {
            end += 1;
        }
        Span::new(start, end)
    }

    fn collect_type_param_groups_in_stmts(
        &self,
        file: usize,
        stmts: &'a [Stmt],
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        for stmt in stmts {
            self.collect_type_param_groups_in_stmt(file, stmt, groups);
        }
    }

    fn collect_type_param_groups_in_stmt(
        &self,
        file: usize,
        stmt: &'a Stmt,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        match stmt {
            Stmt::Var(v) => self.collect_type_param_groups_in_var_stmt(file, v, groups),
            Stmt::Func(f) => self.collect_type_param_groups_in_function(file, f, groups),
            Stmt::Class(c) => self.collect_type_param_groups_in_class(file, c, groups),
            Stmt::Interface(i) => {
                let group_idx = if self.is_last_interface_declaration(i)
                    && !self.is_exported_nested_interface(file, i)
                    && !self.interface_has_separate_value_declaration(i)
                {
                    self.collect_type_param_group(file, i.type_params.as_ref(), groups)
                } else {
                    None
                };
                if let Some(group_idx) = group_idx {
                    let group_names: Vec<String> = groups[group_idx]
                        .params
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                    let mut refs = Vec::new();
                    let mut shadowed = Vec::new();
                    if let Some(sym) = self.interface_symbol(i) {
                        let decls: Vec<&InterfaceDecl> = self
                            .symbol(sym)
                            .decls
                            .iter()
                            .filter_map(|decl| match decl {
                                crate::binder::Decl::Interface(other) => Some(*other),
                                _ => None,
                            })
                            .collect();
                        for decl in decls {
                            self.collect_type_param_refs_in_interface_decl(
                                &group_names,
                                &mut shadowed,
                                decl,
                                &mut refs,
                            );
                        }
                    } else {
                        self.collect_type_param_refs_in_interface_decl(
                            &group_names,
                            &mut shadowed,
                            i,
                            &mut refs,
                        );
                    }
                    groups[group_idx].refs.extend(refs);
                }
                for ext in &i.extends {
                    if let Some(args) = &ext.type_args {
                        for arg in args {
                            self.collect_type_param_groups_in_type(file, arg, groups);
                        }
                    }
                }
                self.collect_type_param_groups_in_type_members(file, &i.members, groups);
            }
            Stmt::TypeAlias(t) => {
                let group_idx = if Self::is_intrinsic_type_alias_body(&t.ty) {
                    None
                } else {
                    self.collect_type_param_group(file, t.type_params.as_ref(), groups)
                };
                if let Some(group_idx) = group_idx {
                    let group_names: Vec<String> = groups[group_idx]
                        .params
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                    let mut refs = Vec::new();
                    let mut shadowed = Vec::new();
                    self.collect_type_param_refs_in_type(
                        &group_names,
                        &mut shadowed,
                        &t.ty,
                        &mut refs,
                    );
                    groups[group_idx].refs.extend(refs);
                }
                self.collect_type_param_groups_in_type(file, &t.ty, groups);
            }
            Stmt::Enum(e) => {
                for m in &e.members {
                    if let Some(init) = &m.init {
                        self.collect_type_param_groups_in_expr(file, init, groups);
                    }
                }
            }
            Stmt::Namespace(n) => self.collect_type_param_groups_in_stmts(file, &n.body, groups),
            Stmt::With { obj, body, .. } => {
                self.collect_type_param_groups_in_expr(file, obj, groups);
                self.collect_type_param_groups_in_stmt(file, body, groups);
            }
            Stmt::Return { expr, .. } => {
                if let Some(expr) = expr {
                    self.collect_type_param_groups_in_expr(file, expr, groups);
                }
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.collect_type_param_groups_in_expr(file, cond, groups);
                self.collect_type_param_groups_in_stmt(file, then, groups);
                if let Some(els) = els {
                    self.collect_type_param_groups_in_stmt(file, els, groups);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.collect_type_param_groups_in_expr(file, cond, groups);
                self.collect_type_param_groups_in_stmt(file, body, groups);
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.collect_type_param_groups_in_stmt(file, body, groups);
                self.collect_type_param_groups_in_expr(file, cond, groups);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                if let Some(init) = init {
                    self.collect_type_param_groups_in_for_init(file, init, groups);
                }
                if let Some(cond) = cond {
                    self.collect_type_param_groups_in_expr(file, cond, groups);
                }
                if let Some(incr) = incr {
                    self.collect_type_param_groups_in_expr(file, incr, groups);
                }
                self.collect_type_param_groups_in_stmt(file, body, groups);
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                self.collect_type_param_groups_in_for_init(file, left, groups);
                self.collect_type_param_groups_in_expr(file, expr, groups);
                self.collect_type_param_groups_in_stmt(file, body, groups);
            }
            Stmt::Block(b) => self.collect_type_param_groups_in_stmts(file, &b.stmts, groups),
            Stmt::Expr { expr, .. } | Stmt::Throw { expr, .. } => {
                self.collect_type_param_groups_in_expr(file, expr, groups);
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                self.collect_type_param_groups_in_stmts(file, &block.stmts, groups);
                if let Some(catch) = catch {
                    if let Some(param) = &catch.param {
                        self.collect_type_param_groups_in_param(file, param, groups);
                    }
                    self.collect_type_param_groups_in_stmts(file, &catch.block.stmts, groups);
                }
                if let Some(finally) = finally {
                    self.collect_type_param_groups_in_stmts(file, &finally.stmts, groups);
                }
            }
            Stmt::Switch { expr, cases, .. } => {
                self.collect_type_param_groups_in_expr(file, expr, groups);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.collect_type_param_groups_in_expr(file, test, groups);
                    }
                    self.collect_type_param_groups_in_stmts(file, &case.stmts, groups);
                }
            }
            Stmt::Labeled { stmt, .. } => {
                self.collect_type_param_groups_in_stmt(file, stmt, groups);
            }
            Stmt::ExportDefault { expr, .. } | Stmt::ExportAssign { expr, .. } => {
                self.collect_type_param_groups_in_expr(file, expr, groups);
            }
            Stmt::Empty { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Import(_)
            | Stmt::ExportNamed(_)
            | Stmt::ImportEquals { .. }
            | Stmt::Missing { .. } => {}
        }
    }

    fn collect_type_param_groups_in_for_init(
        &self,
        file: usize,
        init: &'a ForInit,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        match init {
            ForInit::Var(v) => self.collect_type_param_groups_in_var_stmt(file, v, groups),
            ForInit::Expr(e) => self.collect_type_param_groups_in_expr(file, e, groups),
        }
    }

    fn collect_type_param_groups_in_var_stmt(
        &self,
        file: usize,
        v: &'a VarStmt,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        for d in &v.decls {
            if let Some(ty) = &d.ty {
                self.collect_type_param_groups_in_type(file, ty, groups);
            }
            if let Some(init) = &d.init {
                self.collect_type_param_groups_in_expr(file, init, groups);
            }
        }
    }

    fn collect_type_param_groups_in_function(
        &self,
        file: usize,
        f: &'a FunctionLike,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        self.collect_type_param_groups_in_function_with_mode(file, f, groups, true);
    }

    fn collect_type_param_groups_in_function_with_mode(
        &self,
        file: usize,
        f: &'a FunctionLike,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
        check_own_type_params: bool,
    ) {
        if check_own_type_params && self.should_check_function_like_type_params(f) {
            let group_idx = self.collect_type_param_group(file, f.type_params.as_ref(), groups);
            self.collect_type_param_refs_for_group_signature(
                groups.as_mut_slice(),
                group_idx,
                &f.params,
                f.return_type.as_ref(),
            );
            self.collect_type_param_refs_for_group_function_body(
                groups.as_mut_slice(),
                group_idx,
                f,
            );
        }
        for p in &f.params {
            self.collect_type_param_groups_in_param(file, p, groups);
        }
        if let Some(rt) = &f.return_type {
            self.collect_type_param_groups_in_type(file, rt, groups);
        }
        match &f.body {
            Some(FuncBody::Block(b)) => {
                self.collect_type_param_groups_in_stmts(file, &b.stmts, groups)
            }
            Some(FuncBody::Expr(e)) => self.collect_type_param_groups_in_expr(file, e, groups),
            None => {}
        }
    }

    fn collect_type_param_groups_in_class(
        &self,
        file: usize,
        c: &'a ClassDecl,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        let group_idx = self.collect_type_param_group(file, c.type_params.as_ref(), groups);
        if let Some(group_idx) = group_idx {
            groups[group_idx].refs_only = true;
            let group_names: Vec<String> = groups[group_idx]
                .params
                .iter()
                .map(|p| p.name.clone())
                .collect();
            let mut refs = Vec::new();
            let mut shadowed = Vec::new();
            if let Some(ext) = &c.extends {
                self.collect_type_param_refs_in_expr(
                    &group_names,
                    &mut shadowed,
                    &ext.expr,
                    &mut refs,
                );
                if let Some(args) = &ext.type_args {
                    for arg in args {
                        self.collect_type_param_refs_in_type(
                            &group_names,
                            &mut shadowed,
                            arg,
                            &mut refs,
                        );
                    }
                }
            }
            for imp in &c.implements {
                if let Some(args) = &imp.type_args {
                    for arg in args {
                        self.collect_type_param_refs_in_type(
                            &group_names,
                            &mut shadowed,
                            arg,
                            &mut refs,
                        );
                    }
                }
            }
            for member in &c.members {
                self.collect_type_param_refs_in_class_member(
                    &group_names,
                    &mut shadowed,
                    member,
                    &mut refs,
                );
            }
            groups[group_idx].refs.extend(refs);
        }
        if let Some(ext) = &c.extends {
            self.collect_type_param_groups_in_expr(file, &ext.expr, groups);
            if let Some(args) = &ext.type_args {
                for arg in args {
                    self.collect_type_param_groups_in_type(file, arg, groups);
                }
            }
        }
        for imp in &c.implements {
            if let Some(args) = &imp.type_args {
                for arg in args {
                    self.collect_type_param_groups_in_type(file, arg, groups);
                }
            }
        }
        for m in &c.members {
            match m {
                ClassMember::StaticBlock(b) => {
                    self.collect_type_param_groups_in_stmts(file, &b.stmts, groups)
                }
                ClassMember::Property(p) => {
                    if let Some(ty) = &p.ty {
                        self.collect_type_param_groups_in_type(file, ty, groups);
                    }
                    if let Some(init) = &p.init {
                        self.collect_type_param_groups_in_expr(file, init, groups);
                    }
                }
                ClassMember::Method(f) | ClassMember::Constructor(f) => {
                    self.collect_type_param_groups_in_function(file, f, groups)
                }
                ClassMember::Index(ix) => {
                    self.collect_type_param_groups_in_type(file, &ix.key_type, groups);
                    self.collect_type_param_groups_in_type(file, &ix.value_type, groups);
                }
            }
        }
    }

    fn collect_type_param_groups_in_param(
        &self,
        file: usize,
        p: &'a Param,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        if let Some(ty) = &p.ty {
            self.collect_type_param_groups_in_type(file, ty, groups);
        }
        if let Some(init) = &p.initializer {
            self.collect_type_param_groups_in_expr(file, init, groups);
        }
    }

    fn collect_type_param_groups_in_type_members(
        &self,
        file: usize,
        members: &'a [TypeMember],
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        let mut last_overload: HashMap<(u8, Option<String>), usize> = HashMap::new();
        for (idx, member) in members.iter().enumerate() {
            if let Some(key) = Self::type_member_overload_key(member) {
                last_overload.insert(key, idx);
            }
        }
        for (idx, m) in members.iter().enumerate() {
            match m {
                TypeMember::Prop(p) => {
                    if let Some(ty) = &p.ty {
                        self.collect_type_param_groups_in_type(file, ty, groups);
                    }
                }
                TypeMember::Method(m) => {
                    let group_idx =
                        if Self::type_member_overload_key(&TypeMember::Method(m.clone()))
                            .and_then(|key| last_overload.get(&key).copied())
                            == Some(idx)
                        {
                            self.collect_type_param_group(file, m.type_params.as_ref(), groups)
                        } else {
                            None
                        };
                    self.collect_type_param_refs_for_group_signature(
                        groups.as_mut_slice(),
                        group_idx,
                        &m.params,
                        m.return_type.as_ref(),
                    );
                    for p in &m.params {
                        self.collect_type_param_groups_in_param(file, p, groups);
                    }
                    if let Some(rt) = &m.return_type {
                        self.collect_type_param_groups_in_type(file, rt, groups);
                    }
                }
                TypeMember::Call(s) | TypeMember::Ctor(s) => {
                    let group_idx = if Self::type_member_overload_key(m)
                        .and_then(|key| last_overload.get(&key).copied())
                        == Some(idx)
                    {
                        self.collect_type_param_group(file, s.type_params.as_ref(), groups)
                    } else {
                        None
                    };
                    self.collect_type_param_refs_for_group_signature(
                        groups.as_mut_slice(),
                        group_idx,
                        &s.params,
                        s.return_type.as_ref(),
                    );
                    for p in &s.params {
                        self.collect_type_param_groups_in_param(file, p, groups);
                    }
                    if let Some(rt) = &s.return_type {
                        self.collect_type_param_groups_in_type(file, rt, groups);
                    }
                }
                TypeMember::Index(ix) => {
                    self.collect_type_param_groups_in_type(file, &ix.key_type, groups);
                    self.collect_type_param_groups_in_type(file, &ix.value_type, groups);
                }
            }
        }
    }

    fn type_member_overload_key(member: &TypeMember) -> Option<(u8, Option<String>)> {
        match member {
            TypeMember::Method(m) => Some((0, m.name.text())),
            TypeMember::Call(_) => Some((1, None)),
            TypeMember::Ctor(_) => Some((2, None)),
            _ => None,
        }
    }

    fn collect_type_param_groups_in_type(
        &self,
        file: usize,
        ty: &'a TypeNode,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        match ty {
            TypeNode::Ref(r) => {
                if let Some(args) = &r.type_args {
                    for arg in args {
                        self.collect_type_param_groups_in_type(file, arg, groups);
                    }
                }
            }
            TypeNode::Array { elem, .. }
            | TypeNode::Paren { inner: elem, .. }
            | TypeNode::Keyof { ty: elem, .. }
            | TypeNode::ReadonlyOp { ty: elem, .. } => {
                self.collect_type_param_groups_in_type(file, elem, groups)
            }
            TypeNode::Tuple { elems, .. } => {
                for elem in elems {
                    self.collect_type_param_groups_in_type(file, &elem.ty, groups);
                }
            }
            TypeNode::Union { members, .. } | TypeNode::Intersection { members, .. } => {
                for m in members {
                    self.collect_type_param_groups_in_type(file, m, groups);
                }
            }
            TypeNode::Function(f) | TypeNode::Ctor(f) => {
                let group_idx = self.collect_type_param_group(file, f.type_params.as_ref(), groups);
                self.collect_type_param_refs_for_group_signature(
                    groups.as_mut_slice(),
                    group_idx,
                    &f.params,
                    Some(&f.return_type),
                );
                for p in &f.params {
                    self.collect_type_param_groups_in_param(file, p, groups);
                }
                self.collect_type_param_groups_in_type(file, &f.return_type, groups);
            }
            TypeNode::TypeLiteral { members, .. } => {
                self.collect_type_param_groups_in_type_members(file, members, groups)
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                self.collect_type_param_groups_in_type(file, obj, groups);
                self.collect_type_param_groups_in_type(file, index, groups);
            }
            TypeNode::Conditional(c) => {
                self.collect_type_param_groups_in_type(file, &c.check, groups);
                self.collect_type_param_groups_in_type(file, &c.extends_ty, groups);
                self.collect_type_param_groups_in_type(file, &c.true_ty, groups);
                self.collect_type_param_groups_in_type(file, &c.false_ty, groups);
            }
            TypeNode::Predicate { ty: Some(ty), .. } => {
                self.collect_type_param_groups_in_type(file, ty, groups)
            }
            TypeNode::Mapped(m) => {
                self.collect_type_param_groups_in_type(file, &m.constraint, groups);
                if let Some(name_type) = &m.name_type {
                    self.collect_type_param_groups_in_type(file, name_type, groups);
                }
                if let Some(value) = &m.value {
                    self.collect_type_param_groups_in_type(file, value, groups);
                }
            }
            TypeNode::TemplateLit { parts, .. } => {
                for (part, _) in parts {
                    self.collect_type_param_groups_in_type(file, part, groups);
                }
            }
            TypeNode::Infer {
                constraint: Some(constraint),
                ..
            } => {
                self.collect_type_param_groups_in_type(file, constraint, groups);
            }
            TypeNode::TypeQuery { type_args, .. } => {
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_groups_in_type(file, arg, groups);
                    }
                }
            }
            TypeNode::Keyword(..)
            | TypeNode::This(_)
            | TypeNode::LiteralString { .. }
            | TypeNode::LiteralNumber { .. }
            | TypeNode::LiteralBigInt { .. }
            | TypeNode::LiteralBool { .. }
            | TypeNode::Predicate { ty: None, .. }
            | TypeNode::Infer {
                constraint: None, ..
            } => {}
        }
    }

    fn collect_type_param_groups_in_expr(
        &self,
        file: usize,
        expr: &'a Expr,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        match expr {
            Expr::Array { elements, .. } => {
                for e in elements {
                    self.collect_type_param_groups_in_expr(file, e, groups);
                }
            }
            Expr::Object { props, .. } => {
                for p in props {
                    match p {
                        ObjectProp::Property { value, .. } => {
                            self.collect_type_param_groups_in_expr(file, value, groups)
                        }
                        ObjectProp::Method(f) => self
                            .collect_type_param_groups_in_function_with_mode(
                                file, f, groups, false,
                            ),
                        ObjectProp::Spread { expr, .. } => {
                            self.collect_type_param_groups_in_expr(file, expr, groups)
                        }
                        ObjectProp::Shorthand { .. } => {}
                    }
                }
            }
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.collect_type_param_groups_in_function_with_mode(file, f, groups, false)
            }
            Expr::ClassExpr(c) => self.collect_type_param_groups_in_class(file, c, groups),
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                self.collect_type_param_groups_in_expr(file, callee, groups);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_groups_in_type(file, arg, groups);
                    }
                }
                for arg in args {
                    self.collect_type_param_groups_in_expr(file, arg, groups);
                }
            }
            Expr::New {
                callee,
                type_args,
                args,
                ..
            } => {
                self.collect_type_param_groups_in_expr(file, callee, groups);
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.collect_type_param_groups_in_type(file, arg, groups);
                    }
                }
                if let Some(args) = args {
                    for arg in args {
                        self.collect_type_param_groups_in_expr(file, arg, groups);
                    }
                }
            }
            Expr::PropAccess { obj, .. } => {
                self.collect_type_param_groups_in_expr(file, obj, groups)
            }
            Expr::ElemAccess { obj, index, .. } => {
                self.collect_type_param_groups_in_expr(file, obj, groups);
                self.collect_type_param_groups_in_expr(file, index, groups);
            }
            Expr::Unary { operand, .. }
            | Expr::Update { operand, .. }
            | Expr::Paren { inner: operand, .. }
            | Expr::NonNull { expr: operand, .. }
            | Expr::Spread { expr: operand, .. }
            | Expr::Await { expr: operand, .. } => {
                self.collect_type_param_groups_in_expr(file, operand, groups)
            }
            Expr::Binary { left, right, .. } => {
                self.collect_type_param_groups_in_expr(file, left, groups);
                self.collect_type_param_groups_in_expr(file, right, groups);
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                self.collect_type_param_groups_in_expr(file, cond, groups);
                self.collect_type_param_groups_in_expr(file, when_true, groups);
                self.collect_type_param_groups_in_expr(file, when_false, groups);
            }
            Expr::Assertion { expr, ty, .. } => {
                self.collect_type_param_groups_in_expr(file, expr, groups);
                self.collect_type_param_groups_in_type(file, ty, groups);
            }
            Expr::Yield { expr: Some(e), .. } => {
                self.collect_type_param_groups_in_expr(file, e, groups)
            }
            Expr::ImportCall { args, .. } => {
                for arg in args {
                    self.collect_type_param_groups_in_expr(file, arg, groups);
                }
            }
            Expr::JsxElement(e) => self.collect_type_param_groups_in_jsx(file, e, groups),
            Expr::Ident(_)
            | Expr::NumLit { .. }
            | Expr::StrLit { .. }
            | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. }
            | Expr::NullLit { .. }
            | Expr::RegexLit { .. }
            | Expr::Template { .. }
            | Expr::This { .. }
            | Expr::Super { .. }
            | Expr::Yield { expr: None, .. }
            | Expr::ImportMeta { .. }
            | Expr::Missing { .. } => {}
        }
    }

    fn collect_type_param_groups_in_jsx(
        &self,
        file: usize,
        jsx: &'a JsxElement,
        groups: &mut Vec<TypeParamUsageGroup<'a>>,
    ) {
        for attr in &jsx.attrs {
            if let Some(value) = &attr.value {
                self.collect_type_param_groups_in_expr(file, value, groups);
            }
        }
        for child in &jsx.children {
            match child {
                JsxChild::Element(e) => self.collect_type_param_groups_in_jsx(file, e, groups),
                JsxChild::Expr(e) => self.collect_type_param_groups_in_expr(file, e, groups),
                JsxChild::Text => {}
            }
        }
    }

    /// 6198 (all destructured elements unused) / 6199 (all variables unused)
    fn check_unused_groups(&mut self) {
        let as_error = self.options.no_unused_locals;
        let mut grouped: HashSet<SymbolId> = HashSet::new();
        let pattern_groups = self.bind.pattern_groups.clone();
        let var_groups = self.bind.var_stmt_groups.clone();
        for (file, span, syms) in pattern_groups {
            if file == self.lib_file || syms.is_empty() {
                continue;
            }
            // tsc reports 6198 ("all destructured elements are unused") for a
            // fully-unused destructuring *parameter* even as a suggestion, but
            // never for a destructuring *variable* unless noUnusedLocals is on —
            // it does not surface unused locals as suggestions (an unused
            // `const a = 1` is silent too). Gate the variable case accordingly.
            let is_param = matches!(
                self.bind.symbols[syms[0].0 as usize].decls.first(),
                Some(crate::binder::Decl::PatternParam(_))
            );
            if !is_param && !as_error {
                continue;
            }
            if syms.iter().all(|s| {
                !self.symuse.used_symbols.contains(s) && !self.symuse.assigned_symbols.contains(s)
            }) {
                for s in &syms {
                    grouped.insert(*s);
                }
                let prev = self.current_file;
                self.current_file = file;
                self.unused_diag(
                    span,
                    &gen::All_destructured_elements_are_unused,
                    &[],
                    as_error,
                );
                self.current_file = prev;
            }
        }
        for (file, span, syms) in var_groups {
            if file == self.lib_file || syms.len() < 2 {
                continue;
            }
            if syms.iter().all(|s| {
                !self.symuse.used_symbols.contains(s) && !self.symuse.assigned_symbols.contains(s)
            }) {
                for s in &syms {
                    grouped.insert(*s);
                }
                let prev = self.current_file;
                self.current_file = file;
                self.unused_diag(span, &gen::All_variables_are_unused, &[], as_error);
                self.current_file = prev;
            }
        }
        // suppress per-symbol 6133 for grouped reports
        for s in grouped {
            self.symuse.used_symbols.insert(s);
        }
    }

    fn check_unused_private_members(&mut self) {
        let mut to_report: Vec<(usize, Span, String, bool)> = Vec::new();
        for i in 0..self.bind.symbols.len() {
            let sym = SymbolId(i as u32);
            let s = &self.bind.symbols[i];
            if s.file == self.lib_file || self.symuse.used_symbols.contains(&sym) {
                continue;
            }
            let Some(decl) = s.decls.first().copied() else {
                continue;
            };
            // (is_private_member, is_parameter_property)
            let (is_private_member, is_param_prop) = match decl {
                crate::binder::Decl::PropertyDecl(p) => (
                    p.modifiers
                        .iter()
                        .any(|m| m.kind == crate::ast::ModifierKind::Private),
                    false,
                ),
                crate::binder::Decl::Method(f) => (
                    f.modifiers
                        .iter()
                        .any(|m| m.kind == crate::ast::ModifierKind::Private),
                    false,
                ),
                // a private parameter-property declares a class member (flagged
                // PROPERTY) alongside the parameter symbol; tsc reports the
                // unused member as TS6138. Only the member side carries PROPERTY.
                crate::binder::Decl::Param(p) if s.flags & crate::binder::flags::PROPERTY != 0 => (
                    p.modifiers
                        .iter()
                        .any(|m| m.kind == crate::ast::ModifierKind::Private),
                    true,
                ),
                _ => (false, false),
            };
            if is_private_member {
                to_report.push((s.file, decl.name_span(), s.name.clone(), is_param_prop));
            }
        }
        let as_error = self.options.no_unused_locals;
        for (file, span, name, is_param_prop) in to_report {
            let prev = self.current_file;
            self.current_file = file;
            let msg = if is_param_prop {
                &gen::Property_0_is_declared_but_its_value_is_never_read
            } else {
                &gen::_0_is_declared_but_its_value_is_never_read
            };
            self.unused_diag(span, msg, &[name], as_error);
            self.current_file = prev;
        }
    }

    /// An `infer X` declared in a conditional's extends clause but never
    /// referenced in the true branch is dead — tsc surfaces this as TS6133
    /// (a suggestion). Scans type-alias bodies for conditionals.
    fn check_unused_infer_params(&mut self) {
        let as_error = self.options.no_unused_locals;
        let mut to_report: Vec<(usize, Span, String)> = Vec::new();
        for i in 0..self.bind.symbols.len() {
            let s = &self.bind.symbols[i];
            if s.file == self.lib_file {
                continue;
            }
            let Some(crate::binder::Decl::Alias(a)) = s.decls.first().copied() else {
                continue;
            };
            let mut conds: Vec<&ConditionalTypeNode> = Vec::new();
            walk_type(&a.ty, &mut |n| {
                if let TypeNode::Conditional(c) = n {
                    conds.push(c);
                }
            });
            for c in conds {
                let mut infers: Vec<(&str, Span)> = Vec::new();
                walk_type(&c.extends_ty, &mut |n| {
                    if let TypeNode::Infer { name, span, .. } = n {
                        infers.push((name.name.as_str(), *span));
                    }
                });
                if infers.is_empty() {
                    continue;
                }
                let mut refs: Vec<&str> = Vec::new();
                walk_type(&c.true_ty, &mut |n| {
                    if let TypeNode::Ref(r) = n {
                        if r.name.parts.len() == 1 {
                            refs.push(r.name.parts[0].name.as_str());
                        }
                    }
                });
                for (name, span) in infers {
                    if !refs.contains(&name) {
                        to_report.push((s.file, span, name.to_string()));
                    }
                }
            }
        }
        for (file, span, name) in to_report {
            let prev = self.current_file;
            self.current_file = file;
            self.unused_diag(
                span,
                &gen::_0_is_declared_but_its_value_is_never_read,
                &[name],
                as_error,
            );
            self.current_file = prev;
        }
    }

    fn check_unused_imports(&mut self) {
        use std::collections::HashMap as Map;
        // group import alias symbols by their import declaration
        let mut groups: Map<usize, (usize, Span, Vec<(SymbolId, String, Span)>)> = Map::new();
        for i in 0..self.bind.symbols.len() {
            let sym = SymbolId(i as u32);
            let s = &self.bind.symbols[i];
            if s.flags & flags::ALIAS == 0 || s.file == self.lib_file {
                continue;
            }
            let Some(decl) = s.decls.first().copied() else {
                continue;
            };
            let (idecl_key, idecl_span, name, name_span, file) = match decl {
                crate::binder::Decl::Import(spec, idecl) => (
                    crate::ast::node_key(idecl),
                    idecl.span,
                    spec.name.name.clone(),
                    spec.name.span,
                    s.file,
                ),
                crate::binder::Decl::ImportDefault(idecl) => {
                    let n = idecl.default_name.as_ref().unwrap();
                    (
                        crate::ast::node_key(idecl),
                        idecl.span,
                        n.name.clone(),
                        n.span,
                        s.file,
                    )
                }
                crate::binder::Decl::ImportNamespace(idecl) => {
                    let n = idecl.namespace_name.as_ref().unwrap();
                    (
                        crate::ast::node_key(idecl),
                        idecl.span,
                        n.name.clone(),
                        n.span,
                        s.file,
                    )
                }
                _ => continue,
            };
            let entry = groups
                .entry(idecl_key)
                .or_insert((file, idecl_span, Vec::new()));
            entry.2.push((sym, name, name_span));
        }
        let mut to_report: Vec<(usize, Span, Option<(String, Span)>)> = Vec::new();
        for (_k, (file, ispan, specs)) in groups {
            let unused: Vec<&(SymbolId, String, Span)> = specs
                .iter()
                .filter(|(s, _, _)| !self.symuse.used_symbols.contains(s))
                .collect();
            if unused.is_empty() {
                continue;
            }
            if unused.len() == specs.len() {
                to_report.push((file, ispan, None));
            } else {
                for (_, n, sp) in unused {
                    to_report.push((file, *sp, Some((n.clone(), *sp))));
                }
            }
        }
        let imports_as_error = self.options.no_unused_locals;
        for (file, span, detail) in to_report {
            let prev = self.current_file;
            self.current_file = file;
            match detail {
                None => self.unused_diag(
                    span,
                    &gen::All_imports_in_import_declaration_are_unused,
                    &[],
                    imports_as_error,
                ),
                Some((name, nspan)) => self.unused_diag(
                    nspan,
                    &gen::_0_is_declared_but_its_value_is_never_read,
                    &[name],
                    imports_as_error,
                ),
            }
            self.current_file = prev;
        }
    }

    /// Emit an unused-code diagnostic. When its controlling flag (noUnusedLocals
    /// / noUnusedParameters) is on it is an error; otherwise it is a tsc-style
    /// *suggestion* (category 2 — the editor "faded" hint), matching tsc's
    /// getSuggestionDiagnostics. `reportsUnnecessary` comes from the message.
    fn diagnostic_at(
        &mut self,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
        category_override: Option<Category>,
    ) {
        let mut chain = MessageChain::new(msg, args);
        if let Some(category) = category_override {
            chain.category = category;
        }
        self.diags.push(Diagnostic {
            file: Some(self.current_file),
            start: span.start,
            length: span.len(),
            message: chain,
            related: Vec::new(),
        });
    }

    /// Emit an unused-code diagnostic. When its controlling flag (noUnusedLocals
    /// / noUnusedParameters) is on it is an error; otherwise it is a tsc-style
    /// *suggestion* (category 2 — the editor "faded" hint), matching tsc's
    /// getSuggestionDiagnostics. `reportsUnnecessary` comes from the message.
    fn unused_diag(
        &mut self,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
        as_error: bool,
    ) {
        let category = if as_error {
            None
        } else {
            Some(Category::Suggestion)
        };
        self.diagnostic_at(span, msg, args, category);
    }

    // --- Scoped traversal-stack guards -------------------------------------
    //
    // These combinators push a context frame, run `f`, and pop it on *every*
    // return path out of `f` — including an early `return` inside the closure.
    // That is the property hand-balanced `push(...); ...; pop();` pairs cannot
    // guarantee: the Phase-1 "600 new FPs from one missing pop" regression was
    // exactly a leaked frame. Prefer these over raw `stacks.*.push/pop`.
    //
    // They are intentionally NOT panic-safe (a panic in `f` skips the pop): the
    // batch harness runs each file under `catch_unwind` and discards the
    // panicked file's `Checker`, so a leaked frame can never outlive the panic.
    // Keeping them panic-oblivious avoids a `Drop` guard, which would need a raw
    // pointer into `self` (this crate is `unsafe`-free) or would borrow-lock
    // `self` for the whole scope (the body needs `&mut self`).

    /// Push `sym` on `this_type_stack` for the duration of `f`.
    pub(crate) fn with_this_type<R>(
        &mut self,
        sym: SymbolId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.stacks.this_type_stack.push(sym);
        let r = f(self);
        self.stacks.this_type_stack.pop();
        r
    }

    /// Like `with_this_type`, but the push is elided when `sym` is `None`
    /// (the frame count is unchanged, so nested reads see the same stack).
    pub(crate) fn with_opt_this_type<R>(
        &mut self,
        sym: Option<SymbolId>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        match sym {
            Some(s) => self.with_this_type(s, f),
            None => f(self),
        }
    }

    /// Push a `ThisContainer` on `this_container_stack` for the duration of `f`.
    pub(crate) fn with_this_container<R>(
        &mut self,
        tc: ThisContainer,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.stacks.this_container_stack.push(tc);
        let r = f(self);
        self.stacks.this_container_stack.pop();
        r
    }

    /// Like `with_this_container`, but the push is elided when `tc` is `None`.
    pub(crate) fn with_opt_this_container<R>(
        &mut self,
        tc: Option<ThisContainer>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        match tc {
            Some(c) => self.with_this_container(c, f),
            None => f(self),
        }
    }

    /// Push a constructor-field context for the duration of `f` (elided when
    /// `ctx` is `None`). Classic class-field initializers/type annotations use
    /// this to reject constructor-scope references.
    pub(crate) fn with_ctor_field<R>(
        &mut self,
        ctx: Option<CtorFieldContext>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        match ctx {
            Some(c) => {
                self.cflags.ctor_field_stack.push(c);
                let r = f(self);
                self.cflags.ctor_field_stack.pop();
                r
            }
            None => f(self),
        }
    }

    /// Push a namespace-body context for the duration of `f` — used by `this`
    /// resolution to distinguish a namespace-body `this` from a nested
    /// class/function `this`.
    pub(crate) fn with_namespace<R>(
        &mut self,
        ctx: NamespaceContext,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.cflags.namespace_stack.push(ctx);
        let r = f(self);
        self.cflags.namespace_stack.pop();
        r
    }

    /// Push the function-body frames — the active `FnCtx` on `fn_stack` and its
    /// paired `this_container_stack` entry — for the duration of `f`, popping
    /// both on every exit. These are always pushed and popped together at a
    /// function-body entry and `fn_stack` is used nowhere else, so guarding them
    /// here makes the bracketed discipline (whose violation caused the Phase-1
    /// regression) impossible to break with a stray early return. Pops mirror the
    /// original order (fn_stack then this_container); the two are independent
    /// vectors, so the order is immaterial to behavior.
    pub(crate) fn with_fn_ctx<R>(
        &mut self,
        fn_ctx: FnCtx,
        tc: ThisContainer,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.stacks.fn_stack.push(fn_ctx);
        self.stacks.this_container_stack.push(tc);
        let r = f(self);
        self.stacks.fn_stack.pop();
        self.stacks.this_container_stack.pop();
        r
    }

    /// Push the class-body frames — the class symbol on `class_stack` and its
    /// class-body `this_container_stack` entry — for the duration of `f`,
    /// popping both on every exit. Analogous to `with_fn_ctx`; `class_stack` is
    /// used only here. Pops mirror the original order (class_stack then
    /// this_container); the two are independent vectors, so the order is
    /// immaterial to behavior.
    pub(crate) fn with_class_body<R>(
        &mut self,
        class_sym: SymbolId,
        tc: ThisContainer,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.stacks.class_stack.push(class_sym);
        self.stacks.this_container_stack.push(tc);
        let r = f(self);
        self.stacks.class_stack.pop();
        self.stacks.this_container_stack.pop();
        r
    }

    pub fn error_at(&mut self, span: Span, msg: &'static DiagnosticMessage, args: &[String]) {
        self.error_at_with_related(span, msg, args, Vec::new());
    }

    pub(crate) fn suggestion_at(
        &mut self,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
    ) {
        self.diagnostic_at(span, msg, args, Some(Category::Suggestion));
    }

    pub(crate) fn is_canonical_decl_key(&self, key: usize) -> bool {
        let Some(sym) = self.bind.decl_symbol.get(&key).copied() else {
            return true;
        };
        let Some(first) = self.symbol(sym).decls.first() else {
            return true;
        };
        Self::decl_key(first) == Some(key)
    }

    fn decl_key(decl: &crate::binder::Decl<'a>) -> Option<usize> {
        use crate::binder::Decl;
        match decl {
            Decl::Var(d, _) => Some(node_key(*d)),
            Decl::PropertyDecl(d) => Some(node_key(*d)),
            Decl::Param(p) | Decl::CatchVar(p) => Some(node_key(*p)),
            Decl::Func(f) | Decl::Method(f) => Some(node_key(*f)),
            Decl::Class(c) => Some(node_key(*c)),
            Decl::Interface(i) => Some(node_key(*i)),
            Decl::Alias(a) => Some(node_key(*a)),
            Decl::PropSig(p) => Some(node_key(*p)),
            Decl::MethodSig(m) => Some(node_key(*m)),
            Decl::TypeParam(t) => Some(node_key(*t)),
            Decl::Enum(e) => Some(node_key(*e)),
            Decl::EnumMember(m) => Some(node_key(*m)),
            Decl::Namespace(n) => Some(node_key(*n)),
            Decl::ImportEquals(i, _) | Decl::PatternVar(i, _) | Decl::PatternParam(i) => {
                Some(node_key(*i))
            }
            Decl::Import(s, _) => Some(node_key(*s)),
            Decl::ImportDefault(i) | Decl::ImportNamespace(i) => Some(node_key(*i)),
            Decl::DefaultExport => None,
        }
    }

    pub fn error_at_with_related(
        &mut self,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
        related: Vec<RelatedInfo>,
    ) {
        self.diags.push(Diagnostic {
            file: Some(self.current_file),
            start: span.start,
            length: span.len(),
            message: MessageChain::new(msg, args),
            related,
        });
    }

    pub fn related_on_symbol_decl(
        &self,
        sym: SymbolId,
        msg: &'static DiagnosticMessage,
        args: &[String],
    ) -> Option<RelatedInfo> {
        let s = self.symbol(sym);
        let decl = s.decls.first()?;
        let span = decl.name_span();
        Some(RelatedInfo {
            file: Some(s.file),
            start: span.start,
            length: span.len(),
            message: MessageChain::new(msg, args),
        })
    }

    pub fn declared_here_related(&self, sym: SymbolId) -> Option<RelatedInfo> {
        let name = self.symbol(sym).name.clone();
        self.related_on_symbol_decl(sym, &gen::_0_is_declared_here, &[name])
    }

    pub fn error_at_declared_here(
        &mut self,
        span: Span,
        msg: &'static DiagnosticMessage,
        args: &[String],
        sym: SymbolId,
    ) {
        let related = self.declared_here_related(sym).into_iter().collect();
        self.error_at_with_related(span, msg, args, related);
    }

    /// Insert-and-test guard for a node-scoped diagnostic: returns true the first
    /// time `(code, node_key)` is seen this program, false afterwards. Central
    /// "emit this once per node" gate (see `ReportGuards::reported_once_node`).
    pub(crate) fn report_once_node(&mut self, code: u32, node_key: usize) -> bool {
        if self.fresolve.quiet > 0 {
            return false;
        }
        self.reported.reported_once_node.insert((code, node_key))
    }

    /// Insert-and-test guard for a symbol-scoped diagnostic: true the first time
    /// `(code, sym)` is seen, false afterwards (see `reported_once_sym`).
    pub(crate) fn report_once_sym(&mut self, code: u32, sym: SymbolId) -> bool {
        if self.fresolve.quiet > 0 {
            return false;
        }
        self.reported.reported_once_sym.insert((code, sym))
    }

    pub fn report_used_before_assigned(&mut self, span: Span, name: String) {
        if self.fresolve.quiet > 0 {
            return;
        }
        if self
            .reported
            .reported_2454
            .insert((self.current_file, span.start as usize))
        {
            self.error_at(
                span,
                &gen::Variable_0_is_used_before_being_assigned,
                &[name],
            );
        }
    }

    pub fn error_chain_at(&mut self, span: Span, chain: MessageChain) {
        self.diags.push(Diagnostic {
            file: Some(self.current_file),
            start: span.start,
            length: span.len(),
            message: chain,
            related: Vec::new(),
        });
    }

    pub fn symbol(&self, s: SymbolId) -> &crate::binder::Symbol<'a> {
        let i = s.0 as usize;
        let base = self.synth.base as usize;
        if i < base {
            &self.bind.symbols[i]
        } else {
            &self.synth.symbols[i - base]
        }
    }

    pub fn scope_at(&self, id: ScopeId) -> &crate::binder::Scope {
        let i = id.0 as usize;
        let base = self.synth.scope_base as usize;
        if i < base {
            &self.bind.scopes[i]
        } else {
            &self.synth.scopes[i - base]
        }
    }

    pub(crate) fn with_current_scope<R>(
        &mut self,
        scope: ScopeId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.current_scope;
        self.current_scope = scope;
        let r = f(self);
        self.current_scope = prev;
        r
    }

    /// Mint a checker-owned type-parameter symbol in the `synth` side table
    /// (not the shared immutable `bind.symbols`). Returns its offset `SymbolId`.
    fn alloc_synth_symbol(
        &mut self,
        name: String,
        decls: Vec<crate::binder::Decl<'a>>,
    ) -> SymbolId {
        self.synth.symbols.push(crate::binder::Symbol {
            name,
            flags: crate::binder::flags::TYPE_PARAM,
            decls,
            members: crate::binder::Table::default(),
            statics: crate::binder::Table::default(),
            file: self.current_file,
            dup_reported: false,
            parent: None,
        });
        SymbolId(self.synth.base + self.synth.symbols.len() as u32 - 1)
    }

    /// Mint a transient `TypeParams` scope (child of `parent`) holding `tps`, in
    /// the `synth` side table, and return its `ScopeId`. Resolving a signature's
    /// parameter/return types under the returned scope lets its type params
    /// resolve by ordinary lexical lookup — replacing the transient
    /// `infer_mapped_env` push/pop that real signature type params used to need.
    fn push_tp_scope(&mut self, parent: ScopeId, tps: &'a [TypeParamDecl]) -> ScopeId {
        let mut types = crate::binder::Table::default();
        for tp in tps {
            let sym = self.ensure_type_param_symbol(tp);
            types.insert(tp.name.name.clone(), sym);
        }
        self.synth.scopes.push(crate::binder::Scope {
            parent: Some(parent),
            kind: crate::binder::ScopeKind::TypeParams,
            values: crate::binder::Table::default(),
            types,
        });
        let id = ScopeId(self.synth.scope_base + self.synth.scopes.len() as u32 - 1);
        for tp in tps {
            self.synth.decl_scope.insert(crate::ast::node_key(tp), id);
        }
        id
    }

    // ── name resolution ─────────────────────────────────────────────────────

    pub fn lookup_value(&self, mut scope: ScopeId, name: &str) -> Option<SymbolId> {
        loop {
            let s = self.scope_at(scope);
            if let Some(id) = s.values.get(name) {
                return Some(id);
            }
            match s.parent {
                Some(p) => scope = p,
                None => return None,
            }
        }
    }

    pub fn lookup_type(&self, mut scope: ScopeId, name: &str) -> Option<SymbolId> {
        loop {
            let s = self.scope_at(scope);
            if let Some(id) = s.types.get(name) {
                return Some(id);
            }
            match s.parent {
                Some(p) => scope = p,
                None => return None,
            }
        }
    }

    /// tsc's "only refers to a type, but is being used as a value" selection
    pub fn resolve_value_ident(&mut self, id: &Ident, scope: ScopeId) -> Option<SymbolId> {
        if id.name.is_empty() {
            return None; // parse-error identifier
        }
        if let Some(sym) = self.lookup_value(scope, &id.name) {
            let f = self.bind.symbols[sym.0 as usize].flags;
            if f & flags::NAMESPACE != 0
                && f & (flags::FUNCTION | flags::CLASS | flags::ENUM) == 0
                && self.bind.symbols[sym.0 as usize].members.0.is_empty()
            {
                self.error_at(
                    id.span,
                    &gen::Cannot_use_namespace_0_as_a_value,
                    &[id.name.clone()],
                );
                return None;
            }
            self.check_use_before_declaration(sym, id);
            return Some(sym);
        }
        // type-only symbol used as a value?
        // 2663/2662: instance or static member of the enclosing class?
        if let Some(&cls) = self.stacks.class_stack.last() {
            if self.bind.symbols[cls.0 as usize]
                .members
                .get(&id.name)
                .is_some()
            {
                self.error_at(
                    id.span,
                    &gen::Cannot_find_name_0_Did_you_mean_the_instance_member_this_0,
                    &[id.name.clone()],
                );
                return None;
            }
            if self.bind.symbols[cls.0 as usize]
                .statics
                .get(&id.name)
                .is_some()
            {
                let cn = self.symbol(cls).name.clone();
                self.error_at(
                    id.span,
                    &gen::Cannot_find_name_0_Did_you_mean_the_static_member_1_0,
                    &[id.name.clone(), cn],
                );
                return None;
            }
        }
        // well-known global names get targeted suggestions (tsc's tables)
        if let Some((msg, args)) = special_global_suggestion(&id.name) {
            self.error_at(id.span, msg, &args);
            return None;
        }
        // suggestion
        let candidates = self.value_names_in_scope(scope);
        if let Some(suggestion) =
            spelling_suggestion(&id.name, candidates.iter().map(|s| s.as_str()))
        {
            let args = [id.name.clone(), suggestion.to_string()];
            if let Some(sug_sym) = self
                .lookup_value(scope, suggestion)
                .filter(|&s| self.symbol(s).flags & flags::VALUE != 0)
            {
                self.error_at_declared_here(
                    id.span,
                    &gen::Cannot_find_name_0_Did_you_mean_1,
                    &args,
                    sug_sym,
                );
            } else {
                self.error_at(id.span, &gen::Cannot_find_name_0_Did_you_mean_1, &args);
            }
        } else {
            self.error_at(id.span, &gen::Cannot_find_name_0, &[id.name.clone()]);
        }
        None
    }

    fn value_names_in_scope(&self, scope: ScopeId) -> Vec<String> {
        // innermost-out, declaration order; lib globals naturally come last
        // because the global scope is the outermost.
        let mut names = Vec::new();
        let mut cur = Some(scope);
        while let Some(s) = cur {
            let sc = self.scope_at(s);
            for (n, _) in sc.values.iter() {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
            cur = sc.parent;
        }
        names
    }

    pub fn check_use_before_declaration(&mut self, sym: SymbolId, id: &Ident) {
        let s = self.symbol(sym);
        // classes and enums have a TDZ too (2449/2450)
        if s.flags & flags::BLOCK_SCOPED_VARIABLE == 0 || s.flags & flags::PARAMETER != 0 {
            return;
        }
        if s.flags & flags::AMBIENT != 0 {
            return;
        }
        if s.file != self.current_file {
            return;
        }
        let Some(decl) = s.decls.first().copied() else {
            return;
        };
        let (decl_key, decl_end, kind) = match decl {
            crate::binder::Decl::Var(d, k) => (crate::ast::node_key(d), d.span.end, k),
            _ => return,
        };
        let decl_container = self
            .bind
            .decl_container
            .get(&decl_key)
            .copied()
            .unwrap_or(0);
        let use_container = self.stacks.fn_stack.last().map(|f| f.fn_key).unwrap_or(0);
        // tsc's conservative rule for 2454: a *top-level* `let` (decl_container
        // = 0 = module scope) read from inside any function body does not
        // fire 2454. Rationale: module-level bindings can be assigned by any
        // top-level statement before the inner function actually runs, so the
        // strict "before being assigned" claim is not reliable at read time.
        // A `let` declared inside a function body still fires 2454 from an
        // inner class or nested function (this converges with tsc's stricter
        // reading of "inner reads see the outer let uninitialized").
        if decl_container == 0 && use_container != 0 {
            return;
        }
        if id.span.start < decl_end {
            self.error_at_declared_here(
                id.span,
                &gen::Block_scoped_variable_0_used_before_its_declaration,
                &[id.name.clone()],
                sym,
            );
            if self.options.strict_null_checks() && kind == VarKind::Let {
                self.report_used_before_assigned(id.span, id.name.clone());
            }
        }
        // (definite assignment for post-declaration reads moved to the
        // CFG-seeded query — da_check_ident_read in flow/resolver.rs)
    }
}

/// tsc's targeted "Cannot find name" suggestions for well-known globals
fn special_global_suggestion(name: &str) -> Option<(&'static DiagnosticMessage, Vec<String>)> {
    const ES2015: &[&str] = &[
        "Map", "Set", "WeakMap", "WeakSet", "Proxy", "Reflect", "Symbol", "Iterator",
    ];
    const DOM: &[&str] = &[
        "document",
        "window",
        "alert",
        "confirm",
        "prompt",
        "navigator",
        "localStorage",
        "sessionStorage",
        "history",
        "fetch",
    ];
    const NODE: &[&str] = &[
        "require",
        "process",
        "Buffer",
        "module",
        "__dirname",
        "__filename",
        "global",
    ];
    const TEST: &[&str] = &[
        "describe",
        "suite",
        "it",
        "test",
        "expect",
        "beforeEach",
        "afterEach",
        "beforeAll",
        "afterAll",
    ];
    if ES2015.contains(&name) {
        return Some((
            &gen::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_1_or_later,
            vec![name.to_string(), "es2015".to_string()],
        ));
    }
    if DOM.contains(&name) {
        return Some((
            &gen::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_include_dom,
            vec![name.to_string()],
        ));
    }
    if NODE.contains(&name) {
        return Some((
            &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_node_Try_npm_i_save_dev_types_Slashnode_and_then_add_node_to_the_types_field_in_your_tsconfig,
            vec![name.to_string()],
        ));
    }
    if TEST.contains(&name) {
        return Some((
            &gen::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_a_test_runner_Try_npm_i_save_dev_types_Slashjest_or_npm_i_save_dev_types_Slashmocha_and_then_add_jest_or_mocha_to_the_types_field_in_your_tsconfig,
            vec![name.to_string()],
        ));
    }
    None
}

// ── spelling suggestions: port of tsc core.ts getSpellingSuggestion ────────

pub fn spelling_suggestion<'s>(
    name: &str,
    candidates: impl Iterator<Item = &'s str>,
) -> Option<&'s str> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let len = name_utf16.len() as f64;
    let maximum_length_difference = 2.0_f64.max((len * 0.34).floor());
    let mut best_distance = (len * 0.4).floor() + 1.0;
    let name_lower = name.to_lowercase();
    let mut best: Option<&'s str> = None;
    for cand in candidates {
        let cand_utf16_len = cand.encode_utf16().count();
        if !((cand_utf16_len as f64 - len).abs() <= maximum_length_difference) {
            continue;
        }
        if cand == name {
            continue;
        }
        if cand_utf16_len < 3 && cand.to_lowercase() != name_lower {
            continue;
        }
        let Some(distance) =
            levenshtein_with_max(&name_lower, &cand.to_lowercase(), best_distance - 0.1)
        else {
            continue;
        };
        debug_assert!(distance < best_distance);
        best_distance = distance;
        best = Some(cand);
    }
    best
}

fn levenshtein_with_max(s1: &str, s2: &str, max: f64) -> Option<f64> {
    let a: Vec<char> = s1.chars().collect();
    let b: Vec<char> = s2.chars().collect();
    let mut previous: Vec<f64> = (0..=b.len()).map(|i| i as f64).collect();
    let mut current: Vec<f64> = vec![0.0; b.len() + 1];
    let big = max + 0.01;
    for i in 1..=a.len() {
        let c1 = a[i - 1];
        let min_j = if i as f64 > max {
            (i as f64 - max).ceil() as usize
        } else {
            1
        };
        let max_j = if b.len() as f64 > max + i as f64 {
            (max + i as f64).floor() as usize
        } else {
            b.len()
        };
        current[0] = i as f64;
        let mut col_min = i as f64;
        for j in 1..min_j {
            current[j] = big;
        }
        for j in min_j..=max_j {
            // tsc compares case-insensitively here via prior lowercasing; the
            // 0.1 substitution cost applies to case-only differences upstream.
            let substitution = if a[i - 1].to_lowercase().eq(b[j - 1].to_lowercase()) {
                previous[j - 1] + 0.1
            } else {
                previous[j - 1] + 2.0
            };
            let dist = if c1 == b[j - 1] {
                previous[j - 1]
            } else {
                (previous[j] + 1.0)
                    .min(current[j - 1] + 1.0)
                    .min(substitution)
            };
            current[j] = dist;
            col_min = col_min.min(dist);
        }
        for j in (max_j + 1)..=b.len() {
            current[j] = big;
        }
        if col_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let res = previous[b.len()];
    if res > max {
        None
    } else {
        Some(res)
    }
}

/// Visit `node` and every nested type node, invoking `f` on each. Used to scan
/// for conditional nodes, `infer` declarations, and type references.
fn walk_type<'b>(node: &'b TypeNode, f: &mut dyn FnMut(&'b TypeNode)) {
    f(node);
    use TypeNode as T;
    match node {
        T::Ref(r) => {
            if let Some(args) = &r.type_args {
                for a in args {
                    walk_type(a, f);
                }
            }
        }
        T::TypeQuery {
            type_args: Some(args),
            ..
        } => {
            for a in args {
                walk_type(a, f);
            }
        }
        T::Array { elem, .. } => walk_type(elem, f),
        T::Paren { inner, .. } => walk_type(inner, f),
        T::Keyof { ty, .. } | T::ReadonlyOp { ty, .. } => walk_type(ty, f),
        T::Tuple { elems, .. } => {
            for e in elems {
                walk_type(&e.ty, f);
            }
        }
        T::Union { members, .. } | T::Intersection { members, .. } => {
            for m in members {
                walk_type(m, f);
            }
        }
        T::IndexedAccess { obj, index, .. } => {
            walk_type(obj, f);
            walk_type(index, f);
        }
        T::Conditional(c) => {
            walk_type(&c.check, f);
            walk_type(&c.extends_ty, f);
            walk_type(&c.true_ty, f);
            walk_type(&c.false_ty, f);
        }
        T::Predicate { ty: Some(ty), .. } => walk_type(ty, f),
        T::Infer {
            constraint: Some(ty),
            ..
        } => walk_type(ty, f),
        T::Function(ft) | T::Ctor(ft) => {
            for p in &ft.params {
                if let Some(ty) = &p.ty {
                    walk_type(ty, f);
                }
            }
            walk_type(&ft.return_type, f);
            if let Some(tps) = &ft.type_params {
                for tp in tps {
                    if let Some(c) = &tp.constraint {
                        walk_type(c, f);
                    }
                }
            }
        }
        T::Mapped(m) => {
            walk_type(&m.constraint, f);
            if let Some(nt) = &m.name_type {
                walk_type(nt, f);
            }
            if let Some(v) = &m.value {
                walk_type(v, f);
            }
        }
        T::TemplateLit { parts, .. } => {
            for (t, _) in parts {
                walk_type(t, f);
            }
        }
        T::TypeLiteral { members, .. } => {
            for mem in members {
                match mem {
                    TypeMember::Prop(p) => {
                        if let Some(ty) = &p.ty {
                            walk_type(ty, f);
                        }
                    }
                    TypeMember::Method(mt) => {
                        for p in &mt.params {
                            if let Some(ty) = &p.ty {
                                walk_type(ty, f);
                            }
                        }
                        if let Some(rt) = &mt.return_type {
                            walk_type(rt, f);
                        }
                    }
                    TypeMember::Call(s) | TypeMember::Ctor(s) => {
                        for p in &s.params {
                            if let Some(ty) = &p.ty {
                                walk_type(ty, f);
                            }
                        }
                        if let Some(rt) = &s.return_type {
                            walk_type(rt, f);
                        }
                    }
                    TypeMember::Index(ix) => {
                        walk_type(&ix.key_type, f);
                        walk_type(&ix.value_type, f);
                    }
                }
            }
        }
        _ => {}
    }
}
