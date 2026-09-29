//! Boundary evidence for the explicit-stack `compute_transform_flags` walk
//! (F9/F17): nonzero node and array bases with several transform sources in
//! one arena, a deep synthetic subtree built without the parser, and shared
//! children plus a back edge. Every flag result is compared against the
//! former recursive walk (`reference_walk`, a verbatim copy of the replaced
//! implementation) on a clone of the same arena, so the claims are exactly
//! "identical to the previous contract", not stronger.

use std::collections::BTreeSet;

use tsc_syntax::nodes::{ArrayLiteralExpressionData, NonNullExpressionData};
use tsc_syntax::{
    for_each_child, for_each_child_array, parse_source_file, NodeData, NodeId, ParseOptions,
    SyntaxKind,
};
use tsc_types::NodeFlags;

use super::{
    complete_class_transform_flags, compute_transform_flags, factory_child_transform_flags,
    initialize_transform_flags, local_contextual_target_flags, local_transform_flags,
    static_this_substitute_flags,
};
use crate::{TransformArena, TransformError, TransformFlags, TransformNode, TransformSourceId};

/// The replaced recursive walk, copied verbatim (BTreeSet visiting/complete
/// sets, whole-record clone, per-node child vectors).
fn reference_walk(
    arena: &mut TransformArena,
    source: TransformSourceId,
    id: NodeId,
    visiting: &mut BTreeSet<NodeId>,
    complete: &mut BTreeSet<NodeId>,
) -> Result<TransformFlags, TransformError> {
    if complete.contains(&id) {
        return Ok(arena.transform_flags(
            arena
                .node_ref(source, id)
                .expect("completed transform node remains in its arena"),
        ));
    }
    if !visiting.insert(id) {
        return Ok(TransformFlags::NONE);
    }
    let node = arena
        .node_ref(source, id)
        .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, id)))?;
    let record = arena.node(node)?.clone();
    let syntax = arena.source(source)?.syntax();
    let mut children = Vec::new();
    for_each_child(&syntax.arena, &record, |child| {
        children.push(child);
        false
    });
    let mut arrays = Vec::new();
    for_each_child_array(&record, |array| {
        arrays.push(array);
        false
    });

    for child in &children {
        reference_walk(arena, source, *child, visiting, complete)?;
    }
    for array in arrays {
        let array_ref = arena
            .node_array_ref(source, array)
            .expect("generated child array belongs to its source");
        let ids = arena.node_array(array_ref)?.nodes.clone();
        let mut flags = TransformFlags::NONE;
        for child in ids {
            let child_flags = reference_walk(arena, source, child, visiting, complete)?;
            let child = arena
                .node_ref(source, child)
                .expect("generated array child belongs to its source");
            let kind = arena.node(child)?.kind;
            flags |= child_flags & !TransformFlags::subtree_exclusions(kind);
        }
        arena.set_array_transform_flags(array_ref, flags);
    }

    let mut flags =
        local_transform_flags(&record) | local_contextual_target_flags(arena, source, &record)?;
    flags |= factory_child_transform_flags(arena, source, &record)?;
    let flags = complete_class_transform_flags(arena, source, &record, flags)?;
    let flags = flags | static_this_substitute_flags(arena, node);
    arena.set_transform_flags(node, flags);
    visiting.remove(&id);
    complete.insert(id);
    Ok(flags)
}

/// Every node and array flag of `source` in both arenas, in id order.
fn all_flags(arena: &TransformArena, source: TransformSourceId) -> Vec<(u32, i32, bool)> {
    let syntax = arena.source(source).unwrap().syntax();
    let mut rows = Vec::new();
    for id in syntax.arena.node_base()..syntax.arena.node_end() {
        let node = arena.node_ref(source, NodeId::new(id)).unwrap();
        rows.push((id, arena.transform_flags(node).bits(), false));
    }
    for id in syntax.arena.array_base()..syntax.arena.array_end() {
        let array = arena
            .node_array_ref(source, tsc_syntax::NodeArrayId::new(id))
            .unwrap();
        rows.push((id, arena.array_transform_flags(array).bits(), true));
    }
    rows
}

