use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

fn parse(text: &str) -> SourceFile {
    parse_source_file("main.ts", text, ParseOptions::default(), None)
}

#[test]
fn record_flags_preserve_parser_bits_and_hide_the_locals_marker() {
    let mut source = parse("let name;");
    let node = source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == SyntaxKind::Identifier)
        .unwrap();
    let parsed = NodeFlags::JAVA_SCRIPT_FILE.bits() | NodeFlags::AWAIT_CONTEXT.bits();
    source.arena.node_mut(node).flags = parsed;
    let mut binder = BinderWorker::new(&mut source);
    let owned = [
        NodeFlags::EXPORT_CONTEXT,
        NodeFlags::CONTAINS_THIS,
        NodeFlags::HAS_IMPLICIT_RETURN,
        NodeFlags::HAS_EXPLICIT_RETURN,
        NodeFlags::HAS_ASYNC_FUNCTIONS,
        NodeFlags::UNREACHABLE,
    ];
    for combination in 0..64 {
        let expected = owned.iter().enumerate().fold(parsed, |flags, (bit, flag)| {
            flags
                | if combination & (1 << bit) != 0 {
                    flag.bits()
                } else {
                    0
                }
        });
        for has_locals in [true, false] {
            binder.set_has_locals(node, has_locals);
            binder.set_flags_of(node, NodeFlags::from_bits(expected));
            assert_eq!(binder.flags_of(node).bits(), expected);
            assert_eq!(
                binder.source.arena.node(node).flags,
                expected | if has_locals { HAS_LOCALS } else { 0 }
            );
            assert_eq!(
                binder.identifier_is_in_jsdoc_namespace(node),
                combination & (1 << 4) != 0
            );
        }
    }
    assert_eq!(std::mem::size_of::<tsc_syntax::Node>(), 32);
}

#[test]
fn published_flags_and_locals_are_in_the_bound_record() {
    let domain = IdentityDomain::new();
    let mut source = parse_in_domain(
        "/flags.ts",
        "export async function f() { this; return; dead(); }",
        &domain,
    );
    let parsed = source
        .arena
        .nodes()
        .iter()
        .map(|node| node.flags)
        .collect::<Vec<_>>();
    assert!(parsed.iter().all(|flags| flags & HAS_LOCALS == 0));
    let binder = Binder::bind_in_identity_domain(&mut source, &domain).unwrap();
    let data = BindData::from_binder(&binder);
    let mut changed = 0;
    for (id, parsed) in binder.source.arena.node_ids().zip(parsed) {
        let raw = binder.source.arena.node(id).flags;
        let visible = binder.flags_of(id);
        assert_eq!(
            raw & !(BINDER_NODE_FLAG_MASK | HAS_LOCALS),
            parsed & !BINDER_NODE_FLAG_MASK
        );
        assert_eq!(visible.bits() & HAS_LOCALS, 0);
        assert_eq!(published_node_flags(id), visible);
        assert_eq!(data.flags_of(id, &binder.source.arena), visible);
        assert_eq!(published_has_locals(id), binder.locals.contains_key(&id));
        changed += usize::from(visible.bits() != parsed);
    }
    assert!(changed > 0);
}

