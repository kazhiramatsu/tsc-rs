//! Relation pin harness (greenfield M3 stage 4.0).
//!
//! `cargo xtask relpin gen [pins/relations.toml]` regenerates the
//! fixture files next to the pin file and fills every pin's `expect`
//! value by probing the vendored tsc oracle. Classification rule
//! (m3-types-relations-steps.md stage 4.0): ANY semantic diagnostic on
//! the fixture = not related — NOT "2322 present". Assignment failures
//! root as 2322 but ALSO as 2353 (excess property), 2739/2740 (missing
//! properties), 2559/2560 (weak type); the fixture contains no other
//! error source, so presence-of-any-error is the correct oracle.
//! Comparable pins use `s as Target` fixtures the same way (2352
//! family). Syntactic diagnostics mean the pin itself is malformed and
//! abort the run.
//!
//! `cargo xtask relpin run [pins/relations.toml]` asks the engine
//! (tsrs2_checker::relpin::probe_relation) the same question and
//! prints disagreements. Unsupported answers count as failures so the
//! M3 gate ("0 disagreements") cannot pass with a stubbed engine.
//!
//! Pin file format — a deliberately small TOML subset (array-of-tables
//! `[[pair]]`, single-line basic/literal strings, booleans, integers,
//! one-level inline tables) parsed by hand like the rest of this
//! workspace's fixed-format inputs:
//!
//! ```toml
//! [[pair]]
//! source = "{ a: number }"          # type annotation (engine side)
//! target = "{ a?: number }"
//! relation = "comparable"           # optional; default "assignable"
//! options = { strictNullChecks = false }  # optional; emitted as
//!                                   #   `// @option:` directives
//! setup = "interface A { next: B }\ninterface B { next: A }"  # optional
//! expr = "{ a: 1 }"                 # optional; fixture assigns this
//!                                   #   expression (fresh source)
//! expect = "yes"                    # filled by gen, committed
//! ```

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use tsrs2_checker::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};

use crate::find_tsrs2_root;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Relation {
    Assignable,
    Comparable,
}

