//! m2-binder-steps.md stage 3.0: the Rust side of the symbol spot-audit.
//!
//! THE WALK CONTRACT is mirrored from crates/oracle/symbol-dump.mjs —
//! change BOTH sides together:
//!  - top-level statements contribute their declaration names:
//!    function/class/interface/type-alias/enum/module names, variable
//!    statement binding names (recursing through binding patterns),
//!    import-equals name, import clause default/namespace/named names,
//!    export clause namespace/named names;
//!  - one nesting level: class/interface member names and enum member
//!    names (skipping computed names), and for modules the dotted name
//!    chain plus the final ModuleBlock's statements (names only, no
//!    deeper member walk);
//!  - name nodes count only when their kind is Identifier, StringLiteral,
//!    NumericLiteral, or PrivateIdentifier.
//!
//! Line format (positions are the NAME node's [pos, end) in UTF-16):
//!   pos \t end \t escapedName \t flags \t decls \t members \t exports
//! Unresolved names emit "pos \t end \t <no-symbol>".

use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};

/// One audited file of a program, aligned with program.json files order.
pub struct FileAudit {
    pub name: String,
    pub parse_errors: usize,
    pub lines: Vec<String>,
}

pub fn audit_source_file(source: &SourceFile, binder: &tsrs2_binder::Binder<'_>) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(data) = source.arena.node(source.root).data.as_source_file() {
        if let Some(statements) = data.statements {
            for &statement in &source.arena.node_array(statements).nodes {
                visit_statement(source, statement, 0, &mut names);
            }
        }
    }
    let map = tsrs2_diags::compute_line_map(&source.text);
    let to_utf16 = |pos: u32| map.byte_to_utf16.get(pos as usize).copied().unwrap_or(pos);
    names
        .iter()
        .map(|&name| {
            let node = source.arena.node(name);
            let (pos, end) = (to_utf16(node.pos), to_utf16(node.end));
            // Mirror of checker.getSymbolAtLocation on a declaration
            // name: the parent declaration's symbol.
            let symbol = node
                .parent
                .and_then(|parent| binder.node_symbol.get(&parent))
                .copied();
            match symbol {
                None => format!("{pos}\t{end}\t<no-symbol>"),
                Some(symbol) => {
                    let sym = binder.symbols.symbol(symbol);
                    let sorted_keys = |table: &tsrs2_binder::SymbolTable| {
                        let mut keys: Vec<&str> = table.keys().map(String::as_str).collect();
                        keys.sort_unstable();
                        keys.join(",")
                    };
                    format!(
                        "{pos}\t{end}\t{}\t{}\t{}\t{}\t{}",
                        sym.escaped_name,
                        sym.flags.bits(),
                        sym.declarations.len(),
                        sorted_keys(&sym.members),
                        sorted_keys(&sym.exports),
                    )
                }
            }
        })
        .collect()
}

fn is_audit_name_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::PrivateIdentifier
    )
}

fn push_name(source: &SourceFile, name: Option<NodeId>, out: &mut Vec<NodeId>) {
    if let Some(name) = name {
        if is_audit_name_kind(source.arena.node(name).kind) {
            out.push(name);
        }
    }
}

fn push_binding_names(source: &SourceFile, name: Option<NodeId>, out: &mut Vec<NodeId>) {
    let Some(name) = name else { return };
    match &source.arena.node(name).data {
        NodeData::ObjectBindingPattern(pattern) => {
            push_binding_elements(source, pattern.elements, out);
        }
        NodeData::ArrayBindingPattern(pattern) => {
            push_binding_elements(source, pattern.elements, out);
        }
        _ => {
            if source.arena.node(name).kind == SyntaxKind::Identifier {
                out.push(name);
            }
        }
    }
}

fn push_binding_elements(
    source: &SourceFile,
    elements: Option<NodeArrayId>,
    out: &mut Vec<NodeId>,
) {
    let Some(elements) = elements else { return };
    for &element in &source.arena.node_array(elements).nodes {
        if let NodeData::BindingElement(data) = &source.arena.node(element).data {
            push_binding_names(source, data.name, out);
        }
    }
}