#[test]
fn bind_relocation_matches_direct_nonzero_symbols_and_private_serials() {
    let domain = IdentityDomain::new();
    // Leases ahead of the bind keep its bases away from the start of the
    // spaces in a fresh process.
    let _symbol_gap = domain.lease(IdentitySpace::Symbol, 13).unwrap();
    let _serial_gap = domain.lease(IdentitySpace::PrivateNameSerial, 9).unwrap();
    let mut source = parse(
        "export class Box { #value = 1; copy(other: Box) { return other.#value + this.#value; } }",
    );
    source.relocate_into_identity_domain(&domain).unwrap();

    let relocated = Binder::bind_in_identity_domain(&mut source, &domain).unwrap();
    assert!(relocated.identity_owned_by(&domain));
    let symbol_base = relocated.symbols.identity_lease().unwrap().range().start();
    let serial_base = relocated
        .private_name_serial_lease()
        .unwrap()
        .range()
        .start();
    assert!(relocated
        .symbols
        .symbols()
        .iter()
        .flat_map(|symbol| symbol.members().keys())
        .any(|name| name.starts_with(&format!("__#{serial_base}@"))));
    let expected = BindData::from_binder(&relocated);
    // A published tree is bound once at a time: the direct bind follows.
    drop(relocated);

    let mut direct = Binder::with_bases(&mut source, serial_base, symbol_base);
    direct.bind_source_file();
    assert_eq!(expected.symbols, direct.symbols);
    assert_eq!(expected.node_symbol, direct.node_symbol);
    assert_eq!(expected.node_local_symbol, direct.node_local_symbol);
    assert_eq!(expected.locals, direct.locals);
    assert_eq!(expected.classifiable_names, direct.classifiable_names);
    assert_eq!(expected.assigned_symbol_ids, direct.assigned_symbol_ids);
    assert_eq!(expected.next_symbol_id, direct.next_symbol_id);
}

fn statements(source: &SourceFile) -> Vec<NodeId> {
    let data = source
        .arena
        .node(source.root)
        .data()
        .as_source_file()
        .expect("root");
    let statements = data.statements.expect("statements");
    source.arena.node_array(statements).nodes.to_vec()
}

/// The declared node + includes/excludes a statement would get from
/// bindWorker (test-only shim until stage 3.4).
fn masks_for(source: &SourceFile, statement: NodeId) -> (NodeId, SymbolFlags, SymbolFlags) {
    match &source.arena.node(statement).data() {
        NodeData::FunctionDeclaration(_) => (
            statement,
            SymbolFlags::FUNCTION,
            SymbolFlags::FUNCTION_EXCLUDES,
        ),
        NodeData::ClassDeclaration(_) => {
            (statement, SymbolFlags::CLASS, SymbolFlags::CLASS_EXCLUDES)
        }
        NodeData::InterfaceDeclaration(_) => (
            statement,
            SymbolFlags::INTERFACE,
            SymbolFlags::INTERFACE_EXCLUDES,
        ),
        NodeData::EnumDeclaration(_) => (
            statement,
            SymbolFlags::REGULAR_ENUM,
            SymbolFlags::REGULAR_ENUM_EXCLUDES,
        ),
        NodeData::ModuleDeclaration(_) => (
            statement,
            SymbolFlags::VALUE_MODULE,
            SymbolFlags::VALUE_MODULE_EXCLUDES,
        ),
        NodeData::TypeAliasDeclaration(_) => (
            statement,
            SymbolFlags::TYPE_ALIAS,
            SymbolFlags::TYPE_ALIAS_EXCLUDES,
        ),
        NodeData::ExportAssignment(_) => (statement, SymbolFlags::PROPERTY, SymbolFlags::ALL),
        NodeData::VariableStatement(data) => {
            let list = data.declaration_list.expect("list");
            let declarations = match &source.arena.node(list).data() {
                NodeData::VariableDeclarationList(data) => data.declarations.expect("declarations"),
                _ => panic!("not a declaration list"),
            };
            let declaration = source.arena.node_array(declarations).nodes[0];
            let list_flags = crate::node_util::node_flags(source, list);
            if list_flags.intersects(tsc_types::NodeFlags::BLOCK_SCOPED) {
                (
                    declaration,
                    SymbolFlags::BLOCK_SCOPED_VARIABLE,
                    SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES,
                )
            } else {
                (
                    declaration,
                    SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                    SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
                )
            }
        }
        other => panic!("unhandled test statement kind: {other:?}"),
    }
}

/// Declare every top-level statement into one locals table (a mini
/// stand-in for bindWorker's routing), returning the symbols.
fn declare_all(
    binder: &mut Binder<'_>,
    table: TableRef,
    parent: Option<SymbolId>,
) -> Vec<SymbolId> {
    let statements = statements(binder.source);
    statements
        .iter()
        .map(|&statement| {
            let (node, includes, excludes) = masks_for(binder.source, statement);
            binder.declare_symbol(table, parent, node, includes, excludes, false, false)
        })
        .collect()
}

