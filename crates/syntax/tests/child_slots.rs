//! The clone-free child slot walkers agree with `try_visit_each_child` on
//! every parsed node: same children in the same order, an identity mapping
//! keeps the payload, and a mapping or a removal produces the same payload
//! (or the same required-child error) through both paths.

use tsc_syntax::{
    apply_child_slots, child_slots, map_child_slots, try_visit_each_child, ChildSlot,
    JSDocParsingMode, LanguageVariant, NodeArena, NodeArrayId, NodeDataChildVisitor, NodeId,
    ParseOptions, SyntaxKind,
};

const TYPESCRIPT: &str = r#"
/**
 * Doc of f.
 * @param a the first
 * @returns nothing
 */
export async function f<T extends object = {}>(a: string, b = 1, ...rest: T[]): Promise<void> {
    return g?.(a) ?? b;
}
abstract class C<T> extends B<T> implements I, J {
    #p = 1;
    static { init(); }
    declare readonly q?: number;
    get x(): number { return this.#p; }
    set x(v) { this.#p = v; }
    constructor(private readonly y: T, @dec public z?: string) { super(); }
    static async *gen(): AsyncGenerator<number> { yield* other(); }
    [Symbol.iterator]() { return { next() { return { done: true, value: undefined }; } }; }
}
const { a, b: [c = 2, ...d], ...e } = obj as unknown as Record<string, number[]>;
label: for (const k in o) { if (k) continue label; else break; }
for (let i = 0, j = 10; i < j; i++, j--) { switch (i) { case 1: break; default: continue; } }
for await (const item of items) { try { throw new Error(`x${item}`); } catch ({ message }) { } finally { } }
type M<K extends string> = { readonly [P in K]?: `p${P}` } & (K extends "a" ? 1 : never);
type Fn = new (...args: any[]) => typeof C;
interface I { (x: number): string; new (): I; [k: string]: unknown; method?<U>(u: U): U; }
enum E { A = 1, B, C = A | B }
declare module "m" { export import Q = E.A; export default E; }
namespace N.M { export const v = <const>[1, 2]; export type * from "./t"; }
import type { X } from "./x" with { type: "json" };
import def, * as ns from "./y";
export { def as default, ns };
export * as star from "./z";
let t = a satisfies string, u = a!, w = <number>a, n = a?.b?.[c]?.(d);
const tpl = tag`a${b}c${d}e`;
do { x ??= y; x ||= z; x &&= w; } while (false);
with (o) { debugger; }
"#;

const JAVASCRIPT: &str = r#"
/**
 * @typedef {Object} Shape
 * @property {number} size the size
 * @property {string} [name]
 */
/**
 * @template T
 * @param {T} value
 * @param {import("./x").Y} y
 * @returns {Promise<T>}
 * @this {Shape}
 * @callback Cb
 * @enum {number}
 * @see other
 * @overload
 * @satisfies {Shape}
 * @typedef {{ a: number, b?: string }} Inline
 */
function f(value, y) { return value; }
/** @type {Shape} */
const shape = { size: 1 };
class K { /** @readonly */ p = 1; }
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visit {
    Node(NodeId),
    Nodes(NodeArrayId),
}

/// Records the children in visit order and maps every child to itself.
struct Recorder<'a> {
    arena: &'a NodeArena,
    visits: Vec<Visit>,
}

impl NodeDataChildVisitor for Recorder<'_> {
    type Error = ();

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.arena.node(id).kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, ()> {
        self.visits.push(Visit::Node(id));
        Ok(Some(id))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, ()> {
        self.visits.push(Visit::Nodes(id));
        Ok(Some(id))
    }

    fn required_child_removed(&mut self, _parent: SyntaxKind, _field: &'static str) {}
}

/// Maps every child to another id, or removes it.
struct Mapper<'a> {
    arena: &'a NodeArena,
    remove: bool,
}

impl NodeDataChildVisitor for Mapper<'_> {
    type Error = (SyntaxKind, &'static str);

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.arena.node(id).kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        Ok((!self.remove).then_some(NodeId(id.0 ^ 1)))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        Ok((!self.remove).then_some(NodeArrayId(id.0 ^ 1)))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        (parent, field)
    }
}

