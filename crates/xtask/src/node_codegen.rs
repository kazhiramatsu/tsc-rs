//! TypeScript node schema generation shared by xtask entry points.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;

use crate::codegen_common::{
    find_workspace_root, rustfmt_text, strip_line_comment, write_generated,
};

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
    rust_optional: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustFieldType {
    Node,
    NodeArray,
    Bool,
    String,
    Number,
    SyntaxKind,
    JSDocComment,
    Payload,
}

pub(crate) fn codegen_nodes(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_workspace_root()?;
    let tsc = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?;
    let dts = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/typescript.d.ts"))?;

    let child_table = parse_for_each_child_table(&tsc)?;
    let interfaces = parse_dts_interfaces(&dts)?;
    let aliases = parse_dts_type_aliases(&dts);
    let dts_nodes = collect_dts_nodes(&interfaces, &aliases, &child_table)?;
    let schemas = merge_node_schema(child_table, dts_nodes);

    let nodes_rs = rustfmt_text(&render_nodes_rs(&schemas)?)?;
    let for_each_child_rs = rustfmt_text(&render_for_each_child_rs(&schemas)?)?;
    let relocate_rs = rustfmt_text(&render_relocate_rs(&schemas)?)?;
    let observable_fields_rs = rustfmt_text(&render_observable_fields_rs(&schemas)?)?;
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
        &workspace.join("crates/syntax/src/relocate.rs"),
        &relocate_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/observable_fields.rs"),
        &observable_fields_rs,
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

/// Field-level schema gate (impl-nodes.md contract): cross-check
/// crates/syntax/nodes.schema.json against typescript.d.ts as parsed by
/// the VENDORED TypeScript itself (crates/oracle/schema-dump.mjs). A
/// schema field tsc does not declare, or whose payload category or
/// optionality disagrees, is a hard failure (the readonly_* generator-bug
/// class; the fabricated child `optional: true` class).
/// The KIND SETS reconcile exactly in both directions: a d.ts kind the
/// schema does not materialize must be allowlisted in
/// UNMATERIALIZED_KINDS (else a dropped-kind generator bug), a schema
/// kind no d.ts interface claims is a ghost, and stale allowlist entries
/// fail either way.
/// tsc fields the schema does not carry yet — including fields on
/// allowlisted unmaterialized kinds — are tracked exactly in
/// nodes-missing-fields.txt; `--write` regenerates the manifest so its
/// diff is the review surface.
pub(crate) fn schema_audit(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut write = false;
    for arg in args {
        match arg.as_str() {
            "--write" => write = true,
            other => return Err(format!("unexpected schema-audit argument: {other}").into()),
        }
    }
    let workspace = find_workspace_root()?;
    let output = std::process::Command::new("node")
        .arg(workspace.join("crates/oracle/schema-dump.mjs"))
        .arg(workspace.join("vendor/typescript-6.0.3/lib/typescript.d.ts"))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "schema-dump probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let dump: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if let Some(conflicts) = dump["conflicts"].as_array() {
        if !conflicts.is_empty() {
            return Err(format!("schema-dump kind conflicts unresolved: {conflicts:?}").into());
        }
    }
    let tsc_kinds = &dump["kinds"];

    let schema_text = fs::read_to_string(workspace.join("crates/syntax/nodes.schema.json"))?;
    let schema: serde_json::Value = serde_json::from_str(&schema_text)?;

    // pos/end/flags live on the Node header and `parent` in the parent
    // map — header-owned, never schema fields.
    const HEADER_OWNED_FIELDS: &[&str] = &["pos", "end", "flags", "parent"];

    let mut ghosts = Vec::new();
    let mut mismatches = Vec::new();
    let mut kind_errors = Vec::new();
    let mut missing = Vec::new();
    let mut runtime_only = Vec::new();
    let mut unmaterialized = Vec::new();
    let mut rust_kinds = BTreeSet::new();
    let mut kind_count = 0usize;
    for node in schema["nodes"]
        .as_array()
        .ok_or("malformed nodes.schema.json: nodes")?
    {
        let kind_name = node["kindName"]
            .as_str()
            .ok_or("malformed nodes.schema.json: kindName")?;
        rust_kinds.insert(kind_name);
        kind_count += 1;
        let fields = node["fields"]
            .as_array()
            .ok_or("malformed nodes.schema.json: fields")?;
        let tsc_node = &tsc_kinds[kind_name];
        if tsc_node.is_null() {
            kind_errors.push(format!(
                "{kind_name}: in nodes.schema.json but no typescript.d.ts interface claims the kind"
            ));
            continue;
        }
        let mut tsc_by_name = BTreeMap::<&str, (&str, bool)>::new();
        for field in tsc_node["fields"]
            .as_array()
            .ok_or("malformed schema-dump: fields")?
        {
            tsc_by_name.insert(
                field["name"]
                    .as_str()
                    .ok_or("malformed schema-dump: name")?,
                (
                    field["type"]
                        .as_str()
                        .ok_or("malformed schema-dump: type")?,
                    field["optional"].as_bool().unwrap_or(false),
                ),
            );
        }
        let mut ours = Vec::new();
        for field in fields {
            let name = field["name"]
                .as_str()
                .ok_or("malformed nodes.schema.json: field name")?;
            let ty = field["type"]
                .as_str()
                .ok_or("malformed nodes.schema.json: field type")?;
            let child = field["child"].as_bool().unwrap_or(false);
            let optional = field["optional"].as_bool().unwrap_or(false);
            ours.push(name);
            match tsc_by_name.get(name) {
                // A child field is backed by _tsc.js's forEachChildTable
                // (the runtime node shape); the public d.ts strips
                // @internal grammar-error slots, so absence there is
                // expected — tracked in the manifest, not a ghost.
                None if child => runtime_only.push(format!("{kind_name}.{name}")),
                None => ghosts.push(format!("{kind_name}.{name}")),
                Some((tsc_ty, tsc_optional)) => {
                    if ty != *tsc_ty {
                        mismatches.push(format!("{kind_name}.{name}: rust {ty} vs tsc {tsc_ty}"));
                    }
                    if optional != *tsc_optional {
                        mismatches.push(format!(
                            "{kind_name}.{name}: rust optional={optional} vs tsc optional={tsc_optional}"
                        ));
                    }
                }
            }
        }
        for (name, (ty, optional)) in &tsc_by_name {
            if HEADER_OWNED_FIELDS.contains(name) || ours.contains(name) {
                continue;
            }
            missing.push(format!("{kind_name}.{name} type={ty} optional={optional}"));
        }
    }

    // Kind-set reconciliation, both directions: every d.ts kind is either
    // materialized or explicitly allowlisted (with its field debt
    // harvested), and the allowlist carries no stale entries.
    let tsc_kind_map = tsc_kinds
        .as_object()
        .ok_or("malformed schema-dump: kinds")?;
    for (kind_name, tsc_node) in tsc_kind_map {
        if rust_kinds.contains(kind_name.as_str()) {
            continue;
        }
        if !UNMATERIALIZED_KINDS.contains(&kind_name.as_str()) {
            kind_errors.push(format!(
                "{kind_name}: typescript.d.ts kind absent from nodes.schema.json and not \
                 allowlisted in UNMATERIALIZED_KINDS (dropped-kind generator bug?)"
            ));
            continue;
        }
        for field in tsc_node["fields"]
            .as_array()
            .ok_or("malformed schema-dump: fields")?
        {
            let name = field["name"]
                .as_str()
                .ok_or("malformed schema-dump: name")?;
            if HEADER_OWNED_FIELDS.contains(&name) {
                continue;
            }
            let ty = field["type"]
                .as_str()
                .ok_or("malformed schema-dump: type")?;
            let optional = field["optional"].as_bool().unwrap_or(false);
            unmaterialized.push(format!("{kind_name}.{name} type={ty} optional={optional}"));
        }
    }
    for kind_name in UNMATERIALIZED_KINDS {
        if rust_kinds.contains(kind_name) {
            kind_errors.push(format!(
                "{kind_name}: allowlisted in UNMATERIALIZED_KINDS but nodes.schema.json \
                 materializes it; drop the stale entry"
            ));
        }
        if !tsc_kind_map.contains_key(*kind_name) {
            kind_errors.push(format!(
                "{kind_name}: allowlisted in UNMATERIALIZED_KINDS but not a typescript.d.ts kind"
            ));
        }
    }

    if !ghosts.is_empty() || !mismatches.is_empty() || !kind_errors.is_empty() {
        return Err(format!(
            "schema-audit failed: {} ghost field(s) not in typescript.d.ts: {:?}; {} category mismatch(es): {:?}; {} kind-set error(s): {:?}",
            ghosts.len(),
            ghosts,
            mismatches.len(),
            mismatches,
            kind_errors.len(),
            kind_errors
        )
        .into());
    }

    missing.sort();
    runtime_only.sort();
    let mut manifest = String::new();
    manifest.push_str(
        "# tsc 6.0.3 node-interface fields the generated schema does not carry yet\n\
         # (impl-nodes.md field contract debt). Regenerate with\n\
         # `cargo xtask schema-audit --write`; the diff is the review surface.\n",
    );
    for line in &missing {
        manifest.push_str(line);
        manifest.push('\n');
    }
    manifest.push_str(
        "# -- runtime-only child fields: forEachChildTable-backed, stripped\n\
         # -- from the public d.ts as @internal grammar-error slots\n",
    );
    for line in &runtime_only {
        manifest.push_str(line);
        manifest.push('\n');
    }
    unmaterialized.sort();
    manifest.push_str(
        "# -- d.ts fields on unmaterialized kinds (UNMATERIALIZED_KINDS in\n\
         # -- xtask: kind-only token nodes or tsc-synthetic kinds with no\n\
         # -- payload struct generated yet)\n",
    );
    for line in &unmaterialized {
        manifest.push_str(line);
        manifest.push('\n');
    }
    let manifest_path = workspace.join("nodes-missing-fields.txt");
    if write {
        fs::write(&manifest_path, &manifest)?;
        println!(
            "schema-audit: wrote {} ({} tracked entries)",
            manifest_path.display(),
            missing.len()
        );
    } else {
        let current = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "{} unreadable ({error}); run `cargo xtask schema-audit --write`",
                manifest_path.display()
            )
        })?;
        if current != manifest {
            return Err(format!(
                "{} is stale; run `cargo xtask schema-audit --write` and review the diff",
                manifest_path.display()
            )
            .into());
        }
    }
    println!(
        "schema-audit ok: kinds={kind_count} unmaterialized={} ghost=0 mismatch=0 missing-tracked={} unmaterialized-field-debt={}",
        UNMATERIALIZED_KINDS.len(),
        missing.len(),
        unmaterialized.len()
    );
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