fn diag_pins(binder: &Binder<'_>) -> Vec<(u32, u32, u32)> {
    binder
        .bind_diagnostics
        .iter()
        .map(|diag| (diag.code(), diag.start.unwrap(), diag.length.unwrap()))
        .collect()
}

#[test]
fn function_overloads_merge() {
    let mut source = parse("function f(a: string): void;\nfunction f(a: number): void {}\n");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    let symbols = declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(symbols[0], symbols[1]);
    let symbol = binder.symbols.symbol(symbols[0]);
    assert!(symbol.flags.contains(SymbolFlags::FUNCTION));
    assert_eq!(symbol.declarations().len(), 2);
    // setValueDeclaration: the FIRST value declaration wins.
    assert_eq!(symbol.value_declaration, Some(symbol.declarations()[0]));
    assert!(binder.bind_diagnostics.is_empty());
    // Function is NOT in SymbolFlags.Classifiable.
    assert!(!binder.classifiable_names.contains("f"));
}

#[test]
fn namespace_merges_into_function() {
    let mut source = parse("function f() {}\nnamespace f { export const x = 1; }\n");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    let symbols = declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(symbols[0], symbols[1]);
    assert!(binder.bind_diagnostics.is_empty());
}

#[test]
fn interface_merges_into_class() {
    let mut source = parse("class D {}\ninterface D {}\n");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    let symbols = declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(symbols[0], symbols[1]);
    let symbol = binder.symbols.symbol(symbols[0]);
    assert!(symbol
        .flags
        .contains(SymbolFlags::CLASS | SymbolFlags::INTERFACE));
    assert!(binder.bind_diagnostics.is_empty());
}

#[test]
fn block_scoped_redeclaration_reports_2451_and_detaches_fresh_symbol() {
    // Pins from tsc sf.bindDiagnostics on "let x = 1;\nlet x = 2;".
    let mut source = parse("let x = 1;\nlet x = 2;");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    let symbols = declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(diag_pins(&binder), [(2451, 4, 1), (2451, 15, 1)]);
    // The fresh conflict symbol is DETACHED: the table keeps the
    // original, whose declarations stay [decl1].
    assert_ne!(symbols[0], symbols[1]);
    let table = binder.locals.get(&root).expect("locals");
    assert_eq!(table.get("x"), Some(&symbols[0]));
    assert_eq!(binder.symbols.symbol(symbols[0]).declarations().len(), 1);
}

#[test]
fn triple_let_conflicts_against_the_original_symbol() {
    // Pins from tsc: "let y = 1;\nlet y = 2;\nlet y = 3;".
    let mut source = parse("let y = 1;\nlet y = 2;\nlet y = 3;");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(
        diag_pins(&binder),
        [(2451, 4, 1), (2451, 15, 1), (2451, 4, 1), (2451, 26, 1)]
    );
}

#[test]
fn var_then_function_reports_duplicate_identifier() {
    // tsc pins {(2300,21,1),(2300,4,1)} — tsc's order differs
    // because bindEachFunctionsFirst binds the function BEFORE the
    // var (stage 3.4); source-order declaration flips it.
    let mut source = parse("var f: any;\nfunction f() {}");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(diag_pins(&binder), [(2300, 4, 1), (2300, 21, 1)]);
}

#[test]
fn enum_cannot_merge_with_class_reports_2567() {
    // Pins from tsc: "class C {}\nenum C {}".
    let mut source = parse("class C {}\nenum C {}");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    declare_all(&mut binder, TableRef::Locals(root), None);
    assert_eq!(diag_pins(&binder), [(2567, 6, 1), (2567, 16, 1)]);
    // messageNeedsName = false: the 2567 text carries no name.
    assert!(!binder.bind_diagnostics[0]
        .message_text()
        .code_units()
        .any(|unit| unit == u16::from(b'C')));
}

