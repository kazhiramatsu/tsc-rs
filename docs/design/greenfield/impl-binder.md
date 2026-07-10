# impl: binder (phase 3) — copy-level code

Companion to m2-binder-steps.md. Module: `crates/binder/src/`
(`symbols.rs`, `declare.rs`, `containers.rs`, `bind.rs`, `flow.rs`).
Phase 3 also lands the FIRST 2XXX emissions from `declareSymbol`'s
conflict branch: 2300 Duplicate identifier, 2451 block-scope
redeclare, 2567 enum-merge, 2528 multiple default exports (with the
Another_export_default_is_here / and_here /
The_first_export_default_is_here related chains and the
`Did_you_mean_0` "export type { X }" suggestion related), plus 2668
export-modifier-on-ambient-module from `bindModuleDeclaration`.
Build the pin fixtures for them in stage 3.2/3.3. (2440/2661 import
conflicts are CHECKER emissions, not binder — corrected 2026-07-10
against the vendored source: the binder region emits no import
conflict messages.)

## [COPY] Symbol model (stage 3.1)

```rust
pub struct Symbol {
    pub flags: SymbolFlags,               // generated, bit-compatible
    pub escaped_name: String,             // see escape below
    pub declarations: Vec<NodeId>,
    pub value_declaration: NodeId,        // INVALID until first VALUE decl
    pub members: SymbolTable,             // IndexMap — ORDERED, observable
    pub exports: SymbolTable,
    pub parent: SymbolId,
    pub export_symbol: SymbolId,          // local ↔ export link
    pub merged_into: SymbolId,            // getMergedSymbol chases
    pub const_enum_only_module: Option<bool>,
    pub is_replaceable_by_method: bool,
}
pub type SymbolTable = indexmap::IndexMap<String, SymbolId>;

/// tsc escapeLeadingUnderscores (_tsc.js 11438): a name beginning with
/// two underscores gains ONE more so user `__proto__` cannot collide
/// with internal names (`__call` etc. are stored pre-escaped).
pub fn escape_leading_underscores(name: &str) -> String {
    let b = name.as_bytes();
    if b.len() >= 2 && b[0] == b'_' && b[1] == b'_' { format!("_{name}") }
    else { name.to_string() }
}
pub fn unescape_leading_underscores(name: &str) -> &str {
    let b = name.as_bytes();
    if b.len() >= 3 && b[0] == b'_' && b[1] == b'_' && b[2] == b'_' { &name[1..] }
    else { name }
}
```

`InternalSymbolName` constants (`__call`, `__constructor`, `__new`,
`__index`, `__export`, `__global`, `__missing`, `__type`, `__object`,
`__jsxAttributes`, `__computed`, `default`, `__function`, `export=`,
`this`) come from M0 codegen.

## [COPY] declareSymbol (stage 3.2) — `_tsc.js` 42602

The merge engine, full control flow. The masks are DATA (generated
SymbolFlags `*Excludes` members); the only logic is below.

