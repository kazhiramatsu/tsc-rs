#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        None | Some("scaffold-smoke") => scaffold_smoke(),
        Some("expand") => run_or_exit(expand_fixture(args)),
        Some("codegen") => match args.next().as_deref() {
            Some("diags") => run_or_exit(codegen_diags(false)),
            Some("diags-check") => run_or_exit(codegen_diags(true)),
            Some("nodes") => run_or_exit(codegen_nodes(false)),
            Some("nodes-check") => run_or_exit(codegen_nodes(true)),
            Some("enums") => run_or_exit(codegen_enums(false)),
            Some("enums-check") => run_or_exit(codegen_enums(true)),
            Some(other) => {
                eprintln!("unknown codegen target: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing codegen target");
                std::process::exit(2);
            }
        },
        Some(other) => {
            eprintln!("unknown xtask command: {other}");
            std::process::exit(2);
        }
    }
}

fn expand_fixture(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut fixture = None;
    let mut out_dir = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("missing value after --out-dir")?;
                out_dir = Some(PathBuf::from(value));
            }
            _ if fixture.is_none() => fixture = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected expand argument: {arg}").into()),
        }
    }

    let fixture = fixture.ok_or("missing fixture path for expand")?;
    let out_dir = out_dir.ok_or("missing --out-dir for expand")?;
    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
    let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;

    for path in paths {
        println!("{}", path.display());
    }

    Ok(())
}