#[test]
fn multiple_default_export_classes_report_2528_with_relateds() {
    // Pins from tsc: "export default class C {}\nexport default class D {}"
    //   2528@(21,1) related 2753@(47,1); 2528@(47,1) related 2752@(21,1).
    let mut source = parse("export default class C {}\nexport default class D {}");
    let mut binder = Binder::new(&mut source);
    let container = binder.symbols.alloc(
        SymbolFlags::NONE,
        crate::escape_leading_underscores("container"),
    );
    declare_all(&mut binder, TableRef::Exports(container), Some(container));
    assert_eq!(diag_pins(&binder), [(2528, 21, 1), (2528, 47, 1)]);
    let first = &binder.bind_diagnostics[0];
    assert_eq!(
        (first.related[0].message.code, first.related[0].start),
        (2753, Some(47))
    );
    let second = &binder.bind_diagnostics[1];
    assert_eq!(
        (second.related[0].message.code, second.related[0].start),
        (2752, Some(21))
    );
    // Both bound under the "default" export name.
    assert!(binder
        .symbols
        .symbol(container)
        .exports()
        .contains_key("default"));
}

#[test]
fn multiple_export_assignments_report_2528_full_statement_spans() {
    // Pins from tsc: "export default 1;\nexport default 2;"
    //   2528@(0,17) related 2753@(18,17); 2528@(18,17) related 2752@(0,17).
    let mut source = parse("export default 1;\nexport default 2;");
    let mut binder = Binder::new(&mut source);
    let container = binder.symbols.alloc(
        SymbolFlags::NONE,
        crate::escape_leading_underscores("container"),
    );
    declare_all(&mut binder, TableRef::Exports(container), Some(container));
    assert_eq!(diag_pins(&binder), [(2528, 0, 17), (2528, 18, 17)]);
    let first = &binder.bind_diagnostics[0];
    assert_eq!(
        (
            first.related[0].message.code,
            first.related[0].start,
            first.related[0].length
        ),
        (2753, Some(18), Some(17))
    );
}

#[test]
fn escaped_names_key_the_table() {
    let mut source = parse("let __proto__ = 1;");
    let root = source.root;
    let mut binder = Binder::new(&mut source);
    declare_all(&mut binder, TableRef::Locals(root), None);
    let table = binder.locals.get(&root).expect("locals");
    assert!(table.contains_key("___proto__"));
    assert!(!table.contains_key("__proto__"));
}

fn parse_in_domain(name: &str, text: &str, domain: &IdentityDomain) -> SourceFile {
    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        name,
        tsc_diagnostics::TextSnapshot::new(
            text.to_owned(),
            tsc_diagnostics::DocumentVersion::default(),
        ),
        ParseOptions::default(),
        None,
        domain,
    )
    .unwrap()
}

#[test]
fn a_reserved_bind_that_outgrows_its_room_relocates_to_a_direct_binds_symbols() {
    // A room of two symbols for a bind of seven (the file's symbol, `f` and
    // its export twin, and `f`'s four expando members).
    let domain = IdentityDomain::new();
    let mut source = parse_in_domain(
        "/expando.ts",
        "export function f() {}\nf.a = 0;\nf.b = 0;\nf.c = 0;\nf.d = 0;\n",
        &domain,
    );
    let room = 2;
    let symbol_lease = domain.lease(IdentitySpace::Symbol, room).unwrap();
    let serial_lease = domain
        .lease(
            IdentitySpace::PrivateNameSerial,
            u32::try_from(source.arena.nodes().len()).unwrap(),
        )
        .unwrap();
    let mut reserved = Binder::bind_reserved(
        &mut source,
        symbol_lease.range(),
        serial_lease.range().start(),
    );
    assert_eq!(reserved.symbols.symbols().len(), 7);
    // As the checker does: the reservations stay held until the relocation.
    assert!(!reserved
        .attach_reserved_leases(&domain, symbol_lease.clone(), serial_lease.clone())
        .unwrap());
    reserved.relocate_into_identity_domain(&domain).unwrap();
    let symbol_base = reserved.symbols.identity_lease().unwrap().range().start();
    let serial_base = reserved
        .private_name_serial_lease()
        .unwrap()
        .range()
        .start();
    assert!(symbol_base >= symbol_lease.range().end());
    let expected = BindData::from_binder(&reserved);
    // A published tree is bound once at a time: the direct bind follows.
    drop(reserved);

    let mut direct = Binder::with_bases(&mut source, serial_base, symbol_base);
    direct.bind_source_file();
    assert_eq!(expected.symbols, direct.symbols);
    assert_eq!(expected.node_symbol, direct.node_symbol);
    assert_eq!(expected.node_local_symbol, direct.node_local_symbol);
    assert_eq!(expected.locals, direct.locals);
    assert_eq!(expected.assigned_symbol_ids, direct.assigned_symbol_ids);
    assert_eq!(expected.next_symbol_id, direct.next_symbol_id);
}

