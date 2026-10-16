//! Stable type ordering: tsgo's `CompareTypes` (checker/utilities.go:414-622
//! at the vendored 7.1 commit), the order TypeScript 7 always uses. The port
//! started from tsc 6.0.3's `--stableTypeOrdering` comparator (`compareTypes`,
//! _tsc.js:90610-90777); where the two differ the 7.1 rule is the one here
//! and its comment cites the Go source.
//!
//! tsrs-native: with the option off, union members are kept in type-id
//! order (tsc's default, the creation order of one checker). With the
//! option on, members are kept in the content order computed here, which
//! does not depend on the order in which types were created, so a sharded
//! check orders them as the serial check and as tsc/tsgo do. The checker
//! supplies the parts of the comparison that need the binder (symbol
//! declarations, node positions, tuple labels) and the mapper arena
//! through [`TypeOrderContext`]; everything else reads the type tables.

use std::cmp::Ordering;

use crate::tables::TypeTables;
use crate::ty::{LiteralValue, PseudoBigInt};
use crate::{MapperId, ObjectFlags, SymbolId, TypeData, TypeFlags, TypeId};

/// The checker-owned half of the comparison. Node ids are passed as their
/// `u32` index because the types crate does not depend on the syntax crate.
pub trait TypeOrderContext {
    /// tsc compareSymbols (90581-90596).
    fn compare_symbols(&self, a: Option<SymbolId>, b: Option<SymbolId>) -> Ordering;
    /// `compareComparableValues(s1.escapedName, s2.escapedName)` — JavaScript's
    /// UTF-16 code-unit string order of the two symbol names.
    fn compare_symbol_names(&self, a: SymbolId, b: SymbolId) -> Ordering;
    /// tsc compareNodes (90597-90609): Program file index, then `pos`.
    fn compare_nodes(&self, a: u32, b: u32) -> Ordering;
    /// tsc compareElementLabels (90837-90848) over labeled tuple element
    /// declarations.
    fn compare_element_labels(&self, a: Option<u32>, b: Option<u32>) -> Ordering;
    /// The `node` of a deferred type reference (checker TypeLinks).
    fn deferred_node(&self, ty: TypeId) -> Option<u32>;
    /// tsc's `type.mapper` of a deferred reference or an instantiated
    /// anonymous object type (checker TypeLinks); mapped types carry theirs
    /// in their type data.
    fn object_mapper(&self, ty: TypeId) -> Option<MapperId>;
    /// tsc compareTypeMappers (90859-90898); the implementation recurses
    /// into [`compare_types`] for the mapper's sources and targets.
    fn compare_mappers(
        &self,
        tables: &TypeTables,
        a: Option<MapperId>,
        b: Option<MapperId>,
    ) -> Ordering;
    /// The first declaration of a symbol, as `compareNodes` reads it for an
    /// instantiation expression type (tsgo CompareTypes,
    /// checker/utilities.go:443-454).
    fn first_declaration(&self, symbol: Option<SymbolId>) -> Option<u32>;
    /// The instantiation a mapped type was made with: `instantiateAnonymousType`
    /// combines a mapping of the mapped type's own type parameter to a fresh
    /// one with the instantiation's mapper, and the order compares the second
    /// (tsgo CompareTypes, checker/utilities.go:512-521).
    fn mapped_instantiation_mapper(&self, mapper: Option<MapperId>) -> Option<MapperId>;
}

/// The member order of unions: `None` keeps tsc's default type-id order,
/// `Some(context)` the stable content order.
pub type TypeOrder<'c> = Option<&'c dyn TypeOrderContext>;

