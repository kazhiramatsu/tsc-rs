//! tsgo `checker/exports.go` (19dadef8) and the other entry points of its
//! checker that TypeScript 7.1's API calls (`services.go`, `printer.go`):
//! what the API asks beyond the queries the checker already answers for the
//! type and symbol baselines (`GetTypeAtLocation`, `GetSymbolAtLocation`).

use std::sync::Arc;

use tsc_binder::{node_util, SymbolTable};
use tsc_emitter::{
    create_printer, EmitNodeBuilderFlags, NewLineKind, PrintRequest, PrinterOptions,
    StandaloneWriter,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckMode, JsString, SymbolFlags, SymbolId, TypeId};

use crate::state::{CheckResult, CheckerState};
use crate::type_order::order_ctx;

/// The intrinsic types of tsgo's getters (`GetAnyType` … `GetNonPrimitiveType`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicType {
    Any,
    String,
    Number,
    Boolean,
    Void,
    Undefined,
    Null,
    Never,
    Unknown,
    BigInt,
    ESSymbol,
    NonPrimitive,
}

/// tsgo `TypeFormatFlagsNoTruncation`.
pub const TYPE_FORMAT_NO_TRUNCATION: u32 = 1;
/// tsgo `TypeFormatFlagsMultilineObjectLiterals`.
const TYPE_FORMAT_MULTILINE_OBJECT_LITERALS: u32 = 1 << 10;
/// tsgo `TypeFormatFlagsAllowUniqueESSymbolType |
/// TypeFormatFlagsUseAliasDefinedOutsideCurrentScope`, `typeToString`'s.
pub const TYPE_FORMAT_DEFAULT: u32 = 1 << 20 | 1 << 14;
/// tsgo `TypeFormatFlagsNodeBuilderFlagsMask`.
const TYPE_FORMAT_NODE_BUILDER_FLAGS_MASK: u32 = 1
    | 1 << 1
    | 1 << 2
    | 1 << 3
    | 1 << 5
    | 1 << 6
    | 1 << 8
    | 1 << 10
    | 1 << 11
    | 1 << 12
    | 1 << 13
    | 1 << 14
    | 1 << 20
    | 1 << 23
    | 1 << 30
    | 1 << 28
    | 1 << 29
    | 1 << 25;
/// tsgo `nodebuilder.FlagsIgnoreErrors`.
const NODE_BUILDER_IGNORE_ERRORS: u32 = 70_221_824;
/// tsgo `defaultMaximumTruncationLength` and
/// `noTruncationMaximumTruncationLength`.
const DEFAULT_MAXIMUM_TRUNCATION_LENGTH: usize = 160;
const NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH: usize = 1_000_000;

/// tsgo `GetConstantValue`'s value: an enum member's string or number.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstantValue {
    String(JsString),
    Number(f64),
}