#[test]
#[should_panic(expected = "a published tree is bound once at a time")]
fn a_published_tree_is_bound_once_at_a_time() {
    let domain = IdentityDomain::new();
    let mut source = parse_in_domain("/once.ts", "let a = 1;", &domain);
    let _first = Binder::bind_in_identity_domain(&mut source, &domain)
        .unwrap()
        .into_bind_data();
    let _second = Binder::bind_in_identity_domain(&mut source, &domain).unwrap();
}

#[test]
fn a_dropped_bind_lets_its_tree_be_bound_again() {
    let domain = IdentityDomain::new();
    let mut source = parse_in_domain("/again.ts", "let a = 1;", &domain);
    let first = Binder::bind_in_identity_domain(&mut source, &domain).unwrap();
    let expected = BindData::from_binder(&first);
    drop(first);
    let second = Binder::bind_in_identity_domain(&mut source, &domain).unwrap();
    // The second bind's symbols have identities of their own; the tables
    // name the same nodes.
    assert_eq!(expected.classifiable_names, second.classifiable_names);
    assert!(expected
        .node_symbol
        .iter()
        .map(|(node, _)| node)
        .eq(second.node_symbol.iter().map(|(node, _)| node)));
}

#[test]
fn node_flow_slots_keep_flows_past_two_bytes_in_the_overflow() {
    use crate::flow::FlowId;
    let source = parse("let a = 1;\na = 2;\na = 3;\na = 4;\n");
    let nodes = statements(&source);
    let mut map = NodeFlowMap::for_tree(&source.arena);
    // Positions (index + 1) up to 0xFFFE fit a slot; 0xFFFF and above do not.
    let small = FlowId::new(6);
    let last_in_slot = FlowId::new(0xFFFD);
    let first_past = FlowId::new(0xFFFE);
    let large = FlowId::new(70_000);
    map.insert(nodes[0], small);
    map.insert(nodes[1], last_in_slot);
    map.insert(nodes[2], first_past);
    map.insert(nodes[3], large);
    assert_eq!(map.overflow.len(), 2);
    assert_eq!(
        map.iter().collect::<Vec<_>>(),
        [
            (nodes[0], small),
            (nodes[1], last_in_slot),
            (nodes[2], first_past),
            (nodes[3], large)
        ]
    );
    // A replacement moves a flow between the slot and the overflow.
    map.insert(nodes[2], small);
    map.insert(nodes[0], large);
    assert_eq!(map.get(&nodes[0]), Some(large));
    assert_eq!(map.get(&nodes[2]), Some(small));
    assert_eq!(map.overflow.len(), 2);
    assert_eq!(map.remove(&nodes[0]), Some(large));
    assert_eq!(map.remove(&nodes[3]), Some(large));
    assert!(map.overflow.is_empty());
    assert!(!map.contains_key(&nodes[0]));
    assert_eq!(map.get(&nodes[1]), Some(last_in_slot));
    assert_eq!(map.remove(&nodes[0]), None);
}