impl Relation {
    fn as_str(self) -> &'static str {
        match self {
            Relation::Assignable => "assignable",
            Relation::Comparable => "comparable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expect {
    Yes,
    No,
}

impl Expect {
    fn as_str(self) -> &'static str {
        match self {
            Expect::Yes => "yes",
            Expect::No => "no",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TomlValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Table(Vec<(String, TomlValue)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Pin {
    /// 1-based position in the pin file; names the fixture (p001.ts).
    index: usize,
    source: String,
    target: String,
    relation: Relation,
    options: Vec<(String, TomlValue)>,
    setup: Option<String>,
    expr: Option<String>,
    expect: Option<Expect>,
    /// 0-based line bookkeeping for the in-place expect rewrite.
    last_key_line: usize,
    expect_line: Option<usize>,
}

impl Pin {
    fn id(&self) -> String {
        format!("p{:03}", self.index)
    }
}

pub fn gen(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let pins_path = parse_pins_path_arg("relpin gen", args)?;
    let workspace = find_tsrs2_root()?;
    let pins_path = resolve_pins_path(&workspace, pins_path);
    let text = fs::read_to_string(&pins_path)
        .map_err(|err| format!("failed to read {}: {err}", pins_path.display()))?;
    let pins = parse_pins(&text)?;
    if pins.is_empty() {
        return Err(format!("no [[pair]] entries in {}", pins_path.display()).into());
    }

    let fixtures_dir = fixtures_dir_for(&pins_path)?;
    if fixtures_dir.exists() {
        fs::remove_dir_all(&fixtures_dir)?;
    }
    fs::create_dir_all(&fixtures_dir)?;

    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let scratch = std::env::temp_dir().join(format!("tsrs2-relpin-{}", std::process::id()));
    if scratch.exists() {
        fs::remove_dir_all(&scratch)?;
    }
    let pool = tsrs2_oracle::OraclePool::new(1)?;

    let mut expects = Vec::with_capacity(pins.len());
    let mut yes_count = 0usize;
    let mut no_count = 0usize;
    for pin in &pins {
        let fixture_name = format!("{}.ts", pin.id());
        let fixture = fixture_text(pin);
        fs::write(fixtures_dir.join(&fixture_name), &fixture)?;

        let program_json = expand_pin_fixture(pin, &fixture_name, &fixture, &vendor_lib_dir)?;
        let out_dir = scratch.join(pin.id());
        let paths =
            tsrs2_harness::write_program_jsons(std::slice::from_ref(&program_json), &out_dir)?;
        let diagnostics = pool.diagnostics(&paths[0])?;

        let syntactic: Vec<u32> = diagnostics
            .iter()
            .filter(|diag| diag.pass.as_deref() == Some("syntactic"))
            .map(|diag| diag.code)
            .collect();
        if !syntactic.is_empty() {
            return Err(format!(
                "{}: fixture has syntactic diagnostics {:?} — the pin is malformed:\n{fixture}",
                pin.id(),
                syntactic
            )
            .into());
        }

        let semantic: Vec<u32> = diagnostics
            .iter()
            .filter(|diag| diag.pass.as_deref() == Some("semantic"))
            .map(|diag| diag.code)
            .collect();
        let expect = if semantic.is_empty() {
            yes_count += 1;
            Expect::Yes
        } else {
            no_count += 1;
            Expect::No
        };
        match expect {
            Expect::Yes => println!(
                "{} {} {:?} -> {:?}: yes",
                pin.id(),
                pin.relation.as_str(),
                pin.source,
                pin.target
            ),
            Expect::No => println!(
                "{} {} {:?} -> {:?}: no {:?}",
                pin.id(),
                pin.relation.as_str(),
                pin.source,
                pin.target,
                semantic
            ),
        }
        expects.push(expect);
    }

    let rewritten = rewrite_expects(&text, &pins, &expects);
    fs::write(&pins_path, rewritten)?;
    fs::remove_dir_all(&scratch)?;

    println!(
        "relpin gen: pins={} yes={yes_count} no={no_count} -> {}",
        pins.len(),
        pins_path.display()
    );
    Ok(())
}

pub fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let pins_path = parse_pins_path_arg("relpin run", args)?;
    let workspace = find_tsrs2_root()?;
    let pins_path = resolve_pins_path(&workspace, pins_path);
    let text = fs::read_to_string(&pins_path)
        .map_err(|err| format!("failed to read {}: {err}", pins_path.display()))?;
    let pins = parse_pins(&text)?;
    if pins.is_empty() {
        return Err(format!("no [[pair]] entries in {}", pins_path.display()).into());
    }

    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut agree = 0usize;
    let mut disagreements = Vec::new();
    let mut unsupported: Vec<(String, String)> = Vec::new();
    for pin in &pins {
        let Some(expect) = pin.expect else {
            return Err(format!(
                "{}: missing expect value — run `cargo xtask relpin gen` first",
                pin.id()
            )
            .into());
        };

        // Expand the exact fixture the oracle saw so both sides read
        // the pin's options through the same harness path.
        let fixture_name = format!("{}.ts", pin.id());
        let fixture = fixture_text(pin);
        let program_json = expand_pin_fixture(pin, &fixture_name, &fixture, &vendor_lib_dir)?;
        let options = tsrs2_conformance::compiler_options_from_program(&program_json);

        let query = RelpinQuery {
            setup: pin.setup.as_deref().unwrap_or(""),
            source: &pin.source,
            target: &pin.target,
            source_is_fresh: pin.expr.is_some(),
            relation: match pin.relation {
                Relation::Assignable => RelpinRelation::Assignable,
                Relation::Comparable => RelpinRelation::Comparable,
            },
            options: &options,
        };
        match probe_relation(&query) {
            RelpinVerdict::Unsupported { reason } => unsupported.push((pin.id(), reason)),
            verdict => {
                let engine = match verdict {
                    RelpinVerdict::Related => Expect::Yes,
                    RelpinVerdict::NotRelated => Expect::No,
                    RelpinVerdict::Unsupported { .. } => unreachable!(),
                };
                if engine == expect {
                    agree += 1;
                } else {
                    disagreements.push(format!(
                        "disagree {} {} {:?} -> {:?}: expect={} engine={}",
                        pin.id(),
                        pin.relation.as_str(),
                        pin.source,
                        pin.target,
                        expect.as_str(),
                        engine.as_str()
                    ));
                }
            }
        }
    }

    for line in disagreements.iter().take(50) {
        println!("{line}");
    }
    if disagreements.len() > 50 {
        println!("(+{} more disagreements)", disagreements.len() - 50);
    }
    if !unsupported.is_empty() {
        let mut by_reason: Vec<(String, usize)> = Vec::new();
        for (_, reason) in &unsupported {
            match by_reason.iter_mut().find(|(known, _)| known == reason) {
                Some((_, count)) => *count += 1,
                None => by_reason.push((reason.clone(), 1)),
            }
        }
        for (reason, count) in &by_reason {
            println!("unsupported: {count} pins — {reason}");
        }
    }

    println!(
        "relpin run: pins={} agree={agree} disagree={} unsupported={}",
        pins.len(),
        disagreements.len(),
        unsupported.len()
    );
    if !disagreements.is_empty() || !unsupported.is_empty() {
        return Err(format!(
            "relpin run failed: {} disagreements, {} unsupported",
            disagreements.len(),
            unsupported.len()
        )
        .into());
    }
    Ok(())
}

fn parse_pins_path_arg(
    command: &str,
    args: impl Iterator<Item = String>,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut path = None;
    for arg in args {
        if path.is_none() && !arg.starts_with('-') {
            path = Some(PathBuf::from(arg));
        } else {
            return Err(format!("unexpected {command} argument: {arg}").into());
        }
    }
    Ok(path)
}

fn resolve_pins_path(workspace: &Path, path: Option<PathBuf>) -> PathBuf {
    match path {
        Some(path) if path.is_absolute() || path.exists() => path,
        Some(path) => workspace.join(path),
        None => workspace.join("pins/relations.toml"),
    }
}

/// pins/relations.toml -> pins/fixtures/relations/
fn fixtures_dir_for(pins_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let stem = pins_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("pin file has no UTF-8 stem: {}", pins_path.display()))?;
    let dir = pins_path
        .parent()
        .ok_or_else(|| format!("pin file has no parent: {}", pins_path.display()))?;
    Ok(dir.join("fixtures").join(stem))
}

fn expand_pin_fixture(
    pin: &Pin,
    fixture_name: &str,
    fixture: &str,
    vendor_lib_dir: &Path,
) -> Result<tsrs2_harness::ProgramJson, Box<dyn Error>> {
    let mut programs = tsrs2_harness::expand_fixture_text(fixture_name, fixture, vendor_lib_dir)
        .map_err(|err| format!("{}: fixture expansion failed: {err}", pin.id()))?;
    if programs.len() != 1 {
        return Err(format!(
            "{}: fixture expanded to {} programs — pin options must not be matrix-valued",
            pin.id(),
            programs.len()
        )
        .into());
    }
    Ok(programs.remove(0))
}

/// The oracle fixture (and the engine probe's scratch program). The
/// assignable shape is `declare var s: Source; var t: Target = s;`;
/// `expr` pins assign the expression directly so the source stays a
/// FRESH literal (excess-property / weak-type pins need it).
/// Comparable pins probe via `s as Target` (2352 family presence).
fn fixture_text(pin: &Pin) -> String {
    let mut out = String::new();
    let has_lib_option = pin
        .options
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("nolib") || key.eq_ignore_ascii_case("lib"));
    if !has_lib_option {
        out.push_str("// @noLib: true\n");
    }
    for (key, value) in &pin.options {
        let _ = writeln!(out, "// @{key}: {}", directive_value(value));
    }
    out.push('\n');
    let _ = writeln!(
        out,
        "// relpin {}: {} source={:?} target={:?}",
        pin.id(),
        pin.relation.as_str(),
        pin.source,
        pin.target
    );
    if let Some(setup) = &pin.setup {
        out.push_str(setup);
        if !setup.ends_with('\n') {
            out.push('\n');
        }
    }
    match (pin.relation, &pin.expr) {
        (Relation::Assignable, None) => {
            let _ = writeln!(out, "declare var s: {};", pin.source);
            let _ = writeln!(out, "var t: {} = s;", pin.target);
        }
        (Relation::Assignable, Some(expr)) => {
            let _ = writeln!(out, "var t: {} = {expr};", pin.target);
        }
        (Relation::Comparable, None) => {
            let _ = writeln!(out, "declare var s: {};", pin.source);
            let _ = writeln!(out, "var t = s as {};", pin.target);
        }
        (Relation::Comparable, Some(expr)) => {
            let _ = writeln!(out, "var t = ({expr}) as {};", pin.target);
        }
    }
    out
}

fn directive_value(value: &TomlValue) -> String {
    match value {
        TomlValue::Str(value) => value.clone(),
        TomlValue::Bool(value) => value.to_string(),
        TomlValue::Int(value) => value.to_string(),
        TomlValue::Table(_) => unreachable!("option values are validated scalar"),
    }
}

/// Replace (or append) each pair's `expect = ".."` line in place,
/// leaving comments and hand formatting untouched.
fn rewrite_expects(text: &str, pins: &[Pin], expects: &[Expect]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    // Bottom-up so insertions don't shift earlier line numbers.
    for (pin, expect) in pins.iter().zip(expects).rev() {
        let line = format!("expect = \"{}\"", expect.as_str());
        match pin.expect_line {
            Some(index) => lines[index] = line,
            None => lines.insert(pin.last_key_line + 1, line),
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn parse_pins(text: &str) -> Result<Vec<Pin>, Box<dyn Error>> {
    struct Builder {
        header_line: usize,
        last_key_line: usize,
        expect_line: Option<usize>,
        source: Option<String>,
        target: Option<String>,
        relation: Option<Relation>,
        options: Option<Vec<(String, TomlValue)>>,
        setup: Option<String>,
        expr: Option<String>,
        expect: Option<Expect>,
    }

    fn finish(builder: Builder, index: usize) -> Result<Pin, Box<dyn Error>> {
        let line = builder.header_line + 1;
        let source = builder
            .source
            .ok_or_else(|| format!("[[pair]] at line {line}: missing source"))?;
        let target = builder
            .target
            .ok_or_else(|| format!("[[pair]] at line {line}: missing target"))?;
        Ok(Pin {
            index,
            source,
            target,
            relation: builder.relation.unwrap_or(Relation::Assignable),
            options: builder.options.unwrap_or_default(),
            setup: builder.setup,
            expr: builder.expr,
            expect: builder.expect,
            last_key_line: builder.last_key_line,
            expect_line: builder.expect_line,
        })
    }

    let mut pins = Vec::new();
    let mut current: Option<Builder> = None;
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[pair]]" {
            if let Some(builder) = current.take() {
                pins.push(finish(builder, pins.len() + 1)?);
            }
            current = Some(Builder {
                header_line: index,
                last_key_line: index,
                expect_line: None,
                source: None,
                target: None,
                relation: None,
                options: None,
                setup: None,
                expr: None,
                expect: None,
            });
            continue;
        }
        if line.starts_with('[') {
            return Err(format!("line {}: only [[pair]] tables are supported", index + 1).into());
        }

        let builder = current
            .as_mut()
            .ok_or_else(|| format!("line {}: key outside [[pair]]", index + 1))?;
        let equals = line
            .find('=')
            .ok_or_else(|| format!("line {}: expected key = value", index + 1))?;
        let key = line[..equals].trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(format!("line {}: invalid key {key:?}", index + 1).into());
        }
        let (value, rest) = parse_value(line[equals + 1..].trim_start())
            .map_err(|err| format!("line {}: {err}", index + 1))?;
        let rest = rest.trim_start();
        if !rest.is_empty() && !rest.starts_with('#') {
            return Err(
                format!("line {}: trailing content after value: {rest:?}", index + 1).into(),
            );
        }

        let single_line_string = |value: &TomlValue, key: &str| -> Result<String, String> {
            match value {
                TomlValue::Str(text) if !text.contains('\n') && !text.contains('\r') => {
                    Ok(text.clone())
                }
                TomlValue::Str(_) => Err(format!("{key} must be a single-line string")),
                _ => Err(format!("{key} must be a string")),
            }
        };
        let occupied = |name: &str| format!("line {}: duplicate key {name}", index + 1);
        match key {
            "source" => {
                if builder.source.is_some() {
                    return Err(occupied("source").into());
                }
                builder.source = Some(single_line_string(&value, "source")?);
            }
            "target" => {
                if builder.target.is_some() {
                    return Err(occupied("target").into());
                }
                builder.target = Some(single_line_string(&value, "target")?);
            }
            "expr" => {
                if builder.expr.is_some() {
                    return Err(occupied("expr").into());
                }
                builder.expr = Some(single_line_string(&value, "expr")?);
            }
            "setup" => {
                if builder.setup.is_some() {
                    return Err(occupied("setup").into());
                }
                match value {
                    TomlValue::Str(text) => builder.setup = Some(text),
                    _ => return Err(format!("line {}: setup must be a string", index + 1).into()),
                }
            }
            "relation" => {
                if builder.relation.is_some() {
                    return Err(occupied("relation").into());
                }
                builder.relation = Some(match single_line_string(&value, "relation")?.as_str() {
                    "assignable" => Relation::Assignable,
                    "comparable" => Relation::Comparable,
                    other => return Err(format!(
                        "line {}: relation must be \"assignable\" or \"comparable\", got {other:?}",
                        index + 1
                    )
                    .into()),
                });
            }
            "expect" => {
                if builder.expect.is_some() {
                    return Err(occupied("expect").into());
                }
                builder.expect = Some(match single_line_string(&value, "expect")?.as_str() {
                    "yes" => Expect::Yes,
                    "no" => Expect::No,
                    other => {
                        return Err(format!(
                            "line {}: expect must be \"yes\" or \"no\", got {other:?}",
                            index + 1
                        )
                        .into())
                    }
                });
                builder.expect_line = Some(index);
            }
            "options" => {
                if builder.options.is_some() {
                    return Err(occupied("options").into());
                }
                let TomlValue::Table(entries) = value else {
                    return Err(
                        format!("line {}: options must be an inline table", index + 1).into(),
                    );
                };
                for (name, entry) in &entries {
                    match entry {
                        TomlValue::Str(text) => {
                            if text.contains(',') || text.contains('*') {
                                return Err(format!(
                                    "line {}: option {name} would expand to a fixture matrix: {text:?}",
                                    index + 1
                                )
                                .into());
                            }
                        }
                        TomlValue::Bool(_) | TomlValue::Int(_) => {}
                        TomlValue::Table(_) => {
                            return Err(format!(
                                "line {}: option {name} must be a scalar",
                                index + 1
                            )
                            .into())
                        }
                    }
                }
                builder.options = Some(entries);
            }
            other => {
                return Err(format!("line {}: unknown pin key {other:?}", index + 1).into());
            }
        }
        builder.last_key_line = index;
    }
    if let Some(builder) = current.take() {
        pins.push(finish(builder, pins.len() + 1)?);
    }
    Ok(pins)
}

