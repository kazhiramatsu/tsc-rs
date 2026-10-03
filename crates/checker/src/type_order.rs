//! The checker half of stable type ordering (tsc 6.0.3
//! `--stableTypeOrdering`, _tsc.js:46478-46479 and 90575-90609): the parts
//! of `compareTypes` that read symbols, nodes and type mappers, plus the
//! symbol sorts the option adds (`getNamedMembers` 50145-50189 and
//! `sortSymbolsIfTSGoCompat` 90575-90580).
//!
//! tsrs-native: [`OrderCtx`] borrows the binder, the links and the mapper
//! arena as separate fields of the checker state, so a table method can take
//! the order while the state's type tables are borrowed mutably.

use std::cmp::Ordering;

use tsc_syntax::{NodeData, NodeId};
use tsc_types::tables::TypeTables;
use tsc_types::type_order::{compare_type_lists, compare_types};
use tsc_types::{MapperId, SymbolFlags, SymbolId, TypeId, TypeOrder, TypeOrderContext};

use crate::instantiate::TypeMapper;
use crate::links::LinksTables;
use crate::program::ProgramBinder;
use crate::state::CheckerState;

/// The order context of one checker state. [`OrderCtx::order`] is `None`
/// with the option off (tsc's type-id order), so the table methods keep
/// their default path.
pub(crate) struct OrderCtx<'s, 'a> {
    binder: &'s ProgramBinder<'a>,
    links: &'s LinksTables,
    mappers: &'s [TypeMapper],
    mapper_lists: &'s [TypeId],
    stable: bool,
}

/// Build the [`OrderCtx`] of a checker state from its fields, leaving the
/// state's `tables` free for a mutable borrow in the same expression.
macro_rules! order_ctx {
    ($state:expr) => {
        $crate::type_order::OrderCtx::new(
            &$state.binder,
            &$state.links,
            &$state.mappers,
            &$state.mapper_lists,
            $state.stable_type_ordering,
        )
    };
}
pub(crate) use order_ctx;

impl<'s, 'a> OrderCtx<'s, 'a> {
    pub(crate) fn new(
        binder: &'s ProgramBinder<'a>,
        links: &'s LinksTables,
        mappers: &'s [TypeMapper],
        mapper_lists: &'s [TypeId],
        stable: bool,
    ) -> Self {
        Self {
            binder,
            links,
            mappers,
            mapper_lists,
            stable,
        }
    }

