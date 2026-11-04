//! The clone-free child slot walkers agree with `try_visit_each_child` on
//! every parsed node: same children in the same order, an identity mapping
//! keeps the payload, and a mapping or a removal produces the same payload
//! (or the same required-child error) through both paths.

use tsc_syntax::{
    apply_child_slots, child_slots, map_child_slots, try_visit_each_child, ChildSlot,
    JSDocParsingMode, NodeArena, NodeArrayId, NodeDataChildVisitor, NodeId, ParseOptions,
    ScriptKind, SyntaxKind,
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
        Ok((!self.remove).then_some(NodeId::new(id.index() ^ 1)))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        Ok((!self.remove).then_some(NodeArrayId::new(id.index() ^ 1)))
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
            script_kind: ScriptKind::ensure_from_file_name(name),
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
        let mut data = node.data();
        try_visit_each_child(&mut data, &mut recorder).unwrap();
        assert_eq!(
            data,
            node.data(),
            "{name}: identity visit changed {:?}",
            node.kind
        );
        let slots = child_slots(&node.data(), &recorder);
        assert_eq!(
            slot_visits(slots.as_slice()),
            recorder.visits,
            "{name}: child slots of {:?} differ from the child visit",
            node.kind
        );

        // An identity mapping reports no change and keeps the payload.
        let mut identity = slots;
        assert!(!map_child_slots(&mut identity, &mut recorder).unwrap());
        let mut applied = node.data();
        apply_child_slots(&mut applied, &identity, &mut recorder).unwrap();
        assert_eq!(
            applied,
            node.data(),
            "{name}: identity slots changed {:?}",
            node.kind
        );

        // A mapping and a removal agree with the child visit's payload or error.
        for remove in [false, true] {
            let mut mapper = Mapper { arena, remove };
            let mut through_visit = node.data();
            let visit_result = try_visit_each_child(&mut through_visit, &mut mapper);
            let mut mapped = child_slots(&node.data(), &mapper);
            let changed = map_child_slots(&mut mapped, &mut mapper).unwrap();
            assert_eq!(
                changed,
                !recorder.visits.is_empty(),
                "{name}: change report of {:?} (remove = {remove})",
                node.kind
            );
            let mut through_slots = node.data();
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

/// Each shared field's slot read (`Node::<field>_field`) agrees with the
/// decoded payload's field as the observable field view reports it (an
/// absent field, or a kind without it, reads `None`).
fn assert_field_reads_agree_with_the_payload(name: &str, text: &str) {
    use tsc_syntax::{for_each_observable_field, Node, ObservableField};
    type Read = fn(&Node) -> Option<u32>;
    let reads: [(&str, Read); 29] = [
        ("name", |n| n.name_field().map(NodeId::position)),
        ("type", |n| n.type_field().map(NodeId::position)),
        ("expression", |n| n.expression_field().map(NodeId::position)),
        ("modifiers", |n| {
            n.modifiers_field().map(NodeArrayId::position)
        }),
        ("tagName", |n| n.tag_name_field().map(NodeId::position)),
        ("typeParameters", |n| {
            n.type_parameters_field().map(NodeArrayId::position)
        }),
        ("parameters", |n| {
            n.parameters_field().map(NodeArrayId::position)
        }),
        ("typeExpression", |n| {
            n.type_expression_field().map(NodeId::position)
        }),
        ("initializer", |n| {
            n.initializer_field().map(NodeId::position)
        }),
        ("questionToken", |n| {
            n.question_token_field().map(NodeId::position)
        }),
        ("typeArguments", |n| {
            n.type_arguments_field().map(NodeArrayId::position)
        }),
        ("body", |n| n.body_field().map(NodeId::position)),
        ("elements", |n| {
            n.elements_field().map(NodeArrayId::position)
        }),
        ("statement", |n| n.statement_field().map(NodeId::position)),
        ("attributes", |n| n.attributes_field().map(NodeId::position)),
        ("members", |n| n.members_field().map(NodeArrayId::position)),
        ("statements", |n| {
            n.statements_field().map(NodeArrayId::position)
        }),
        ("questionDotToken", |n| {
            n.question_dot_token_field().map(NodeId::position)
        }),
        ("exclamationToken", |n| {
            n.exclamation_token_field().map(NodeId::position)
        }),
        ("dotDotDotToken", |n| {
            n.dot_dot_dot_token_field().map(NodeId::position)
        }),
        ("asteriskToken", |n| {
            n.asterisk_token_field().map(NodeId::position)
        }),
        ("types", |n| n.types_field().map(NodeArrayId::position)),
        ("right", |n| n.right_field().map(NodeId::position)),
        ("propertyName", |n| {
            n.property_name_field().map(NodeId::position)
        }),
        ("moduleSpecifier", |n| {
            n.module_specifier_field().map(NodeId::position)
        }),
        ("literal", |n| n.literal_field().map(NodeId::position)),
        ("left", |n| n.left_field().map(NodeId::position)),
        ("label", |n| n.label_field().map(NodeId::position)),
        ("heritageClauses", |n| {
            n.heritage_clauses_field().map(NodeArrayId::position)
        }),
    ];
    let source = parse(name, text);
    let arena = &source.arena;
    let mut compared = 0;
    for node in arena.nodes() {
        let mut observed = std::collections::HashMap::new();
        for_each_observable_field(arena, node, |field, value| match value {
            ObservableField::Node(id) => {
                observed.insert(field, id.position());
            }
            ObservableField::NodeArray(id) => {
                observed.insert(field, id.position());
            }
            _ => {}
        });
        for (field, read) in &reads {
            assert_eq!(
                read(node),
                observed.get(field).copied(),
                "{name}: {:?}.{field}",
                node.kind
            );
            compared += usize::from(observed.contains_key(field));
        }
    }
    assert!(
        compared > 50,
        "{name}: only {compared} present fields compared"
    );
}

#[test]
fn field_reads_agree_with_the_payload() {
    assert_field_reads_agree_with_the_payload("slots.ts", TYPESCRIPT);
    assert_field_reads_agree_with_the_payload("slots.js", JAVASCRIPT);
}