fn run_or_exit(result: Result<(), Box<dyn Error>>) {
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn scaffold_smoke() {
    let harness_diags = tsrs2_harness::check_empty_program().diagnostics.len();
    let conformance_diags = tsrs2_conformance::run_empty_engine_smoke();

    if harness_diags != 0 || conformance_diags != 0 {
        eprintln!("empty-engine scaffold emitted diagnostics");
        std::process::exit(1);
    }

    println!("tsrs2 scaffold ready");
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumMember {
    name: String,
    value: EnumValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EnumValue {
    Int(i32),
    Str(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumTable {
    name: String,
    members: Vec<EnumMember>,
}

#[derive(Clone, Copy)]
struct SourceEnum {
    name: &'static str,
    file: &'static str,
}

const RUNTIME_ENUMS: &[&str] = &[
    "SyntaxKind",
    "NodeFlags",
    "ModifierFlags",
    "RelationComparisonResult",
    "FlowFlags",
    "SymbolFlags",
    "TypeFlags",
    "ObjectFlags",
    "SignatureFlags",
    "DiagnosticCategory",
    "ModuleKind",
    "TypeFacts",
    "CheckMode",
];

const CONST_ENUMS: &[SourceEnum] = &[
    SourceEnum {
        name: "TokenFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "UnionReduction",
        file: "types.ts",
    },
    SourceEnum {
        name: "ContextFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "CheckFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "InternalSymbolName",
        file: "types.ts",
    },
    SourceEnum {
        name: "ElementFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "AccessFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "TypeMapKind",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferencePriority",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferenceFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "Ternary",
        file: "types.ts",
    },
    SourceEnum {
        name: "ScriptTarget",
        file: "types.ts",
    },
    SourceEnum {
        name: "CharacterCodes",
        file: "types.ts",
    },
    SourceEnum {
        name: "IntersectionState",
        file: "checker.ts",
    },
    SourceEnum {
        name: "RecursionFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ExpandingFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ParsingContext",
        file: "parser.ts",
    },
];

fn codegen_enums(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc_path = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let tsc = fs::read_to_string(&tsc_path)?;

    let mut runtime_tables = BTreeMap::new();
    for name in RUNTIME_ENUMS {
        let table = parse_runtime_enum(&tsc, name)?;
        runtime_tables.insert((*name).to_owned(), table);
    }

    let mut source_tables = BTreeMap::new();
    for source in CONST_ENUMS {
        let path = compiler_source_path(&workspace, source.file)?;
        let text = fs::read_to_string(path)?;
        let table = parse_source_enum(&text, source.name)?;
        source_tables.insert(source.name.to_owned(), table);
    }

    let syntax = runtime_tables
        .remove("SyntaxKind")
        .ok_or("missing generated SyntaxKind")?;
    let kind_rs = rustfmt_text(&render_syntax_kind(&syntax)?)?;

    let mut flags_tables: Vec<EnumTable> = runtime_tables.into_values().collect();
    flags_tables.extend(source_tables.into_values());
    flags_tables.sort_by(|a, b| a.name.cmp(&b.name));
    let flags_rs = rustfmt_text(&render_flags(&flags_tables)?)?;

    let kind_path = workspace.join("crates/syntax/src/kind.rs");
    let flags_path = workspace.join("crates/types/src/flags.rs");
    write_generated(&kind_path, &kind_rs, check)?;
    write_generated(&flags_path, &flags_rs, check)?;

    if check {
        println!("generated enum files are up to date");
    } else {
        println!("generated enum files");
    }

    Ok(())
}

fn find_tsrs2_root() -> Result<PathBuf, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(dir.to_owned());
        }

        let nested = dir.join("tsrs2");
        if nested.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(nested);
        }
    }

    Err("could not find tsrs2 workspace root".into())
}

fn compiler_source_path(workspace: &Path, file: &str) -> Result<PathBuf, Box<dyn Error>> {
    let vendored = workspace
        .join("vendor/typescript-6.0.3/src/compiler")
        .join(file);
    if vendored.is_file() {
        return Ok(vendored);
    }

    let checkout = workspace
        .parent()
        .ok_or("tsrs2 workspace has no parent")?
        .join("ts-tests/src/compiler")
        .join(file);
    if checkout.is_file() {
        return Ok(checkout);
    }

    Err(format!("missing TypeScript compiler source file for const enum extraction: {file}").into())
}

fn write_generated(path: &Path, text: &str, check: bool) -> Result<(), Box<dyn Error>> {
    if check {
        let current = fs::read_to_string(path)?;
        if current != text {
            return Err(format!("{} is not up to date", path.display()).into());
        }
    } else {
        fs::write(path, text)?;
    }
    Ok(())
}

fn rustfmt_text(text: &str) -> Result<String, Box<dyn Error>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or("failed to open rustfmt stdin")?
        .write_all(text.as_bytes())?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn parse_runtime_enum(tsc: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let start_marker = format!("var {enum_name} = /* @__PURE__ */ ((");
    let start = tsc
        .find(&start_marker)
        .ok_or_else(|| format!("runtime enum {enum_name} not found in _tsc.js"))?;
    let after_start = &tsc[start..];
    let end = after_start
        .find(&format!("return {enum_name}"))
        .ok_or_else(|| format!("runtime enum {enum_name} has no return sentinel"))?;
    let block = &after_start[..end];

    let mut members = Vec::new();
    for line in block.lines() {
        if let Some(member) = parse_runtime_member(line)? {
            members.push(member);
        }
    }

    if members.is_empty() {
        return Err(format!("runtime enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn parse_runtime_member(line: &str) -> Result<Option<EnumMember>, Box<dyn Error>> {
    let Some(name_marker_start) = line.find("[\"") else {
        return Ok(None);
    };
    let name_start = name_marker_start + 2;
    let name_end = line[name_start..]
        .find("\"]")
        .map(|offset| name_start + offset)
        .ok_or_else(|| format!("malformed runtime enum line: {line}"))?;
    let name = &line[name_start..name_end];

    let after_name = &line[name_end + 2..];
    let equals = after_name
        .find('=')
        .ok_or_else(|| format!("runtime enum member has no value: {line}"))?;
    let value_text = after_name[equals + 1..].trim_start();
    let value_end = value_text
        .char_indices()
        .find_map(|(idx, ch)| {
            if (idx == 0 && ch == '-') || ch.is_ascii_digit() {
                None
            } else {
                Some(idx)
            }
        })
        .unwrap_or(value_text.len());
    let value: i32 = value_text[..value_end].parse()?;

    Ok(Some(EnumMember {
        name: name.to_owned(),
        value: EnumValue::Int(value),
    }))
}

fn parse_source_enum(source: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let block = source_enum_block(source, enum_name)?;
    let mut values = BTreeMap::<String, EnumValue>::new();
    let mut members = Vec::new();
    let mut next_auto_int = Some(0i32);
    let mut in_block_comment = false;

    for raw_line in block.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }

        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }

        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        let without_comment = strip_line_comment(line);
        let mut entry = without_comment.trim().trim_end_matches(',').trim();
        if entry.is_empty() {
            continue;
        }

        if entry.starts_with("export ") {
            continue;
        }

        let (name, value) = if let Some(eq) = entry.find('=') {
            let name = entry[..eq].trim();
            let expr = entry[eq + 1..].trim();
            let value = if is_string_literal(expr) {
                EnumValue::Str(unquote_string(expr)?)
            } else {
                EnumValue::Int(eval_int_expr(expr, &values)?)
            };
            (name, value)
        } else {
            let value = next_auto_int.ok_or_else(|| {
                format!("cannot auto-increment after string enum member: {entry}")
            })?;
            (entry, EnumValue::Int(value))
        };

        if name.is_empty() {
            return Err(format!("empty member name in enum {enum_name}").into());
        }

        entry = name;
        values.insert(entry.to_owned(), value.clone());
        next_auto_int = match value {
            EnumValue::Int(value) => Some(value + 1),
            EnumValue::Str(_) => None,
        };
        members.push(EnumMember {
            name: entry.to_owned(),
            value,
        });
    }

    if members.is_empty() {
        return Err(format!("source enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn source_enum_block<'a>(source: &'a str, enum_name: &str) -> Result<&'a str, Box<dyn Error>> {
    let needle = format!("enum {enum_name}");
    let enum_pos = source
        .find(&needle)
        .ok_or_else(|| format!("source enum {enum_name} not found"))?;
    let after_enum = &source[enum_pos..];
    let open_rel = after_enum
        .find('{')
        .ok_or_else(|| format!("source enum {enum_name} has no opening brace"))?;
    let open = enum_pos + open_rel;
    let mut depth = 0usize;
    let mut close = None;

    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }

    let close = close.ok_or_else(|| format!("source enum {enum_name} has no closing brace"))?;
    Ok(&source[open + 1..close])
}

fn strip_line_comment(line: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    let mut prev = '\0';

    for (idx, ch) in line.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
        } else if prev == '/' && ch == '/' {
            return line[..idx - 1].to_owned();
        }
        prev = ch;
    }

    line.to_owned()
}

fn is_string_literal(expr: &str) -> bool {
    expr.starts_with('"') && expr.ends_with('"')
}

fn unquote_string(expr: &str) -> Result<String, Box<dyn Error>> {
    if !is_string_literal(expr) {
        return Err(format!("not a string literal: {expr}").into());
    }

    Ok(expr[1..expr.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\"))
}

fn eval_int_expr(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    let mut result = 0i32;

    for part in expr.split('|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        result |= eval_shift_expr(part, values)?;
    }

    Ok(result)
}

fn eval_shift_expr(
    expr: &str,
    values: &BTreeMap<String, EnumValue>,
) -> Result<i32, Box<dyn Error>> {
    if let Some(shift) = expr.find("<<") {
        let left = eval_atom(&expr[..shift], values)?;
        let right = eval_atom(&expr[shift + 2..], values)?;
        return Ok(left << right);
    }

    eval_atom(expr, values)
}

fn eval_atom(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    if let Some(rest) = expr.strip_prefix('-') {
        return Ok(-eval_atom(rest, values)?);
    }

    if let Some(hex) = expr.strip_prefix("0x").or_else(|| expr.strip_prefix("0X")) {
        return Ok(i32::from_str_radix(hex, 16)?);
    }

    if expr.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(expr.parse()?);
    }

    match values.get(expr) {
        Some(EnumValue::Int(value)) => Ok(*value),
        Some(EnumValue::Str(_)) => {
            Err(format!("string enum member used as integer: {expr}").into())
        }
        None => Err(format!("unknown enum value expression: {expr}").into()),
    }
}

fn trim_wrapping_parens(mut expr: &str) -> &str {
    loop {
        let trimmed = expr.trim();
        if trimmed.starts_with(')') || !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }

        let mut depth = 0i32;
        let mut wraps = true;
        for (idx, ch) in trimmed.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != trimmed.len() - 1 {
                        wraps = false;
                        break;
                    }
                }
                _ => {}
            }
        }

        if wraps {
            expr = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn render_syntax_kind(table: &EnumTable) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "#![allow(non_upper_case_globals)]")?;
    writeln!(out)?;
    writeln!(out, "#[repr(u16)]")?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub enum SyntaxKind {{")?;

    let mut canonical = BTreeMap::<i32, String>::new();
    let mut aliases = Vec::<(&EnumMember, String)>::new();
    for member in &table.members {
        let value = member_int(member, &table.name)?;
        if let Some(existing) = canonical.get(&value) {
            aliases.push((member, existing.clone()));
            continue;
        }

        canonical.insert(value, member.name.clone());
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(out, "    {} = {},", member.name, value)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl SyntaxKind {{")?;
    for (member, target) in aliases {
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(
            out,
            "    pub const {}: Self = Self::{};",
            member.name, target
        )?;
    }
    writeln!(out)?;
    writeln!(out, "    pub const fn value(self) -> u16 {{")?;
    writeln!(out, "        self as u16")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn from_u16(value: u16) -> Option<Self> {{"
    )?;
    writeln!(out, "        match value {{")?;
    for (value, name) in &canonical {
        writeln!(out, "            {} => Some(Self::{}),", value, name)?;
    }
    writeln!(out, "            _ => None,")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::Identifier as u16, 80);"
    )?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::FirstAssignment.value(), SyntaxKind::EqualsToken as u16);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_flags(tables: &[EnumTable]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;

    for table in tables {
        if table
            .members
            .iter()
            .all(|member| matches!(member.value, EnumValue::Int(_)))
        {
            render_int_table(&mut out, table)?;
        } else {
            render_string_table(&mut out, table)?;
        }
        writeln!(out)?;
    }

    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(TypeFlags::STRING_LITERAL.bits(), 1024);"
    )?;
    writeln!(
        out,
        "        assert_eq!(FlowFlags::TRUE_CONDITION.bits(), 32);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_int_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct {}(i32);", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;

    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let value = member_int(member, &table.name)?;
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(out, "    pub const {}: Self = Self({});", const_name, value)?;
    }

    writeln!(out)?;
    writeln!(out, "    pub const fn from_bits(bits: i32) -> Self {{")?;
    writeln!(out, "        Self(bits)")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn bits(self) -> i32 {{")?;
    writeln!(out, "        self.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn is_empty(self) -> bool {{")?;
    writeln!(out, "        self.0 == 0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn contains(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) == other.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn intersects(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) != 0")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOr for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitor(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 | rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitAnd for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitand(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 & rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOrAssign for {} {{", table.name)?;
    writeln!(out, "    fn bitor_assign(&mut self, rhs: Self) {{")?;
    writeln!(out, "        self.0 |= rhs.0;")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(())
}

fn render_string_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(out, "pub struct {};", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;
    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let EnumValue::Str(value) = &member.value else {
            return Err(format!("mixed string/int enum is not supported: {}", table.name).into());
        };
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(
            out,
            "    pub const {}: &'static str = {:?};",
            const_name, value
        )?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn member_int(member: &EnumMember, enum_name: &str) -> Result<i32, Box<dyn Error>> {
    match member.value {
        EnumValue::Int(value) => Ok(value),
        EnumValue::Str(_) => Err(format!("{enum_name}.{} is not an integer", member.name).into()),
    }
}

fn disambiguate_const_name(name: String, used: &mut BTreeMap<String, usize>) -> String {
    let count = used.entry(name.clone()).or_default();
    *count += 1;
    if *count == 1 {
        name
    } else {
        format!("{name}_{}", *count)
    }
}

fn screaming_const_name(ts_name: &str) -> String {
    if ts_name == "$" {
        return "DOLLAR".to_owned();
    }
    if ts_name == "_" {
        return "UNDERSCORE".to_owned();
    }

    let mut out = String::new();
    let chars: Vec<char> = ts_name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }

        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }

        out.push(ch.to_ascii_uppercase());
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "VALUE".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

#[derive(Clone, Debug)]
struct DtsField {
    name: String,
    type_text: String,
    optional: bool,
}

#[derive(Clone, Debug, Default)]
struct InterfaceDecl {
    bases: Vec<String>,
    fields: Vec<DtsField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildKind {
    Node,
    Nodes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChildVisit {
    name: String,
    kind: ChildKind,
}

#[derive(Clone, Debug)]
struct NodeSchema {
    kind_name: String,
    data_name: String,
    fields: Vec<SchemaField>,
    children: Vec<ChildVisit>,
}

#[derive(Clone, Debug)]
struct SchemaField {
    ts_name: String,
    rust_name: String,
    ty: RustFieldType,
    optional: bool,
    child: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustFieldType {
    Node,
    NodeArray,
    Bool,
    String,
    Number,
    SyntaxKind,
    Payload,
}

fn codegen_nodes(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?;
    let dts = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/typescript.d.ts"))?;

    let child_table = parse_for_each_child_table(&tsc)?;
    let interfaces = parse_dts_interfaces(&dts)?;
    let mut dts_nodes = collect_dts_nodes(&interfaces)?;
    seed_token_payload_nodes(&mut dts_nodes);
    let schemas = merge_node_schema(child_table, dts_nodes);

    let nodes_rs = rustfmt_text(&render_nodes_rs(&schemas)?)?;
    let for_each_child_rs = rustfmt_text(&render_for_each_child_rs(&schemas)?)?;
    let schema_json = render_nodes_schema_json(&schemas)?;

    write_generated(
        &workspace.join("crates/syntax/src/nodes.rs"),
        &nodes_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/for_each_child.rs"),
        &for_each_child_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/nodes.schema.json"),
        &schema_json,
        check,
    )?;

    if check {
        println!("generated node schema files are up to date");
    } else {
        println!("generated node schema files");
    }

    Ok(())
}

fn parse_for_each_child_table(
    tsc: &str,
) -> Result<BTreeMap<String, Vec<ChildVisit>>, Box<dyn Error>> {
    let table = extract_balanced_after(tsc, "var forEachChildTable = ", '{', '}')?;
    let mut helper_cache = BTreeMap::<String, Vec<ChildVisit>>::new();
    let mut result = BTreeMap::<String, Vec<ChildVisit>>::new();

    for entry in split_top_level_entries(table) {
        let Some(kind_start) = entry.find("/*") else {
            continue;
        };
        let kind_name_start = kind_start + 2;
        let kind_name_end = entry[kind_name_start..]
            .find("*/")
            .map(|offset| kind_name_start + offset)
            .ok_or_else(|| format!("malformed forEachChildTable entry: {entry}"))?;
        let kind_name = entry[kind_name_start..kind_name_end].trim().to_owned();
        let value = entry
            .split_once(':')
            .map(|(_, value)| value.trim())
            .ok_or_else(|| format!("forEachChildTable entry has no value: {entry}"))?;

        let visits = if value.starts_with("function ") {
            extract_visits(value)
        } else {
            let helper_name = value.trim_end_matches(',').trim();
            if let Some(visits) = helper_cache.get(helper_name) {
                visits.clone()
            } else {
                let helper = extract_function(tsc, helper_name)?;
                let visits = extract_visits(helper);
                helper_cache.insert(helper_name.to_owned(), visits.clone());
                visits
            }
        };
        result.insert(kind_name, visits);
    }

    if result.is_empty() {
        return Err("forEachChildTable extraction produced no entries".into());
    }

    Ok(result)
}

fn extract_balanced_after<'a>(
    text: &'a str,
    marker: &str,
    open_ch: char,
    close_ch: char,
) -> Result<&'a str, Box<dyn Error>> {
    let marker_pos = text
        .find(marker)
        .ok_or_else(|| format!("marker not found: {marker}"))?;
    let after_marker = marker_pos + marker.len();
    let open_rel = text[after_marker..]
        .find(open_ch)
        .ok_or_else(|| format!("opening delimiter not found after marker: {marker}"))?;
    let open = after_marker + open_rel;
    let mut depth = 0usize;
    let mut close = None;
    for (offset, ch) in text[open..].char_indices() {
        if ch == open_ch {
            depth += 1;
        } else if ch == close_ch {
            depth -= 1;
            if depth == 0 {
                close = Some(open + offset);
                break;
            }
        }
    }
    let close =
        close.ok_or_else(|| format!("closing delimiter not found after marker: {marker}"))?;
    Ok(&text[open + 1..close])
}

fn extract_function<'a>(text: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    extract_balanced_after(text, &format!("function {name}("), '{', '}')
}

fn split_top_level_entries(block: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in block.char_indices() {
        match ch {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                let entry = block[start..idx].trim();
                if !entry.is_empty() {
                    entries.push(entry.to_owned());
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = block[start..].trim();
    if !tail.is_empty() {
        entries.push(tail.to_owned());
    }
    entries
}

fn extract_visits(text: &str) -> Vec<ChildVisit> {
    let mut visits = Vec::new();
    for (needle, kind) in [
        ("visitNode2(cbNode, node.", ChildKind::Node),
        ("visitNodes(cbNode, cbNodes, node.", ChildKind::Nodes),
    ] {
        let mut rest = text;
        while let Some(pos) = rest.find(needle) {
            let field_start = pos + needle.len();
            let after = &rest[field_start..];
            let field_len = after
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if field_len > 0 {
                visits.push(ChildVisit {
                    name: after[..field_len].to_owned(),
                    kind,
                });
            }
            rest = &after[field_len..];
        }
    }

    visits.sort_by_key(|visit| {
        text.find(&format!("node.{}", visit.name))
            .unwrap_or(usize::MAX)
    });
    visits.dedup();
    visits
}

fn parse_dts_interfaces(dts: &str) -> Result<BTreeMap<String, InterfaceDecl>, Box<dyn Error>> {
    let mut interfaces = BTreeMap::<String, InterfaceDecl>::new();
    let lines: Vec<&str> = dts.lines().collect();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim();
        let Some(interface_pos) = line.find("interface ") else {
            idx += 1;
            continue;
        };
        if !line[..interface_pos].trim().is_empty() {
            idx += 1;
            continue;
        }

        let header = line;
        let name_start = interface_pos + "interface ".len();
        let name_end = header[name_start..]
            .find(['<', ' ', '{'])
            .map(|offset| name_start + offset)
            .unwrap_or(header.len());
        let name = header[name_start..name_end].to_owned();
        let bases = parse_interface_bases(header);

        let mut body = String::new();
        let mut depth = header.matches('{').count() as i32 - header.matches('}').count() as i32;
        if let Some(open) = header.find('{') {
            body.push_str(&header[open + 1..]);
            body.push('\n');
        }

        idx += 1;
        while idx < lines.len() && depth > 0 {
            let body_line = lines[idx];
            depth += body_line.matches('{').count() as i32;
            depth -= body_line.matches('}').count() as i32;
            if depth >= 0 {
                body.push_str(body_line);
                body.push('\n');
            }
            idx += 1;
        }

        let fields = parse_interface_fields(&body);
        let decl = interfaces.entry(name).or_default();
        for base in bases {
            if !decl.bases.contains(&base) {
                decl.bases.push(base);
            }
        }
        for field in fields {
            merge_dts_field(&mut decl.fields, field);
        }
    }

    Ok(interfaces)
}

fn parse_interface_bases(header: &str) -> Vec<String> {
    let Some(extends_pos) = header.find(" extends ") else {
        return Vec::new();
    };
    let bases_text = header[extends_pos + " extends ".len()..]
        .split('{')
        .next()
        .unwrap_or_default();
    bases_text
        .split(',')
        .filter_map(|base| {
            let base = base.trim();
            if base.is_empty() {
                return None;
            }
            Some(
                base.split(|ch: char| ch == '<' || ch.is_whitespace())
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .filter(|base| !base.is_empty())
        .collect()
}

fn parse_interface_fields(body: &str) -> Vec<DtsField> {
    let mut fields = Vec::new();
    let mut entry = String::new();
    let mut in_block_comment = false;

    for raw_line in body.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }
        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }
        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        entry.push_str(line);
        entry.push(' ');
        if line.ends_with(';') {
            if let Some(field) = parse_dts_field(&entry) {
                fields.push(field);
            }
            entry.clear();
        }
    }

    fields
}

fn parse_dts_field(entry: &str) -> Option<DtsField> {
    let entry = strip_line_comment(entry)
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_owned();
    if entry.is_empty() || entry.contains('(') || entry.starts_with('[') {
        return None;
    }
    let entry = entry
        .strip_prefix("readonly ")
        .unwrap_or(&entry)
        .strip_prefix("/** @internal */ ")
        .unwrap_or(entry.as_str())
        .trim();
    let colon = entry.find(':')?;
    let mut name = entry[..colon].trim();
    let optional = name.ends_with('?') || entry[colon + 1..].contains("undefined");
    name = name.trim_end_matches('?').trim();
    if name.starts_with('_') || name == "parent" {
        return None;
    }
    Some(DtsField {
        name: name.trim_matches('"').to_owned(),
        type_text: entry[colon + 1..].trim().to_owned(),
        optional,
    })
}

fn merge_dts_field(fields: &mut Vec<DtsField>, field: DtsField) {
    if let Some(existing) = fields
        .iter_mut()
        .find(|existing| existing.name == field.name)
    {
        *existing = field;
    } else {
        fields.push(field);
    }
}

fn collect_dts_nodes(
    interfaces: &BTreeMap<String, InterfaceDecl>,
) -> Result<BTreeMap<String, Vec<DtsField>>, Box<dyn Error>> {
    let mut nodes = BTreeMap::<String, Vec<DtsField>>::new();
    for (interface_name, decl) in interfaces {
        let Some(kind_field) = decl.fields.iter().find(|field| field.name == "kind") else {
            continue;
        };
        let kinds = syntax_kinds_from_type(&kind_field.type_text);
        if kinds.is_empty() {
            continue;
        }
        let fields = collect_interface_fields(interface_name, interfaces, &mut Vec::new())?;
        let fields: Vec<DtsField> = fields
            .into_iter()
            .filter(|field| field.name != "kind")
            .collect();
        for kind in kinds {
            nodes.entry(kind).or_insert_with(|| fields.clone());
        }
    }
    Ok(nodes)
}

fn seed_token_payload_nodes(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in ["Identifier", "PrivateIdentifier"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "escapedText".to_owned(),
                    type_text: "__String".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
            ]
        });
    }

    for kind in [
        "StringLiteral",
        "NumericLiteral",
        "BigIntLiteral",
        "RegularExpressionLiteral",
        "NoSubstitutionTemplateLiteral",
        "JsxText",
    ] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![DtsField {
                name: "text".to_owned(),
                type_text: "string".to_owned(),
                optional: false,
            }]
        });
    }

    for kind in ["TemplateHead", "TemplateMiddle", "TemplateTail"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "rawText".to_owned(),
                    type_text: "string".to_owned(),
                    optional: true,
                },
            ]
        });
    }
}