impl CheckerState<'_> {
    /// tsgo `GetAnyType` … `GetNonPrimitiveType`.
    pub fn get_intrinsic_type(&self, intrinsic: IntrinsicType) -> TypeId {
        let intrinsics = &self.tables.intrinsics;
        match intrinsic {
            IntrinsicType::Any => intrinsics.any,
            IntrinsicType::String => intrinsics.string,
            IntrinsicType::Number => intrinsics.number,
            IntrinsicType::Boolean => intrinsics.boolean,
            IntrinsicType::Void => intrinsics.void,
            IntrinsicType::Undefined => intrinsics.undefined,
            IntrinsicType::Null => intrinsics.null,
            IntrinsicType::Never => intrinsics.never,
            IntrinsicType::Unknown => intrinsics.unknown,
            IntrinsicType::BigInt => intrinsics.bigint,
            IntrinsicType::ESSymbol => intrinsics.es_symbol,
            IntrinsicType::NonPrimitive => intrinsics.non_primitive,
        }
    }

    /// tsgo `GetErrorType`.
    pub fn get_error_type(&self) -> TypeId {
        self.tables.intrinsics.error
    }

    /// Sort `symbols` by tsgo `CompareSymbols`: by their first declaration
    /// (file, then position), then by name.
    pub fn sort_symbols(&self, symbols: &mut [SymbolId]) {
        order_ctx!(self).sort_symbols(symbols);
    }

    /// Whether tsgo's `ObjectFlagsMembersResolved` is set on `ty`: its
    /// structured members have been resolved.
    pub fn members_resolved(&self, ty: TypeId) -> bool {
        self.links
            .read_ty(ty, |links| links.resolved_members.resolved())
            .is_some()
    }

    /// tsgo `MappedType.ResolveComponents`: the type parameter, constraint,
    /// name and template types of a mapped type.
    pub fn mapped_type_components(&mut self, ty: TypeId) -> CheckResult<[Option<TypeId>; 4]> {
        Ok([
            Some(self.get_type_parameter_from_mapped_type(ty)?),
            Some(self.get_constraint_type_from_mapped_type(ty)?),
            self.get_name_type_from_mapped_type(ty)?,
            Some(self.get_template_type_from_mapped_type(ty)?),
        ])
    }

    /// tsgo `GetTypeAtLocation` of a token the tree does not keep, which
    /// tsgo's source file creates for the scanner's token: a `this` in a
    /// type (its node is the ThisType) is part of the type node, any other
    /// token is none of the nodes `getTypeOfNode` answers for.
    pub fn get_type_at_token(&mut self, kind: SyntaxKind, parent: NodeId) -> CheckResult<TypeId> {
        if kind == SyntaxKind::ThisKeyword && self.kind_of(parent) == SyntaxKind::ThisType {
            return self.get_type_from_type_node(parent);
        }
        Ok(self.get_error_type())
    }

    /// tsgo `GetExportSymbolOfSymbol` (services.go:472-474).
    pub fn get_export_symbol_of_symbol(&self, symbol: SymbolId) -> SymbolId {
        let export_symbol = self.binder.symbol(symbol).export_symbol;
        self.get_merged_symbol(export_symbol.unwrap_or(symbol))
    }

    /// tsgo `GetAliasedSymbol`: `resolveAlias`, which only takes an alias.
    pub fn get_aliased_symbol(&mut self, symbol: SymbolId) -> CheckResult<SymbolId> {
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            panic!("Should only get alias here");
        }
        self.resolve_alias(symbol)
    }

    /// tsgo `GetExportSpecifierLocalTargetSymbol` (services.go:476-493).
    pub fn get_export_specifier_local_target_symbol(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        let meaning =
            SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS;
        match self.data_of(node) {
            NodeData::ExportSpecifier(specifier) => {
                let declaration = self
                    .parent_of(node)
                    .and_then(|exports| self.parent_of(exports))
                    .expect("an export specifier is in a declaration");
                let has_module_specifier = matches!(
                    self.data_of(declaration),
                    NodeData::ExportDeclaration(data) if data.module_specifier.is_some()
                );
                if has_module_specifier {
                    return self.get_external_module_member(declaration, node, false);
                }
                let name = specifier
                    .property_name
                    .or(specifier.name)
                    .expect("an export specifier has a name");
                // `export { "x" }` is invalid syntax.
                if self.kind_of(name) == SyntaxKind::StringLiteral {
                    return Ok(None);
                }
                self.resolve_entity_name(name, meaning, true, None)
            }
            NodeData::Identifier(_) => self.resolve_entity_name(node, meaning, true, None),
            _ => panic!(
                "Unhandled case in getExportSpecifierLocalTargetSymbol, node should be ExportSpecifier | Identifier"
            ),
        }
    }

    /// tsgo `GetShorthandAssignmentValueSymbol` (services.go:495-500).
    pub fn get_shorthand_assignment_value_symbol(
        &mut self,
        location: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        let NodeData::ShorthandPropertyAssignment(assignment) = self.data_of(location) else {
            return Ok(None);
        };
        let Some(name) = assignment.name else {
            return Ok(None);
        };
        self.resolve_entity_name(name, SymbolFlags::VALUE | SymbolFlags::ALIAS, true, None)
    }

    /// tsgo `Symbol.Exports`: the binder's table, but the globals for the
    /// `globalThis` symbol, whose exports tsgo's checker sets to them.
    pub fn symbol_exports(&self, symbol: SymbolId) -> Arc<SymbolTable> {
        if symbol == self.global_this_symbol {
            return Arc::clone(&self.globals);
        }
        Arc::clone(self.binder.symbol(symbol).exports())
    }

    /// tsgo `getExportsOfModule`, which reads `globalThis`'s exports as
    /// [`Self::symbol_exports`] does.
    fn module_exports(&mut self, module_symbol: SymbolId) -> CheckResult<Arc<SymbolTable>> {
        if module_symbol == self.global_this_symbol {
            return Ok(Arc::clone(&self.globals));
        }
        self.get_exports_of_module(module_symbol)
    }

    /// tsgo `GetExportsOfModule` (services.go:132-134), in the module's
    /// export table order.
    pub fn get_exports_of_module_symbols(
        &mut self,
        module_symbol: SymbolId,
    ) -> CheckResult<Vec<SymbolId>> {
        Ok(self
            .module_exports(module_symbol)?
            .values()
            .copied()
            .collect())
    }

    /// tsgo `TryGetMemberInModuleExports` (services.go:314-317).
    pub fn try_get_member_in_module_exports(
        &mut self,
        member_name: &str,
        module_symbol: SymbolId,
    ) -> CheckResult<Option<SymbolId>> {
        let exports = self.module_exports(module_symbol)?;
        Ok(exports.get(member_name).copied())
    }

    /// tsgo `GetTypeOfSymbolAtLocation` (checker.go:16773-16809): the type
    /// of `symbol` narrowed at a reference to it, else its declared type.
    pub fn get_type_of_symbol_at_location(
        &mut self,
        symbol: SymbolId,
        location: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let symbol = self.get_export_symbol_of_value_symbol_if_exported(symbol);
        if let Some(mut location) = location {
            // At an identifier or property access that is a dotted name
            // expression and not an assignment target, the type of the
            // expression (with control flow analysis), when the expression
            // resolved to the symbol.
            let kind = self.kind_of(location);
            let parent_kind = self.parent_of(location).map(|parent| self.kind_of(parent));
            if matches!(kind, SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier)
                && !(self.is_jsx_tag_name(location)
                    || matches!(
                        parent_kind,
                        Some(SyntaxKind::JsxAttribute | SyntaxKind::JsxNamespacedName)
                    ))
            {
                if self.is_right_side_of_qualified_name_or_property_access(location) {
                    location = self.parent_of(location).expect("a right side has a parent");
                }
                let write = self.is_write_access(location);
                if self.is_expression_node_like_tsgo(location)
                    && (!node_util::is_assignment_target(
                        self.binder.source_of_node(location),
                        location,
                    ) || write)
                {
                    let ty = if write
                        && self.kind_of(location) == SyntaxKind::PropertyAccessExpression
                    {
                        self.check_property_access_expression(location, CheckMode::NORMAL, true)?
                    } else {
                        self.get_type_of_expression(location)?
                    };
                    let resolved = self
                        .links
                        .read_node(location, |links| links.resolved_symbol.resolved());
                    if resolved.map(|resolved| {
                        self.get_export_symbol_of_value_symbol_if_exported(resolved)
                    }) == Some(symbol)
                    {
                        return Ok(self.remove_optional_type_marker(ty));
                    }
                }
            }
            if self.is_declaration_name(location) {
                let parent = self.parent_of(location).expect("a name has a parent");
                if self.kind_of(parent) == SyntaxKind::SetAccessor
                    && self.annotated_accessor_type_node(Some(parent)).is_some()
                {
                    let accessor = self.node_symbol(parent).expect("an accessor has a symbol");
                    return self.get_write_type_of_accessors(accessor);
                }
            }
            // A location that is not a reference to the symbol asks what
            // type a reference there would have: without control flow
            // information for it, the declared type.
            if self.is_right_side_of_access_expression(location) {
                let parent = self.parent_of(location).expect("a right side has a parent");
                if self.is_write_access(parent) {
                    return self.get_write_type_of_symbol(symbol);
                }
            }
        }
        self.get_non_missing_type_of_symbol(symbol)
    }

    /// tsgo `isRightSideOfAccessExpression`: the name of a property access
    /// or the argument of an element access.
    fn is_right_side_of_access_expression(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::PropertyAccessExpression(access) => access.name == Some(node),
            NodeData::ElementAccessExpression(access) => access.argument_expression == Some(node),
            _ => false,
        }
    }

    /// tsgo `GetConstantValue` (services.go:870-899): the value of an enum
    /// member, or of a reference to a const enum's member.
    pub fn get_constant_value(&mut self, node: NodeId) -> CheckResult<Option<ConstantValue>> {
        if self.kind_of(node) == SyntaxKind::EnumMember {
            return self.enum_member_constant_value(node);
        }
        let resolved = |state: &Self| {
            state
                .links
                .read_node(node, |links| links.resolved_symbol.resolved())
        };
        if resolved(self).is_none() {
            // Ensure the cached resolved symbol is set.
            self.check_expression_cached(node, CheckMode::NORMAL)?;
        }
        let mut symbol = resolved(self);
        if symbol.is_none()
            && node_util::is_entity_name_expression(self.binder.source_of_node(node), node)
        {
            symbol = self.resolve_entity_name(node, SymbolFlags::VALUE, true, None)?;
        }
        if let Some(symbol) = symbol {
            if self
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::ENUM_MEMBER)
            {
                // Inline property and index accesses only for const enums.
                let member = self
                    .binder
                    .symbol(symbol)
                    .value_declaration
                    .expect("an enum member has a declaration");
                let enum_declaration = self.parent_of(member).expect("a member has an enum");
                if self.is_enum_const(enum_declaration) {
                    return self.enum_member_constant_value(member);
                }
            }
        }
        Ok(None)
    }

    fn enum_member_constant_value(&mut self, member: NodeId) -> CheckResult<Option<ConstantValue>> {
        Ok(match self.get_enum_member_value(member)?.value {
            Some(crate::evaluate::EvalValue::Str(text)) => Some(ConstantValue::String(text)),
            Some(crate::evaluate::EvalValue::Num(value)) => Some(ConstantValue::Number(value)),
            None => None,
        })
    }

    /// tsgo `TypeToStringEx` (printer.go:59-117) without a verbosity
    /// context: the type node the node builder builds for `ty`, printed on
    /// one line without comments (but the unresolved type's), cut at
    /// tsgo's absolute maximum length.
    pub fn type_to_string_with_flags(
        &mut self,
        ty: TypeId,
        enclosing_declaration: Option<NodeId>,
        flags: u32,
    ) -> Result<String, String> {
        let no_truncation = self.options.no_error_truncation == Some(true)
            || flags & TYPE_FORMAT_NO_TRUNCATION != 0;
        let mut combined =
            (flags & TYPE_FORMAT_NODE_BUILDER_FLAGS_MASK) | NODE_BUILDER_IGNORE_ERRORS;
        if no_truncation {
            combined |= EmitNodeBuilderFlags::NO_TRUNCATION.0;
        }
        // The display arena must know every file a type node may reach (a
        // reused annotation, a declaration's name).
        for index in 0..self.binder.file_count() {
            self.emit_display_target(index);
        }
        let file_index =
            enclosing_declaration.map_or(0, |node| self.binder.file_index_of_node(node));
        let target = self.emit_display_target(file_index);
        let mut display = self.take_emit_display();
        let built = crate::node_builder::type_to_type_node(
            self,
            display
                .arena_mut()
                .expect("checker display result remains live"),
            target,
            ty,
            enclosing_declaration,
            Some(EmitNodeBuilderFlags(combined)),
            None,
            None,
            None,
            None,
            None,
        );
        self.restore_emit_display(display);
        let node = match built {
            Ok(Some(node)) => node,
            Ok(None) => panic!("should always get typenode"),
            Err(error) => return Err(format!("type node builder: {error}")),
        };
        // The unresolved type keeps the comment that marks its `any`.
        let options = PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(ty != self.tables.intrinsics.unresolved)
            .with_declaration_syntax(true);
        let writer = if flags & TYPE_FORMAT_MULTILINE_OBJECT_LITERALS != 0 {
            StandaloneWriter::MultiLine
        } else {
            StandaloneWriter::SingleLine
        };
        let printed = self
            .with_emit_display(|display| {
                create_printer(options).print(
                    display,
                    PrintRequest::StandaloneNode { node, writer },
                    None,
                )
            })
            .map_err(|error| format!("type node printer: {error:?}"))?;
        let mut result = JsString::from_code_units(printed.text_utf16().as_ref())
            .to_string_lossy()
            .into_owned();
        let max_length = 2 * if no_truncation {
            NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH
        } else {
            DEFAULT_MAXIMUM_TRUNCATION_LENGTH
        };
        if !result.is_empty() && result.len() >= max_length {
            let mut cut = max_length - "...".len();
            while !result.is_char_boundary(cut) {
                cut -= 1;
            }
            result.truncate(cut);
            result.push_str("...");
        }
        Ok(result)
    }
}