fn parse(name: &str, text: &str) -> tsc_syntax::SourceFile {
    tsc_syntax::parse_source_file(
        name,
        text,
        ParseOptions {
            javascript_file: name.ends_with(".js"),
            language_variant: if name.ends_with("x") {
                LanguageVariant::Jsx
            } else {
                LanguageVariant::Standard
            },
            js_doc_parsing_mode: JSDocParsingMode::ParseAll,
            ..ParseOptions::default()
        },
        None,
    )
}

fn slot_visits(slots: &[ChildSlot]) -> Vec<Visit> {
    slots
        .iter()
        .filter_map(|slot| match slot {
            ChildSlot::Absent => None,
            ChildSlot::Node(id) => Some(Visit::Node(*id)),
            ChildSlot::Nodes(id) | ChildSlot::JsDocNodes(id) => Some(Visit::Nodes(*id)),
        })
        .collect()
}

fn assert_slots_agree_with_each_child(name: &str, text: &str, min_kinds: usize) {
    let source = parse(name, text);
    let arena = &source.arena;
    let mut kinds_seen = std::collections::BTreeSet::new();
    for node in arena.nodes() {
        kinds_seen.insert(node.kind);
        // Same children, same order.
        let mut recorder = Recorder {
            arena,
            visits: Vec::new(),
        };
        let mut data = node.data.clone();
        try_visit_each_child(&mut data, &mut recorder).unwrap();
        assert_eq!(
            data, node.data,
            "{name}: identity visit changed {:?}",
            node.kind
        );
        let slots = child_slots(&node.data, &recorder);
        assert_eq!(
            slot_visits(slots.as_slice()),
            recorder.visits,
            "{name}: child slots of {:?} differ from the child visit",
            node.kind
        );

        // An identity mapping reports no change and keeps the payload.
        let mut identity = slots;
        assert!(!map_child_slots(&mut identity, &mut recorder).unwrap());
        let mut applied = node.data.clone();
        apply_child_slots(&mut applied, &identity, &mut recorder).unwrap();
        assert_eq!(
            applied, node.data,
            "{name}: identity slots changed {:?}",
            node.kind
        );

        // A mapping and a removal agree with the child visit's payload or error.
        for remove in [false, true] {
            let mut mapper = Mapper { arena, remove };
            let mut through_visit = node.data.clone();
            let visit_result = try_visit_each_child(&mut through_visit, &mut mapper);
            let mut mapped = child_slots(&node.data, &mapper);
            let changed = map_child_slots(&mut mapped, &mut mapper).unwrap();
            assert_eq!(
                changed,
                !recorder.visits.is_empty(),
                "{name}: change report of {:?} (remove = {remove})",
                node.kind
            );
            let mut through_slots = node.data.clone();
            let slot_result = apply_child_slots(&mut through_slots, &mapped, &mut mapper);
            assert_eq!(
                visit_result, slot_result,
                "{name}: results of {:?} differ (remove = {remove})",
                node.kind
            );
            if visit_result.is_ok() {
                assert_eq!(
                    through_visit, through_slots,
                    "{name}: payloads of {:?} differ (remove = {remove})",
                    node.kind
                );
            }
        }
    }
    assert!(
        kinds_seen.len() >= min_kinds,
        "{name}: the fixture exercises only {} node kinds",
        kinds_seen.len()
    );
}

#[test]
fn child_slots_agree_with_each_child_visit_on_typescript() {
    assert_slots_agree_with_each_child("slots.ts", TYPESCRIPT, 60);
}

#[test]
fn child_slots_agree_with_each_child_visit_on_javascript_jsdoc() {
    assert_slots_agree_with_each_child("slots.js", JAVASCRIPT, 25);
}

#[test]
fn child_slots_agree_with_each_child_visit_on_jsx() {
    assert_slots_agree_with_each_child(
        "slots.tsx",
        "const e = <div a=\"1\" {...p} b={2}>text{x}<A.B c /><></></div>;\nnamespace J { export const f = <T,>(t: T) => <span>{t}</span>; }\n",
        20,
    );
}