const RICH_SOURCES: [&str; 3] = [
    "import { dep } from './dep';\n@decorate class C<T> extends Base implements I { private readonly x: number = 1; static #y = 2; constructor(public p: string) { super(); } async method(): Promise<void> { await this.p; } get v() { return this.x; } }\nexport const chained = obj?.a?.[0]?.(1) ?? dep!;\nexport enum E { A = 1, B }\nnamespace N { export const t = `a${1}b${`nested`}`; }\nexport type T = keyof typeof E;\nfor (const [a, ...rest] of items) { label: while (true) { break label; } }\nexport default async function* gen(...args: number[]) { yield* args; }\n",
    "export {};\nlet unused = 1;\nclass D { static { console.log(D); } accessor value = 1; declare field: string; }\nconst o = { ...spread, [computed]: 1, method() {}, get g() { return 1; } };\nfunction f(this: Window, a = 1, { b, c }: { b: number; c: string }) { return arguments; }\nconst arrow = async <U,>(u: U) => u satisfies unknown as U;\n",
    "/** @deprecated */\nexport interface I { call(): void; new (): I; readonly [k: string]: unknown; }\nabstract class A { abstract m(): void; protected constructor() {} }\nexport * as ns from './m';\nimport type { Only } from './t';\nexport = A;\n",
];

fn parsed_with_bases(index: usize, text: &str) -> tsc_syntax::SourceFile {
    parse_source_file(
        format!("/f{index}.ts"),
        text,
        ParseOptions {
            node_id_base: 10_000 + (index as u32) * 100_000,
            node_array_id_base: 3_000 + (index as u32) * 10_000,
            ..ParseOptions::default()
        },
        None,
    )
}

#[test]
fn explicit_walk_matches_the_recursive_reference_on_nonzero_bases_and_multiple_sources() {
    let parsed: Vec<_> = RICH_SOURCES
        .iter()
        .enumerate()
        .map(|(index, text)| parsed_with_bases(index, text))
        .collect();
    let mut explicit = TransformArena::new();
    let sources: Vec<TransformSourceId> = parsed
        .iter()
        .map(|source| explicit.add_source(source, None))
        .collect();
    assert!(
        explicit
            .source(sources[0])
            .unwrap()
            .syntax()
            .arena
            .node_base()
            >= 10_000
    );
    let mut reference = explicit.clone();
    // The reference walk runs on the clone, the explicit walk on the original.
    for &source in &sources {
        let root = reference.root(source).unwrap().node();
        reference_walk(
            &mut reference,
            source,
            root,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )
        .unwrap();
    }
    for (position, &source) in sources.iter().enumerate() {
        // Walking one source never touches another source's tables.
        for &other in &sources[position..] {
            assert!(all_flags(&explicit, other)
                .iter()
                .all(|&(_, bits, _)| bits == 0));
        }
        initialize_transform_flags(&mut explicit, source).unwrap();
        assert_eq!(
            all_flags(&explicit, source),
            all_flags(&reference, source),
            "source {position}"
        );
    }
    // Observable sanity: the first source carries TypeScript, ES2017 (async)
    // and ES2020 (optional chaining) content at its root.
    let root = explicit.root(sources[0]).unwrap();
    let flags = explicit.transform_flags(root);
    assert!(flags.contains(TransformFlags::CONTAINS_TYPE_SCRIPT));
    assert!(flags.contains(TransformFlags::CONTAINS_ES_2017));
    assert!(flags.contains(TransformFlags::CONTAINS_ES_2020));
}