```rust
impl Binder<'_> {
    pub fn declare_symbol(&mut self, table_owner: TableRef, parent: SymbolId,
            node: NodeId, includes: SymbolFlags, excludes: SymbolFlags,
            is_replaceable_by_method: bool, is_computed_name: bool) -> SymbolId {
        debug_assert!(is_computed_name || !self.has_dynamic_name(node));
        let is_default_export = self.has_syntactic_modifier(node, ModifierFlags::DEFAULT)
            || self.is_export_specifier_named_default(node);

        let name: Option<String> = if is_computed_name {
            Some(InternalSymbolName::COMPUTED.into())
        } else if is_default_export && parent != SymbolId::INVALID {
            Some(InternalSymbolName::DEFAULT.into())
        } else {
            self.get_declaration_name(node)      // None ⇒ __missing path below
        };

        let symbol = match name {
            None => self.create_symbol(SymbolFlags::empty(), InternalSymbolName::MISSING.into()),
            Some(name) => {
                match self.table(table_owner).get(&name).copied() {
                    None => {
                        let s = self.create_symbol(SymbolFlags::empty(), name.clone());
                        self.table_mut(table_owner).insert(name, s);
                        if is_replaceable_by_method { self.symbol_mut(s).is_replaceable_by_method = true; }
                        s
                    }
                    Some(existing) if is_replaceable_by_method
                        && !self.symbol(existing).is_replaceable_by_method => {
                        return existing;   // method cannot replace non-replaceable
                    }
                    Some(existing) if self.symbol(existing).flags.intersects(excludes) => {
                        if self.symbol(existing).is_replaceable_by_method {
                            // ok to replace a synthetic method binding
                            let s = self.create_symbol(SymbolFlags::empty(), name.clone());
                            self.table_mut(table_owner).insert(name, s);
                            s
                        } else if !(includes.intersects(SymbolFlags::VARIABLE)
                                    && self.symbol(existing).flags.contains(SymbolFlags::ASSIGNMENT)) {
                            // CONFLICT: report per tsc's message selection —
                            //   block-scoped kinds → 2451 Cannot_redeclare_block_scoped_variable
                            //   enum vs enum-incompatible merge → 2567
                            //   default exports → 2528/2652 family
                            //   else → 2300 Duplicate_identifier
                            // relatedInformation points at EVERY prior declaration
                            // (and the prior ones gain "also declared here" relateds).
                            self.report_duplicate(existing, node, &name, includes);
                            // FRESH symbol so later references do not re-report
                            self.create_symbol(SymbolFlags::empty(), name)
                        } else {
                            existing        // var/assignment JS merge
                        }
                    }
                    Some(existing) => existing,   // clean MERGE
                }
            }
        };
        self.add_declaration_to_symbol(symbol, node, includes);
        if self.symbol(symbol).parent == SymbolId::INVALID {
            self.symbol_mut(symbol).parent = parent;
        } else {
            debug_assert!(self.symbol(symbol).parent == parent, "merged decls keep one parent");
        }
        symbol
    }

    pub fn add_declaration_to_symbol(&mut self, symbol: SymbolId,
            node: NodeId, includes: SymbolFlags) {
        let s = self.symbol_mut(symbol);
        s.flags |= includes;
        s.declarations.push(node);
        self.node_links_mut(node).symbol = symbol;      // node → symbol side table
        if includes.intersects(SymbolFlags::VALUE) {
            let vd = self.symbol(symbol).value_declaration;
            // first value decl wins; assignment-declarations lose to real ones
            if vd == NodeId::INVALID || self.should_replace_value_declaration(vd, node) {
                self.symbol_mut(symbol).value_declaration = node;
            }
        }
        // members/exports tables created lazily on first member (tsc parity)
        todo_port!("addDeclarationToSymbol tail: members/exports table init, _tsc.js near 42560");
    }
}
```

`report_duplicate`'s message-selection switch: transcribe from the
conflict block inside `declareSymbol` (42602 body) — the selection
between 2300/2451/2567/2528 and the relatedInformation wiring is ~40
lines of straight transcription and is 2XXX-observable, so it gets
its own oracle-probed pin set (one micro per message).

## [COPY] declareModuleMember (stage 3.3) — `_tsc.js` 42675

```rust
pub fn declare_module_member(&mut self, node: NodeId,
        includes: SymbolFlags, excludes: SymbolFlags) -> SymbolId {
    let has_export = self.get_combined_modifier_flags(node).contains(ModifierFlags::EXPORT)
        || self.is_jsdoc_export_like(node);   // ts-only: modifier check
    if includes.contains(SymbolFlags::ALIAS) {
        if self.node_kind(node) == SyntaxKind::ExportSpecifier
            || (self.node_kind(node) == SyntaxKind::ImportEqualsDeclaration && has_export) {
            return self.declare_symbol(TableRef::Exports(self.container_symbol()),
                self.container_symbol(), node, includes, excludes, false, false);
        }
        return self.declare_symbol(TableRef::Locals(self.container),
            SymbolId::INVALID, node, includes, excludes, false, false);
    }
    if !has_export && !(self.node_flags(node).contains(NodeFlags::AMBIENT)
                        && self.in_external_module_augmentation(node)) {
        return self.declare_symbol(TableRef::Locals(self.container),
            SymbolId::INVALID, node, includes, excludes, false, false);
    }
    // exported: LOCAL symbol + EXPORT symbol, linked via export_symbol
    let export_kind = if includes.intersects(SymbolFlags::VALUE)
        { SymbolFlags::EXPORT_VALUE } else { SymbolFlags::empty() };
    let local = self.declare_symbol(TableRef::Locals(self.container),
        SymbolId::INVALID, node, export_kind, SymbolFlags::empty(), false, false);
    let exported = self.declare_symbol(TableRef::Exports(self.container_symbol()),
        self.container_symbol(), node, includes, excludes, false, false);
    self.symbol_mut(local).export_symbol = exported;
    self.node_links_mut(node).local_symbol = local;
    exported
}
```