    /// The order to pass to the type tables.
    pub(crate) fn order(&self) -> TypeOrder<'_> {
        if self.stable {
            Some(self)
        } else {
            None
        }
    }

    fn node_position(&self, node: NodeId) -> (usize, u32) {
        (
            self.binder.file_index_of_node(node),
            self.binder.node_record(node).pos,
        )
    }

    /// The `name.escapedText` of a labeled tuple element declaration
    /// (a named tuple member or a parameter); `None` for a binding pattern.
    fn element_label_name(&self, node: NodeId) -> Option<tsc_types::EscapedName> {
        let name = match &self.binder.node_record(node).data {
            NodeData::NamedTupleMember(data) => data.name,
            NodeData::Parameter(data) => data.name,
            _ => None,
        }?;
        match &self.binder.node_record(name).data {
            NodeData::Identifier(data) => Some(data.escaped_text),
            _ => None,
        }
    }

    fn mapper_kind(mapper: &TypeMapper) -> u8 {
        // tsc TypeMapKind: Simple, Array, Deferred, Function, Composite, Merged.
        match mapper {
            TypeMapper::Simple { .. } => 0,
            TypeMapper::Array { .. } => 1,
            TypeMapper::Deferred(_) => 2,
            TypeMapper::Function(_) => 3,
            TypeMapper::Composite { .. } => 4,
            TypeMapper::Merged { .. } => 5,
        }
    }

    /// The position part of a symbol's `compareSymbols` key: declared
    /// symbols first, by the Program file index and position of their first
    /// declaration (`compareNodes`); undeclared symbols after them.
    fn symbol_key(&self, symbol: SymbolId) -> SymbolKey {
        match self.binder.symbol(symbol).declarations.first() {
            Some(&declaration) => {
                let (file, pos) = self.node_position(declaration);
                SymbolKey {
                    undeclared: false,
                    file,
                    pos,
                    symbol,
                }
            }
            None => SymbolKey {
                undeclared: true,
                file: 0,
                pos: 0,
                symbol,
            },
        }
    }

    /// `compareSymbols` on two precomputed keys: the declaration position,
    /// then the name (UTF-16 code units) and the arena index on ties, as
    /// [`TypeOrderContext::compare_symbols`] decides them.
    fn compare_keys(&self, a: &SymbolKey, b: &SymbolKey) -> Ordering {
        if a.symbol == b.symbol {
            return Ordering::Equal;
        }
        a.undeclared
            .cmp(&b.undeclared)
            .then(a.file.cmp(&b.file))
            .then(a.pos.cmp(&b.pos))
            .then_with(|| self.compare_symbol_names(a.symbol, b.symbol))
            .then_with(|| a.symbol.index().cmp(&b.symbol.index()))
    }

    /// Sort `symbols` by `compareSymbols`. The declaration position of each
    /// symbol is looked up once, not once per comparison: the member lists
    /// of every resolved object type pass through here, so the lookups
    /// (which route through the owning file's node slice) dominated the
    /// cost of stable ordering.
    pub(crate) fn sort_symbols(&self, symbols: &mut [SymbolId]) {
        if symbols.len() < 2 {
            return;
        }
        let mut keys: Vec<SymbolKey> = symbols
            .iter()
            .map(|&symbol| self.symbol_key(symbol))
            .collect();
        keys.sort_by(|a, b| self.compare_keys(a, b));
        for (slot, key) in symbols.iter_mut().zip(&keys) {
            *slot = key.symbol;
        }
    }
}

/// A symbol's precomputed `compareSymbols` position (see
/// [`OrderCtx::symbol_key`]).
#[derive(Clone, Copy)]
struct SymbolKey {
    undeclared: bool,
    file: usize,
    pos: u32,
    symbol: SymbolId,
}