/// Parse one TOML value from the start of `text`; returns the value
/// and the unconsumed remainder of the line.
fn parse_value(text: &str) -> Result<(TomlValue, &str), String> {
    let mut chars = text.char_indices();
    match chars.next() {
        Some((_, '"')) => {
            let mut value = String::new();
            let mut iter = text[1..].char_indices();
            while let Some((offset, ch)) = iter.next() {
                match ch {
                    '"' => return Ok((TomlValue::Str(value), &text[1 + offset + 1..])),
                    '\\' => {
                        let (_, escape) = iter
                            .next()
                            .ok_or_else(|| "unterminated escape in string".to_owned())?;
                        match escape {
                            'n' => value.push('\n'),
                            't' => value.push('\t'),
                            'r' => value.push('\r'),
                            '"' => value.push('"'),
                            '\\' => value.push('\\'),
                            'u' => {
                                let mut code = 0u32;
                                for _ in 0..4 {
                                    let (_, digit) = iter
                                        .next()
                                        .ok_or_else(|| "truncated \\u escape".to_owned())?;
                                    code = code * 16
                                        + digit
                                            .to_digit(16)
                                            .ok_or_else(|| "invalid \\u escape".to_owned())?;
                                }
                                value.push(
                                    char::from_u32(code)
                                        .ok_or_else(|| "invalid \\u code point".to_owned())?,
                                );
                            }
                            other => return Err(format!("unsupported escape \\{other}")),
                        }
                    }
                    ch => value.push(ch),
                }
            }
            Err("unterminated basic string".to_owned())
        }
        Some((_, '\'')) => {
            let rest = &text[1..];
            let end = rest
                .find('\'')
                .ok_or_else(|| "unterminated literal string".to_owned())?;
            Ok((TomlValue::Str(rest[..end].to_owned()), &rest[end + 1..]))
        }
        Some((_, '{')) => {
            let mut entries = Vec::new();
            let mut rest = text[1..].trim_start();
            if let Some(after) = rest.strip_prefix('}') {
                return Ok((TomlValue::Table(entries), after));
            }
            loop {
                let equals = rest
                    .find('=')
                    .ok_or_else(|| "inline table entry missing =".to_owned())?;
                let key = rest[..equals].trim();
                if key.is_empty()
                    || !key
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                {
                    return Err(format!("invalid inline table key {key:?}"));
                }
                let (value, after_value) = parse_value(rest[equals + 1..].trim_start())?;
                if matches!(value, TomlValue::Table(_)) {
                    return Err("nested inline tables are not supported".to_owned());
                }
                entries.push((key.to_owned(), value));
                rest = after_value.trim_start();
                if let Some(after) = rest.strip_prefix(',') {
                    rest = after.trim_start();
                    continue;
                }
                if let Some(after) = rest.strip_prefix('}') {
                    return Ok((TomlValue::Table(entries), after));
                }
                return Err(format!("expected , or }} in inline table, got {rest:?}"));
            }
        }
        Some((_, 't')) if text.starts_with("true") => {
            Ok((TomlValue::Bool(true), &text["true".len()..]))
        }
        Some((_, 'f')) if text.starts_with("false") => {
            Ok((TomlValue::Bool(false), &text["false".len()..]))
        }
        Some((_, ch)) if ch == '-' || ch.is_ascii_digit() => {
            let end = text
                .char_indices()
                .skip(1)
                .find(|(_, ch)| !ch.is_ascii_digit())
                .map(|(index, _)| index)
                .unwrap_or(text.len());
            let value = text[..end]
                .parse::<i64>()
                .map_err(|err| format!("invalid integer {:?}: {err}", &text[..end]))?;
            Ok((TomlValue::Int(value), &text[end..]))
        }
        _ => Err(format!("unsupported value syntax: {text:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_pin_with_defaults() {
        let pins = parse_pins("[[pair]]\nsource = \"1\"\ntarget = \"number\"\n").expect("parses");
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].source, "1");
        assert_eq!(pins[0].target, "number");
        assert_eq!(pins[0].relation, Relation::Assignable);
        assert_eq!(pins[0].expect, None);
        assert!(pins[0].options.is_empty());
    }

    #[test]
    fn parses_full_pin() {
        let text = concat!(
            "# section comment\n",
            "[[pair]]\n",
            "source = '\"a\"'      # literal string keeps quotes\n",
            "target = \"string\"\n",
            "relation = \"comparable\"\n",
            "options = { strictNullChecks = false, target = \"es5\" }\n",
            "setup = \"interface A { next: B }\\ninterface B { next: A }\"\n",
            "expr = \"{ a: 1 }\"\n",
            "expect = \"yes\"\n",
        );
        let pins = parse_pins(text).expect("parses");
        assert_eq!(pins[0].source, "\"a\"");
        assert_eq!(pins[0].relation, Relation::Comparable);
        assert_eq!(
            pins[0].options,
            vec![
                ("strictNullChecks".to_owned(), TomlValue::Bool(false)),
                ("target".to_owned(), TomlValue::Str("es5".to_owned())),
            ]
        );
        assert_eq!(
            pins[0].setup.as_deref(),
            Some("interface A { next: B }\ninterface B { next: A }")
        );
        assert_eq!(pins[0].expr.as_deref(), Some("{ a: 1 }"));
        assert_eq!(pins[0].expect, Some(Expect::Yes));
        assert_eq!(pins[0].expect_line, Some(8));
    }

    #[test]
    fn rejects_malformed_pins() {
        for (text, needle) in [
            ("source = \"1\"\n", "key outside"),
            ("[[pair]]\ntarget = \"number\"\n", "missing source"),
            ("[[pair]]\nsource = \"1\"\n", "missing target"),
            (
                "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nbogus = \"x\"\n",
                "unknown pin key",
            ),
            (
                "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nrelation = \"identity\"\n",
                "relation must be",
            ),
            (
                "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nexpect = \"maybe\"\n",
                "expect must be",
            ),
            (
                "[[pair]]\nsource = \"1\"\ntarget = \"n\"\noptions = { target = \"es5, es2015\" }\n",
                "fixture matrix",
            ),
            (
                "[[pair]]\nsource = \"a\\nb\"\ntarget = \"n\"\n",
                "single-line",
            ),
            (
                "[[pair]]\nsource = \"1\"\nsource = \"2\"\ntarget = \"n\"\n",
                "duplicate key",
            ),
            ("[table]\n", "only [[pair]]"),
        ] {
            let err = parse_pins(text).expect_err(text);
            assert!(
                err.to_string().contains(needle),
                "{text:?}: {err} should contain {needle:?}"
            );
        }
    }

    fn pin(relation: Relation, expr: Option<&str>) -> Pin {
        Pin {
            index: 7,
            source: "{ a: number }".to_owned(),
            target: "{ a?: number }".to_owned(),
            relation,
            options: vec![("strictNullChecks".to_owned(), TomlValue::Bool(false))],
            setup: None,
            expr: expr.map(str::to_owned),
            expect: None,
            last_key_line: 0,
            expect_line: None,
        }
    }

    #[test]
    fn fixture_text_assignable_snapshot() {
        assert_eq!(
            fixture_text(&pin(Relation::Assignable, None)),
            "// @noLib: true\n\
             // @strictNullChecks: false\n\
             \n\
             // relpin p007: assignable source=\"{ a: number }\" target=\"{ a?: number }\"\n\
             declare var s: { a: number };\n\
             var t: { a?: number } = s;\n"
        );
    }

    #[test]
    fn fixture_text_expr_and_comparable_snapshots() {
        assert_eq!(
            fixture_text(&pin(Relation::Assignable, Some("{ a: 1 }")))
                .lines()
                .last(),
            Some("var t: { a?: number } = { a: 1 };")
        );
        assert_eq!(
            fixture_text(&pin(Relation::Comparable, None))
                .lines()
                .last(),
            Some("var t = s as { a?: number };")
        );
        assert_eq!(
            fixture_text(&pin(Relation::Comparable, Some("{ a: 1 }")))
                .lines()
                .last(),
            Some("var t = ({ a: 1 }) as { a?: number };")
        );
    }

    #[test]
    fn fixture_text_includes_setup_and_skips_nolib_when_overridden() {
        let mut with_setup = pin(Relation::Assignable, None);
        with_setup.setup = Some("interface A { next: B }\ninterface B { next: A }".to_owned());
        let text = fixture_text(&with_setup);
        assert!(text.contains("interface A { next: B }\ninterface B { next: A }\ndeclare var s:"));

        let mut no_lib_false = pin(Relation::Assignable, None);
        no_lib_false.options = vec![("noLib".to_owned(), TomlValue::Bool(false))];
        let text = fixture_text(&no_lib_false);
        assert_eq!(text.matches("noLib").count(), 1);
        assert!(text.starts_with("// @noLib: false\n"));
    }

    #[test]
    fn rewrite_inserts_and_replaces_expects() {
        let text = concat!(
            "# header comment\n",
            "[[pair]]\n",
            "source = \"1\"\n",
            "target = \"number\"\n",
            "\n",
            "# next section\n",
            "[[pair]]\n",
            "source = \"2\"\n",
            "target = \"string\"\n",
            "expect = \"yes\"\n",
        );
        let pins = parse_pins(text).expect("parses");
        let rewritten = rewrite_expects(text, &pins, &[Expect::Yes, Expect::No]);
        assert_eq!(
            rewritten,
            concat!(
                "# header comment\n",
                "[[pair]]\n",
                "source = \"1\"\n",
                "target = \"number\"\n",
                "expect = \"yes\"\n",
                "\n",
                "# next section\n",
                "[[pair]]\n",
                "source = \"2\"\n",
                "target = \"string\"\n",
                "expect = \"no\"\n",
            )
        );
        // Regenerating is idempotent: parse the rewritten file and
        // rewrite with the same verdicts.
        let pins = parse_pins(&rewritten).expect("reparses");
        assert_eq!(
            rewrite_expects(&rewritten, &pins, &[Expect::Yes, Expect::No]),
            rewritten
        );
    }
}
