use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

fn parse(text: &str) -> SourceFile {
    parse_source_file("main.ts", text, ParseOptions::default(), None)
}

#[test]
fn bind_relocation_matches_direct_nonzero_symbols_and_private_serials() {
    let domain = IdentityDomain::reclaiming();
    let symbol_gap = domain.lease(IdentitySpace::Symbol, 13).unwrap();
    let serial_gap = domain.lease(IdentitySpace::PrivateNameSerial, 9).unwrap();
    let mut source = parse(
        "export class Box { #value = 1; copy(other: Box) { return other.#value + this.#value; } }",
    );
    source.relocate_into_identity_domain(&domain).unwrap();
    let options = tsc_types::CompilerOptions::default();

    let relocated = Binder::bind_in_identity_domain(&source, &options, &domain).unwrap();
    let mut direct = Binder::with_bases(&source, &options, 10, 13);
    direct.bind_source_file();

    assert!(relocated.identity_owned_by(&domain));
    assert_eq!(
        relocated.symbols.identity_lease().unwrap().range().start(),
        13
    );
    assert_eq!(
        relocated
            .private_name_serial_lease()
            .unwrap()
            .range()
            .start(),
        10
    );
    assert_eq!(relocated.symbols, direct.symbols);
    assert_eq!(relocated.node_symbol, direct.node_symbol);
    assert_eq!(relocated.node_local_symbol, direct.node_local_symbol);
    assert_eq!(relocated.locals, direct.locals);
    assert_eq!(
        relocated.js_global_augmentations,
        direct.js_global_augmentations
    );
    assert_eq!(relocated.classifiable_names, direct.classifiable_names);
    assert_eq!(relocated.assigned_symbol_ids, direct.assigned_symbol_ids);
    assert_eq!(relocated.next_symbol_id, direct.next_symbol_id);
    assert!(relocated
        .symbols
        .symbols()
        .iter()
        .flat_map(|symbol| symbol.members.keys())
        .any(|name| name.starts_with("__#10@")));

    drop(relocated);
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Symbol)
            .active_ranges,
        1
    );
    drop(direct);
    drop(source);
    drop(symbol_gap);
    drop(serial_gap);
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Symbol)
            .active_ranges,
        0
    );
}

fn statements(source: &SourceFile) -> Vec<NodeId> {
    let data = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("root");
    let statements = data.statements.expect("statements");
    source.arena.node_array(statements).nodes.clone()
}

/// The declared node + includes/excludes a statement would get from
/// bindWorker (test-only shim until stage 3.4).
fn masks_for(source: &SourceFile, statement: NodeId) -> (NodeId, SymbolFlags, SymbolFlags) {
    match &source.arena.node(statement).data {
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
            let declarations = match &source.arena.node(list).data {
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
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(symbols[0], symbols[1]);
    let symbol = binder.symbols.symbol(symbols[0]);
    assert!(symbol.flags.contains(SymbolFlags::FUNCTION));
    assert_eq!(symbol.declarations.len(), 2);
    // setValueDeclaration: the FIRST value declaration wins.
    assert_eq!(symbol.value_declaration, Some(symbol.declarations[0]));
    assert!(binder.bind_diagnostics.is_empty());
    // Function is NOT in SymbolFlags.Classifiable.
    assert!(!binder.classifiable_names.contains("f"));
}

#[test]
fn namespace_merges_into_function() {
    let source = parse("function f() {}\nnamespace f { export const x = 1; }\n");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(symbols[0], symbols[1]);
    assert!(binder.bind_diagnostics.is_empty());
}

#[test]
fn interface_merges_into_class() {
    let source = parse("class D {}\ninterface D {}\n");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
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
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(diag_pins(&binder), [(2451, 4, 1), (2451, 15, 1)]);
    // The fresh conflict symbol is DETACHED: the table keeps the
    // original, whose declarations stay [decl1].
    assert_ne!(symbols[0], symbols[1]);
    let table = binder.locals.get(&source.root).expect("locals");
    assert_eq!(table.get("x"), Some(&symbols[0]));
    assert_eq!(binder.symbols.symbol(symbols[0]).declarations.len(), 1);
}

#[test]
fn triple_let_conflicts_against_the_original_symbol() {
    // Pins from tsc: "let y = 1;\nlet y = 2;\nlet y = 3;".
    let source = parse("let y = 1;\nlet y = 2;\nlet y = 3;");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
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
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(diag_pins(&binder), [(2300, 4, 1), (2300, 21, 1)]);
}

#[test]
fn enum_cannot_merge_with_class_reports_2567() {
    // Pins from tsc: "class C {}\nenum C {}".
    let source = parse("class C {}\nenum C {}");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
    assert_eq!(diag_pins(&binder), [(2567, 6, 1), (2567, 16, 1)]);
    // messageNeedsName = false: the 2567 text carries no name.
    assert!(!binder.bind_diagnostics[0].message_text().contains('C'));
}

#[test]
fn multiple_default_export_classes_report_2528_with_relateds() {
    // Pins from tsc: "export default class C {}\nexport default class D {}"
    //   2528@(21,1) related 2753@(47,1); 2528@(47,1) related 2752@(21,1).
    let source = parse("export default class C {}\nexport default class D {}");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    let container = binder
        .symbols
        .alloc(SymbolFlags::NONE, "container".to_owned());
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
        .exports
        .contains_key("default"));
}

#[test]
fn multiple_export_assignments_report_2528_full_statement_spans() {
    // Pins from tsc: "export default 1;\nexport default 2;"
    //   2528@(0,17) related 2753@(18,17); 2528@(18,17) related 2752@(0,17).
    let source = parse("export default 1;\nexport default 2;");
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    let container = binder
        .symbols
        .alloc(SymbolFlags::NONE, "container".to_owned());
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
    let options: &'static tsc_types::CompilerOptions =
        Box::leak(Box::new(tsc_types::CompilerOptions::default()));
    let mut binder = Binder::new(&source, options);
    declare_all(&mut binder, TableRef::Locals(source.root), None);
    let table = binder.locals.get(&source.root).expect("locals");
    assert!(table.contains_key("___proto__"));
    assert!(!table.contains_key("__proto__"));
}
