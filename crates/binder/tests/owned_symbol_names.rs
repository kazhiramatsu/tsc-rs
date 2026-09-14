use tsc_binder::bind_source_file;
use tsc_syntax::{for_each_child, NodeId, ParseOptions, SourceFile};
use tsc_types::{CompilerOptions, JsString, ScriptTarget};

struct Case {
    id: &'static str,
    source: &'static str,
    kind: u16,
    table: &'static str,
    entries: &'static [(&'static [u16], usize)],
    patterns: &'static [(&'static [u16], &'static [u16])],
    diagnostics: &'static [(u32, &'static [u16])],
}

include!("fixtures/utf16-binder-names.rs");

#[test]
fn source_module_names_and_duplicate_diagnostic_paths_keep_js_values() {
    let mut names = Vec::new();
    for unit in [0xd800, 0xd801, 0xfffd] {
        let mut file_name = JsString::from("/work/");
        file_name.push_code_unit(unit);
        file_name.push_str(".ts");
        let source = tsc_syntax::parse_source_file(
            &file_name,
            "export {}; let a; let a;",
            ParseOptions::default(),
            None,
        );
        let options = CompilerOptions::default();
        let binder = bind_source_file(&source, &options);
        let symbol = binder.symbols.symbol(binder.node_symbol[&source.root]);
        let mut expected = "\"/work/".encode_utf16().collect::<Vec<_>>();
        expected.extend([unit, 0x22]);
        assert_eq!(symbol.escaped_name.as_js().to_utf16(), expected);
        names.push(symbol.escaped_name.clone());
        assert!(!binder.bind_diagnostics.is_empty());
        assert!(binder
            .bind_diagnostics
            .iter()
            .all(|d| d.file_name.as_ref() == Some(&file_name)));
    }
    assert_eq!(
        names
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
}

fn find_kind(source: &SourceFile, id: NodeId, kind: u16) -> Option<NodeId> {
    if source.arena.node(id).kind as u16 == kind {
        return Some(id);
    }
    let mut found = None;
    for_each_child(&source.arena, source.arena.node(id), |child| {
        found = find_kind(source, child, kind);
        found.is_some()
    });
    found
}

#[test]
fn binder_names_match_independent_typescript_observations() {
    for case in CASES {
        let source = tsc_syntax::parse_source_file(
            "main.ts",
            case.source,
            ParseOptions {
                script_target: ScriptTarget::ES2015,
                ..ParseOptions::default()
            },
            None,
        );
        assert!(source.parse_diagnostics.is_empty(), "{}", case.id);
        let options = CompilerOptions::default();
        let binder = bind_source_file(&source, &options);
        let selected = find_kind(&source, source.root, case.kind).unwrap();
        let table = match case.table {
            "locals" => &binder.locals[&selected],
            "members" => &binder.symbols.symbol(binder.node_symbol[&selected]).members,
            "exports" => &binder.symbols.symbol(binder.node_symbol[&selected]).exports,
            _ => unreachable!(),
        };
        let expected = case
            .entries
            .iter()
            .map(|(name, count)| (name.to_vec(), *count))
            .collect::<Vec<_>>();
        let entries = table
            .iter()
            .map(|(name, symbol)| {
                (
                    name.as_js().to_utf16(),
                    binder.symbols.symbol(*symbol).declarations.len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(entries, expected, "{}", case.id);
        for (index, (units, _)) in case.entries.iter().enumerate() {
            let key = JsString::from_code_units(units);
            assert_eq!(
                table.get(key.as_js()),
                table.get_index(index).map(|(_, id)| id),
                "{}",
                case.id
            );
            if let Some(scalar) = key.as_str() {
                assert_eq!(table.get(scalar), table.get(key.as_js()), "{}", case.id);
            }
        }
        let patterns = binder
            .pattern_ambient_modules
            .iter()
            .map(|(prefix, suffix, _)| (prefix.to_utf16(), suffix.to_utf16()))
            .collect::<Vec<_>>();
        assert_eq!(
            patterns,
            case.patterns
                .iter()
                .map(|(a, b)| (a.to_vec(), b.to_vec()))
                .collect::<Vec<_>>(),
            "{}",
            case.id
        );
        let diagnostics = binder
            .bind_diagnostics
            .iter()
            .map(|d| (d.code(), d.message.text.to_utf16()))
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            case.diagnostics
                .iter()
                .map(|(code, units)| (*code, units.to_vec()))
                .collect::<Vec<_>>(),
            "{}",
            case.id
        );
    }
}