## [COPY] Flow constructors (stage 3.5)

```rust
impl Binder<'_> {
    fn create_flow(&mut self, flags: FlowFlags, node: NodeId, antecedent: FlowId) -> FlowId {
        self.set_flow_referenced(antecedent);
        self.flow_arena.push(FlowNode { flags, node, antecedents: smallvec![antecedent] })
    }
    pub fn create_branch_label(&mut self) -> FlowId {
        self.flow_arena.push(FlowNode { flags: FlowFlags::BRANCH_LABEL, ..Default::default() })
    }
    pub fn create_loop_label(&mut self) -> FlowId {
        self.flow_arena.push(FlowNode { flags: FlowFlags::LOOP_LABEL, ..Default::default() })
    }
    pub fn add_antecedent(&mut self, label: FlowId, antecedent: FlowId) {
        if self.flow(antecedent).flags.contains(FlowFlags::UNREACHABLE) { return; }
        if self.flow(label).antecedents.contains(&antecedent) { return; }
        self.flow_mut(label).antecedents.push(antecedent);
        self.set_flow_referenced(antecedent);
    }
    /// a label with one antecedent COLLAPSES to it; none ⇒ unreachable
    pub fn finish_flow_label(&mut self, label: FlowId) -> FlowId {
        match self.flow(label).antecedents.len() {
            0 => self.unreachable_flow,
            1 => self.flow(label).antecedents[0],
            _ => label,
        }
    }
    fn set_flow_referenced(&mut self, f: FlowId) {
        // second reference marks Shared (the shared-flow cache keys on it)
        let fl = self.flow_mut(f);
        if fl.flags.contains(FlowFlags::REFERENCED) { fl.flags |= FlowFlags::SHARED; }
        else { fl.flags |= FlowFlags::REFERENCED; }
    }
}
```

`bindCondition` (43193), `bindWhileStatement` (43218) and the rest of
the bind*Statement family: transcribe with the loop-label pattern
from syntax-and-binder §3.3; the logical-operator edges come from
binding the sub-expressions under true/false target labels
(`doWithConditionalBranches`), NOT from extra condition nodes.

Two source facts the skeletons above do not show (audit 2026-07-10):

- `createFlowCondition`/`createFlowMutation`/`createFlowCall` first
  consult the narrowing predicates (`isNarrowingExpression` et al,
  42977–43076) and return the antecedent unchanged when the
  expression cannot narrow — port the predicates BEFORE the
  constructors or every flow shape gains spurious nodes.
- `createBindBinaryExpressionFlow` (43540) is a non-recursive
  work-stack state machine (onEnter/onLeft/onOperator/onRight/
  onExit); transcribe the trampoline, not a recursive equivalent —
  deep binary chains in the corpus overflow a recursive binder.

## [PORT TABLE] binder, in port order

| # | Function | Anchor | 2XXX output |
|---|---|---|---|
| 1 | symbol model + escaping | 11438 | — |
| 2 | `declareSymbol` + `addDeclarationToSymbol` + `report_duplicate` | 42602 | 2300/2451/2567/2528 family |
| 3 | `declareModuleMember` | 42675 | (enables later 2305/2339 correctness) |
| 4 | `getContainerFlags` + `bindContainer` | near 42734 | — |
| 5 | `bindSourceFile` + `bind` + `bindWorker` arms | 42408 / 44287 | strict-mode 1100/1101… (non-2xxx but same pass) |
| 6 | flow constructors + bind*Statement family | 43193+ | — (consumed in phase 8) |
| 7 | `getModuleInstanceState` | grep | — (phase 9 suggestion band) |

Gate additions for phase 3 (beyond m2-binder-steps.md): a
`pins/dup-decl/` fixture directory with one micro per duplicate
message, oracle-probed; `cargo xtask conformance --band 2xxx` first
non-zero measurement recorded in the phase-closing commit.