fn collect_interface_fields(
    interface_name: &str,
    interfaces: &BTreeMap<String, InterfaceDecl>,
    stack: &mut Vec<String>,
) -> Result<Vec<DtsField>, Box<dyn Error>> {
    if stack.iter().any(|name| name == interface_name) {
        return Ok(Vec::new());
    }
    let Some(decl) = interfaces.get(interface_name) else {
        return Ok(Vec::new());
    };

    stack.push(interface_name.to_owned());
    let mut fields = Vec::new();
    for base in &decl.bases {
        for field in collect_interface_fields(base, interfaces, stack)? {
            merge_dts_field(&mut fields, field);
        }
    }
    for field in &decl.fields {
        merge_dts_field(&mut fields, field.clone());
    }
    stack.pop();
    Ok(fields)
}

fn syntax_kinds_from_type(type_text: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    let mut rest = type_text;
    while let Some(pos) = rest.find("SyntaxKind.") {
        let start = pos + "SyntaxKind.".len();
        let after = &rest[start..];
        let len = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .map(char::len_utf8)
            .sum::<usize>();
        if len > 0 {
            kinds.push(after[..len].to_owned());
        }
        rest = &after[len..];
    }
    kinds
}

fn merge_node_schema(
    child_table: BTreeMap<String, Vec<ChildVisit>>,
    dts_nodes: BTreeMap<String, Vec<DtsField>>,
) -> Vec<NodeSchema> {
    let mut schemas = BTreeMap::<String, NodeSchema>::new();
    for (kind_name, dts_fields) in dts_nodes {
        let children = child_table.get(&kind_name).cloned().unwrap_or_default();
        schemas.insert(
            kind_name.clone(),
            build_node_schema(kind_name, dts_fields, children),
        );
    }
    for (kind_name, children) in child_table {
        schemas.entry(kind_name.clone()).or_insert_with(|| {
            let dts_fields = children
                .iter()
                .map(|child| DtsField {
                    name: child.name.clone(),
                    type_text: match child.kind {
                        ChildKind::Node => "Node".to_owned(),
                        ChildKind::Nodes => "NodeArray<Node>".to_owned(),
                    },
                    optional: true,
                })
                .collect();
            build_node_schema(kind_name, dts_fields, children)
        });
    }
    schemas.into_values().collect()
}

