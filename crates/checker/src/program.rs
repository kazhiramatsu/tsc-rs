//! ProgramBinder — the checker's program-wide view over per-file
//! binder runs (M4 5.0).
//!
//! tsc's checker sees one heap: nodes and symbols from every file (and
//! the checker's own transient symbols) share an identity space. The
//! greenfield equivalent: each file parses with a NodeId/NodeArrayId
//! base and binds with a SymbolId base, so ids are program-unique by
//! construction and this struct only routes an id to its owning
//! per-file arena. Owner indexes are independently base-sorted and may contain
//! holes; cached documents retain their leases when Program order changes.
//! Checker transient symbols use the tagged high half of `SymbolId`.

use tsc_binder::{Binder, Symbol, SymbolArena, SymbolId, SymbolTable};
use tsc_syntax::{NodeArray, NodeArrayId, NodeId, SourceFile};
use tsc_types::{SymbolFlags, TRANSIENT_SYMBOL_BIT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArenaOwner {
    start: u32,
    end: u32,
    file: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramIdentitySpace {
    Node,
    NodeArray,
    PersistentSymbol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramIdentityError {
    EmptyProgram,
    EmptyOwner {
        space: ProgramIdentitySpace,
        file: usize,
    },
    Overlap {
        space: ProgramIdentitySpace,
        first_file: usize,
        second_file: usize,
        first_end: u32,
        second_start: u32,
    },
    PersistentSymbolUsesTransientPartition {
        file: usize,
        start: u32,
        end: u32,
    },
    PartialIdentityOwnership {
        file: usize,
    },
    MixedIdentityOwnership,
    IdentityDomainMismatch {
        file: usize,
    },
}

impl std::fmt::Display for ProgramIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("a program has no source files"),
            Self::EmptyOwner { space, file } => {
                write!(formatter, "program file {file} has an empty {space:?} arena")
            }
            Self::Overlap {
                space,
                first_file,
                second_file,
                first_end,
                second_start,
            } => write!(
                formatter,
                "program {space:?} owners overlap: file {first_file} ends at {first_end}, file {second_file} starts at {second_start}"
            ),
            Self::PersistentSymbolUsesTransientPartition { file, start, end } => write!(
                formatter,
                "program file {file} persistent symbols use tagged range {start}..{end}"
            ),
            Self::PartialIdentityOwnership { file } => write!(
                formatter,
                "program file {file} has only a subset of the required identity leases"
            ),
            Self::MixedIdentityOwnership => formatter.write_str(
                "a Program cannot mix identity-owned and unmanaged source/bind records",
            ),
            Self::IdentityDomainMismatch { file } => write!(
                formatter,
                "program file {file} belongs to a different identity domain",
            ),
        }
    }
}

impl std::error::Error for ProgramIdentityError {}

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
    /// Persistent symbol intervals in identity allocation order.
    symbol_owners: Vec<ArenaOwner>,
    /// Checker-side symbols (tsc createSymbol 47652 adds Transient).
    transient: SymbolArena,
}

fn validate_owner_intervals(
    space: ProgramIdentitySpace,
    owners: &[ArenaOwner],
    require_every_owner_nonempty: bool,
) -> Result<(), ProgramIdentityError> {
    if require_every_owner_nonempty {
        for owner in owners {
            if owner.start >= owner.end {
                return Err(ProgramIdentityError::EmptyOwner {
                    space,
                    file: owner.file,
                });
            }
        }
    }
    for pair in owners.windows(2) {
        if pair[1].start < pair[0].end {
            return Err(ProgramIdentityError::Overlap {
                space,
                first_file: pair[0].file,
                second_file: pair[1].file,
                first_end: pair[0].end,
                second_start: pair[1].start,
            });
        }
    }
    Ok(())
}

fn validate_identity_domains(file_binders: &[&Binder<'_>]) -> Result<(), ProgramIdentityError> {
    let mut program_anchor: Option<&tsc_types::IdentityLease> = None;
    let mut managed_program = None;
    for (file, binder) in file_binders.iter().enumerate() {
        let leases = [
            binder.source.node_identity_lease(),
            binder.source.array_identity_lease(),
            binder.symbol_identity_lease(),
            binder.private_name_serial_lease(),
        ];
        let present = leases.iter().filter(|lease| lease.is_some()).count();
        let managed = match present {
            0 => false,
            4 => true,
            _ => return Err(ProgramIdentityError::PartialIdentityOwnership { file }),
        };
        if let Some(expected) = managed_program {
            if expected != managed {
                return Err(ProgramIdentityError::MixedIdentityOwnership);
            }
        } else {
            managed_program = Some(managed);
        }
        if managed {
            let anchor = leases[0].expect("managed source has its node lease");
            if leases
                .iter()
                .flatten()
                .any(|lease| !anchor.same_domain(lease))
            {
                return Err(ProgramIdentityError::IdentityDomainMismatch { file });
            }
            if let Some(program_anchor) = program_anchor {
                if !program_anchor.same_domain(anchor) {
                    return Err(ProgramIdentityError::IdentityDomainMismatch { file });
                }
            } else {
                program_anchor = Some(anchor);
            }
        }
    }
    Ok(())
}

impl<'a> ProgramBinder<'a> {
    /// tsrs-native: constructs the multi-file arena routing tables; tsc
    /// nodes and symbols are direct JavaScript references.
    pub fn new(file_binders: Vec<&'a Binder<'a>>) -> Self {
        Self::try_new(file_binders).expect("invalid Program identity ownership")
    }

    pub fn try_new(file_binders: Vec<&'a Binder<'a>>) -> Result<Self, ProgramIdentityError> {
        if file_binders.is_empty() {
            return Err(ProgramIdentityError::EmptyProgram);
        }
        validate_identity_domains(&file_binders)?;

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

        let mut symbol_owners: Vec<ArenaOwner> = file_binders
            .iter()
            .enumerate()
            .filter_map(|(file, binder)| {
                let start = binder.symbols.base();
                let end = binder.symbols.next_id().0;
                (start != end).then_some(ArenaOwner { start, end, file })
            })
            .collect();
        symbol_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        validate_owner_intervals(ProgramIdentitySpace::Node, &node_owners, true)?;
        validate_owner_intervals(ProgramIdentitySpace::NodeArray, &array_owners, true)?;
        validate_owner_intervals(
            ProgramIdentitySpace::PersistentSymbol,
            &symbol_owners,
            false,
        )?;
        for owner in &symbol_owners {
            if owner.start >= TRANSIENT_SYMBOL_BIT || owner.end > TRANSIENT_SYMBOL_BIT {
                return Err(
                    ProgramIdentityError::PersistentSymbolUsesTransientPartition {
                        file: owner.file,
                        start: owner.start,
                        end: owner.end,
                    },
                );
            }
        }

        Ok(Self {
            file_binders,
            node_owners,
            array_owners,
            symbol_owners,
            transient: SymbolArena::with_base(TRANSIENT_SYMBOL_BIT),
        })
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
        if id.0 & TRANSIENT_SYMBOL_BIT != 0 {
            return Err(());
        }
        Ok(Self::owner_file(
            &self.symbol_owners,
            id.0,
            "persistent SymbolId",
        ))
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
    pub fn flags_of(&self, node: NodeId) -> tsc_types::NodeFlags {
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
#[path = "../tests/unit/program/tests.rs"]
mod tests;
