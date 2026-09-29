//! Relation pins: 415 source/target type pairs whose assignability or
//! comparability TypeScript 6.0.3 decided once (`pins/relations.toml`,
//! recorded from the fixtures in `pins/fixtures/relations/`).
//!
//! Each pin becomes the fixture the oracle saw; the harness expands its
//! `// @option` directives into compiler options exactly as for any other
//! fixture, and the checker's relation probe answers the same question.
//! An assignable fixture is `declare var s: Source; var t: Target = s;`
//! (any semantic diagnostic = not related); a comparable fixture is
//! `s as Target`; an `expr` pin assigns the expression itself so the
//! source stays a fresh literal.

use std::fmt::Write as _;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Value};
use tsc_checker::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};

const PINS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../pins/relations.toml"
));
const PIN_KEYS: &[&str] = &[
    "source", "target", "relation", "options", "setup", "expr", "expect",
];

struct Pin {
    id: String,
    source: String,
    target: String,
    relation: RelpinRelation,
    /// `// @name: value` directives, in file order.
    options: Vec<(String, String)>,
    setup: Option<String>,
    expr: Option<String>,
    related: bool,
}

fn string(table: &toml_edit::Table, id: &str, key: &str) -> Option<String> {
    table.get(key).map(|item| {
        item.as_str()
            .unwrap_or_else(|| panic!("{id}: {key} must be a string"))
            .to_owned()
    })
}

fn directive_value(id: &str, name: &str, value: &Value) -> String {
    match value {
        Value::String(text) => {
            let text = text.value();
            assert!(
                !text.contains(',') && !text.contains('*'),
                "{id}: option {name} would expand to a fixture matrix"
            );
            text.clone()
        }
        Value::Boolean(flag) => flag.value().to_string(),
        Value::Integer(number) => number.value().to_string(),
        _ => panic!("{id}: option {name} must be a scalar"),
    }
}

fn pins() -> Vec<Pin> {
    let document: DocumentMut = PINS.parse().expect("pins/relations.toml is TOML");
    let Some(Item::ArrayOfTables(pairs)) = document.get("pair") else {
        panic!("pins/relations.toml has no [[pair]] tables");
    };
    pairs
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let id = format!("p{:03}", index + 1);
            for (key, _) in table.iter() {
                assert!(PIN_KEYS.contains(&key), "{id}: unknown pin key {key}");
            }
            let relation = match string(table, &id, "relation").as_deref() {
                None | Some("assignable") => RelpinRelation::Assignable,
                Some("comparable") => RelpinRelation::Comparable,
                Some(other) => panic!("{id}: unknown relation {other}"),
            };
            let options = match table.get("options") {
                None => Vec::new(),
                Some(item) => item
                    .as_inline_table()
                    .unwrap_or_else(|| panic!("{id}: options must be an inline table"))
                    .iter()
                    .map(|(name, value)| (name.to_owned(), directive_value(&id, name, value)))
                    .collect(),
            };
            let related = match string(table, &id, "expect").as_deref() {
                Some("yes") => true,
                Some("no") => false,
                other => panic!("{id}: expect must be \"yes\" or \"no\", got {other:?}"),
            };
            Pin {
                source: string(table, &id, "source").unwrap_or_else(|| panic!("{id}: no source")),
                target: string(table, &id, "target").unwrap_or_else(|| panic!("{id}: no target")),
                relation,
                options,
                setup: string(table, &id, "setup"),
                expr: string(table, &id, "expr"),
                related,
                id,
            }
        })
        .collect()
}

/// The fixture the oracle answered for `pin`.
fn fixture_text(pin: &Pin) -> String {
    let mut out = String::new();
    let sets_lib = pin
        .options
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("nolib") || name.eq_ignore_ascii_case("lib"));
    if !sets_lib {
        out.push_str("// @noLib: true\n");
    }
    for (name, value) in &pin.options {
        let _ = writeln!(out, "// @{name}: {value}");
    }
    let relation = match pin.relation {
        RelpinRelation::Assignable => "assignable",
        RelpinRelation::Comparable => "comparable",
    };
    let _ = writeln!(
        out,
        "\n// relpin {}: {relation} source={:?} target={:?}",
        pin.id, pin.source, pin.target
    );
    if let Some(setup) = &pin.setup {
        out.push_str(setup);
        if !setup.ends_with('\n') {
            out.push('\n');
        }
    }
    match (pin.relation, &pin.expr) {
        (RelpinRelation::Assignable, None) => {
            let _ = writeln!(
                out,
                "declare var s: {};\nvar t: {} = s;",
                pin.source, pin.target
            );
        }
        (RelpinRelation::Assignable, Some(expr)) => {
            let _ = writeln!(out, "var t: {} = {expr};", pin.target);
        }
        (RelpinRelation::Comparable, None) => {
            let _ = writeln!(
                out,
                "declare var s: {};\nvar t = s as {};",
                pin.source, pin.target
            );
        }
        (RelpinRelation::Comparable, Some(expr)) => {
            let _ = writeln!(out, "var t = ({expr}) as {};", pin.target);
        }
    }
    out
}

fn fixtures_dir() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../pins/fixtures/relations"
    ))
}

#[test]
fn pin_fixtures_are_the_recorded_oracle_fixtures() {
    let pins = pins();
    assert_eq!(pins.len(), 415, "pin count");
    let recorded = std::fs::read_dir(fixtures_dir())
        .expect("recorded pin fixtures")
        .count();
    assert_eq!(recorded, pins.len(), "one recorded fixture per pin");
    for pin in &pins {
        let path = fixtures_dir().join(format!("{}.ts", pin.id));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(fixture_text(pin), text, "{}: fixture text", pin.id);
    }
}

#[test]
fn relation_pins_match_typescript() {
    let vendor_lib_dir = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-6.0.3/lib"
    ));
    let mut failures = Vec::new();
    for pin in pins() {
        let name = format!("{}.ts", pin.id);
        let mut programs =
            tsc_harness::expand_fixture_text(&name, &fixture_text(&pin), vendor_lib_dir)
                .unwrap_or_else(|error| panic!("{}: fixture expansion failed: {error}", pin.id));
        assert_eq!(
            programs.len(),
            1,
            "{}: pin options must not form a matrix",
            pin.id
        );
        let options = tsc_harness::compiler_options_from_program(&programs.remove(0));
        let verdict = probe_relation(&RelpinQuery {
            setup: pin.setup.as_deref().unwrap_or(""),
            source: &pin.source,
            target: &pin.target,
            source_is_fresh: pin.expr.is_some(),
            relation: pin.relation,
            options: &options,
        });
        let answer = match verdict {
            RelpinVerdict::Related => true,
            RelpinVerdict::NotRelated => false,
            RelpinVerdict::Unavailable { reason } => {
                failures.push(format!("{}: unavailable: {reason}", pin.id));
                continue;
            }
        };
        if answer != pin.related {
            failures.push(format!(
                "{}: {:?} -> {:?} ({:?}): TypeScript {}, tsc-rs {}",
                pin.id, pin.source, pin.target, pin.relation, pin.related, answer
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} relation pins disagree:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