fn build_node_schema(
    kind_name: String,
    dts_fields: Vec<DtsField>,
    children: Vec<ChildVisit>,
) -> NodeSchema {
    let mut fields = Vec::new();
    for dts_field in dts_fields {
        let child = children.iter().find(|child| child.name == dts_field.name);
        let ty = if let Some(child) = child {
            match child.kind {
                ChildKind::Node => RustFieldType::Node,
                ChildKind::Nodes => RustFieldType::NodeArray,
            }
        } else {
            rust_field_type(&dts_field.type_text)
        };
        let optional = dts_field.optional;
        fields.push(SchemaField {
            rust_name: rust_field_name(&dts_field.name),
            ts_name: dts_field.name,
            ty,
            optional,
            child: child.is_some(),
        });
    }
    for child in &children {
        if fields.iter().all(|field| field.ts_name != child.name) {
            fields.push(SchemaField {
                ts_name: child.name.clone(),
                rust_name: rust_field_name(&child.name),
                ty: match child.kind {
                    ChildKind::Node => RustFieldType::Node,
                    ChildKind::Nodes => RustFieldType::NodeArray,
                },
                optional: true,
                child: true,
            });
        }
    }

    NodeSchema {
        data_name: format!("{}Data", kind_name),
        kind_name,
        fields,
        children,
    }
}

