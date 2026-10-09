//! tsgo `parser/references.go`: the module references tsgo's parser
//! records on a source file — its imports (module specifiers of import and
//! export declarations, `import x = require()`, and import calls, import
//! types and, in JavaScript, `require` calls), its module augmentations and
//! the names of its ambient modules.

use tsc_syntax::{JSDocComment, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{JsString, NodeFlags};

use crate::encoder::{tsgo_possibly_contains_dynamic_import, Property};

/// What tsgo `collectExternalModuleReferences` records on a file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModuleReferences {
    /// tsgo `SourceFile.Imports()`: module specifier literals, in tsgo's order.
    pub imports: Vec<NodeId>,
    /// tsgo `SourceFile.ModuleAugmentations`: augmentation names.
    pub module_augmentations: Vec<NodeId>,
    /// tsgo `SourceFile.AmbientModuleNames`.
    pub ambient_module_names: Vec<JsString>,
}

/// tsgo `collectExternalModuleReferences`.
pub fn collect_external_module_references(file: &SourceFile) -> ModuleReferences {
    let mut references = ModuleReferences::default();
    let javascript = is_javascript(file);
    for &statement in statements(file, file.root) {
        collect_module_references(file, statement, false, &mut references);
    }
    if javascript || tsgo_possibly_contains_dynamic_import(file) {
        for_each_dynamic_import_or_require_call(file, |argument| references.imports.push(argument));
    }
    references
}

fn is_javascript(file: &SourceFile) -> bool {
    NodeFlags::from_bits(file.arena.node(file.root).flags).contains(NodeFlags::JAVA_SCRIPT_FILE)
}

fn statements(file: &SourceFile, id: NodeId) -> &[NodeId] {
    let list = match &file.arena.node(id).data {
        NodeData::SourceFile(data) => data.statements,
        NodeData::ModuleBlock(data) => data.statements,
        _ => None,
    };
    list.map(|list| file.arena.node_array(list).nodes)
        .unwrap_or_default()
}

fn string_literal_text(file: &SourceFile, id: NodeId) -> Option<&JsString> {
    match &file.arena.node(id).data {
        NodeData::StringLiteral(literal) => Some(&literal.text),
        _ => None,
    }
}

/// tsgo `collectModuleReferences`.
fn collect_module_references(
    file: &SourceFile,
    id: NodeId,
    in_ambient_module: bool,
    references: &mut ModuleReferences,
) {
    let node = file.arena.node(id);
    // ast.IsAnyImportOrReExport and ast.GetExternalModuleName.
    let module_name = match &node.data {
        NodeData::ImportDeclaration(import) => Some(import.module_specifier),
        NodeData::ExportDeclaration(export) => Some(export.module_specifier),
        NodeData::ImportEqualsDeclaration(import) => Some(import.module_reference.and_then(
            |reference| match &file.arena.node(reference).data {
                NodeData::ExternalModuleReference(external) => external.expression,
                _ => None,
            },
        )),
        _ => None,
    };
    if let Some(module_name) = module_name {
        if let Some(module_name) = module_name {
            if let Some(text) = string_literal_text(file, module_name) {
                if !text.is_empty()
                    && (!in_ambient_module || !is_external_module_name_relative(text))
                {
                    references.imports.push(module_name);
                }
            }
        }
        return;
    }
    let NodeData::ModuleDeclaration(declaration) = &node.data else {
        return;
    };
    let flags = NodeFlags::from_bits(node.flags);
    let Some(name) = declaration.name else {
        return;
    };
    let global = flags.contains(NodeFlags::GLOBAL_AUGMENTATION);
    let name_text = match &file.arena.node(name).data {
        NodeData::StringLiteral(literal) => literal.text.clone(),
        NodeData::Identifier(identifier) if global => {
            JsString::from(identifier.escaped_text.unescape())
        }
        // ast.IsAmbientModule: a string-named or global-scope declaration.
        _ => return,
    };
    let ambient = declaration.modifiers.is_some_and(|modifiers| {
        file.arena
            .node_array(modifiers)
            .nodes
            .iter()
            .any(|&modifier| file.arena.node(modifier).kind == SyntaxKind::DeclareKeyword)
    });
    if !(in_ambient_module || ambient || file.is_declaration_file) {
        return;
    }
    if file.external_module_indicator.is_some()
        || (in_ambient_module && !is_external_module_name_relative(&name_text))
    {
        references.module_augmentations.push(name);
    } else if !in_ambient_module {
        references.ambient_module_names.push(name_text);
        if let Some(body) = declaration.body {
            for &statement in statements(file, body) {
                collect_module_references(file, statement, true, references);
            }
        }
    }
}

/// tsgo `tspath.IsExternalModuleNameRelative`: `.`/`..`-relative or rooted.
fn is_external_module_name_relative(name: &JsString) -> bool {
    let bytes = name.as_bytes();
    let relative = matches!(bytes, b"." | b"..")
        || matches!(bytes, [b'.', b'/' | b'\\', ..])
        || matches!(bytes, [b'.', b'.', b'/' | b'\\', ..]);
    let rooted = matches!(bytes, [b'/' | b'\\', ..])
        || matches!(bytes, [volume, b':', ..] if volume.is_ascii_alphabetic())
        || matches!(bytes, [b'^', b'/', ..]);
    relative || rooted
}