impl TypeOrderContext for OrderCtx<'_, '_> {
    /// tsc-port: compareSymbols @6.0.3
    /// tsc-span: _tsc.js:90581-90596
    fn compare_symbols(&self, s1: Option<SymbolId>, s2: Option<SymbolId>) -> Ordering {
        if s1 == s2 {
            return Ordering::Equal;
        }
        let (Some(s1), Some(s2)) = (s1, s2) else {
            return if s1.is_none() {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        };
        let symbol1 = self.binder.symbol(s1);
        let symbol2 = self.binder.symbol(s2);
        let declarations1: &[NodeId] = &symbol1.declarations;
        let declarations2: &[NodeId] = &symbol2.declarations;
        if !declarations1.is_empty() && !declarations2.is_empty() {
            let c = self.compare_nodes(declarations1[0].index(), declarations2[0].index());
            if c != Ordering::Equal {
                return c;
            }
        } else if !declarations1.is_empty() {
            return Ordering::Less;
        } else if !declarations2.is_empty() {
            return Ordering::Greater;
        }
        if symbol1.escaped_name != symbol2.escaped_name {
            let c = symbol1
                .escaped_name
                .as_js()
                .cmp_utf16(symbol2.escaped_name.as_js());
            if c != Ordering::Equal {
                return c;
            }
        }
        // tsc falls back to getSymbolId, assigned on first use; the arena
        // index is the creation order instead (transient symbols after the
        // program's). Two same-named declaration-less symbols are rare.
        s1.index().cmp(&s2.index())
    }

    fn compare_symbol_names(&self, s1: SymbolId, s2: SymbolId) -> Ordering {
        let name1 = self.binder.symbol(s1).escaped_name;
        let name2 = self.binder.symbol(s2).escaped_name;
        // Interned names: the same text is the same key.
        if name1 == name2 {
            return Ordering::Equal;
        }
        name1.as_js().cmp_utf16(name2.as_js())
    }

    /// tsc-port: compareNodes @6.0.3
    /// tsc-span: _tsc.js:90597-90609
    fn compare_nodes(&self, n1: u32, n2: u32) -> Ordering {
        if n1 == n2 {
            return Ordering::Equal;
        }
        let (file1, pos1) = self.node_position(NodeId::new(n1));
        let (file2, pos2) = self.node_position(NodeId::new(n2));
        if file1 != file2 {
            // Order by index of file in the containing program
            return file1.cmp(&file2);
        }
        // In the same file, order by source position
        pos1.cmp(&pos2)
    }

    /// tsc-port: compareElementLabels @6.0.3
    /// tsc-span: _tsc.js:90837-90848
    fn compare_element_labels(&self, n1: Option<u32>, n2: Option<u32>) -> Ordering {
        if n1 == n2 {
            return Ordering::Equal;
        }
        let (Some(n1), Some(n2)) = (n1, n2) else {
            return if n1.is_none() {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        };
        let name1 = self.element_label_name(NodeId::new(n1));
        let name2 = self.element_label_name(NodeId::new(n2));
        match (name1, name2) {
            (Some(name1), Some(name2)) => name1.as_js().cmp_utf16(name2.as_js()),
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
        }
    }

    fn deferred_node(&self, ty: TypeId) -> Option<u32> {
        self.links.ty(ty).deferred_node.map(NodeId::index)
    }

    fn object_mapper(&self, ty: TypeId) -> Option<MapperId> {
        if self.links.ty(ty).deferred_node.is_some() {
            *self.links.type_cold().deferred_mapper.get(ty)
        } else {
            self.links.ty(ty).instantiated_mapper
        }
    }

    /// tsc-port: compareTypeMappers @6.0.3
    /// tsc-span: _tsc.js:90859-90898
    fn compare_mappers(
        &self,
        tables: &TypeTables,
        m1: Option<MapperId>,
        m2: Option<MapperId>,
    ) -> Ordering {
        let (m1, m2) = match (m1, m2) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(m1), Some(m2)) => (m1, m2),
        };
        if m1 == m2 {
            return Ordering::Equal;
        }
        let mapper1 = &self.mappers[m1.index() as usize];
        let mapper2 = &self.mappers[m2.index() as usize];
        let c = Self::mapper_kind(mapper1).cmp(&Self::mapper_kind(mapper2));
        if c != Ordering::Equal {
            return c;
        }
        match (mapper1, mapper2) {
            (
                TypeMapper::Simple {
                    source: source1,
                    target: target1,
                },
                TypeMapper::Simple {
                    source: source2,
                    target: target2,
                },
            ) => {
                let c = compare_types(tables, self, *source1, *source2);
                if c != Ordering::Equal {
                    return c;
                }
                compare_types(tables, self, *target1, *target2)
            }
            (
                TypeMapper::Array {
                    sources: sources1,
                    targets: targets1,
                },
                TypeMapper::Array {
                    sources: sources2,
                    targets: targets2,
                },
            ) => {
                let c = compare_type_lists(
                    tables,
                    self,
                    Some(&self.mapper_lists[sources1.range()]),
                    Some(&self.mapper_lists[sources2.range()]),
                );
                if c != Ordering::Equal {
                    return c;
                }
                compare_type_lists(
                    tables,
                    self,
                    targets1.map(|list| &self.mapper_lists[list.range()]),
                    targets2.map(|list| &self.mapper_lists[list.range()]),
                )
            }
            (
                TypeMapper::Merged {
                    mapper1: first1,
                    mapper2: second1,
                },
                TypeMapper::Merged {
                    mapper1: first2,
                    mapper2: second2,
                },
            ) => {
                let c = self.compare_mappers(tables, Some(*first1), Some(*first2));
                if c != Ordering::Equal {
                    return c;
                }
                self.compare_mappers(tables, Some(*second1), Some(*second2))
            }
            _ => Ordering::Equal,
        }
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: sortSymbolsIfTSGoCompat @6.0.3
    /// tsc-span: _tsc.js:90575-90580
    pub(crate) fn sort_symbols_if_stable(&self, symbols: &mut [SymbolId]) {
        if self.stable_type_ordering {
            order_ctx!(self).sort_symbols(symbols);
        }
    }

    /// The stable-ordering member order of `getNamedMembers` (50145-50189):
    /// with a class or interface container, the members declared inside
    /// one of its declarations come first; each group is sorted by
    /// `compareSymbols`. With the option off the list is left in its
    /// insertion order, as tsc's default path returns it.
    ///
    /// One sort over keys that carry the group as their first component is
    /// the two sorted groups concatenated; the container's declaration
    /// ranges are read once rather than once per member.
    pub(crate) fn order_named_members_if_stable(
        &self,
        members: &mut [SymbolId],
        container: Option<SymbolId>,
    ) {
        if !self.stable_type_ordering || members.len() < 2 {
            return;
        }
        let ctx = order_ctx!(self);
        let class_or_interface = container.filter(|&container| {
            self.binder
                .symbol(container)
                .flags
                .intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE)
        });
        // getNamedMembers' `isDeclarationContainedBy` (50178-50188): the
        // symbol's value declaration lies within one of the container's
        // declarations (positions compared as tsc does, without a file check).
        let container_ranges: Vec<(u32, u32)> = class_or_interface
            .map(|container| {
                let declarations: &[NodeId] = &self.binder.symbol(container).declarations;
                declarations
                    .iter()
                    .map(|&declaration| (self.pos_of(declaration), self.end_of(declaration)))
                    .collect()
            })
            .unwrap_or_default();
        let outside_container = |symbol: SymbolId| -> bool {
            if container_ranges.is_empty() {
                return false;
            }
            let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
                return true;
            };
            let (pos, end) = (self.pos_of(declaration), self.end_of(declaration));
            !container_ranges
                .iter()
                .any(|&(start, stop)| start <= pos && stop >= end)
        };
        let mut keys: Vec<(bool, SymbolKey)> = members
            .iter()
            .map(|&symbol| (outside_container(symbol), ctx.symbol_key(symbol)))
            .collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| ctx.compare_keys(&a.1, &b.1)));
        for (slot, key) in members.iter_mut().zip(&keys) {
            *slot = key.1.symbol;
        }
    }

    // ---- table twins that take this checker's order ----

    /// `tables.get_regular_type_of_literal_type` with this checker's order.
    pub(crate) fn regular_type_of_literal_type(&mut self, ty: TypeId) -> TypeId {
        let ctx = order_ctx!(self);
        self.tables
            .get_regular_type_of_literal_type(ctx.order(), ty)
    }

    /// `tables.add_optionality` with this checker's order.
    pub(crate) fn add_optionality(
        &mut self,
        ty: TypeId,
        is_property: bool,
        is_optional: bool,
    ) -> TypeId {
        let ctx = order_ctx!(self);
        self.tables
            .add_optionality(ctx.order(), ty, is_property, is_optional)
    }

    /// `tables.get_tuple_target_type` with this checker's order.
    pub(crate) fn tuple_target_type(
        &mut self,
        element_flags: tsc_types::tables::TupleTargetFlags<'_>,
        readonly: bool,
        named_member_declarations: Option<&[Option<u32>]>,
    ) -> TypeId {
        let ctx = order_ctx!(self);
        self.tables.get_tuple_target_type(
            ctx.order(),
            element_flags,
            readonly,
            named_member_declarations,
        )
    }

    /// `tables.try_get_template_literal_type` with this checker's order.
    pub(crate) fn template_literal_type_tables(
        &mut self,
        texts: &[String],
        types: &[TypeId],
    ) -> Result<TypeId, tsc_types::TemplateLiteralTooLarge> {
        let ctx = order_ctx!(self);
        self.tables
            .try_get_template_literal_type(ctx.order(), texts, types)
    }
}

#[cfg(test)]
#[path = "../tests/unit/type_order/tests.rs"]
mod tests;
