use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

fn parse(text: &str) -> SourceFile {
    parse_source_file("main.ts", text, ParseOptions::default(), None)
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

    let relocated = Binder::bind_in_identity_domain(&source, &domain).unwrap();
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

    let mut direct = Binder::with_bases(&source, serial_base, symbol_base);
    direct.bind_source_file();
    assert_eq!(expected.symbols, direct.symbols);
    assert_eq!(
        expected.node_symbol,
        NodeSymbols::from_map(&direct.node_symbol)
    );
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
    let source = parse("function f(a: string): void;\nfunction f(a: number): void {}\n");
    let mut binder = Binder::new(&source);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
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
    let source = parse("function f() {}\nnamespace f { export const x = 1; }\n");
    let mut binder = Binder::new(&source);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(symbols[0], symbols[1]);
    assert!(binder.bind_diagnostics.is_empty());
}

#[test]
fn interface_merges_into_class() {
    let source = parse("class D {}\ninterface D {}\n");
    let mut binder = Binder::new(&source);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
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
    let source = parse("let x = 1;\nlet x = 2;");
    let mut binder = Binder::new(&source);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(diag_pins(&binder), [(2451, 4, 1), (2451, 15, 1)]);
    // The fresh conflict symbol is DETACHED: the table keeps the
    // original, whose declarations stay [decl1].
    assert_ne!(symbols[0], symbols[1]);
    let table = binder.locals.get(&source.root).expect("locals");
    assert_eq!(table.get("x"), Some(&symbols[0]));
    assert_eq!(binder.symbols.symbol(symbols[0]).declarations().len(), 1);
}

#[test]
fn triple_let_conflicts_against_the_original_symbol() {
    // Pins from tsc: "let y = 1;\nlet y = 2;\nlet y = 3;".
    let source = parse("let y = 1;\nlet y = 2;\nlet y = 3;");
    let mut binder = Binder::new(&source);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
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
    let source = parse("var f: any;\nfunction f() {}");
    let mut binder = Binder::new(&source);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(diag_pins(&binder), [(2300, 4, 1), (2300, 21, 1)]);
}

#[test]
fn enum_cannot_merge_with_class_reports_2567() {
    // Pins from tsc: "class C {}\nenum C {}".
    let source = parse("class C {}\nenum C {}");
    let mut binder = Binder::new(&source);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
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
    let source = parse("export default class C {}\nexport default class D {}");
    let mut binder = Binder::new(&source);
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
    let source = parse("export default 1;\nexport default 2;");
    let mut binder = Binder::new(&source);
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
    let source = parse("let __proto__ = 1;");
    let mut binder = Binder::new(&source);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
    let table = binder.locals.get(&source.root).expect("locals");
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

/// The checker binds Program sources locally on worker threads and relocates
/// them in Program order; that must publish exactly the symbol and
/// private-name-serial identities of the sequential in-domain bind.
#[test]
fn local_bind_with_in_order_relocation_matches_direct_binding_at_the_same_bases() {
    let texts = [
        "export class Box { #value = 1; copy(other: Box) { return other.#value + this.#value; } }\nexport const boxed = new Box();\n",
        "class Other { #x = 2; #y = 3; read() { return this.#x + this.#y; } }\nlet other = new Other();\nfunction f(a: number) { let b = a; return b; }\n",
        "export {};\nlet unused = 1;\nnamespace N { export const n = 1; }\n",
    ];

    let domain = IdentityDomain::new();
    let sources: Vec<SourceFile> = texts
        .iter()
        .enumerate()
        .map(|(index, text)| parse_in_domain(&format!("/f{index}.ts"), text, &domain))
        .collect();
    let mut relocated: Vec<Binder<'_>> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .rev()
            .map(|source| scope.spawn(|| Binder::bind_local(source)))
            .collect();
        let mut bound: Vec<Binder<'_>> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        bound.reverse();
        bound
    });
    for binder in &mut relocated {
        assert!(binder.symbol_identity_lease().is_none());
        binder.relocate_into_identity_domain(&domain).unwrap();
        assert!(binder.identity_owned_by(&domain));
    }

    // The three files occupy strictly increasing, gap-free identity ranges.
    let mut previous_symbol_end = None;
    let mut previous_serial_end = None;
    for binder in &relocated {
        let symbols = binder.symbol_identity_lease().unwrap().range();
        let serials = binder.private_name_serial_lease().unwrap().range();
        if let Some(end) = previous_symbol_end {
            assert_eq!(symbols.start(), end);
        }
        if let Some(end) = previous_serial_end {
            assert_eq!(serials.start(), end);
        }
        previous_symbol_end = Some(symbols.end());
        previous_serial_end = Some(serials.end());
    }

    // Each relocated bind equals a bind constructed directly at its bases.
    // The relocated binders go first: a published tree is bound once at a
    // time.
    let expected: Vec<(u32, u32, BindData)> = relocated
        .iter()
        .map(|binder| {
            (
                binder.symbol_identity_lease().unwrap().range().start(),
                binder.private_name_serial_lease().unwrap().range().start(),
                BindData::from_binder(binder),
            )
        })
        .collect();
    drop(relocated);
    for (source, (symbol_base, serial_base, expected)) in sources.iter().zip(expected) {
        let mut direct = Binder::with_bases(source, serial_base, symbol_base);
        direct.bind_source_file();
        assert_eq!(
            expected.private_name_serial_base,
            direct.private_name_serial_base
        );
        assert_eq!(expected.next_symbol_id, direct.next_symbol_id);
        assert_eq!(expected.symbols, direct.symbols);
        assert_eq!(
            expected.node_symbol,
            NodeSymbols::from_map(&direct.node_symbol)
        );
        assert_eq!(expected.node_local_symbol, direct.node_local_symbol);
        assert_eq!(expected.locals, direct.locals);
        assert_eq!(expected.classifiable_names, direct.classifiable_names);
        assert_eq!(expected.assigned_symbol_ids, direct.assigned_symbol_ids);
        assert_eq!(expected.bind_diagnostics, direct.bind_diagnostics);
        assert_eq!(expected.node_flow, direct.node_flow);
        assert_eq!(expected.node_flags_mut, direct.node_flags_mut);
    }
}

#[test]
#[should_panic(expected = "a published tree is bound once at a time")]
fn a_published_tree_is_bound_once_at_a_time() {
    let domain = IdentityDomain::new();
    let source = parse_in_domain("/once.ts", "let a = 1;", &domain);
    let _first = Binder::bind_in_identity_domain(&source, &domain).unwrap();
    let _second = Binder::bind_in_identity_domain(&source, &domain).unwrap();
}

#[test]
fn a_dropped_bind_lets_its_tree_be_bound_again() {
    let domain = IdentityDomain::new();
    let source = parse_in_domain("/again.ts", "let a = 1;", &domain);
    let first = Binder::bind_in_identity_domain(&source, &domain).unwrap();
    let expected = BindData::from_binder(&first);
    drop(first);
    let second = Binder::bind_in_identity_domain(&source, &domain).unwrap();
    // The second bind's symbols have identities of their own; the tables
    // name the same nodes.
    assert_eq!(expected.classifiable_names, second.classifiable_names);
    assert!(expected
        .node_symbol
        .iter()
        .map(|(node, _)| node)
        .eq(second.node_symbol.iter().map(|(node, _)| node)));
}
