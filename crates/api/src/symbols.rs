//! tsgo's file-owned symbols (`api/session.go` at aa8149273, #64518): a
//! binder symbol belongs to the source file whose bind created it and is
//! named by that file's descriptor (`SourceFileDescriptor`) and its number in
//! the file; a checker's symbol belongs to the snapshot that registered it.
//! A file-owned symbol and its parent, export symbol, members and exports are
//! answered from the file's binder data, without a snapshot or a checker.
//!
//! tsgo numbers a symbol with a global counter (`ast.GetSymbolId`). tsc-rs
//! numbers a binder symbol by its place in its file's identities, which every
//! program sharing the file shares, so the number of a file-owned symbol is
//! that place (one more, so it is never 0).

use std::sync::Arc;

use serde::Serialize;
use tsc_binder::{BindData, Symbol};
use tsc_compiler::BoundDocument;
use tsc_syntax::{NodeId, SourceFile, SyntaxKind};
use tsc_types::SymbolId;

use crate::encoder::{build_node_index_table, tsgo_kind, NodeIndexTable, ScriptKind};
use crate::proto::{
    CompactSymbolReference, SourceFileDescriptor, SymbolReference, SYMBOL_OWNER_FILE,
};

/// tsgo `SymbolResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SymbolResponse {
    pub(crate) reference: SymbolReference,
    pub(crate) name: String,
    pub(crate) flags: u32,
    pub(crate) check_flags: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) declarations: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) value_declaration: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<CompactSymbolReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) export_symbol: Option<CompactSymbolReference>,
}

/// The number of a file-owned symbol (tsgo `SymbolHandle`).
pub(crate) fn file_symbol_id(symbol: SymbolId) -> u64 {
    u64::from(symbol.index()) + 1
}

/// The symbol a file-owned number names.
pub(crate) fn symbol_of_file_id(id: u64) -> Option<SymbolId> {
    id.checked_sub(1)
        .and_then(|index| u32::try_from(index).ok())
        .map(SymbolId::new)
}

/// tsgo `newSourceFileDescriptor`: the file name and path, the content hash
/// (the encoding's xxh3, high word first), the parse options key (bit 0
/// JSX, bit 1 Force), the script kind and the parse's node id.
pub(crate) fn source_file_descriptor(
    source: &SourceFile,
    path: &str,
    node_id: u64,
    script_kind: ScriptKind,
) -> SourceFileDescriptor {
    let file_name = source.file_name.to_string_lossy().into_owned();
    let options = source.external_module_indicator_options;
    SourceFileDescriptor {
        content_hash: format!(
            "{:032x}",
            xxhash_rust::xxh3::xxh3_128(source.text().as_bytes())
        ),
        parse_options_key: (u32::from(options.jsx) | u32::from(options.force) << 1).to_string(),
        script_kind: script_kind as i32,
        node_id: node_id.to_string(),
        path: path.to_owned(),
        file_name,
    }
}

/// tsgo's `SymbolFlags` of a symbol record: tsc-rs's first 28 flags are
/// tsgo's, then `ConstEnumOnlyModule` and `ReplaceableByMethod`.
pub(crate) fn record_flags(data: &Symbol) -> u32 {
    let mut flags = data.flags.bits() as u32 & ((1 << 28) - 1);
    if data.extras().const_enum_only_module == Some(true) {
        flags |= 1 << 28;
    }
    if data.extras().is_replaceable_by_method {
        flags |= 1 << 29;
    }
    flags
}

/// tsgo `ast.EscapeSymbolName` of a symbol's name: tsc-rs's names are
/// escaped as tsgo writes them, but a late-bound name starts with tsgo's
/// internal prefix (U+FFFD here), which tsgo writes as `__`.
pub(crate) fn escape_symbol_name(name: &str) -> String {
    match name.strip_prefix('\u{FFFD}') {
        Some(rest) => format!("__{rest}"),
        None => name.to_owned(),
    }
}

/// A symbol's name as tsgo holds it (`Symbol.Name()`), for tsgo's byte
/// order: tsc-rs keeps the escaped name, where tsgo's internal names start
/// with `\xFE` (written `__`, or U+FFFD here for a late-bound name) and a
/// name of the source starting with `__` gains an underscore.
fn tsgo_name(data: &Symbol) -> Vec<u8> {
    let name = data.escaped_name.as_js().to_string_lossy();
    let (internal, rest) = if let Some(rest) = name.strip_prefix('\u{FFFD}') {
        (true, rest)
    } else if let Some(rest) = name.strip_prefix("___") {
        return [b"__", rest.as_bytes()].concat();
    } else if let Some(rest) = name.strip_prefix("__") {
        (true, rest)
    } else {
        (false, &*name)
    };
    let mut bytes = Vec::with_capacity(rest.len() + 1);
    if internal {
        bytes.push(0xFE);
    }
    bytes.extend_from_slice(rest.as_bytes());
    bytes
}

/// A file's binder symbols as a client sees them without a checker (tsgo
/// `newFileSymbolResponse` over a cached source file).
pub(crate) struct FileSymbols<'d> {
    source: &'d SourceFile,
    data: &'d BindData,
    descriptor: SourceFileDescriptor,
    table: NodeIndexTable,
}

