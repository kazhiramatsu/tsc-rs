//! ProgramBinder — the checker's program-wide view over per-file
//! binder runs (M4 5.0).
//!
//! tsc's checker sees one heap: nodes and symbols from every file (and
//! the checker's own transient symbols) share an identity space. The
//! greenfield equivalent: each file parses with a NodeId/NodeArrayId
//! base and binds with a SymbolId base, so ids are program-unique by
//! construction and this struct only routes an id to its owning
//! per-file arena. Parse allocation order may differ from tsc's final
//! program order; node/array owner indexes are therefore independently
//! base-sorted while symbols retain bind/program order. Checker transient
//! symbols (tsc createSymbol 47652) allocate above all files.

use tsrs2_binder::{Binder, Symbol, SymbolArena, SymbolId, SymbolTable};
use tsrs2_syntax::{NodeArray, NodeArrayId, NodeId, SourceFile};
use tsrs2_types::SymbolFlags;

#[derive(Clone, Copy, Debug)]
struct ArenaOwner {
    start: u32,
    end: u32,
    file: usize,
}

pub struct ProgramBinder<'a> {
    /// Per-file binder runs in program order — BORROWED so cached lib
    /// binders (leaked, 'static) and per-program fixture binders can
    /// share one program view; binders are read-only after
    /// bind_source_file (M2 design), which the shared reference now
    /// enforces structurally.
    file_binders: Vec<&'a Binder<'a>>,
    /// Node-id intervals in parse allocation order (ascending by start).
    node_owners: Vec<ArenaOwner>,
    /// Node-array-id intervals in parse allocation order.
    array_owners: Vec<ArenaOwner>,
    /// Cached per-file symbol-id bases (ascending) for owner lookup.
    symbol_bases: Vec<u32>,
    /// Checker-side symbols (tsc createSymbol 47652 adds Transient).
    transient: SymbolArena,
}

impl<'a> ProgramBinder<'a> {
    /// tsrs-native: constructs the multi-file arena routing tables; tsc
    /// nodes and symbols are direct JavaScript references.
    pub fn new(file_binders: Vec<&'a Binder<'a>>) -> Self {
        assert!(
            !file_binders.is_empty(),
            "a program has at least one source file"
        );

        let mut node_owners: Vec<ArenaOwner> = file_binders
            .iter()
            .enumerate()
            .map(|(file, binder)| ArenaOwner {
                start: binder.source.arena.node_base(),
                end: binder.source.arena.node_end(),
                file,
            })
            .collect();
        node_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        let mut array_owners: Vec<ArenaOwner> = file_binders
            .iter()
            .enumerate()
            .map(|(file, binder)| ArenaOwner {
                start: binder.source.arena.array_base(),
                end: binder.source.arena.array_end(),
                file,
            })
            .collect();
        array_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        let symbol_bases: Vec<u32> = file_binders
            .iter()
            .map(|binder| binder.symbols.base())
            .collect();

        // Node and node-array arenas remain one contiguous allocation
        // space, but their allocation order is independent of final
        // program order.
        for owner in &node_owners {
            assert!(
                owner.start < owner.end,
                "program source files must own at least one node"
            );
        }
        for pair in node_owners.windows(2) {
            assert_eq!(
                pair[1].start, pair[0].end,
                "program files must parse with contiguous node bases"
            );
        }
        for owner in &array_owners {
            assert!(
                owner.start < owner.end,
                "program source files must own at least one node array"
            );
        }
        for pair in array_owners.windows(2) {
            assert_eq!(
                pair[1].start, pair[0].end,
                "program files must parse with contiguous node-array bases"
            );
        }

        // Symbols allocate in final bind/program order.
        for pair in file_binders.windows(2) {
            assert_eq!(
                pair[1].symbols.base(),
                pair[0].symbols.next_id().0,
                "program files must bind with contiguous symbol bases"
            );
        }
        let transient_base = file_binders.last().expect("non-empty").symbols.next_id().0;
        Self {
            file_binders,
            node_owners,
            array_owners,
            symbol_bases,
            transient: SymbolArena::with_base(transient_base),
        }
    }

    /// tsrs-native: Rust ProgramBinder collection accessor.
    pub fn file_count(&self) -> usize {
        self.file_binders.len()
    }