/// tsgo `ast.ForEachDynamicImportOrRequireCall` with type-space imports and
/// string-literal arguments: each `import`/`require` in the text, at the
/// innermost node holding it (inside JSDoc in a JavaScript file).
fn for_each_dynamic_import_or_require_call(file: &SourceFile, mut found: impl FnMut(NodeId)) {
    let javascript = is_javascript(file);
    let text = file.text().as_bytes();
    let mut index = 0;
    while let Some((at, size)) = find_import_or_require(text, index) {
        let node_id = node_at_position(file, at as u32, javascript);
        let node = file.arena.node(node_id);
        match &node.data {
            NodeData::CallExpression(call) => {
                let arguments = call
                    .arguments
                    .map(|arguments| file.arena.node_array(arguments).nodes)
                    .unwrap_or_default();
                let callee = call.expression.map(|callee| file.arena.node(callee));
                let require = javascript
                    && arguments.len() == 1
                    && callee.is_some_and(|callee| {
                        matches!(&callee.data, NodeData::Identifier(identifier) if identifier.escaped_text == "require")
                    });
                let import = callee.is_some_and(|callee| {
                    callee.kind == SyntaxKind::ImportKeyword
                        || matches!(&callee.data, NodeData::MetaProperty(meta)
                            if meta.keyword_token == SyntaxKind::ImportKeyword
                                && meta.name.is_some_and(|name| matches!(&file.arena.node(name).data,
                                    NodeData::Identifier(identifier) if identifier.escaped_text == "defer")))
                });
                if let Some(&argument) = arguments.first() {
                    let literal = matches!(
                        file.arena.node(argument).kind,
                        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                    );
                    if (require || import) && literal {
                        found(argument);
                    }
                }
            }
            NodeData::ImportType(import) => {
                // ast.IsLiteralImportTypeNode.
                if let Some(argument) = import.argument {
                    if let NodeData::LiteralType(literal) = &file.arena.node(argument).data {
                        if let Some(literal) = literal.literal {
                            if file.arena.node(literal).kind == SyntaxKind::StringLiteral {
                                found(literal);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        index = at + size;
    }
}

/// tsgo `findImportOrRequire`.
fn find_import_or_require(text: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut index = start;
    while index < text.len() {
        let next = text[index..]
            .iter()
            .position(|&ch| ch == b'i' || ch == b'r')?;
        index += next;
        let (expected, size): (&[u8], usize) = if text[index] == b'i' {
            (b"import", 6)
        } else {
            (b"require", 7)
        };
        if text[index..].starts_with(expected) {
            return Some((index, size));
        }
        index += 1;
    }
    None
}

/// tsgo `ast.GetNodeAtPosition`: the innermost node (not token) containing
/// `position`, descending into JSDoc comments first when asked; a
/// meta-property is not entered.
fn node_at_position(file: &SourceFile, position: u32, include_js_doc: bool) -> NodeId {
    let first_node = SyntaxKind::QualifiedName;
    let contains = |id: NodeId| {
        let node = file.arena.node(id);
        is_node_kind(node.kind, first_node) && node.pos <= position && position < node.end
    };
    let mut current = file.root;
    loop {
        let node = file.arena.node(current);
        let mut child = None;
        if include_js_doc {
            if let Some(js_doc) = node.js_doc {
                child = file
                    .arena
                    .node_array(js_doc)
                    .nodes
                    .iter()
                    .copied()
                    .find(|&id| contains(id));
            }
        }
        if child.is_none() {
            let mut visit = |id: NodeId| {
                if child.is_none() && contains(id) {
                    child = Some(id);
                }
            };
            let elements = |list: Option<NodeArrayId>, visit: &mut dyn FnMut(NodeId)| {
                if let Some(list) = list {
                    file.arena
                        .node_array(list)
                        .nodes
                        .iter()
                        .for_each(|&id| visit(id));
                }
            };
            crate::encoder::for_each_property(node, |property| match property {
                Property::Node(Some(id)) => visit(id),
                Property::List(list)
                | Property::Modifiers(list)
                | Property::Slice(list)
                | Property::HeritageTypes(list) => elements(list, &mut visit),
                Property::Comment(Some(JSDocComment::Nodes(nodes))) => {
                    elements(Some(*nodes), &mut visit)
                }
                _ => {}
            });
        }
        match child {
            Some(id) if file.arena.node(id).kind != SyntaxKind::MetaProperty => current = id,
            _ => return current,
        }
    }
}

/// tsgo `Kind >= KindFirstNode`: a node, not a token.
fn is_node_kind(kind: SyntaxKind, first_node: SyntaxKind) -> bool {
    let tsgo = |kind| crate::encoder::tsgo_kind(kind);
    match (tsgo(kind), tsgo(first_node)) {
        (Some(kind), Some(first)) => kind >= first,
        _ => false,
    }
}