pub(crate) fn extract_balanced_after<'a>(
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
        // JSDocTypeLiteral/JSDocSignature use `forEach` directly
        // because their public fields are readonly arrays rather than
        // NodeArray in typescript.d.ts. They are nevertheless runtime
        // child arrays and belong in the generated arena schema.
        ("forEach(node.", ChildKind::Nodes),
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

    // Whole-identifier occurrence position: a bare `find` would match
    // `node.type` inside `node.typeParameters` and scramble the visit order.
    visits.sort_by_key(|visit| field_occurrence_position(text, &visit.name));
    visits.dedup();
    visits
}

fn field_occurrence_position(text: &str, name: &str) -> usize {
    let needle = format!("node.{name}");
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(&needle) {
        let abs = from + pos;
        let after = text[abs + needle.len()..].chars().next();
        if !matches!(after, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_') {
            return abs;
        }
        from = abs + needle.len();
    }
    usize::MAX
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
    // `/** @internal */` and `readonly` may stack in either order.
    let mut rest = entry.as_str();
    loop {
        let stripped = rest.trim_start();
        if let Some(next) = stripped.strip_prefix("/** @internal */") {
            rest = next;
        } else if let Some(next) = stripped.strip_prefix("readonly ") {
            rest = next;
        } else {
            rest = stripped;
            break;
        }
    }
    let entry = rest;
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

/// Scalar payload fields admitted into the generated node schema at this
/// stage, keyed by kind name. forEachChildTable-backed children need no
/// listing — they always merge, carrying their d.ts optionality. The
/// field data (type, optionality, order)
/// comes from the parsed typescript.d.ts; listing a field the d.ts does
/// not carry is a hard error, so the schema cannot drift from the vendor
/// contract. tsc fields not admitted here are surfaced by the
/// schema-audit missing-field manifest rather than silently dropped.
const DTS_SCALAR_ADMISSIONS: &[(&str, &[&str])] = &[
    ("BigIntLiteral", &["text"]),
    ("ExportAssignment", &["isExportEquals"]),
    ("ExportDeclaration", &["isTypeOnly"]),
    ("ExportSpecifier", &["isTypeOnly"]),
    ("HeritageClause", &["token"]),
    ("Identifier", &["escapedText", "text"]),
    ("ImportAttributes", &["token", "multiLine"]),
    ("ImportClause", &["isTypeOnly", "phaseModifier"]),
    ("ImportEqualsDeclaration", &["isTypeOnly"]),
    ("ImportSpecifier", &["isTypeOnly"]),
    ("ImportType", &["isTypeOf"]),
    ("JSDocCallbackTag", &["name"]),
    ("JSDocFunctionType", &["name", "typeParameters"]),
    ("JSDocLink", &["text"]),
    ("JSDocLinkCode", &["text"]),
    ("JSDocLinkPlain", &["text"]),
    ("JSDocNamepathType", &["type"]),
    ("JSDocNonNullableType", &["postfix"]),
    ("JSDocNullableType", &["postfix"]),
    ("JSDocParameterTag", &["isBracketed", "isNameFirst"]),
    ("JSDocPropertyTag", &["isBracketed", "isNameFirst"]),
    ("JSDocText", &["text"]),
    ("JSDocTypeLiteral", &["isArrayType"]),
    ("JSDocTypedefTag", &["name"]),
    ("JsxText", &["text", "containsOnlyTriviaWhiteSpaces"]),
    ("MetaProperty", &["keywordToken"]),
    ("NoSubstitutionTemplateLiteral", &["text", "rawText"]),
    ("NumericLiteral", &["text"]),
    ("PostfixUnaryExpression", &["operator"]),
    ("PrefixUnaryExpression", &["operator"]),
    ("PrivateIdentifier", &["escapedText", "text"]),
    ("RegularExpressionLiteral", &["text", "isUnterminated"]),
    ("StringLiteral", &["text", "hasExtendedUnicodeEscape"]),
    ("TemplateHead", &["text", "rawText"]),
    ("TemplateMiddle", &["text", "rawText"]),
    ("TemplateTail", &["text", "rawText"]),
    ("TypeOperator", &["operator"]),
];

/// Kinds with neither forEachChild visits nor admitted scalars, listed so
/// they still generate (fieldless) node data.
const FIELDLESS_KINDS: &[&str] = &[
    "DebuggerStatement",
    "EmptyStatement",
    "JSDocAllType",
    "JSDocUnknownType",
    // Transform-only statement anchor used when TypeScript syntax is erased.
    // It owns an original source range without carrying printable children.
    "NotEmittedStatement",
    "OmittedExpression",
    "SyntaxList",
];

/// typescript.d.ts SyntaxKinds deliberately NOT materialized as payload
/// structs (absent from nodes.schema.json). schema-audit enforces this
/// list exactly, in both directions: a d.ts kind absent from the schema
/// must be listed here, a listed kind must stay absent from the schema
/// and present in the d.ts, and any d.ts fields these kinds declare are
/// tracked in nodes-missing-fields.txt so the debt stays visible.
const UNMATERIALIZED_KINDS: &[&str] = &[
    // Keyword-literal expressions and fieldless markers: the parser
    // allocates kind-only token nodes (finish_kind_only_node); their
    // d.ts interfaces add nothing beyond the Node header, except
    // SemicolonClassElement's ClassElement-inherited optional `name`
    // (tracked as debt).
    "FalseKeyword",
    "ImportKeyword",
    "JsxClosingFragment",
    "JsxOpeningFragment",
    "NullKeyword",
    "SemicolonClassElement",
    "SuperKeyword",
    "ThisKeyword",
    "ThisType",
    "TrueKeyword",
    // Synthetic kinds tsc itself never parses: checker/transform/emit
    // fabrications.
    "Bundle",
    "NotEmittedTypeElement",
    "SyntheticExpression",
];

fn collect_dts_nodes(
    interfaces: &BTreeMap<String, InterfaceDecl>,
    aliases: &BTreeMap<String, String>,
    child_table: &BTreeMap<String, Vec<ChildVisit>>,
) -> Result<BTreeMap<String, Vec<DtsField>>, Box<dyn Error>> {
    let admissions: BTreeMap<&str, &[&str]> = DTS_SCALAR_ADMISSIONS.iter().copied().collect();

    // Interfaces claiming each kind via their own (non-inherited) `kind`
    // field. Ties resolve to the interface named after the kind (e.g.
    // JsonMinusNumericLiteral re-declares PrefixUnaryExpression's kind),
    // else to the unique interface declaring the kind as a single literal
    // (ConstructorTypeNode beats FunctionOrConstructorTypeNodeBase, whose
    // own kind is the FunctionType | ConstructorType union).
    let mut claimants = BTreeMap::<String, Vec<(&str, bool)>>::new();
    for (interface_name, decl) in interfaces {
        let Some(kind_field) = decl.fields.iter().find(|field| field.name == "kind") else {
            continue;
        };
        let kinds = syntax_kinds_from_type(&kind_field.type_text);
        let single_literal = kinds.len() == 1;
        for kind in kinds {
            claimants
                .entry(kind)
                .or_default()
                .push((interface_name.as_str(), single_literal));
        }
    }

    let mut nodes = BTreeMap::<String, Vec<DtsField>>::new();
    for (kind, interface_names) in claimants {
        if !child_table.contains_key(&kind)
            && !admissions.contains_key(kind.as_str())
            && !FIELDLESS_KINDS.contains(&kind.as_str())
        {
            continue;
        }
        let interface_name = if let [(single, _)] = interface_names.as_slice() {
            *single
        } else if let Some((exact, _)) = interface_names.iter().find(|(name, _)| *name == kind) {
            *exact
        } else {
            let single_literal: Vec<&str> = interface_names
                .iter()
                .filter(|(_, single)| *single)
                .map(|(name, _)| *name)
                .collect();
            match single_literal.as_slice() {
                [one] => *one,
                _ => {
                    return Err(format!(
                        "kind {kind} is claimed by multiple interfaces with no unique resolution: {interface_names:?}"
                    )
                    .into())
                }
            }
        };

        let admitted = admissions.get(kind.as_str()).copied().unwrap_or(&[]);
        let children: &[ChildVisit] = child_table.get(&kind).map(Vec::as_slice).unwrap_or(&[]);
        let merged = collect_interface_fields(interface_name, interfaces, &mut Vec::new())?;
        let mut fields = Vec::new();
        for mut field in merged {
            // forEachChildTable-backed children ride along unconditionally:
            // their TYPE comes from the table (Node vs NodeArray, in
            // build_node_schema), but their OPTIONALITY is d.ts truth the
            // table cannot see (schema-audit compares it).
            let is_child = children.iter().any(|child| child.name == field.name);
            if !is_child && !admitted.contains(&field.name.as_str()) {
                continue;
            }
            field.type_text = resolve_alias_type(&field.type_text, aliases);
            if !is_child && rust_field_type(&field.type_text) == RustFieldType::Payload {
                return Err(format!(
                    "admitted field {kind}.{} has unmappable type `{}`",
                    field.name, field.type_text
                )
                .into());
            }
            fields.push(field);
        }
        let missing: Vec<&&str> = admitted
            .iter()
            .filter(|name| fields.iter().all(|field| field.name != **name))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "admitted fields for {kind} not found in typescript.d.ts: {missing:?}"
            )
            .into());
        }
        nodes.insert(kind, fields);
    }
    Ok(nodes)
}