impl<'d> FileSymbols<'d> {
    pub(crate) fn new(document: &'d BoundDocument, descriptor: SourceFileDescriptor) -> Self {
        Self {
            source: document.source(),
            data: &document.data,
            table: build_node_index_table(document.source()),
            descriptor,
        }
    }

    /// Whether `symbol` is one of this file's binder symbols.
    pub(crate) fn owns(&self, symbol: SymbolId) -> bool {
        self.data.symbols.contains(symbol)
    }

    /// The file's symbol a file-owned number names.
    pub(crate) fn symbol(&self, id: u64) -> Option<SymbolId> {
        symbol_of_file_id(id).filter(|&symbol| self.owns(symbol))
    }

    pub(crate) fn record(&self, symbol: SymbolId) -> &Symbol {
        self.data.symbols.symbol(symbol)
    }

    /// The node an index of the file's encoding names: `None` past the
    /// encoding, `Some(None)` for its sentinel and node lists.
    pub(crate) fn node(&self, index: u32) -> Option<Option<NodeId>> {
        self.table.nodes().get(index as usize).copied()
    }

    /// tsgo `ast.IsDeclaration`: the node's data embeds `DeclarationBase`,
    /// which holds a node's symbol (tsgo's node types at aa8149273; a type
    /// parameter's parent is always set in a parsed tree).
    pub(crate) fn is_declaration(&self, node: NodeId) -> bool {
        matches!(
            self.source.arena.node(node).kind,
            SyntaxKind::SourceFile
                | SyntaxKind::ArrowFunction
                | SyntaxKind::BinaryExpression
                | SyntaxKind::BindingElement
                | SyntaxKind::CallExpression
                | SyntaxKind::CallSignature
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::ConstructSignature
                | SyntaxKind::ConstructorType
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::EnumMember
                | SyntaxKind::ExportAssignment
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportSpecifier
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::FunctionType
                | SyntaxKind::GetAccessor
                | SyntaxKind::ImportClause
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::ImportSpecifier
                | SyntaxKind::IndexSignature
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::JSDocSignature
                | SyntaxKind::JSDocTypeLiteral
                | SyntaxKind::JsxAttribute
                | SyntaxKind::JsxAttributes
                | SyntaxKind::JsxSpreadAttribute
                | SyntaxKind::MappedType
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::MissingDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::NamedTupleMember
                | SyntaxKind::NamespaceExport
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::NamespaceImport
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::Parameter
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::SemicolonClassElement
                | SyntaxKind::SetAccessor
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::SpreadAssignment
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::TypeLiteral
                | SyntaxKind::TypeParameter
                | SyntaxKind::VariableDeclaration
        )
    }

    /// The symbol a node's bind gave it (tsgo `node.Symbol()`).
    pub(crate) fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId> {
        self.data.node_symbol.get(&node).copied()
    }

    /// tsgo `nodeHandleFrom`: `index.kind.path`.
    fn node_handle(&self, node: NodeId) -> String {
        let index = self.table.get_index(node);
        let kind = match index {
            0 => tsgo_kind(self.source.arena.node(node).kind).unwrap_or_default(),
            index => self.table.kind(index),
        };
        format!("{index}.{kind}.{}", self.descriptor.path)
    }

    /// tsgo `newSymbolReference` of a symbol of this file.
    pub(crate) fn compact(&self, symbol: Option<SymbolId>) -> Option<CompactSymbolReference> {
        symbol.map(|symbol| CompactSymbolReference {
            id: file_symbol_id(symbol),
            file: self.descriptor.node_id.clone(),
        })
    }

    /// tsgo `newFileSymbolResponse`.
    pub(crate) fn response(&self, symbol: SymbolId) -> SymbolResponse {
        let data = self.record(symbol);
        SymbolResponse {
            reference: SymbolReference {
                kind: SYMBOL_OWNER_FILE,
                file: Some(self.descriptor.clone()),
                id: file_symbol_id(symbol),
                ..SymbolReference::default()
            },
            name: escape_symbol_name(&data.escaped_name.as_js().to_string_lossy()),
            flags: record_flags(data),
            check_flags: 0,
            declarations: data
                .declarations
                .iter()
                .map(|&declaration| self.node_handle(declaration))
                .collect(),
            value_declaration: data
                .value_declaration
                .map(|declaration| self.node_handle(declaration))
                .unwrap_or_default(),
            parent: self.compact(data.parent),
            export_symbol: self.compact(data.export_symbol),
        }
    }

    /// tsgo `resolveSymbolTablePropertyOfSymbol` of a file-owned symbol: the
    /// table's symbols by their first declaration's position (those without
    /// one last), then name, then number.
    pub(crate) fn table_responses(
        &self,
        table: &Arc<tsc_binder::SymbolTable>,
    ) -> Vec<SymbolResponse> {
        let mut symbols = table.values().copied().collect::<Vec<_>>();
        symbols.sort_by(|&left, &right| {
            let (left_data, right_data) = (self.record(left), self.record(right));
            let position = |data: &Symbol| {
                data.declarations
                    .first()
                    .map(|&declaration| self.source.arena.node(declaration).pos)
            };
            match (position(left_data), position(right_data)) {
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (left_position, right_position) => left_position.cmp(&right_position),
            }
            .then_with(|| tsgo_name(left_data).cmp(&tsgo_name(right_data)))
            .then_with(|| left.index().cmp(&right.index()))
        });
        symbols
            .into_iter()
            .map(|symbol| self.response(symbol))
            .collect()
    }
}