    /// tsrs-native: Rust ProgramBinder iterator over borrowed file
    /// binders.
    pub fn files(&self) -> impl Iterator<Item = &'a Binder<'a>> + '_ {
        self.file_binders.iter().copied()
    }

    /// tsrs-native: Rust ProgramBinder indexed file accessor.
    pub fn file(&self, index: usize) -> &'a Binder<'a> {
        self.file_binders[index]
    }

    /// tsrs-native: Rust ProgramBinder SourceFile projection.
    pub fn source(&self, index: usize) -> &'a SourceFile {
        self.file_binders[index].source
    }

    /// Owning file of a node id (nodes allocate contiguously per file).
    /// tsrs-native: binary-search routing for Rust's process-wide
    /// numeric NodeId arena; tsc carries object identity directly.
    pub fn file_index_of_node(&self, node: NodeId) -> usize {
        Self::owner_file(&self.node_owners, node.0, "NodeId")
    }

    /// tsrs-native: multi-file arena routing for a numeric NodeId; tsc
    /// carries the SourceFile/object relationship directly.
    pub fn source_of_node(&self, node: NodeId) -> &'a SourceFile {
        self.file_binders[self.file_index_of_node(node)].source
    }

    fn binder_of_node(&self, node: NodeId) -> &'a Binder<'a> {
        self.file_binders[self.file_index_of_node(node)]
    }

    /// Owning file's arena lookup for a node-array id (arrays allocate
    /// contiguously per file, like nodes).
    /// tsrs-native: multi-file arena routing for Rust's numeric
    /// NodeArrayId.
    pub fn node_array(&self, id: NodeArrayId) -> &'a NodeArray {
        let index = Self::owner_file(&self.array_owners, id.0, "NodeArrayId");
        self.file_binders[index].source.arena.node_array(id)
    }

    fn owner_file(owners: &[ArenaOwner], id: u32, kind: &str) -> usize {
        let index = owners
            .partition_point(|owner| owner.start <= id)
            .checked_sub(1)
            .unwrap_or_else(|| panic!("{kind} {id} precedes the first program arena"));
        let owner = owners[index];
        assert!(id < owner.end, "{kind} {id} is outside every program arena");
        owner.file
    }

    fn owner_of_symbol(&self, id: SymbolId) -> Result<usize, ()> {
        if self.transient.contains(id) {
            return Err(());
        }
        match self.symbol_bases.binary_search(&id.0) {
            Ok(index) => Ok(index),
            Err(insert) => Ok(insert - 1),
        }
    }

    /// tsrs-native: routes a numeric SymbolId to its binder or
    /// checker-owned transient arena; tsc carries object references.
    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => self.file_binders[file].symbols.symbol(id),
            Err(()) => self.transient.symbol(id),
        }
    }

    /// TRANSIENT symbols only: file binders are read-only after bind
    /// (shared lib binders make this structural — merge writes go to
    /// transient clones, and the merged-symbol mapping lives on
    /// CheckerState).
    /// tsrs-native: mutable arena routing required by Rust ownership;
    /// tsc mutates Symbol objects directly.
    pub fn symbol_mut(&mut self, id: SymbolId) -> &mut Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => unreachable!(
                "file-binder symbol {id:?} (file {file}) mutated post-bind —                  mergeSymbol clones non-transient targets and the mergedSymbols                  map lives on CheckerState"
            ),
            Err(()) => self.transient.symbol_mut(id),
        }
    }

    /// tsc-port: createSymbol @6.0.3
    /// tsc-hash: b9b2c65d71ec1e9d3a55d36fe5224e5f31dd618ee1428293b371d2f2881ad16a
    /// tsc-span: _tsc.js:47652-47658
    ///
    /// Checker-side symbol creation: always Transient. (tsc also seeds
    /// links.checkFlags here; ours live in LinksTables and default 0 —
    /// callers that need CheckFlags set them through the links API.)
    pub fn create_symbol(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        self.transient
            .alloc(flags | SymbolFlags::TRANSIENT, escaped_name)
    }

    /// tsc container.locals of a scope-owning node.
    /// tsrs-native: binder-table projection for tsc's direct
    /// `container.locals` property access.
    pub fn locals_of(&self, scope: NodeId) -> Option<&SymbolTable> {
        self.binder_of_node(scope).locals.get(&scope)
    }

    /// tsc node.symbol (addDeclarationToSymbol).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.symbol` property access.
    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder_of_node(node).node_symbol.get(&node).copied()
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.flags` property access.
    pub fn flags_of(&self, node: NodeId) -> tsrs2_types::NodeFlags {
        self.binder_of_node(node).flags_of(node)
    }

    /// tsc isExternalOrCommonJsModule for the file owning `node`.
    /// tsc-port: isExternalOrCommonJsModule @6.0.3
    /// tsc-hash: e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d
    /// tsc-span: _tsc.js:14119-14121
    pub fn is_external_or_common_js_module_of_node(&self, node: NodeId) -> bool {
        let binder = self.binder_of_node(node);
        binder.source.external_module_indicator.is_some()
            || binder.common_js_module_indicator.is_some()
    }

    /// tsc-port: isExternalModule @6.0.3
    /// tsc-hash: 5effe04fdce706cc75f238b5c4efbb1f317b3f6bd665389fb71a79a119e7ceaa
    /// tsc-span: _tsc.js:28910-28912
    ///
    /// For the file owning `node` (getSourceFileOfNode folded in):
    /// the externalModuleIndicator ONLY — the CJS indicator the
    /// variant above also admits would over-filter trySymbolTable's
    /// UMD leg (50341).
    pub fn is_external_module_of_node(&self, node: NodeId) -> bool {
        self.binder_of_node(node)
            .source
            .external_module_indicator
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsrs2_syntax::{parse_source_file, ParseOptions};
    use tsrs2_types::CompilerOptions;

    #[test]
    fn routes_parse_order_arenas_without_changing_program_order() {
        // tsc may request the root before its dependency while exposing
        // the dependency first from Program.getSourceFiles().
        let root = parse_source_file(
            "/a.ts",
            r#"import { b } from "./b"; export const a = b;"#,
            ParseOptions::default(),
            None,
        );
        let dependency = parse_source_file(
            "/b.ts",
            "export const b = 1;",
            ParseOptions {
                node_id_base: root.arena.node_end(),
                node_array_id_base: root.arena.array_end(),
                ..ParseOptions::default()
            },
            None,
        );
        let options = CompilerOptions::default();

        let mut dependency_binder = Binder::with_bases(&dependency, &options, 1, 0);
        dependency_binder.bind_source_file();
        let mut root_binder = Binder::with_bases(
            &root,
            &options,
            dependency_binder.next_symbol_id(),
            dependency_binder.symbols.next_id().0,
        );
        root_binder.bind_source_file();

        let program = ProgramBinder::new(vec![&dependency_binder, &root_binder]);
        assert_eq!(
            program
                .files()
                .map(|binder| binder.source.file_name.as_str())
                .collect::<Vec<_>>(),
            ["/b.ts", "/a.ts"]
        );
        assert_eq!(program.source(0).file_name, "/b.ts");
        assert_eq!(program.source(1).file_name, "/a.ts");

        for id in dependency.arena.node_ids() {
            assert_eq!(program.file_index_of_node(id), 0);
            assert!(std::ptr::eq(program.source_of_node(id), &dependency));
        }
        for id in root.arena.node_ids() {
            assert_eq!(program.file_index_of_node(id), 1);
            assert!(std::ptr::eq(program.source_of_node(id), &root));
        }

        for raw in dependency.arena.array_base()..dependency.arena.array_end() {
            let id = NodeArrayId(raw);
            assert!(std::ptr::eq(
                program.node_array(id),
                dependency.arena.node_array(id)
            ));
        }
        for raw in root.arena.array_base()..root.arena.array_end() {
            let id = NodeArrayId(raw);
            assert!(std::ptr::eq(
                program.node_array(id),
                root.arena.node_array(id)
            ));
        }

        for raw in dependency_binder.symbols.base()..dependency_binder.symbols.next_id().0 {
            let id = SymbolId(raw);
            assert!(std::ptr::eq(
                program.symbol(id),
                dependency_binder.symbols.symbol(id)
            ));
        }
        for raw in root_binder.symbols.base()..root_binder.symbols.next_id().0 {
            let id = SymbolId(raw);
            assert!(std::ptr::eq(
                program.symbol(id),
                root_binder.symbols.symbol(id)
            ));
        }
    }

    #[test]
    fn owner_lookup_rejects_ids_outside_every_interval() {
        let owners = [
            ArenaOwner {
                start: 10,
                end: 12,
                file: 1,
            },
            ArenaOwner {
                start: 15,
                end: 18,
                file: 0,
            },
        ];

        assert_eq!(ProgramBinder::owner_file(&owners, 10, "test id"), 1);
        assert_eq!(ProgramBinder::owner_file(&owners, 17, "test id"), 0);
        for id in [9, 12, 14, 18] {
            assert!(
                std::panic::catch_unwind(|| ProgramBinder::owner_file(&owners, id, "test id"))
                    .is_err(),
                "id {id} must fail closed"
            );
        }
    }
}