/// Single-line `type Name = ...;` aliases from the d.ts, for resolving
/// alias-named scalar field types (PrefixUnaryOperator and friends) to
/// their SyntaxKind unions. Multi-line aliases stay unresolved.
fn parse_dts_type_aliases(dts: &str) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for line in dts.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("type ") else {
            continue;
        };
        let Some((name, rhs)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let Some(rhs) = rhs.trim().strip_suffix(';') else {
            continue;
        };
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            aliases.insert(name.to_owned(), rhs.trim().to_owned());
        }
    }
    aliases
}

fn resolve_alias_type(type_text: &str, aliases: &BTreeMap<String, String>) -> String {
    let bare = type_text
        .trim()
        .trim_start_matches("undefined |")
        .trim()
        .trim_end_matches("| undefined")
        .trim();
    match aliases.get(bare) {
        // Optionality was computed from the original text; the alias RHS
        // only needs to carry the payload category.
        Some(rhs) => rhs.clone(),
        None => type_text.to_owned(),
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
        let ty = if dts_field.name == "comment"
            && dts_field.type_text.contains("string")
            && dts_field.type_text.contains("NodeArray<JSDocComment>")
        {
            RustFieldType::JSDocComment
        } else if let Some(child) = child {
            match child.kind {
                ChildKind::Node => RustFieldType::Node,
                ChildKind::Nodes => RustFieldType::NodeArray,
            }
        } else {
            rust_field_type(&dts_field.type_text)
        };
        let optional = dts_field.optional;
        // JSDocParser creates JSDocNamepathType(undefined) while recovering
        // even though the public d.ts declares `type` as required. Preserve
        // the honest schema bit and model the observable runtime shape in
        // Rust, just as all forEachChild-backed children are Option-typed.
        let rust_optional = optional
            || child.is_some()
            || (kind_name == "JSDocNamepathType" && dts_field.name == "type");
        fields.push(SchemaField {
            rust_name: rust_field_name(&dts_field.name),
            ts_name: dts_field.name,
            ty,
            optional,
            child: child.is_some(),
            rust_optional,
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
                rust_optional: true,
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
    if type_text.contains("string") && type_text.contains("NodeArray<JSDocComment>") {
        RustFieldType::JSDocComment
    } else if type_text.contains("NodeArray<") {
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
    writeln!(out, "pub enum JSDocComment {{")?;
    writeln!(out, "    Text(String),")?;
    writeln!(out, "    Nodes(NodeArrayId),")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl JSDocComment {{")?;
    writeln!(
        out,
        "    pub fn nodes(&self) -> Option<NodeArrayId> {{ match self {{ Self::Nodes(nodes) => Some(*nodes), Self::Text(_) => None }} }}"
    )?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]")?;
    writeln!(out, "pub struct NodeArray {{")?;
    writeln!(out, "    pub nodes: Vec<NodeId>,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub has_trailing_comma: bool,")?;
    writeln!(out, "    /// tsc createMissingList's isMissingList marker.")?;
    writeln!(out, "    pub is_missing_list: bool,")?;
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
    writeln!(
        out,
        "    /// tsc NumericLiteral.numericLiteralFlags; zero on every other node kind."
    )?;
    writeln!(out, "    pub numeric_literal_flags: i32,")?;
    writeln!(
        out,
        "    /// Parser-owned TemplateLiteralLikeNode.templateFlags; zero on other node kinds."
    )?;
    writeln!(out, "    pub template_flags: i32,")?;
    writeln!(
        out,
        "    /// tsc's internal Array/Object/Block.multiLine parser bit."
    )?;
    writeln!(out, "    pub multi_line: Option<bool>,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub parent: Option<NodeId>,")?;
    writeln!(
        out,
        "    /// tsc's internal Node.jsDoc attachment; not an ordinary forEachChild edge."
    )?;
    writeln!(out, "    pub js_doc: Option<NodeArrayId>,")?;
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
                if is_js_string_text(&schema.kind_name, field) {
                    "tsc_types::JsString".to_owned()
                } else {
                    render_field_type(field)
                }
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
    writeln!(out)?;
    writeln!(out, "    pub fn missing(kind: SyntaxKind) -> Self {{")?;
    writeln!(out, "        match kind {{")?;
    for schema in schemas {
        writeln!(
            out,
            "            SyntaxKind::{} => Self::{}({} {{",
            schema.kind_name, schema.kind_name, schema.data_name
        )?;
        for field in &schema.fields {
            writeln!(
                out,
                "                {}: {},",
                field.rust_name,
                render_missing_field_value(field, &schema.kind_name)
            )?;
        }
        writeln!(out, "            }}),")?;
    }
    writeln!(out, "            _ => Self::Token,")?;
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
    writeln!(out, "#[path = \"../tests/unit/nodes/tests.rs\"]")?;
    writeln!(out, "mod tests;")?;

    Ok(out)
}

/// Rust-side optionality. Scalars follow the d.ts contract, but node/array
/// CHILDREN are always Option-typed regardless of it — the layout every
/// parser/binder/checker site is written against, and `NodeData::missing`
/// fills None for every child where tsc materializes per-kind missing
/// tokens. The schema JSON carries the honest d.ts `optional` (schema-audit
/// cross-checks it against the vendored tsc); flipping d.ts-required
/// children to bare NodeId needs the recovery-guarantee census first and
/// is tracked pre-M7 debt (m1-review-2026-07-22.md #8).
fn rust_optional(field: &SchemaField) -> bool {
    field.rust_optional
}

fn render_missing_field_value(field: &SchemaField, kind_name: &str) -> String {
    if rust_optional(field) {
        return "None".to_owned();
    }

    if is_js_string_text(kind_name, field) {
        return "tsc_types::JsString::new()".to_owned();
    }

    match field.ty {
        RustFieldType::Node => "NodeId::default()".to_owned(),
        RustFieldType::NodeArray => "NodeArrayId::default()".to_owned(),
        RustFieldType::Bool => "false".to_owned(),
        RustFieldType::String => "String::new()".to_owned(),
        RustFieldType::Number => "0.0".to_owned(),
        RustFieldType::SyntaxKind => format!("SyntaxKind::{kind_name}"),
        RustFieldType::JSDocComment => "JSDocComment::Text(String::new())".to_owned(),
        RustFieldType::Payload => "NodePayload::String(String::new())".to_owned(),
    }
}

/// Rust owns arbitrary UTF-16 values on these literal fields. The d.ts/schema
/// category remains `string`; identifiers, numeric text and raw source stay UTF-8.
fn is_js_string_text(kind_name: &str, field: &SchemaField) -> bool {
    field.ts_name == "text"
        && matches!(
            kind_name,
            "StringLiteral"
                | "NoSubstitutionTemplateLiteral"
                | "TemplateHead"
                | "TemplateMiddle"
                | "TemplateTail"
        )
}

fn render_field_type(field: &SchemaField) -> String {
    let base = match field.ty {
        RustFieldType::Node => "NodeId",
        RustFieldType::NodeArray => "NodeArrayId",
        RustFieldType::Bool => "bool",
        RustFieldType::String => "String",
        RustFieldType::Number => "f64",
        RustFieldType::SyntaxKind => "SyntaxKind",
        RustFieldType::JSDocComment => "JSDocComment",
        RustFieldType::Payload => "NodePayload",
    };
    if rust_optional(field) {
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
        "use crate::nodes::{{JSDocComment, Node, NodeArray, NodeArrayId, NodeData, NodeId}};"
    )?;
    writeln!(out, "use crate::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(out, "pub trait NodeLookup {{")?;
    writeln!(out, "    fn node(&self, id: NodeId) -> &Node;")?;
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
            if matches!(
                schema.kind_name.as_str(),
                "JSDocParameterTag" | "JSDocPropertyTag"
            ) {
                writeln!(
                    out,
                    "            if let Some(result) = visit_optional_node(data.tag_name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            if data.is_name_first {{")?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.type_expression, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            }} else {{")?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.type_expression, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            }}")?;
                writeln!(
                    out,
                    "            if let Some(result) = visit_optional_jsdoc_comment(lookup, data.comment.as_ref(), &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            None")?;
                writeln!(out, "        }}")?;
                continue;
            }
            if schema.kind_name == "JSDocTypedefTag" {
                writeln!(
                    out,
                    "            if let Some(result) = visit_optional_node(data.tag_name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(
                    out,
                    "            let type_expression_first = data.type_expression.is_some_and(|node| lookup.node(node).kind == SyntaxKind::JSDocTypeExpression);"
                )?;
                writeln!(out, "            if type_expression_first {{")?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.type_expression, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.full_name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            }} else {{")?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.full_name, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(
                    out,
                    "                if let Some(result) = visit_optional_node(data.type_expression, &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            }}")?;
                writeln!(
                    out,
                    "            if let Some(result) = visit_optional_jsdoc_comment(lookup, data.comment.as_ref(), &mut cb) {{ return Some(result); }}"
                )?;
                writeln!(out, "            None")?;
                writeln!(out, "        }}")?;
                continue;
            }
            for child in &schema.children {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| field.ts_name == child.name)
                    .ok_or_else(|| format!("missing generated field for child {}", child.name))?;
                if field.ty == RustFieldType::JSDocComment {
                    writeln!(
                        out,
                        "            if let Some(result) = visit_optional_jsdoc_comment(lookup, data.{}.as_ref(), &mut cb) {{ return Some(result); }}",
                        field.rust_name
                    )?;
                    continue;
                }
                let helper = match (child.kind, rust_optional(field)) {
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
        "pub fn for_each_child_array<F>(node: &Node, mut cb: F) -> Option<NodeArrayId>"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    F: FnMut(NodeArrayId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match &node.data {{")?;
    writeln!(out, "        NodeData::Token => None,")?;
    for schema in schemas {
        let array_children = schema
            .children
            .iter()
            .filter_map(|child| {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| field.ts_name == child.name)?;
                matches!(
                    field.ty,
                    RustFieldType::NodeArray | RustFieldType::JSDocComment
                )
                .then_some(field)
            })
            .collect::<Vec<_>>();
        if array_children.is_empty() {
            writeln!(
                out,
                "        NodeData::{}(_data) => None,",
                schema.kind_name
            )?;
            continue;
        }
        writeln!(out, "        NodeData::{}(data) => {{", schema.kind_name)?;
        for field in array_children {
            match (field.ty, rust_optional(field)) {
                (RustFieldType::NodeArray, true) => writeln!(
                    out,
                    "            if let Some(id) = data.{} {{ if cb(id) {{ return Some(id); }} }}",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, false) => writeln!(
                    out,
                    "            if cb(data.{}) {{ return Some(data.{}); }}",
                    field.rust_name, field.rust_name
                )?,
                (RustFieldType::JSDocComment, true) => writeln!(
                    out,
                    "            if let Some(JSDocComment::Nodes(id)) = data.{}.as_ref() {{ if cb(*id) {{ return Some(*id); }} }}",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, false) => writeln!(
                    out,
                    "            if let JSDocComment::Nodes(id) = &data.{} {{ if cb(*id) {{ return Some(*id); }} }}",
                    field.rust_name
                )?,
                _ => unreachable!("child-array rendering received a non-array field"),
            }
        }
        writeln!(out, "            None")?;
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "/// Fallible, replacement-aware counterpart to `for_each_child` used by"
    )?;
    writeln!(
        out,
        "/// emit-session transforms. Parsed syntax remains immutable; callers map a"
    )?;
    writeln!(
        out,
        "/// cloned `NodeData` value and install it through their own node factory."
    )?;
    writeln!(out, "pub trait NodeDataChildVisitor {{")?;
    writeln!(out, "    type Error;")?;
    writeln!(out)?;
    writeln!(out, "    fn node_kind(&self, id: NodeId) -> SyntaxKind;")?;
    writeln!(
        out,
        "    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error>;"
    )?;
    writeln!(out, "    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error>;")?;
    writeln!(out, "    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error;")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "pub fn try_visit_each_child<V>(data: &mut NodeData, visitor: &mut V) -> Result<(), V::Error>")?;
    writeln!(out, "where")?;
    writeln!(out, "    V: NodeDataChildVisitor,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match data {{")?;
    writeln!(out, "        NodeData::Token => Ok(()),")?;
    for schema in schemas {
        if schema.children.is_empty() {
            writeln!(
                out,
                "        NodeData::{}(_data) => Ok(()),",
                schema.kind_name
            )?;
            continue;
        }
        writeln!(out, "        NodeData::{}(data) => {{", schema.kind_name)?;
        if matches!(
            schema.kind_name.as_str(),
            "JSDocParameterTag" | "JSDocPropertyTag"
        ) {
            writeln!(
                out,
                "            map_optional_node(&mut data.tag_name, visitor)?;"
            )?;
            writeln!(out, "            if data.is_name_first {{")?;
            writeln!(
                out,
                "                map_optional_node(&mut data.name, visitor)?;"
            )?;
            writeln!(
                out,
                "                map_optional_node(&mut data.type_expression, visitor)?;"
            )?;
            writeln!(out, "            }} else {{")?;
            writeln!(
                out,
                "                map_optional_node(&mut data.type_expression, visitor)?;"
            )?;
            writeln!(
                out,
                "                map_optional_node(&mut data.name, visitor)?;"
            )?;
            writeln!(out, "            }}")?;
            writeln!(
                out,
                "            map_optional_jsdoc_comment(&mut data.comment, visitor)?;"
            )?;
            writeln!(out, "            Ok(())")?;
            writeln!(out, "        }}")?;
            continue;
        }
        if schema.kind_name == "JSDocTypedefTag" {
            writeln!(
                out,
                "            map_optional_node(&mut data.tag_name, visitor)?;"
            )?;
            writeln!(out, "            let type_expression_first = data.type_expression.is_some_and(|node| visitor.node_kind(node) == SyntaxKind::JSDocTypeExpression);")?;
            writeln!(out, "            if type_expression_first {{")?;
            writeln!(
                out,
                "                map_optional_node(&mut data.type_expression, visitor)?;"
            )?;
            writeln!(
                out,
                "                map_optional_node(&mut data.full_name, visitor)?;"
            )?;
            writeln!(out, "            }} else {{")?;
            writeln!(
                out,
                "                map_optional_node(&mut data.full_name, visitor)?;"
            )?;
            writeln!(
                out,
                "                map_optional_node(&mut data.type_expression, visitor)?;"
            )?;
            writeln!(out, "            }}")?;
            writeln!(
                out,
                "            map_optional_jsdoc_comment(&mut data.comment, visitor)?;"
            )?;
            writeln!(out, "            Ok(())")?;
            writeln!(out, "        }}")?;
            continue;
        }
        for child in &schema.children {
            let field = schema
                .fields
                .iter()
                .find(|field| field.ts_name == child.name)
                .ok_or_else(|| format!("missing generated field for child {}", child.name))?;
            if field.ty == RustFieldType::JSDocComment {
                if rust_optional(field) {
                    writeln!(
                        out,
                        "            map_optional_jsdoc_comment(&mut data.{}, visitor)?;",
                        field.rust_name
                    )?;
                } else {
                    writeln!(
                        out,
                        "            map_jsdoc_comment(&mut data.{}, SyntaxKind::{}, \"{}\", visitor)?;",
                        field.rust_name, schema.kind_name, field.rust_name
                    )?;
                }
                continue;
            }
            let helper = match (child.kind, rust_optional(field)) {
                (ChildKind::Node, true) => "map_optional_node",
                (ChildKind::Nodes, true) => "map_optional_nodes",
                (ChildKind::Node, false) => "map_required_node",
                (ChildKind::Nodes, false) => "map_required_nodes",
            };
            if rust_optional(field) {
                writeln!(
                    out,
                    "            {}(&mut data.{}, visitor)?;",
                    helper, field.rust_name
                )?;
            } else {
                writeln!(
                    out,
                    "            {}(&mut data.{}, SyntaxKind::{}, \"{}\", visitor)?;",
                    helper, field.rust_name, schema.kind_name, field.rust_name
                )?;
            }
        }
        writeln!(out, "            Ok(())")?;
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn map_optional_node<V: NodeDataChildVisitor>(slot: &mut Option<NodeId>, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(
        out,
        "    if let Some(id) = *slot {{ *slot = visitor.visit_node(id)?; }}"
    )?;
    writeln!(out, "    Ok(())")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn map_optional_nodes<V: NodeDataChildVisitor>(slot: &mut Option<NodeArrayId>, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(
        out,
        "    if let Some(id) = *slot {{ *slot = visitor.visit_nodes(id)?; }}"
    )?;
    writeln!(out, "    Ok(())")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[allow(dead_code)]")?;
    writeln!(out, "fn map_required_node<V: NodeDataChildVisitor>(slot: &mut NodeId, parent: SyntaxKind, field: &'static str, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(out, "    *slot = visitor.visit_node(*slot)?.ok_or_else(|| visitor.required_child_removed(parent, field))?;")?;
    writeln!(out, "    Ok(())")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[allow(dead_code)]")?;
    writeln!(out, "fn map_required_nodes<V: NodeDataChildVisitor>(slot: &mut NodeArrayId, parent: SyntaxKind, field: &'static str, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(out, "    *slot = visitor.visit_nodes(*slot)?.ok_or_else(|| visitor.required_child_removed(parent, field))?;")?;
    writeln!(out, "    Ok(())")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn map_optional_jsdoc_comment<V: NodeDataChildVisitor>(slot: &mut Option<JSDocComment>, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(
        out,
        "    let Some(JSDocComment::Nodes(id)) = slot.as_ref() else {{ return Ok(()); }};"
    )?;
    writeln!(
        out,
        "    *slot = visitor.visit_nodes(*id)?.map(JSDocComment::Nodes);"
    )?;
    writeln!(out, "    Ok(())")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[allow(dead_code)]")?;
    writeln!(out, "fn map_jsdoc_comment<V: NodeDataChildVisitor>(slot: &mut JSDocComment, parent: SyntaxKind, field: &'static str, visitor: &mut V) -> Result<(), V::Error> {{")?;
    writeln!(out, "    if let JSDocComment::Nodes(id) = slot {{")?;
    writeln!(out, "        *id = visitor.visit_nodes(*id)?.ok_or_else(|| visitor.required_child_removed(parent, field))?;")?;
    writeln!(out, "    }}")?;
    writeln!(out, "    Ok(())")?;
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
    writeln!(out, "fn visit_optional_jsdoc_comment<L, F>(lookup: &L, comment: Option<&JSDocComment>, cb: &mut F) -> Option<NodeId>")?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{")?;
    writeln!(out, "    match comment {{")?;
    writeln!(
        out,
        "        Some(JSDocComment::Nodes(nodes)) => visit_nodes(lookup, *nodes, cb),"
    )?;
    writeln!(out, "        _ => None,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn visit_optional_nodes<L, F>(lookup: &L, id: Option<NodeArrayId>, cb: &mut F) -> Option<NodeId>")?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_nodes(lookup, id, cb)) }}")?;
    Ok(out)
}

/// Generate the complete mutable identity walk from the same field schema as
/// `NodeData`. Unlike `forEachChild`, relocation owns every ID-bearing field,
/// including compatibility fields which are not ordinary syntax children.
fn render_relocate_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use crate::arena::SyntaxIdentityRelocation;")?;
    writeln!(
        out,
        "use crate::nodes::{{JSDocComment, NodeArrayId, NodeData, NodeId}};"
    )?;
    writeln!(out, "use tsc_types::IdentityError;")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub(crate) fn relocate_node_data(data: &mut NodeData, relocation: &SyntaxIdentityRelocation) -> Result<(), IdentityError> {{"
    )?;
    writeln!(out, "    match data {{")?;
    writeln!(out, "        NodeData::Token => Ok(()),")?;
    for schema in schemas {
        let identity_fields = schema
            .fields
            .iter()
            .filter(|field| {
                matches!(
                    field.ty,
                    RustFieldType::Node | RustFieldType::NodeArray | RustFieldType::JSDocComment
                )
            })
            .collect::<Vec<_>>();
        let binding = if identity_fields.is_empty() {
            "_data"
        } else {
            "data"
        };
        writeln!(
            out,
            "        NodeData::{}({binding}) => {{",
            schema.kind_name
        )?;
        for field in identity_fields {
            let optional = rust_optional(field);
            match (field.ty, optional) {
                (RustFieldType::Node, true) => writeln!(
                    out,
                    "            if let Some(id) = &mut data.{} {{ relocation.node(id)?; }}",
                    field.rust_name
                )?,
                (RustFieldType::Node, false) => writeln!(
                    out,
                    "            relocation.node(&mut data.{})?;",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, true) => writeln!(
                    out,
                    "            if let Some(id) = &mut data.{} {{ relocation.node_array(id)?; }}",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, false) => writeln!(
                    out,
                    "            relocation.node_array(&mut data.{})?;",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, true) => writeln!(
                    out,
                    "            if let Some(JSDocComment::Nodes(id)) = &mut data.{} {{ relocation.node_array(id)?; }}",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, false) => writeln!(
                    out,
                    "            if let JSDocComment::Nodes(id) = &mut data.{} {{ relocation.node_array(id)?; }}",
                    field.rust_name
                )?,
                _ => unreachable!("identity field filter and rendering disagree"),
            }
        }
        writeln!(out, "            Ok(())")?;
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub(crate) fn collect_node_data_ids(data: &NodeData, nodes: &mut Vec<NodeId>, arrays: &mut Vec<NodeArrayId>) {{"
    )?;
    writeln!(out, "    match data {{")?;
    writeln!(out, "        NodeData::Token => {{}}")?;
    for schema in schemas {
        let identity_fields = schema
            .fields
            .iter()
            .filter(|field| {
                matches!(
                    field.ty,
                    RustFieldType::Node | RustFieldType::NodeArray | RustFieldType::JSDocComment
                )
            })
            .collect::<Vec<_>>();
        let binding = if identity_fields.is_empty() {
            "_data"
        } else {
            "data"
        };
        writeln!(
            out,
            "        NodeData::{}({binding}) => {{",
            schema.kind_name
        )?;
        for field in identity_fields {
            match (field.ty, rust_optional(field)) {
                (RustFieldType::Node, true) => writeln!(
                    out,
                    "            if let Some(id) = data.{} {{ nodes.push(id); }}",
                    field.rust_name
                )?,
                (RustFieldType::Node, false) => writeln!(
                    out,
                    "            nodes.push(data.{});",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, true) => writeln!(
                    out,
                    "            if let Some(id) = data.{} {{ arrays.push(id); }}",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, false) => writeln!(
                    out,
                    "            arrays.push(data.{});",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, true) => writeln!(
                    out,
                    "            if let Some(JSDocComment::Nodes(id)) = data.{}.as_ref() {{ arrays.push(*id); }}",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, false) => writeln!(
                    out,
                    "            if let JSDocComment::Nodes(id) = &data.{} {{ arrays.push(*id); }}",
                    field.rust_name
                )?,
                _ => unreachable!("identity field filter and collection disagree"),
            }
        }
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub(crate) fn remap_node_data_ids<N, A>(data: &mut NodeData, mut node: N, mut array: A)"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    N: FnMut(NodeId) -> NodeId,")?;
    writeln!(out, "    A: FnMut(NodeArrayId) -> NodeArrayId,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match data {{")?;
    writeln!(out, "        NodeData::Token => {{}}")?;
    for schema in schemas {
        let identity_fields = schema
            .fields
            .iter()
            .filter(|field| {
                matches!(
                    field.ty,
                    RustFieldType::Node | RustFieldType::NodeArray | RustFieldType::JSDocComment
                )
            })
            .collect::<Vec<_>>();
        let binding = if identity_fields.is_empty() {
            "_data"
        } else {
            "data"
        };
        writeln!(
            out,
            "        NodeData::{}({binding}) => {{",
            schema.kind_name
        )?;
        for field in identity_fields {
            match (field.ty, rust_optional(field)) {
                (RustFieldType::Node, true) => writeln!(
                    out,
                    "            if let Some(id) = &mut data.{} {{ *id = node(*id); }}",
                    field.rust_name
                )?,
                (RustFieldType::Node, false) => writeln!(
                    out,
                    "            data.{} = node(data.{});",
                    field.rust_name, field.rust_name
                )?,
                (RustFieldType::NodeArray, true) => writeln!(
                    out,
                    "            if let Some(id) = &mut data.{} {{ *id = array(*id); }}",
                    field.rust_name
                )?,
                (RustFieldType::NodeArray, false) => writeln!(
                    out,
                    "            data.{} = array(data.{});",
                    field.rust_name, field.rust_name
                )?,
                (RustFieldType::JSDocComment, true) => writeln!(
                    out,
                    "            if let Some(JSDocComment::Nodes(id)) = &mut data.{} {{ *id = array(*id); }}",
                    field.rust_name
                )?,
                (RustFieldType::JSDocComment, false) => writeln!(
                    out,
                    "            if let JSDocComment::Nodes(id) = &mut data.{} {{ *id = array(*id); }}",
                    field.rust_name
                )?,
                _ => unreachable!("identity field filter and remapping disagree"),
            }
        }
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn optional_node_equal<N>(left: Option<NodeId>, right: Option<NodeId>, node: &mut N) -> bool"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    N: FnMut(NodeId, NodeId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match (left, right) {{")?;
    writeln!(
        out,
        "        (Some(left), Some(right)) => node(left, right),"
    )?;
    writeln!(out, "        (None, None) => true,")?;
    writeln!(out, "        _ => false,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn optional_array_equal<A>(left: Option<NodeArrayId>, right: Option<NodeArrayId>, array: &mut A) -> bool"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    A: FnMut(NodeArrayId, NodeArrayId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match (left, right) {{")?;
    writeln!(
        out,
        "        (Some(left), Some(right)) => array(left, right),"
    )?;
    writeln!(out, "        (None, None) => true,")?;
    writeln!(out, "        _ => false,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn jsdoc_comment_equal<A>(left: &JSDocComment, right: &JSDocComment, array: &mut A) -> bool"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    A: FnMut(NodeArrayId, NodeArrayId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match (left, right) {{")?;
    writeln!(
        out,
        "        (JSDocComment::Text(left), JSDocComment::Text(right)) => left == right,"
    )?;
    writeln!(
        out,
        "        (JSDocComment::Nodes(left), JSDocComment::Nodes(right)) => array(*left, *right),"
    )?;
    writeln!(out, "        _ => false,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn optional_jsdoc_comment_equal<A>(left: Option<&JSDocComment>, right: Option<&JSDocComment>, array: &mut A) -> bool"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    A: FnMut(NodeArrayId, NodeArrayId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match (left, right) {{")?;
    writeln!(
        out,
        "        (Some(left), Some(right)) => jsdoc_comment_equal(left, right, array),"
    )?;
    writeln!(out, "        (None, None) => true,")?;
    writeln!(out, "        _ => false,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub(crate) fn node_data_structurally_equal<N, A>(left: &NodeData, right: &NodeData, mut node: N, mut array: A) -> bool"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    N: FnMut(NodeId, NodeId) -> bool,")?;
    writeln!(out, "    A: FnMut(NodeArrayId, NodeArrayId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match (left, right) {{")?;
    writeln!(out, "        (NodeData::Token, NodeData::Token) => true,")?;
    for schema in schemas {
        let binding = if schema.fields.is_empty() {
            "_"
        } else {
            "left"
        };
        let other_binding = if schema.fields.is_empty() {
            "_"
        } else {
            "right"
        };
        writeln!(
            out,
            "        (NodeData::{}({binding}), NodeData::{}({other_binding})) => {{",
            schema.kind_name, schema.kind_name,
        )?;
        if schema.fields.is_empty() {
            writeln!(out, "            true")?;
        }
        for (index, field) in schema.fields.iter().enumerate() {
            let expression = match (field.ty, rust_optional(field)) {
                (RustFieldType::Node, true) => format!(
                    "optional_node_equal(left.{0}, right.{0}, &mut node)",
                    field.rust_name
                ),
                (RustFieldType::Node, false) => {
                    format!("node(left.{0}, right.{0})", field.rust_name)
                }
                (RustFieldType::NodeArray, true) => format!(
                    "optional_array_equal(left.{0}, right.{0}, &mut array)",
                    field.rust_name
                ),
                (RustFieldType::NodeArray, false) => {
                    format!("array(left.{0}, right.{0})", field.rust_name)
                }
                (RustFieldType::JSDocComment, true) => format!(
                    "optional_jsdoc_comment_equal(left.{0}.as_ref(), right.{0}.as_ref(), &mut array)",
                    field.rust_name
                ),
                (RustFieldType::JSDocComment, false) => format!(
                    "jsdoc_comment_equal(&left.{0}, &right.{0}, &mut array)",
                    field.rust_name
                ),
                _ => format!("left.{0} == right.{0}", field.rust_name),
            };
            if index == 0 {
                writeln!(out, "            {expression}")?;
            } else {
                writeln!(out, "                && {expression}")?;
            }
        }
        writeln!(out, "        }}")?;
    }
    writeln!(out, "        _ => false,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    Ok(out)
}

/// Generate the runtime field view used by exact AST oracles.
///
/// `forEachChild` is deliberately not a field reflection API: tsc stores
/// compatibility fields that it does not visit, and JSDoc comment payloads
/// may be either strings or node arrays.  Keeping this table generated from
/// the same schema as `NodeData` prevents inspection tools from growing a
/// second, hand-maintained node model.
fn render_observable_fields_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    let has_payload = schemas
        .iter()
        .flat_map(|schema| &schema.fields)
        .any(|field| field.ty == RustFieldType::Payload);
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    if has_payload {
        writeln!(
            out,
            "use crate::nodes::{{JSDocComment, Node, NodeArrayId, NodeData, NodeId, NodePayload}};"
        )?;
    } else {
        writeln!(
            out,
            "use crate::nodes::{{JSDocComment, Node, NodeArrayId, NodeData, NodeId}};"
        )?;
    }
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]")?;
    writeln!(out, "pub enum ObservableField<'a> {{")?;
    writeln!(out, "    Node(NodeId),")?;
    writeln!(out, "    NodeArray(NodeArrayId),")?;
    writeln!(out, "    Bool(bool),")?;
    writeln!(out, "    String(&'a str),")?;
    writeln!(out, "    JsString(tsc_types::JsStr<'a>),")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub fn for_each_observable_field<'a, F>(node: &'a Node, mut cb: F)"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    F: FnMut(&'static str, ObservableField<'a>),")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match &node.data {{")?;
    writeln!(out, "        NodeData::Token => {{}}")?;

    for schema in schemas {
        let mut fields = schema
            .fields
            .iter()
            .filter(|field| {
                !(matches!(field.ty, RustFieldType::Number | RustFieldType::SyntaxKind)
                    || field.ts_name == "text"
                        && matches!(
                            schema.kind_name.as_str(),
                            "Identifier" | "PrivateIdentifier"
                        ))
            })
            .collect::<Vec<_>>();
        fields.sort_by(|left, right| left.ts_name.cmp(&right.ts_name));
        let binding = if fields.is_empty() { "_data" } else { "data" };
        writeln!(
            out,
            "        NodeData::{}({binding}) => {{",
            schema.kind_name
        )?;
        for field in fields {
            match (field.ty, rust_optional(field)) {
                (RustFieldType::Node, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{} {{ cb({:?}, ObservableField::Node(value)); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::Node, false) => {
                    writeln!(
                        out,
                        "            cb({:?}, ObservableField::Node(data.{}));",
                        field.ts_name, field.rust_name
                    )?;
                }
                (RustFieldType::NodeArray, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{} {{ cb({:?}, ObservableField::NodeArray(value)); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::NodeArray, false) => {
                    writeln!(
                        out,
                        "            cb({:?}, ObservableField::NodeArray(data.{}));",
                        field.ts_name, field.rust_name
                    )?;
                }
                (RustFieldType::Bool, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{} {{ cb({:?}, ObservableField::Bool(value)); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::Bool, false) => {
                    writeln!(
                        out,
                        "            cb({:?}, ObservableField::Bool(data.{}));",
                        field.ts_name, field.rust_name
                    )?;
                }
                (RustFieldType::String, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{}.as_deref() {{ cb({:?}, ObservableField::String(value)); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::String, false) => {
                    if is_js_string_text(&schema.kind_name, field) {
                        writeln!(
                            out,
                            "            cb({:?}, ObservableField::JsString(data.{}.as_js()));",
                            field.ts_name, field.rust_name
                        )?;
                    } else {
                        writeln!(
                            out,
                            "            cb({:?}, ObservableField::String(&data.{}));",
                            field.ts_name, field.rust_name
                        )?;
                    }
                }
                (RustFieldType::JSDocComment, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{}.as_ref() {{ emit_jsdoc_comment({:?}, value, &mut cb); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::JSDocComment, false) => {
                    writeln!(
                        out,
                        "            emit_jsdoc_comment({:?}, &data.{}, &mut cb);",
                        field.ts_name, field.rust_name
                    )?;
                }
                (RustFieldType::Payload, true) => {
                    writeln!(
                        out,
                        "            if let Some(value) = data.{}.as_ref() {{ emit_payload({:?}, value, &mut cb); }}",
                        field.rust_name, field.ts_name
                    )?;
                }
                (RustFieldType::Payload, false) => {
                    writeln!(
                        out,
                        "            emit_payload({:?}, &data.{}, &mut cb);",
                        field.ts_name, field.rust_name
                    )?;
                }
                // The TypeScript oracle intentionally records only the
                // string/boolean/node/node-array surface. Numeric and
                // SyntaxKind-valued fields are outside that contract.
                (RustFieldType::Number | RustFieldType::SyntaxKind, _) => {}
            }
        }
        writeln!(out, "        }}")?;
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn emit_jsdoc_comment<'a, F>(name: &'static str, value: &'a JSDocComment, cb: &mut F)"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    F: FnMut(&'static str, ObservableField<'a>),")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match value {{")?;
    writeln!(
        out,
        "        JSDocComment::Text(text) => cb(name, ObservableField::String(text)),"
    )?;
    writeln!(
        out,
        "        JSDocComment::Nodes(nodes) => cb(name, ObservableField::NodeArray(*nodes)),"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    if has_payload {
        writeln!(out)?;
        writeln!(
            out,
            "fn emit_payload<'a, F>(name: &'static str, value: &'a NodePayload, cb: &mut F)"
        )?;
        writeln!(out, "where")?;
        writeln!(out, "    F: FnMut(&'static str, ObservableField<'a>),")?;
        writeln!(out, "{{")?;
        writeln!(out, "    match value {{")?;
        writeln!(
            out,
            "        NodePayload::Bool(value) => cb(name, ObservableField::Bool(*value)),"
        )?;
        writeln!(
            out,
            "        NodePayload::String(value) => cb(name, ObservableField::String(value)),"
        )?;
        writeln!(
            out,
            "        NodePayload::Number(_) | NodePayload::Kind(_) => {{}}"
        )?;
        writeln!(out, "    }}")?;
        writeln!(out, "}}")?;
    }
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