/// tsc-port: compareTypes @6.0.3
/// tsc-span: _tsc.js:90610-90777
pub fn compare_types(
    tables: &TypeTables,
    ctx: &dyn TypeOrderContext,
    t1: TypeId,
    t2: TypeId,
) -> Ordering {
    if t1 == t2 {
        return Ordering::Equal;
    }
    let c = sort_order_flags(tables, t1).cmp(&sort_order_flags(tables, t2));
    if c != Ordering::Equal {
        return c;
    }
    let c = compare_type_names(tables, ctx, t1, t2);
    if c != Ordering::Equal {
        return c;
    }
    let flags = tables.flags_of(t1);
    if flags.intersects(TypeFlags::from_bits(
        TypeFlags::ANY.bits()
            | TypeFlags::UNKNOWN.bits()
            | TypeFlags::STRING.bits()
            | TypeFlags::NUMBER.bits()
            | TypeFlags::BOOLEAN.bits()
            | TypeFlags::BIG_INT.bits()
            | TypeFlags::ES_SYMBOL.bits()
            | TypeFlags::VOID.bits()
            | TypeFlags::UNDEFINED.bits()
            | TypeFlags::NULL.bits()
            | TypeFlags::NEVER.bits()
            | TypeFlags::NON_PRIMITIVE.bits(),
    )) {
        // Only distinguished by type ids, handled below.
    } else if flags.intersects(TypeFlags::OBJECT) {
        if tables
            .object_flags_of(t1)
            .intersects(ObjectFlags::INSTANTIATION_EXPRESSION_TYPE)
            && tables
                .object_flags_of(t2)
                .intersects(ObjectFlags::INSTANTIATION_EXPRESSION_TYPE)
        {
            // tsgo CompareTypes (checker/utilities.go:440-457): one
            // instantiation expression can make a type for each member of a
            // union, so the declarations of the sources are compared before
            // the expression the types share.
            let c = compare_optional_nodes(
                ctx,
                ctx.first_declaration(tables.type_of(t1).symbol),
                ctx.first_declaration(tables.type_of(t2).symbol),
            );
            if c != Ordering::Equal {
                return c;
            }
            let c = compare_optional_nodes(ctx, ctx.deferred_node(t1), ctx.deferred_node(t2));
            if c != Ordering::Equal {
                return c;
            }
        } else {
            let c = ctx.compare_symbols(tables.type_of(t1).symbol, tables.type_of(t2).symbol);
            if c != Ordering::Equal {
                return c;
            }
        }
        let reference1 = tables
            .object_flags_of(t1)
            .intersects(ObjectFlags::REFERENCE);
        let reference2 = tables
            .object_flags_of(t2)
            .intersects(ObjectFlags::REFERENCE);
        if reference1 && reference2 {
            let target1 = reference_target(tables, t1);
            let target2 = reference_target(tables, t2);
            if tables
                .object_flags_of(target1)
                .intersects(ObjectFlags::TUPLE)
                && tables
                    .object_flags_of(target2)
                    .intersects(ObjectFlags::TUPLE)
            {
                let c = compare_tuple_types(tables, ctx, target1, target2);
                if c != Ordering::Equal {
                    return c;
                }
            }
            let node1 = ctx.deferred_node(t1);
            let node2 = ctx.deferred_node(t2);
            if node1.is_none() && node2.is_none() {
                let c = compare_type_lists(
                    tables,
                    ctx,
                    tables.try_type_arguments(t1),
                    tables.try_type_arguments(t2),
                );
                if c != Ordering::Equal {
                    return c;
                }
            } else {
                let c = compare_optional_nodes(ctx, node1, node2);
                if c != Ordering::Equal {
                    return c;
                }
                let c = ctx.compare_mappers(tables, ctx.object_mapper(t1), ctx.object_mapper(t2));
                if c != Ordering::Equal {
                    return c;
                }
            }
        } else if reference1 {
            return Ordering::Less;
        } else if reference2 {
            return Ordering::Greater;
        } else {
            let kind1 =
                tables.object_flags_of(t1).bits() & ObjectFlags::OBJECT_TYPE_KIND_MASK.bits();
            let kind2 =
                tables.object_flags_of(t2).bits() & ObjectFlags::OBJECT_TYPE_KIND_MASK.bits();
            let c = kind1.cmp(&kind2);
            if c != Ordering::Equal {
                return c;
            }
            // tsgo CompareTypes (checker/utilities.go:497-509): reverse mapped
            // types are ordered by their source, mapped type and constraint.
            if let (TypeData::ReverseMapped(data1), TypeData::ReverseMapped(data2)) =
                (&tables.type_of(t1).data, &tables.type_of(t2).data)
            {
                let c = compare_types(tables, ctx, data1.source, data2.source);
                if c != Ordering::Equal {
                    return c;
                }
                let c = compare_types(tables, ctx, data1.mapped_type, data2.mapped_type);
                if c != Ordering::Equal {
                    return c;
                }
                let c = compare_types(tables, ctx, data1.constraint_type, data2.constraint_type);
                if c != Ordering::Equal {
                    return c;
                }
            }
            let c = ctx.compare_mappers(
                tables,
                anonymous_or_mapped_mapper(tables, ctx, t1),
                anonymous_or_mapped_mapper(tables, ctx, t2),
            );
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::UNION) {
        let (
            TypeData::Union {
                types: types1,
                origin: origin1,
            },
            TypeData::Union {
                types: types2,
                origin: origin2,
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        else {
            unreachable!("union flag implies union data");
        };
        match (origin1, origin2) {
            (None, None) => {
                let c = compare_type_lists(tables, ctx, Some(types1), Some(types2));
                if c != Ordering::Equal {
                    return c;
                }
            }
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(origin1), Some(origin2)) => {
                let c = compare_types(tables, ctx, *origin1, *origin2);
                if c != Ordering::Equal {
                    return c;
                }
            }
        }
    } else if flags.intersects(TypeFlags::INTERSECTION) {
        let (TypeData::Intersection { types: types1 }, TypeData::Intersection { types: types2 }) =
            (&tables.type_of(t1).data, &tables.type_of(t2).data)
        else {
            unreachable!("intersection flag implies intersection data");
        };
        let c = compare_type_lists(tables, ctx, Some(types1), Some(types2));
        if c != Ordering::Equal {
            return c;
        }
    } else if flags.intersects(TypeFlags::from_bits(
        TypeFlags::ENUM.bits()
            | TypeFlags::ENUM_LITERAL.bits()
            | TypeFlags::UNIQUE_ES_SYMBOL.bits(),
    )) {
        let c = ctx.compare_symbols(tables.type_of(t1).symbol, tables.type_of(t2).symbol);
        if c != Ordering::Equal {
            return c;
        }
    } else if flags.intersects(TypeFlags::STRING_LITERAL) {
        if let (
            TypeData::Literal {
                value: LiteralValue::String(value1),
            },
            TypeData::Literal {
                value: LiteralValue::String(value2),
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = value1.units().cmp(value2.units());
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::NUMBER_LITERAL) {
        if let (
            TypeData::Literal {
                value: LiteralValue::Number(value1),
            },
            TypeData::Literal {
                value: LiteralValue::Number(value2),
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_numbers(*value1, *value2);
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
        // tsgo CompareTypes (checker/utilities.go:563-566): bigint literal
        // types are ordered by their values.
        if let (
            TypeData::Literal {
                value: LiteralValue::BigInt(value1),
            },
            TypeData::Literal {
                value: LiteralValue::BigInt(value2),
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_pseudo_big_ints(value1, value2);
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
        let b1 = is_true_literal(tables, t1);
        let b2 = is_true_literal(tables, t2);
        if b1 != b2 {
            return if b1 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
    } else if flags.intersects(TypeFlags::TYPE_PARAMETER) {
        let c = ctx.compare_symbols(tables.type_of(t1).symbol, tables.type_of(t2).symbol);
        if c != Ordering::Equal {
            return c;
        }
    } else if flags.intersects(TypeFlags::INDEX) {
        if let (
            TypeData::Index {
                ty: type1,
                index_flags: flags1,
            },
            TypeData::Index {
                ty: type2,
                index_flags: flags2,
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_types(tables, ctx, *type1, *type2);
            if c != Ordering::Equal {
                return c;
            }
            let c = flags1.bits().cmp(&flags2.bits());
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::INDEXED_ACCESS) {
        if let (
            TypeData::IndexedAccess {
                object_type: object1,
                index_type: index1,
                ..
            },
            TypeData::IndexedAccess {
                object_type: object2,
                index_type: index2,
                ..
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_types(tables, ctx, *object1, *object2);
            if c != Ordering::Equal {
                return c;
            }
            let c = compare_types(tables, ctx, *index1, *index2);
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::CONDITIONAL) {
        if let (TypeData::Conditional(data1), TypeData::Conditional(data2)) =
            (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = ctx.compare_nodes(
                tables.conditional_root(data1.root).node,
                tables.conditional_root(data2.root).node,
            );
            if c != Ordering::Equal {
                return c;
            }
            let c = ctx.compare_mappers(tables, data1.mapper, data2.mapper);
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::SUBSTITUTION) {
        if let (TypeData::Substitution(data1), TypeData::Substitution(data2)) =
            (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_types(tables, ctx, data1.base_type, data2.base_type);
            if c != Ordering::Equal {
                return c;
            }
            let c = compare_types(tables, ctx, data1.constraint, data2.constraint);
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
        if let (
            TypeData::TemplateLiteral {
                texts: texts1,
                types: types1,
            },
            TypeData::TemplateLiteral {
                texts: texts2,
                types: types2,
            },
        ) = (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = slices_compare_string(texts1, texts2);
            if c != Ordering::Equal {
                return c;
            }
            let c = compare_type_lists(tables, ctx, Some(types1), Some(types2));
            if c != Ordering::Equal {
                return c;
            }
        }
    } else if flags.intersects(TypeFlags::STRING_MAPPING) {
        if let (TypeData::StringMapping { ty: type1 }, TypeData::StringMapping { ty: type2 }) =
            (&tables.type_of(t1).data, &tables.type_of(t2).data)
        {
            let c = compare_types(tables, ctx, *type1, *type2);
            if c != Ordering::Equal {
                return c;
            }
        }
    }
    // Fall back to type ids. This results in type creation order for
    // built-in types.
    t1.index().cmp(&t2.index())
}

/// tsc-port: getSortOrderFlags @6.0.3
/// tsc-span: _tsc.js:90780-90785
fn sort_order_flags(tables: &TypeTables, ty: TypeId) -> i64 {
    let flags = tables.flags_of(ty);
    if flags.intersects(TypeFlags::from_bits(
        TypeFlags::ENUM_LITERAL.bits() | TypeFlags::ENUM.bits(),
    )) && !flags.intersects(TypeFlags::UNION)
    {
        return i64::from(TypeFlags::ENUM.bits());
    }
    i64::from(flags.bits())
}

/// tsc-port: getTypeNameSymbol @6.0.3
/// tsc-span: _tsc.js:90803-90811
fn type_name_symbol(tables: &TypeTables, ty: TypeId) -> Option<SymbolId> {
    let r#type = tables.type_of(ty);
    if r#type.alias_symbol.is_some() {
        return r#type.alias_symbol;
    }
    if r#type.flags.intersects(TypeFlags::from_bits(
        TypeFlags::TYPE_PARAMETER.bits() | TypeFlags::STRING_MAPPING.bits(),
    )) || r#type.object_flags.intersects(ObjectFlags::from_bits(
        ObjectFlags::CLASS_OR_INTERFACE.bits() | ObjectFlags::REFERENCE.bits(),
    )) {
        return r#type.symbol;
    }
    None
}

/// tsc-port: compareTypeNames @6.0.3
/// tsc-span: _tsc.js:90786-90802
fn compare_type_names(
    tables: &TypeTables,
    ctx: &dyn TypeOrderContext,
    t1: TypeId,
    t2: TypeId,
) -> Ordering {
    let s1 = type_name_symbol(tables, t1);
    let s2 = type_name_symbol(tables, t2);
    if s1 == s2 {
        if tables.type_of(t1).alias_type_arguments.is_some() {
            return compare_type_lists(
                tables,
                ctx,
                tables.type_of(t1).alias_type_arguments.as_deref(),
                tables.type_of(t2).alias_type_arguments.as_deref(),
            );
        }
        return Ordering::Equal;
    }
    match (s1, s2) {
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        // tsgo compareTypeNames (checker/utilities.go:644-648): two
        // declarations with one name stay apart before their alias arguments
        // or their structure are compared.
        (Some(s1), Some(s2)) => ctx
            .compare_symbol_names(s1, s2)
            .then_with(|| ctx.compare_symbols(Some(s1), Some(s2))),
    }
}

/// tsgo compareNodes (checker/utilities.go:392-412) over nodes that may be
/// absent: an absent node follows a present one.
fn compare_optional_nodes(
    ctx: &dyn TypeOrderContext,
    n1: Option<u32>,
    n2: Option<u32>,
) -> Ordering {
    match (n1, n2) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(n1), Some(n2)) => ctx.compare_nodes(n1, n2),
    }
}

/// tsc-port: compareTypeLists @6.0.3
/// tsc-span: _tsc.js:90849-90858
pub fn compare_type_lists(
    tables: &TypeTables,
    ctx: &dyn TypeOrderContext,
    s1: Option<&[TypeId]>,
    s2: Option<&[TypeId]>,
) -> Ordering {
    let s1 = s1.unwrap_or(&[]);
    let s2 = s2.unwrap_or(&[]);
    if s1.len() != s2.len() {
        return s1.len().cmp(&s2.len());
    }
    for (a, b) in s1.iter().zip(s2) {
        let c = compare_types(tables, ctx, *a, *b);
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

/// tsc-port: compareTupleTypes @6.0.3
/// tsc-span: _tsc.js:90812-90836
fn compare_tuple_types(
    tables: &TypeTables,
    ctx: &dyn TypeOrderContext,
    target1: TypeId,
    target2: TypeId,
) -> Ordering {
    if target1 == target2 {
        return Ordering::Equal;
    }
    let (TypeData::TupleTarget(data1), TypeData::TupleTarget(data2)) =
        (&tables.type_of(target1).data, &tables.type_of(target2).data)
    else {
        return Ordering::Equal;
    };
    if data1.readonly != data2.readonly {
        return if data1.readonly {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    if data1.element_flags.len() != data2.element_flags.len() {
        return data1.element_flags.len().cmp(&data2.element_flags.len());
    }
    for (flags1, flags2) in data1.element_flags.iter().zip(data2.element_flags.iter()) {
        let c = flags1.bits().cmp(&flags2.bits());
        if c != Ordering::Equal {
            return c;
        }
    }
    // tsgo compareTupleTypes (checker/utilities.go:683-687) compares the
    // label of every element, so a tuple without labels precedes a labeled
    // one of the same shape (tsc 6.0.3 read only the labels of the first).
    let labels1 = data1.labeled_element_declarations.as_deref().unwrap_or(&[]);
    let labels2 = data2.labeled_element_declarations.as_deref().unwrap_or(&[]);
    for index in 0..data1.element_flags.len() {
        let label1 = labels1.get(index).copied().flatten();
        let label2 = labels2.get(index).copied().flatten();
        let c = ctx.compare_element_labels(label1, label2);
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

/// compareTypes' `slicesCompareString` (90762-90776): JavaScript string order
/// of the template texts, with the function's own length handling.
fn slices_compare_string(
    s1: &[crate::ty::TemplateText],
    s2: &[crate::ty::TemplateText],
) -> Ordering {
    for (index, text1) in s1.iter().enumerate() {
        if index > s2.len() {
            return Ordering::Greater;
        }
        let c = match s2.get(index) {
            // compareComparableValues(v1, undefined) === 1
            None => Ordering::Greater,
            Some(text2) => text1.units().cmp(text2.units()),
        };
        if c != Ordering::Equal {
            return c;
        }
    }
    if s1.len() < s2.len() {
        return Ordering::Less;
    }
    Ordering::Equal
}

/// `compareComparableValues` over two literal numbers: `a === b ? 0 : a < b ? -1 : 1`.
fn compare_numbers(a: f64, b: f64) -> Ordering {
    if a == b {
        Ordering::Equal
    } else if a < b {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

fn is_true_literal(tables: &TypeTables, ty: TypeId) -> bool {
    matches!(
        &tables.type_of(ty).data,
        TypeData::Intrinsic { name: "true", .. }
    )
}

/// The `target` of a type reference; generic class/interface and tuple
/// targets are their own targets.
fn reference_target(tables: &TypeTables, ty: TypeId) -> TypeId {
    match &tables.type_of(ty).data {
        TypeData::Reference { target, .. } => *target,
        _ => ty,
    }
}

/// tsc's `type.mapper` of a non-reference object type: a mapped type carries
/// its instantiation mapper in its type data, an instantiated anonymous type
/// in the checker's links, and reverse mapped types have none.
fn anonymous_or_mapped_mapper(
    tables: &TypeTables,
    ctx: &dyn TypeOrderContext,
    ty: TypeId,
) -> Option<MapperId> {
    match &tables.type_of(ty).data {
        TypeData::Mapped(data) => ctx.mapped_instantiation_mapper(data.mapper),
        TypeData::ReverseMapped(_) => None,
        _ => ctx.object_mapper(ty),
    }
}

/// tsgo-port: slices.SortFunc @go1.27 (`pdqsortCmpFunc` in
/// `slices/zsortanyfunc.go`): pattern-defeating quicksort, with insertion
/// sort for 12 elements or fewer and heapsort after too many bad pivots.
/// tsgo sorts with it comparisons that are not a total order (auto-import
/// fixes compare some kinds only against their own kind), so the same
/// algorithm gives tsgo's order and never panics on an inconsistent
/// comparison as the standard sorts may.
pub fn sort_func_like_go<T>(data: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let n = data.len();
    // bits.Len(uint(n))
    let limit = (usize::BITS - n.leading_zeros()) as usize;
    pdqsort_go(data, 0, n, limit, &mut cmp);
}

/// Go's `cmp(data[i], data[j]) < 0`.
fn go_less<T>(data: &[T], i: usize, j: usize, cmp: &mut impl FnMut(&T, &T) -> Ordering) -> bool {
    cmp(&data[i], &data[j]) == Ordering::Less
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortedHint {
    Unknown,
    Increasing,
    Decreasing,
}

fn pdqsort_go<T>(
    data: &mut [T],
    mut a: usize,
    mut b: usize,
    mut limit: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    const MAX_INSERTION: usize = 12;
    // Whether the last partitioning was reasonably balanced, and whether the
    // slice was already partitioned.
    let mut was_balanced = true;
    let mut was_partitioned = true;
    loop {
        let length = b - a;
        if length <= MAX_INSERTION {
            insertion_sort_go(data, a, b, cmp);
            return;
        }
        // Fall back to heapsort if too many bad choices were made.
        if limit == 0 {
            heap_sort_go(data, a, b, cmp);
            return;
        }
        // If the last partitioning was imbalanced, break patterns.
        if !was_balanced {
            break_patterns_go(data, a, b);
            limit -= 1;
        }
        let (mut pivot, mut hint) = choose_pivot_go(data, a, b, cmp);
        if hint == SortedHint::Decreasing {
            data[a..b].reverse();
            // The pivot was pivot-a elements after the start; after the
            // reversal it is pivot-a elements before the end.
            pivot = (b - 1) - (pivot - a);
            hint = SortedHint::Increasing;
        }
        // The slice is likely already sorted.
        if was_balanced
            && was_partitioned
            && hint == SortedHint::Increasing
            && partial_insertion_sort_go(data, a, b, cmp)
        {
            return;
        }
        // Probably many duplicate elements: partition into the elements
        // equal to the pivot and the elements greater than it.
        if a > 0 && !go_less(data, a - 1, pivot, cmp) {
            a = partition_equal_go(data, a, b, pivot, cmp);
            continue;
        }
        let (mid, already_partitioned) = partition_go(data, a, b, pivot, cmp);
        was_partitioned = already_partitioned;
        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort_go(data, a, mid, limit, cmp);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort_go(data, mid + 1, b, limit, cmp);
            b = mid;
        }
    }
}

/// Go `insertionSortCmpFunc`: `data[a..b]` by insertion.
fn insertion_sort_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && go_less(data, j, j - 1, cmp) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

/// Go `siftDownCmpFunc`: the heap property on `data[lo..hi]` of the heap
/// rooted at `first`.
fn sift_down_go<T>(
    data: &mut [T],
    lo: usize,
    hi: usize,
    first: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && go_less(data, first + child, first + child + 1, cmp) {
            child += 1;
        }
        if !go_less(data, first + root, first + child, cmp) {
            return;
        }
        data.swap(first + root, first + child);
        root = child;
    }
}

/// Go `heapSortCmpFunc`.
fn heap_sort_go<T>(data: &mut [T], a: usize, b: usize, cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    let first = a;
    let hi = b - a;
    if hi == 0 {
        return;
    }
    // Build the heap with the greatest element at the top.
    for i in (0..=(hi - 1) / 2).rev() {
        sift_down_go(data, i, hi, first, cmp);
    }
    // Pop the elements, largest first, into the end of data.
    for i in (0..hi).rev() {
        data.swap(first, first + i);
        sift_down_go(data, 0, i, first, cmp);
    }
}

/// Go `partitionCmpFunc`: one quicksort partition around `data[pivot]`;
/// the pivot's new index and whether the range was already partitioned.
fn partition_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    pivot: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> (usize, bool) {
    data.swap(a, pivot);
    // i and j are inclusive of the elements remaining to be partitioned.
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    while i <= j && go_less(data, i as usize, a, cmp) {
        i += 1;
    }
    while i <= j && !go_less(data, j as usize, a, cmp) {
        j -= 1;
    }
    if i > j {
        data.swap(j as usize, a);
        return (j as usize, true);
    }
    data.swap(i as usize, j as usize);
    i += 1;
    j -= 1;
    loop {
        while i <= j && go_less(data, i as usize, a, cmp) {
            i += 1;
        }
        while i <= j && !go_less(data, j as usize, a, cmp) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    data.swap(j as usize, a);
    (j as usize, false)
}

/// Go `partitionEqualCmpFunc`: the elements equal to `data[pivot]`, then
/// the greater ones (none is smaller).
fn partition_equal_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    pivot: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    data.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    loop {
        while i <= j && !go_less(data, a, i as usize, cmp) {
            i += 1;
        }
        while i <= j && go_less(data, a, j as usize, cmp) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    i as usize
}

/// Go `partialInsertionSortCmpFunc`: a few adjacent out-of-order pairs
/// shifted into place; whether the range ends sorted.
fn partial_insertion_sort_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> bool {
    // The maximum number of adjacent out-of-order pairs shifted, and no
    // shifting on short arrays.
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;
    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !go_less(data, i, i - 1, cmp) {
            i += 1;
        }
        if i == b {
            return true;
        }
        if b - a < SHORTEST_SHIFTING {
            return false;
        }
        data.swap(i, i - 1);
        // Shift the smaller one to the left (Go stops at index 1, not at
        // `a + 1`).
        if i - a >= 2 {
            let mut j = i - 1;
            while j >= 1 {
                if !go_less(data, j, j - 1, cmp) {
                    break;
                }
                data.swap(j, j - 1);
                j -= 1;
            }
        }
        // Shift the greater one to the right.
        if b - i >= 2 {
            let mut j = i + 1;
            while j < b {
                if !go_less(data, j, j - 1, cmp) {
                    break;
                }
                data.swap(j, j - 1);
                j += 1;
            }
        }
    }
    false
}

/// Go `breakPatternsCmpFunc`: three elements around the middle swapped with
/// pseudo-random ones (an xorshift seeded by the length).
fn break_patterns_go<T>(data: &mut [T], a: usize, b: usize) {
    let length = b - a;
    if length >= 8 {
        let mut random = length as u64;
        // nextPowerOfTwo: 1 << bits.Len(uint(length))
        let modulus = 1usize << (usize::BITS - length.leading_zeros());
        let middle = a + (length / 4) * 2;
        for index in middle - 1..=middle + 1 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut other = (random as usize) & (modulus - 1);
            if other >= length {
                other -= length;
            }
            data.swap(index, a + other);
        }
    }
}

/// Go `choosePivotCmpFunc`: a static pivot below 8 elements, the median of
/// three below 50, else Tukey's ninther; with how sorted the range looks.
fn choose_pivot_go<T>(
    data: &mut [T],
    a: usize,
    b: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> (usize, SortedHint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;
    let l = b - a;
    let mut swaps = 0;
    let mut i = a + l / 4;
    let mut j = a + l / 4 * 2;
    let mut k = a + l / 4 * 3;
    if l >= 8 {
        if l >= SHORTEST_NINTHER {
            i = median_go(data, i - 1, i, i + 1, &mut swaps, cmp);
            j = median_go(data, j - 1, j, j + 1, &mut swaps, cmp);
            k = median_go(data, k - 1, k, k + 1, &mut swaps, cmp);
        }
        // The median among i, j and k, into j.
        j = median_go(data, i, j, k, &mut swaps, cmp);
    }
    match swaps {
        0 => (j, SortedHint::Increasing),
        MAX_SWAPS => (j, SortedHint::Decreasing),
        _ => (j, SortedHint::Unknown),
    }
}

/// Go `medianCmpFunc` (with `order2CmpFunc`): the index of the median of
/// `data[a]`, `data[b]` and `data[c]`, counting the swaps made.
fn median_go<T>(
    data: &[T],
    a: usize,
    b: usize,
    c: usize,
    swaps: &mut usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    let mut order2 = |x: usize, y: usize, swaps: &mut usize| {
        if go_less(data, y, x, cmp) {
            *swaps += 1;
            (y, x)
        } else {
            (x, y)
        }
    };
    let (a, b) = order2(a, b, swaps);
    let (b, _c) = order2(b, c, swaps);
    let (_a, b) = order2(a, b, swaps);
    b
}

/// tsgo-port: slices.SortStableFunc @go1.26 (`stableCmpFunc` in
/// `slices/zsortanyfunc.go`): insertion sort of 20-element blocks, then
/// SymMerge passes. tsgo sorts a union's members with it
/// (`addTypesToUnion`), and `compareTypes` is not a total order in every
/// case (two same-named aliases compare by their members, which can cycle
/// with the alias-argument comparison), so the same algorithm gives the
/// same order as tsgo where a different stable sort could not, and never
/// panics on an inconsistent comparison as the standard sorts may.
pub fn sort_types_like_tsgo<T: Copy>(data: &mut [T], mut less: impl FnMut(T, T) -> bool) {
    let n = data.len();
    let mut block = 20;
    let (mut a, mut b) = (0, block);
    while b <= n {
        insertion_sort(data, a, b, &mut less);
        a = b;
        b += block;
    }
    insertion_sort(data, a, n, &mut less);
    while block < n {
        let (mut a, mut b) = (0, 2 * block);
        while b <= n {
            sym_merge(data, a, a + block, b, &mut less);
            a = b;
            b += 2 * block;
        }
        let m = a + block;
        if m < n {
            sym_merge(data, a, m, n, &mut less);
        }
        block *= 2;
    }
}

fn insertion_sort<T: Copy>(
    data: &mut [T],
    a: usize,
    b: usize,
    less: &mut impl FnMut(T, T) -> bool,
) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && less(data[j], data[j - 1]) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn sym_merge<T: Copy>(
    data: &mut [T],
    a: usize,
    m: usize,
    b: usize,
    less: &mut impl FnMut(T, T) -> bool,
) {
    if m - a == 1 {
        // Insert data[a] into data[m..b]: the lowest i with data[i] >= data[a].
        let (mut i, mut j) = (m, b);
        while i < j {
            let h = (i + j) >> 1;
            if less(data[h], data[a]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        for k in a..i.saturating_sub(1) {
            data.swap(k, k + 1);
        }
        return;
    }
    if b - m == 1 {
        // Insert data[m] into data[a..m]: the lowest i with data[i] > data[m].
        let (mut i, mut j) = (a, m);
        while i < j {
            let h = (i + j) >> 1;
            if !less(data[m], data[h]) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        let mut k = m;
        while k > i {
            data.swap(k, k - 1);
            k -= 1;
        }
        return;
    }
    let mid = (a + b) >> 1;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = (start + r) >> 1;
        if !less(data[p - c], data[c]) {
            start = c + 1;
        } else {
            r = c;
        }
    }
    let end = n - start;
    if start < m && m < end {
        rotate(data, start, m, end);
    }
    if a < start && start < mid {
        sym_merge(data, a, start, mid, less);
    }
    if mid < end && end < b {
        sym_merge(data, mid, end, b, less);
    }
}

fn rotate<T: Copy>(data: &mut [T], a: usize, m: usize, b: usize) {
    let (mut i, mut j) = (m - a, b - m);
    while i != j {
        if i > j {
            swap_range(data, m - i, m, j);
            i -= j;
        } else {
            swap_range(data, m - i, m + j - i, i);
            j -= i;
        }
    }
    swap_range(data, m - i, m, i);
}

fn swap_range<T: Copy>(data: &mut [T], a: usize, b: usize, n: usize) {
    for i in 0..n {
        data.swap(a + i, b + i);
    }
}

/// Where `ty` sits in `types`, which is sorted by `order`: tsc's
/// `binarySearch(types, type, getTypeId, compareValues)` with the option
/// off and `binarySearch(types, type, identity, compareTypes)` with it on.
pub fn search_type(
    tables: &TypeTables,
    order: TypeOrder<'_>,
    types: &[TypeId],
    ty: TypeId,
) -> Result<usize, usize> {
    match order {
        None => types.binary_search(&ty),
        Some(ctx) => types.binary_search_by(|&candidate| compare_types(tables, ctx, candidate, ty)),
    }
}

/// tsc-port: containsType @6.0.3
/// tsc-span: _tsc.js:61327-61329
pub fn contains_type(
    tables: &TypeTables,
    order: TypeOrder<'_>,
    types: &[TypeId],
    ty: TypeId,
) -> bool {
    search_type(tables, order, types, ty).is_ok()
}

/// tsc-port: insertType @6.0.3
/// tsc-span: _tsc.js:61330-61337
pub fn insert_type(
    tables: &TypeTables,
    order: TypeOrder<'_>,
    types: &mut Vec<TypeId>,
    ty: TypeId,
) -> bool {
    if let Err(index) = search_type(tables, order, types, ty) {
        types.insert(index, ty);
        return true;
    }
    false
}

#[cfg(test)]
#[path = "../tests/unit/type_order/tests.rs"]
mod tests;

/// tsgo PseudoBigInt.Compare (jsnum/pseudobigint.go:41-53): by sign, then
/// by magnitude (digit count, then digits), reversed for negative values.
fn compare_pseudo_big_ints(value1: &PseudoBigInt, value2: &PseudoBigInt) -> Ordering {
    fn digits(value: &PseudoBigInt) -> &str {
        value.base10_value.trim_start_matches('0')
    }
    fn sign(value: &PseudoBigInt) -> i8 {
        if digits(value).is_empty() {
            0
        } else if value.negative {
            -1
        } else {
            1
        }
    }
    let c = sign(value1).cmp(&sign(value2));
    if c != Ordering::Equal {
        return c;
    }
    let (digits1, digits2) = (digits(value1), digits(value2));
    let c = digits1
        .len()
        .cmp(&digits2.len())
        .then_with(|| digits1.cmp(digits2));
    if sign(value1) < 0 {
        c.reverse()
    } else {
        c
    }
}