fn push_member_names(source: &SourceFile, members: Option<NodeArrayId>, out: &mut Vec<NodeId>) {
    let Some(members) = members else { return };
    for &member in &source.arena.node_array(members).nodes {
        let name = match &source.arena.node(member).data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::PropertySignature(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::MethodSignature(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::EnumMember(data) => data.name,
            _ => None,
        };
        push_name(source, name, out);
    }
}

fn visit_statement(source: &SourceFile, statement: NodeId, depth: u32, out: &mut Vec<NodeId>) {
    match &source.arena.node(statement).data {
        NodeData::FunctionDeclaration(data) => push_name(source, data.name, out),
        NodeData::TypeAliasDeclaration(data) => push_name(source, data.name, out),
        NodeData::ClassDeclaration(data) => {
            push_name(source, data.name, out);
            if depth == 0 {
                push_member_names(source, data.members, out);
            }
        }
        NodeData::InterfaceDeclaration(data) => {
            push_name(source, data.name, out);
            if depth == 0 {
                push_member_names(source, data.members, out);
            }
        }
        NodeData::EnumDeclaration(data) => {
            push_name(source, data.name, out);
            if depth == 0 {
                push_member_names(source, data.members, out);
            }
        }
        NodeData::ModuleDeclaration(_) => {
            // Dotted names parse as nested ModuleDeclarations: emit every
            // segment, then walk the final ModuleBlock one level deep.
            let mut current = Some(statement);
            let mut block = None;
            while let Some(id) = current {
                current = None;
                if let NodeData::ModuleDeclaration(data) = &source.arena.node(id).data {
                    push_name(source, data.name, out);
                    if let Some(body) = data.body {
                        match &source.arena.node(body).data {
                            NodeData::ModuleDeclaration(_) => current = Some(body),
                            NodeData::ModuleBlock(module_block) => block = module_block.statements,
                            _ => {}
                        }
                    }
                }
            }
            if depth == 0 {
                if let Some(statements) = block {
                    for &inner in &source.arena.node_array(statements).nodes {
                        visit_statement(source, inner, 1, out);
                    }
                }
            }
        }
        NodeData::VariableStatement(data) => {
            let Some(list) = data.declaration_list else {
                return;
            };
            let NodeData::VariableDeclarationList(list) = &source.arena.node(list).data else {
                return;
            };
            let Some(declarations) = list.declarations else {
                return;
            };
            for &declaration in &source.arena.node_array(declarations).nodes {
                if let NodeData::VariableDeclaration(data) = &source.arena.node(declaration).data {
                    push_binding_names(source, data.name, out);
                }
            }
        }
        NodeData::ImportEqualsDeclaration(data) => push_name(source, data.name, out),
        NodeData::ImportDeclaration(data) => {
            let Some(clause) = data.import_clause else {
                return;
            };
            let NodeData::ImportClause(clause) = &source.arena.node(clause).data else {
                return;
            };
            push_name(source, clause.name, out);
            let Some(bindings) = clause.named_bindings else {
                return;
            };
            match &source.arena.node(bindings).data {
                NodeData::NamespaceImport(data) => push_name(source, data.name, out),
                NodeData::NamedImports(data) => {
                    let Some(elements) = data.elements else {
                        return;
                    };
                    for &element in &source.arena.node_array(elements).nodes {
                        if let NodeData::ImportSpecifier(data) = &source.arena.node(element).data {
                            push_name(source, data.name, out);
                        }
                    }
                }
                _ => {}
            }
        }
        NodeData::ExportDeclaration(data) => {
            let Some(clause) = data.export_clause else {
                return;
            };
            match &source.arena.node(clause).data {
                NodeData::NamespaceExport(data) => push_name(source, data.name, out),
                NodeData::NamedExports(data) => {
                    let Some(elements) = data.elements else {
                        return;
                    };
                    for &element in &source.arena.node_array(elements).nodes {
                        if let NodeData::ExportSpecifier(data) = &source.arena.node(element).data {
                            push_name(source, data.name, out);
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}