fn rust_field_type(type_text: &str) -> RustFieldType {
    if type_text.contains("NodeArray<") {
        RustFieldType::NodeArray
    } else if type_text.contains("boolean") {
        RustFieldType::Bool
    } else if type_text.contains("string") || type_text.contains("__String") {
        RustFieldType::String
    } else if type_text.contains("number") {
        RustFieldType::Number
    } else if type_text.contains("SyntaxKind") {
        RustFieldType::SyntaxKind
    } else if type_text.contains("Node")
        || type_text.contains("Expression")
        || type_text.contains("Declaration")
        || type_text.contains("Identifier")
        || type_text.contains("Token")
        || type_text.contains("Type")
        || type_text.contains("Statement")
        || type_text.contains("Clause")
        || type_text.contains("Element")
        || type_text.contains("Literal")
        || type_text.contains("Name")
    {
        RustFieldType::Node
    } else {
        RustFieldType::Payload
    }
}

fn rust_field_name(ts_name: &str) -> String {
    let snake = snake_case(ts_name);
    match snake.as_str() {
        "type" | "default" | "abstract" | "final" | "box" | "move" | "ref" | "use" => {
            format!("r#{snake}")
        }
        _ => snake,
    }
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }
        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }
        out.push(ch.to_ascii_lowercase());
    }
    out.trim_matches('_').to_owned()
}