/// A chain of 50,000 synthetic `NonNullExpression` nodes, built in the parsed
/// `SourceFile` before it is mounted (the detached clone keeps the synthetic
/// ids): the explicit walk classifies it on an explicitly created 1 MiB
/// thread stack, and every level carries `ContainsTypeScript`. The reference
/// recursion is deliberately not run here (it would need one native frame
/// per level).
#[test]
fn explicit_walk_classifies_a_deep_synthetic_subtree_without_recursion() {
    const DEPTH: usize = 50_000;
    const WALK_STACK_BYTES: usize = 1 << 20;
    let mut parsed = parsed_with_bases(7, "export {};\n");
    let (innermost, top, middle) = {
        let syntax = &mut parsed.arena;
        let innermost = syntax.alloc_token(SyntaxKind::ThisKeyword, 0, 0, NodeFlags::from_bits(0));
        let mut current = innermost;
        let mut middle = innermost;
        for level in 0..DEPTH {
            current = syntax.alloc_node(
                NodeData::NonNullExpression(NonNullExpressionData {
                    expression: Some(current),
                }),
                0,
                0,
                NodeFlags::from_bits(0),
            );
            if level == DEPTH / 2 {
                middle = current;
            }
        }
        (innermost, current, middle)
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    assert!(
        arena.node_ref(source, top).is_some() && arena.node_ref(source, innermost).is_some(),
        "the mounted clone keeps the synthetic node ids"
    );
    let arena = std::thread::Builder::new()
        .name("flag-walk-1mib".to_owned())
        .stack_size(WALK_STACK_BYTES)
        .spawn(move || {
            compute_transform_flags(&mut arena, source, top).unwrap();
            arena
        })
        .expect("spawn the 1 MiB walk thread")
        .join()
        .expect("the explicit walk completes on a 1 MiB stack");
    let flags_of = |id: NodeId| arena.transform_flags(arena.node_ref(source, id).unwrap());
    assert!(flags_of(top).contains(TransformFlags::CONTAINS_TYPE_SCRIPT));
    assert!(flags_of(middle).contains(TransformFlags::CONTAINS_TYPE_SCRIPT));
    assert_eq!(
        flags_of(innermost),
        local_transform_flags(
            arena
                .node(arena.node_ref(source, innermost).unwrap())
                .unwrap()
        )
    );
    // The chain is classified bottom-up: a level's flags are its own local
    // flags plus what its child contributes, so the top and the middle agree
    // on the TypeScript bit and the root of the parsed file stays untouched.
    let root = arena.root(source).unwrap();
    assert_eq!(arena.transform_flags(root), TransformFlags::NONE);
}

/// Shared children (the same node in two array positions and under two
/// parents) and a back edge (an array element that is an ancestor on the
/// walk path), synthesized in the parsed `SourceFile` before mounting,
/// terminate and produce exactly the reference walk's flags.
#[test]
fn explicit_walk_matches_the_reference_with_shared_children_and_a_back_edge() {
    let mut parsed = parsed_with_bases(9, "export {};\n");
    let (parent, inner, shared, leaf, outer_array, inner_array) = {
        let syntax = &mut parsed.arena;
        let none = NodeFlags::from_bits(0);
        let leaf = syntax.alloc_token(SyntaxKind::ThisKeyword, 0, 0, none);
        let shared = syntax.alloc_node(
            NodeData::NonNullExpression(NonNullExpressionData {
                expression: Some(leaf),
            }),
            0,
            0,
            none,
        );
        // inner = [shared]; the back edge to `parent` is appended below.
        let inner_array = syntax.alloc_array(vec![shared], 0, 0, false);
        let inner = syntax.alloc_node(
            NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                elements: Some(inner_array),
            }),
            0,
            0,
            none,
        );
        // parent = [shared, shared, inner]
        let outer_array = syntax.alloc_array(vec![shared, shared, inner], 0, 0, false);
        let parent = syntax.alloc_node(
            NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                elements: Some(outer_array),
            }),
            0,
            0,
            none,
        );
        // Back edge: inner's array now also contains its ancestor `parent`.
        syntax.node_array_mut(inner_array).nodes.push(parent);
        (parent, inner, shared, leaf, outer_array, inner_array)
    };
    let mut explicit = TransformArena::new();
    let source = explicit.add_source(&parsed, None);
    let mut reference = explicit.clone();
    reference_walk(
        &mut reference,
        source,
        parent,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
    .unwrap();
    compute_transform_flags(&mut explicit, source, parent).unwrap();
    assert_eq!(all_flags(&explicit, source), all_flags(&reference, source));

    let node =
        |arena: &TransformArena, id| arena.transform_flags(arena.node_ref(source, id).unwrap());
    let array = |arena: &TransformArena, id| {
        arena.array_transform_flags(arena.node_array_ref(source, id).unwrap())
    };
    // Concrete values, taken from the reference and re-derived here: the
    // shared child is classified once and read twice; the back edge to the
    // still-open `parent` contributes nothing to inner's aggregate, so that
    // aggregate is exactly the shared child's contribution.
    let shared_contribution = node(&explicit, shared)
        & !TransformFlags::subtree_exclusions(SyntaxKind::NonNullExpression);
    assert!(node(&explicit, shared).contains(TransformFlags::CONTAINS_TYPE_SCRIPT));
    assert_eq!(array(&explicit, inner_array), shared_contribution);
    assert_eq!(
        array(&explicit, inner_array),
        array(&reference, inner_array)
    );
    assert_eq!(
        array(&explicit, outer_array),
        shared_contribution
            | (node(&explicit, inner)
                & !TransformFlags::subtree_exclusions(SyntaxKind::ArrayLiteralExpression))
    );
    assert_eq!(node(&explicit, leaf), node(&reference, leaf));
    assert_eq!(node(&explicit, parent), node(&reference, parent));
}