fn render_nodes_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use crate::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeId(pub u32);")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeArrayId(pub u32);")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]")?;
    writeln!(out, "pub struct NodeArray {{")?;
    writeln!(out, "    pub nodes: Vec<NodeId>,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub has_trailing_comma: bool,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodePayload {{")?;
    writeln!(out, "    Bool(bool),")?;
    writeln!(out, "    String(String),")?;
    writeln!(out, "    Number(f64),")?;
    writeln!(out, "    Kind(SyntaxKind),")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub struct Node {{")?;
    writeln!(out, "    pub kind: SyntaxKind,")?;
    writeln!(out, "    pub flags: i32,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub parent: Option<NodeId>,")?;
    writeln!(out, "    pub data: NodeData,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    for schema in schemas {
        writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
        writeln!(out, "pub struct {} {{", schema.data_name)?;
        for field in &schema.fields {
            writeln!(
                out,
                "    pub {}: {},",
                field.rust_name,
                render_field_type(field)
            )?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodeData {{")?;
    writeln!(out, "    Token,")?;
    for schema in schemas {
        writeln!(out, "    {}({}),", schema.kind_name, schema.data_name)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl NodeData {{")?;
    writeln!(out, "    pub const fn kind(&self) -> Option<SyntaxKind> {{")?;
    writeln!(out, "        match self {{")?;
    writeln!(out, "            Self::Token => None,")?;
    for schema in schemas {
        writeln!(
            out,
            "            Self::{}(_) => Some(SyntaxKind::{}),",
            schema.kind_name, schema.kind_name
        )?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    for schema in schemas {
        let accessor = format!("as_{}", snake_case(&schema.kind_name));
        writeln!(out)?;
        writeln!(
            out,
            "    pub fn {}(&self) -> Option<&{}> {{",
            accessor, schema.data_name
        )?;
        writeln!(out, "        match self {{")?;
        writeln!(
            out,
            "            Self::{}(data) => Some(data),",
            schema.kind_name
        )?;
        writeln!(out, "            _ => None,")?;
        writeln!(out, "        }}")?;
        writeln!(out, "    }}")?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_node_schema_has_core_nodes() {{")?;
    writeln!(out, "        assert_eq!(NodeData::Token.kind(), None);")?;
    writeln!(
        out,
        "        let _ = IdentifierData {{ escaped_text: String::new(), text: String::new() }};"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_field_type(field: &SchemaField) -> String {
    let base = match field.ty {
        RustFieldType::Node => "NodeId",
        RustFieldType::NodeArray => "NodeArrayId",
        RustFieldType::Bool => "bool",
        RustFieldType::String => "String",
        RustFieldType::Number => "f64",
        RustFieldType::SyntaxKind => "SyntaxKind",
        RustFieldType::Payload => "NodePayload",
    };
    if field.optional {
        format!("Option<{base}>")
    } else {
        base.to_owned()
    }
}

fn render_for_each_child_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "use crate::nodes::{{Node, NodeArray, NodeArrayId, NodeData, NodeId}};"
    )?;
    writeln!(out)?;
    writeln!(out, "pub trait NodeLookup {{")?;
    writeln!(
        out,
        "    fn node_array(&self, id: NodeArrayId) -> &NodeArray;"
    )?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub fn for_each_child<L, F>(lookup: &L, node: &Node, mut cb: F) -> Option<NodeId>"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    L: NodeLookup,")?;
    writeln!(out, "    F: FnMut(NodeId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match &node.data {{")?;
    writeln!(out, "        NodeData::Token => None,")?;
    for schema in schemas {
        if schema.children.is_empty() {
            writeln!(
                out,
                "        NodeData::{}(_data) => None,",
                schema.kind_name
            )?;
        } else {
            writeln!(out, "        NodeData::{}(data) => {{", schema.kind_name)?;
            for child in &schema.children {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| field.ts_name == child.name)
                    .ok_or_else(|| format!("missing generated field for child {}", child.name))?;
                let helper = match (child.kind, field.optional) {
                    (ChildKind::Node, false) => "visit_node",
                    (ChildKind::Node, true) => "visit_optional_node",
                    (ChildKind::Nodes, false) => "visit_nodes",
                    (ChildKind::Nodes, true) => "visit_optional_nodes",
                };
                if child.kind == ChildKind::Node {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                } else {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(lookup, data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                }
            }
            writeln!(out, "            None")?;
            writeln!(out, "        }}")?;
        }
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_node<F>(id: NodeId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ if cb(id) {{ Some(id) }} else {{ None }} }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_optional_node<F>(id: Option<NodeId>, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_node(id, cb)) }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_nodes<L, F>(lookup: &L, id: NodeArrayId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{")?;
    writeln!(out, "    for node in &lookup.node_array(id).nodes {{")?;
    writeln!(out, "        if cb(*node) {{ return Some(*node); }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "    None")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn visit_optional_nodes<L, F>(lookup: &L, id: Option<NodeArrayId>, cb: &mut F) -> Option<NodeId>")?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_nodes(lookup, id, cb)) }}")?;
    Ok(out)
}

fn render_nodes_schema_json(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(out, "{{")?;
    writeln!(out, "  \"schema\": 1,")?;
    writeln!(out, "  \"nodes\": [")?;
    for (idx, schema) in schemas.iter().enumerate() {
        writeln!(out, "    {{")?;
        writeln!(out, "      \"kindName\": {:?},", schema.kind_name)?;
        writeln!(out, "      \"dataName\": {:?},", schema.data_name)?;
        writeln!(out, "      \"fields\": [")?;
        for (field_idx, field) in schema.fields.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"rustName\": {:?}, \"type\": {:?}, \"optional\": {}, \"child\": {}}}{}",
                field.ts_name,
                field.rust_name,
                format!("{:?}", field.ty),
                field.optional,
                field.child,
                if field_idx + 1 == schema.fields.len() { "" } else { "," }
            )?;
        }
        writeln!(out, "      ],")?;
        writeln!(out, "      \"children\": [")?;
        for (child_idx, child) in schema.children.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"array\": {}}}{}",
                child.name,
                child.kind == ChildKind::Nodes,
                if child_idx + 1 == schema.children.len() {
                    ""
                } else {
                    ","
                }
            )?;
        }
        writeln!(out, "      ]")?;
        writeln!(
            out,
            "    }}{}",
            if idx + 1 == schemas.len() { "" } else { "," }
        )?;
    }
    writeln!(out, "  ]")?;
    writeln!(out, "}}")?;
    Ok(out)
}

#[derive(Clone, Debug)]
struct DiagnosticEntry {
    name: String,
    code: u32,
    category: String,
    text: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

#[derive(Clone, Debug)]
struct DiagnosticEntryFields {
    code: u32,
    category: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

fn codegen_diags(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let path = workspace.join("vendor/typescript-6.0.3/lib/diagnosticMessages.json");
    let raw = fs::read_to_string(path)?;
    let mut entries = parse_diagnostic_catalog(&raw)?;

    entries.sort_by_key(|entry| entry.code);
    let gen_rs = rustfmt_text(&render_diags_gen(&entries)?)?;
    write_generated(&workspace.join("crates/diags/src/gen.rs"), &gen_rs, check)?;

    if check {
        println!("generated diagnostic messages are up to date");
    } else {
        println!("generated diagnostic messages");
    }

    Ok(())
}

fn parse_diagnostic_catalog(src: &str) -> Result<Vec<DiagnosticEntry>, Box<dyn Error>> {
    let mut json = JsonReader::new(src);
    json.ws();
    json.expect('{')?;
    let mut entries = Vec::new();

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let text = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        let fields = parse_diagnostic_entry(&mut json)?;
        entries.push(DiagnosticEntry {
            name: diagnostic_static_name(&text),
            code: fields.code,
            category: fields.category,
            text,
            reports_unnecessary: fields.reports_unnecessary,
            reports_deprecated: fields.reports_deprecated,
            elided_in_compatibility_pyramid: fields.elided_in_compatibility_pyramid,
        });

        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' after diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    let mut names = BTreeMap::<String, u32>::new();
    for entry in &entries {
        if let Some(existing) = names.insert(entry.name.clone(), entry.code) {
            return Err(format!(
                "diagnostic static name collision: {} for codes {} and {}",
                entry.name, existing, entry.code
            )
            .into());
        }
    }

    Ok(entries)
}

fn parse_diagnostic_entry(
    json: &mut JsonReader<'_>,
) -> Result<DiagnosticEntryFields, Box<dyn Error>> {
    json.expect('{')?;
    let mut code = None;
    let mut category = None;
    let mut reports_unnecessary = false;
    let mut reports_deprecated = false;
    let mut elided = false;

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let key = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        match key.as_str() {
            "code" => code = Some(json.number()? as u32),
            "category" => category = Some(json.string()?),
            "reportsUnnecessary" => reports_unnecessary = json.boolean()?,
            "reportsDeprecated" => reports_deprecated = json.boolean()?,
            "elidedInCompatabilityPyramid" => elided = json.boolean()?,
            _ => json.skip_value()?,
        }
        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' in diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    Ok(DiagnosticEntryFields {
        code: code.ok_or("diagnostic entry missing code")?,
        category: category.ok_or("diagnostic entry missing category")?,
        reports_unnecessary,
        reports_deprecated,
        elided_in_compatibility_pyramid: elided,
    })
}

fn render_diags_gen(entries: &[DiagnosticEntry]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen diags`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use super::{{DiagnosticCategory, DiagnosticMessage}};")?;
    writeln!(out)?;

    for entry in entries {
        writeln!(
            out,
            "pub static {}: DiagnosticMessage = DiagnosticMessage {{",
            entry.name
        )?;
        writeln!(out, "    code: {},", entry.code)?;
        writeln!(out, "    category: DiagnosticCategory::{},", entry.category)?;
        writeln!(out, "    text: {:?},", entry.text)?;
        writeln!(
            out,
            "    reports_unnecessary: {},",
            entry.reports_unnecessary
        )?;
        writeln!(out, "    reports_deprecated: {},", entry.reports_deprecated)?;
        writeln!(
            out,
            "    elided_in_compatibility_pyramid: {},",
            entry.elided_in_compatibility_pyramid
        )?;
        writeln!(out, "}};")?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "pub static ALL_BY_CODE: &[(u32, &DiagnosticMessage)] = &["
    )?;
    for entry in entries {
        writeln!(out, "    ({}, &{}),", entry.code, entry.name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_diagnostic_pins_match_tsc() {{")?;
    writeln!(
        out,
        "        assert_eq!(Unterminated_string_literal.code, 1002);"
    )?;
    writeln!(out, "        assert_eq!(_0_expected.code, 1005);")?;
    writeln!(
        out,
        "        assert_eq!(ALL_BY_CODE.len(), {});",
        entries.len()
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn diagnostic_static_name(message: &str) -> String {
    let mut out = String::new();
    let mut previous_was_separator = false;

    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            out.push('_');
            previous_was_separator = true;
        }
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "Diagnostic".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if is_rust_keyword(&out) {
        out.insert_str(0, "r#");
    }
    out
}

struct JsonReader<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonReader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            bytes: src.as_bytes(),
            index: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.index).copied().map(char::from)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.index += 1;
        }
        ch
    }

    fn expect(&mut self, expected: char) -> Result<(), Box<dyn Error>> {
        match self.bump() {
            Some(actual) if actual == expected => Ok(()),
            actual => Err(format!(
                "expected {expected:?}, got {actual:?} at byte {}",
                self.index
            )
            .into()),
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn string(&mut self) -> Result<String, Box<dyn Error>> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let ch = self.bump().ok_or("unterminated JSON string")?;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.bump().ok_or("unterminated JSON escape")?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                hex.push(self.bump().ok_or("short JSON unicode escape")?);
                            }
                            let code = u32::from_str_radix(&hex, 16)?;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => return Err(format!("unknown JSON escape \\{other}").into()),
                    }
                }
                _ if ch.is_ascii() => out.push(ch),
                _ => {
                    self.index -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.index..])?;
                    let decoded = rest.chars().next().ok_or("invalid UTF-8 in JSON string")?;
                    self.index += decoded.len_utf8();
                    out.push(decoded);
                }
            }
        }
    }

    fn number(&mut self) -> Result<i64, Box<dyn Error>> {
        let start = self.index;
        while matches!(self.peek(), Some('-' | '+' | '0'..='9')) {
            self.index += 1;
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.index])?.parse()?)
    }

    fn boolean(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.bytes[self.index..].starts_with(b"true") {
            self.index += 4;
            Ok(true)
        } else if self.bytes[self.index..].starts_with(b"false") {
            self.index += 5;
            Ok(false)
        } else {
            Err(format!("expected JSON boolean at byte {}", self.index).into())
        }
    }

    fn skip_value(&mut self) -> Result<(), Box<dyn Error>> {
        self.ws();
        match self.peek() {
            Some('"') => {
                self.string()?;
            }
            Some('{') => self.skip_balanced('{', '}')?,
            Some('[') => self.skip_balanced('[', ']')?,
            Some('t') | Some('f') => {
                self.boolean()?;
            }
            Some('n') if self.bytes[self.index..].starts_with(b"null") => {
                self.index += 4;
            }
            Some('-' | '+' | '0'..='9') => {
                self.number()?;
            }
            other => {
                return Err(
                    format!("unexpected JSON value {other:?} at byte {}", self.index).into(),
                )
            }
        }
        Ok(())
    }

    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), Box<dyn Error>> {
        self.expect(open)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.bump() {
                Some('"') => {
                    self.index -= 1;
                    self.string()?;
                }
                Some(ch) if ch == open => depth += 1,
                Some(ch) if ch == close => depth -= 1,
                Some(_) => {}
                None => return Err("unterminated JSON container".into()),
            }
        }
        Ok(())
    }
}
