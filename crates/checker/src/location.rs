//! The node-level queries of tsgo's checker that the language service and
//! the test harness use: the type at a location (`GetTypeAtLocation`,
//! checker.go:32418-32516 and :32647) and the symbol at a location
//! (`GetSymbolAtLocation`, :32069-32417). Their semantics are deliberately
//! "fuzzy" (tsgo's own comment): they return *some* type or symbol for the
//! node and are not used by the checker itself.

use tsc_binder::node_util;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckMode, NodeFlags, SymbolFlags, SymbolId, TypeFlags, TypeId};

use crate::state::{CheckResult, CheckerState};

impl CheckerState<'_> {
    /// tsgo `GetTypeAtLocation` (checker.go:32647-32649). The JSDoc
    /// reparse of this port lives beside the tree, so there is no reparsed
    /// clone to redirect to (`GetReparsedNodeForNode`).
    pub fn get_type_at_location(&mut self, node: NodeId) -> CheckResult<TypeId> {
        self.get_type_of_node(node)
    }

    /// tsgo `getTypeOfNode` (checker.go:32418-32516).
    fn get_type_of_node(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let error = self.tables.intrinsics.error;
        if self.kind_of(node) == SyntaxKind::SourceFile {
            let file = self.binder.file_index_of_node(node);
            if !self.is_external_or_common_js_module_file(file) {
                return Ok(error);
            }
        }
        if self
            .node_flags_of(node)
            .contains(NodeFlags::IN_WITH_STATEMENT)
        {
            // We cannot answer semantic questions within a with block.
            return Ok(error);
        }

        let heritage = self.try_get_class_implementing_or_extending_heritage_clause_element(node);
        let class_type = match heritage {
            Some((class_declaration, _)) => {
                let symbol = self.get_symbol_of_declaration(class_declaration)?;
                Some(self.get_declared_type_of_class_or_interface(symbol)?)
            }
            None => None,
        };

        if self.is_part_of_type_node(node) {
            let type_from_type_node = self.get_type_from_type_node(node)?;
            if let Some(class_type) = class_type {
                let this_type = self.interface_this_type(class_type);
                return self.get_type_with_this_argument(type_from_type_node, this_type, false);
            }
            return Ok(type_from_type_node);
        }

        if self.is_expression_node_like_tsgo(node) {
            return self.get_regular_type_of_expression_at(node);
        }

        if let (Some(class_type), Some((_, false))) = (class_type, heritage) {
            // An ExpressionWithTypeArguments is a type node except in the
            // extends clause of a class.
            let base_types = self.get_base_types(class_type)?;
            if let Some(&base_type) = base_types.first() {
                let this_type = self.interface_this_type(class_type);
                return self.get_type_with_this_argument(base_type, this_type, false);
            }
            return Ok(error);
        }

        if self.is_type_declaration(node) {
            let symbol = self.get_symbol_of_declaration(node)?;
            return self.get_declared_type_of_symbol(symbol);
        }

        if self.is_type_declaration_name(node) {
            return match self.get_symbol_at_location_ex(node, false)? {
                Some(symbol) => self.get_declared_type_of_symbol(symbol),
                None => Ok(error),
            };
        }

        if self.kind_of(node) == SyntaxKind::BindingElement {
            return Ok(self
                .get_type_for_variable_like_declaration(node, true, CheckMode::NORMAL)?
                .unwrap_or(error));
        }

        if self.is_declaration_like_tsgo(node) {
            let symbol = self.get_symbol_of_declaration(node)?;
            if symbol != self.unknown_symbol {
                return self.get_type_of_symbol(symbol);
            }
            return Ok(error);
        }

        if self.is_declaration_name_or_import_property_name(node) {
            return match self.get_symbol_at_location_ex(node, false)? {
                Some(symbol) => self.get_type_of_symbol(symbol),
                None => Ok(error),
            };
        }

        if node_util::is_binding_pattern(self.binder.source_of_node(node), node) {
            let parent = self
                .parent_of(node)
                .expect("a binding pattern has a parent");
            return Ok(self
                .get_type_for_variable_like_declaration(parent, true, CheckMode::NORMAL)?
                .unwrap_or(error));
        }

        if self.is_in_right_side_of_import_or_export_assignment(node) {
            if let Some(symbol) = self.get_symbol_at_location_ex(node, false)? {
                let declared_type = self.get_declared_type_of_symbol(symbol)?;
                if !self.tables.is_error_type(declared_type) {
                    return Ok(declared_type);
                }
                return self.get_type_of_symbol(symbol);
            }
        }

        if let Some(parent) = self.parent_of(node) {
            if let NodeData::MetaProperty(data) = self.data_of(parent) {
                if data.keyword_token == self.kind_of(node) {
                    // tsgo checkMetaPropertyKeyword (checker.go:10999-11002)
                    // is a stub returning the error type.
                    return Ok(error);
                }
            }
        }

        // ImportAttributes: tsgo checkImportAttributesExpression; the
        // walker never asks for one (it is neither an expression nor a
        // declaration name).
        Ok(error)
    }

    /// tsgo `getRegularTypeOfExpression` (checker.go:32603-32608).
    fn get_regular_type_of_expression_at(&mut self, expression: NodeId) -> CheckResult<TypeId> {
        let mut expression = expression;
        if self.is_right_side_of_qualified_name_or_property_access(expression) {
            expression = self
                .parent_of(expression)
                .expect("a right side has a parent");
        }
        let ty = self.get_type_of_expression(expression)?;
        Ok(self.regular_type_of_literal_type(ty))
    }

    /// tsgo `GetSymbolAtLocation` (checker.go:32069-32076): lookups the API
    /// makes report no errors.
    pub fn get_symbol_at_location(&mut self, node: NodeId) -> CheckResult<Option<SymbolId>> {
        self.get_symbol_at_location_ex(node, true)
    }

    /// tsgo `getSymbolAtLocation` (checker.go:32082-32266).
    fn get_symbol_at_location_ex(
        &mut self,
        node: NodeId,
        ignore_errors: bool,
    ) -> CheckResult<Option<SymbolId>> {
        if self.kind_of(node) == SyntaxKind::SourceFile {
            let file = self.binder.file_index_of_node(node);
            if self.is_external_or_common_js_module_file(file) {
                return Ok(self.node_symbol(node).map(|s| self.get_merged_symbol(s)));
            }
            return Ok(None);
        }
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        let grand_parent = self.parent_of(parent);

        if self
            .node_flags_of(node)
            .contains(NodeFlags::IN_WITH_STATEMENT)
        {
            return Ok(None);
        }

        if self.is_declaration_name_or_import_property_name(node) {
            // This is a declaration: getSymbolOfNode.
            let parent_symbol = self.get_symbol_of_declaration(parent)?;
            let parent_symbol = (parent_symbol != self.unknown_symbol).then_some(parent_symbol);
            if self.is_import_or_export_specifier_property_name(parent, node) {
                return match parent_symbol {
                    Some(symbol) => self.get_immediate_aliased_symbol(symbol),
                    None => Ok(None),
                };
            }
            return Ok(parent_symbol);
        } else if self.is_literal_computed_property_declaration_name(node) {
            let grand_parent = grand_parent.expect("a computed name has a grandparent");
            let symbol = self.get_symbol_of_declaration(grand_parent)?;
            return Ok((symbol != self.unknown_symbol).then_some(symbol));
        }

        if self.kind_of(node) == SyntaxKind::Identifier {
            if self.is_in_right_side_of_import_or_export_assignment(node) {
                return self.get_symbol_of_name_or_property_access_expression(node);
            } else if let (SyntaxKind::BindingElement, Some(grand_parent)) =
                (self.kind_of(parent), grand_parent)
            {
                let is_property_name = matches!(
                    self.data_of(parent),
                    NodeData::BindingElement(data) if data.property_name == Some(node)
                );
                if is_property_name
                    && self.kind_of(grand_parent) == SyntaxKind::ObjectBindingPattern
                {
                    let type_of_pattern = self.get_type_of_node(grand_parent)?;
                    let text = self.identifier_text_of(node).unwrap_or_default().to_owned();
                    if let Some(property) =
                        self.get_property_of_type_full(type_of_pattern, text.as_str())?
                    {
                        return Ok(Some(property));
                    }
                }
            } else if let NodeData::MetaProperty(data) = self.data_of(parent) {
                if data.name == Some(node) {
                    let keyword = data.keyword_token;
                    let text = self.identifier_text_of(node).unwrap_or_default().to_owned();
                    if keyword == SyntaxKind::NewKeyword && text == "target" {
                        // `target` in `new.target`
                        let ty = self.check_new_target_meta_property(parent)?;
                        return Ok(self.tables.type_of(ty).symbol);
                    }
                    // tsgo answers `meta` in `import.meta` with the transient
                    // `meta` member of its ImportMetaExpression type; the port
                    // has no such symbol yet.
                    return Ok(None);
                }
            }
            // The JSDoc parameter-tag arm (`@param x`) is not reached: the
            // walker never visits JSDoc nodes.
        }

        match self.kind_of(node) {
            SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::QualifiedName
                if !self.is_this_in_type_query(node) =>
            {
                self.get_symbol_of_name_or_property_access_expression(node)
            }
            SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::QualifiedName
            | SyntaxKind::ThisKeyword => {
                if let Some(container) =
                    crate::expr::get_this_container_full(self, node, false, false)
                {
                    if node_util::is_function_like_kind(self.kind_of(container)) {
                        let signature = self.get_signature_from_declaration(container)?;
                        if let Some(this_parameter) = self.signature_of(signature).this_parameter {
                            return Ok(Some(this_parameter));
                        }
                    }
                }
                if node_util::is_in_expression_context(self.binder.source_of_node(node), node) {
                    let ty = self.check_expression(node, CheckMode::NORMAL)?;
                    return Ok(self.tables.type_of(ty).symbol);
                }
                let ty = self.get_type_from_this_type_node(node)?;
                Ok(self.tables.type_of(ty).symbol)
            }
            SyntaxKind::ThisType => {
                let ty = self.get_type_from_this_type_node(node)?;
                Ok(self.tables.type_of(ty).symbol)
            }
            SyntaxKind::SuperKeyword => {
                let ty = self.check_expression(node, CheckMode::NORMAL)?;
                Ok(self.tables.type_of(ty).symbol)
            }
            SyntaxKind::ConstructorKeyword => {
                // constructor keyword for an overload: the class.
                if self.kind_of(parent) == SyntaxKind::Constructor {
                    let class = self.parent_of(parent).expect("a constructor has a class");
                    return Ok(self.node_symbol(class));
                }
                Ok(None)
            }
            SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral => {
                // 1). import x = require("./mod")  2). an import declaration's
                // module name  3). require in JavaScript  4). import("./foo").
                if self.is_module_specifier_literal(node, parent, grand_parent) {
                    return self.resolve_external_module_name(node, node, ignore_errors);
                }
                if self.kind_of(parent) == SyntaxKind::CallExpression
                    && tsc_binder::assignment::is_bindable_object_define_property_call(
                        self.binder.source_of_node(parent),
                        parent,
                    )
                    && self.call_argument(parent, 1) == Some(node)
                {
                    let symbol = self.get_symbol_of_declaration(parent)?;
                    return Ok((symbol != self.unknown_symbol).then_some(symbol));
                }
                self.get_index_access_literal_symbol(node, parent, grand_parent)
            }
            SyntaxKind::NumericLiteral => {
                self.get_index_access_literal_symbol(node, parent, grand_parent)
            }
            SyntaxKind::DefaultKeyword
            | SyntaxKind::FunctionKeyword
            | SyntaxKind::EqualsGreaterThanToken
            | SyntaxKind::ClassKeyword => self.get_symbol_of_node(parent),
            SyntaxKind::ImportType => {
                if let Some(literal) = self.literal_import_type_literal(node) {
                    return self.get_symbol_at_location_ex(literal, ignore_errors);
                }
                Ok(None)
            }
            SyntaxKind::ExportKeyword => {
                if self.kind_of(parent) == SyntaxKind::ExportAssignment {
                    return Ok(self.node_symbol(parent));
                }
                Ok(None)
            }
            SyntaxKind::ImportKeyword | SyntaxKind::NewKeyword => {
                if let NodeData::MetaProperty(data) = self.data_of(parent) {
                    if self.kind_of(node) == SyntaxKind::ImportKeyword {
                        let name = data.name;
                        if name
                            .and_then(|name| self.identifier_text_of(name).map(str::to_owned))
                            .as_deref()
                            == Some("defer")
                        {
                            return Ok(None);
                        }
                    }
                    // checkMetaPropertyKeyword is a stub (the error type).
                    return Ok(None);
                }
                Ok(None)
            }
            SyntaxKind::InstanceOfKeyword => {
                if let NodeData::BinaryExpression(data) = self.data_of(parent) {
                    let Some(right) = data.right else {
                        return Ok(None);
                    };
                    let ty = self.get_type_of_expression(right)?;
                    if let Some(has_instance) =
                        self.get_symbol_has_instance_method_of_object_type(ty)?
                    {
                        if let Some(symbol) = self.tables.type_of(has_instance).symbol {
                            return Ok(Some(symbol));
                        }
                    }
                    return Ok(self.tables.type_of(ty).symbol);
                }
                Ok(None)
            }
            SyntaxKind::MetaProperty => {
                let ty = self.check_expression(node, CheckMode::NORMAL)?;
                Ok(self.tables.type_of(ty).symbol)
            }
            SyntaxKind::JsxNamespacedName => {
                if self.is_jsx_tag_name(node) && self.is_jsx_intrinsic_tag_name(node) {
                    let symbol = self.get_intrinsic_tag_symbol(parent)?;
                    return Ok((symbol != self.unknown_symbol).then_some(symbol));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// tsgo `getSymbolOfNameOrPropertyAccessExpression`
    /// (checker.go:32268-32417).
    fn get_symbol_of_name_or_property_access_expression(
        &mut self,
        name: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        if self.is_declaration_name(name) {
            let parent = self
                .parent_of(name)
                .expect("a declaration name has a parent");
            return self.get_symbol_of_node(parent);
        }
        let parent = self.parent_of(name).expect("a name has a parent");
        let source = self.binder.source_of_node(name);
        if self.kind_of(parent) == SyntaxKind::ExportAssignment
            && node_util::is_entity_name_expression(source, name)
        {
            // Even an entity name expression that doesn't resolve as an
            // entity name may still typecheck as a property access.
            let success = self.resolve_entity_name_ex(
                name,
                SymbolFlags::VALUE
                    | SymbolFlags::TYPE
                    | SymbolFlags::NAMESPACE
                    | SymbolFlags::ALIAS,
                true,
                None,
                false,
            )?;
            if let Some(success) = success.filter(|&s| s != self.unknown_symbol) {
                return Ok(Some(success));
            }
        } else if self.is_entity_name(name)
            && self.is_in_right_side_of_import_or_export_assignment(name)
        {
            // Since ExportAssignment was checked above, this is an import.
            return self.get_symbol_of_part_of_right_hand_side_of_import_equals(name);
        }

        if self.is_entity_name(name) {
            if let Some(import_type) = self.is_import_type_qualifier_part(name) {
                self.get_type_from_type_node(import_type)?;
                let resolved = self
                    .links
                    .read_node(name, |links| links.resolved_symbol.resolved());
                return Ok(resolved.filter(|&s| s != self.unknown_symbol));
            }
        }

        let mut name = name;
        while self.is_right_side_of_qualified_name_or_property_access(name) {
            name = self.parent_of(name).expect("a right side has a parent");
        }
        let parent = self.parent_of(name).expect("a name has a parent");

        if self.is_in_name_of_expression_with_type_arguments_or_heritage_type_reference(name) {
            let mut meaning = if matches!(
                self.kind_of(parent),
                SyntaxKind::ExpressionWithTypeArguments | SyntaxKind::TypeReference
            ) {
                // A heritage element name may appear in type space, value
                // space, or both; the meaning follows its context.
                let mut meaning = if self.is_part_of_type_node(name) {
                    SymbolFlags::TYPE
                } else {
                    SymbolFlags::VALUE
                };
                // In a class 'extends' clause we are also looking for a value.
                if self.is_expression_with_type_arguments_in_class_extends_clause(parent) {
                    meaning |= SymbolFlags::VALUE;
                }
                meaning
            } else {
                SymbolFlags::NAMESPACE
            };
            meaning |= SymbolFlags::ALIAS;
            let source = self.binder.source_of_node(name);
            if node_util::is_entity_name_expression(source, name) {
                if let Some(symbol) =
                    self.resolve_entity_name_ex(name, meaning, true, None, false)?
                {
                    return Ok(Some(symbol));
                }
            }
        }

        if self.is_expression_node_like_tsgo(name) {
            let source = self.binder.source_of_node(name);
            if node_util::node_is_missing(source, Some(name)) {
                return Ok(None);
            }
            let is_jsdoc = self.is_jsdoc_name_reference_context(name);
            match self.kind_of(name) {
                SyntaxKind::Identifier => {
                    if self.is_jsx_tag_name(name) && self.is_jsx_intrinsic_tag_name(name) {
                        let symbol = self.get_intrinsic_tag_symbol(parent)?;
                        return Ok((symbol != self.unknown_symbol).then_some(symbol));
                    }
                    let meaning = if is_jsdoc {
                        SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE
                    } else {
                        SymbolFlags::VALUE
                    };
                    let location = if is_jsdoc {
                        self.get_host_signature_from_jsdoc(name)
                    } else {
                        None
                    };
                    let result =
                        self.resolve_entity_name_ex(name, meaning, true, location, true)?;
                    // The JSDoc class-member fallback is not reached: the
                    // walker never visits JSDoc nodes.
                    Ok(result.filter(|&s| s != self.unknown_symbol))
                }
                SyntaxKind::PrivateIdentifier => {
                    self.get_symbol_for_private_identifier_expression(name)
                }
                SyntaxKind::PropertyAccessExpression | SyntaxKind::QualifiedName => {
                    if let Some(resolved) = self
                        .links
                        .read_node(name, |links| links.resolved_symbol.resolved())
                    {
                        return Ok((resolved != self.unknown_symbol).then_some(resolved));
                    }
                    if self.kind_of(name) == SyntaxKind::PropertyAccessExpression {
                        self.check_property_access_expression(name, CheckMode::NORMAL, false)?;
                        // tsgo falls back to getApplicableIndexSymbol for an
                        // access answered by an index signature; the port
                        // has no index symbols yet.
                    } else {
                        self.check_qualified_name(name, CheckMode::NORMAL)?;
                    }
                    let resolved = self
                        .links
                        .read_node(name, |links| links.resolved_symbol.resolved())
                        .filter(|&s| s != self.unknown_symbol);
                    if resolved.is_none()
                        && is_jsdoc
                        && self.kind_of(name) == SyntaxKind::QualifiedName
                    {
                        return self.resolve_jsdoc_member_name(name, true, None);
                    }
                    Ok(resolved)
                }
                _ => Ok(None),
            }
        } else if self.is_entity_name(name) && self.is_type_reference_identifier(name) {
            let meaning = if self.kind_of(parent) == SyntaxKind::TypeReference {
                SymbolFlags::TYPE
            } else {
                SymbolFlags::NAMESPACE
            };
            let symbol = self.resolve_entity_name_ex(name, meaning, true, None, true)?;
            if let Some(symbol) = symbol.filter(|&s| s != self.unknown_symbol) {
                return Ok(Some(symbol));
            }
            if self.is_name_of_heritage_clause_type_reference(name) {
                return Ok(None);
            }
            Ok(Some(self.get_unresolved_symbol_for_entity_name(name)))
        } else if self.kind_of(parent) == SyntaxKind::TypePredicate {
            self.resolve_entity_name_ex(
                name,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                true,
                None,
                false,
            )
        } else {
            Ok(None)
        }
    }

    /// tsgo `getSymbolOfNode` (checker.go:14628-14634): the declaration's
    /// own symbol, late-bound and merged.
    fn get_symbol_of_node(&mut self, node: NodeId) -> CheckResult<Option<SymbolId>> {
        let Some(symbol) = self.node_symbol(node) else {
            return Ok(None);
        };
        let symbol = self.get_late_bound_symbol(symbol)?;
        Ok(Some(self.get_merged_symbol(symbol)))
    }

    // ----- helpers -----------------------------------------------------

    fn node_flags_of(&self, node: NodeId) -> NodeFlags {
        self.binder.flags_of(node)
    }

    /// `classType.AsInterfaceType().thisType`: the synthesized this-type of
    /// a class or interface's declared (generic) type.
    fn interface_this_type(&self, class_type: TypeId) -> Option<TypeId> {
        match &self.tables.type_of(class_type).data {
            tsc_types::TypeData::GenericType { this_type, .. } => Some(*this_type),
            _ => None,
        }
    }

    fn is_external_or_common_js_module_file(&self, file: usize) -> bool {
        self.binder.source(file).external_module_indicator.is_some()
            || self.binder.file(file).common_js_module_indicator.is_some()
    }

    /// tsgo `IsExpressionNode` (ast/utilities.go:1997-2030): the port's
    /// classifier, minus the callee of `import.defer(...)`.
    pub(crate) fn is_expression_node_like_tsgo(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        if self.kind_of(node) == SyntaxKind::MetaProperty {
            if let Some(parent) = self.parent_of(node) {
                if self.is_import_call(parent)
                    && matches!(self.data_of(parent), NodeData::CallExpression(data) if data.expression == Some(node))
                {
                    return false;
                }
            }
            return true;
        }
        node_util::is_expression_node(source, node)
    }

    /// tsgo `IsDeclaration` (ast/utilities.go:1302-1307).
    fn is_declaration_like_tsgo(&self, node: NodeId) -> bool {
        if self.kind_of(node) == SyntaxKind::TypeParameter {
            return self.parent_of(node).is_some();
        }
        node_util::is_declaration(self.binder.source_of_node(node), node)
    }

    /// tsgo `IsDeclarationName` (ast/utilities.go:1310-1312): the `name`
    /// slot of a declaration.
    pub(crate) fn is_declaration_name(&self, name: NodeId) -> bool {
        let source = self.binder.source_of_node(name);
        if self.kind_of(name) == SyntaxKind::SourceFile
            || node_util::is_binding_pattern(source, name)
        {
            return false;
        }
        let Some(parent) = self.parent_of(name) else {
            return false;
        };
        self.is_declaration_like_tsgo(parent)
            && node_util::name_field_of(source, parent) == Some(name)
    }

    /// tsgo `IsDeclarationNameOrImportPropertyName` (ast/utilities.go:1315-1322).
    pub(crate) fn is_declaration_name_or_import_property_name(&self, name: NodeId) -> bool {
        match self.parent_of(name).map(|parent| self.kind_of(parent)) {
            Some(SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier) => matches!(
                self.kind_of(name),
                SyntaxKind::Identifier | SyntaxKind::StringLiteral
            ),
            _ => self.is_declaration_name(name),
        }
    }

    fn is_import_or_export_specifier_property_name(&self, parent: NodeId, node: NodeId) -> bool {
        match self.data_of(parent) {
            NodeData::ImportSpecifier(data) => data.property_name == Some(node),
            NodeData::ExportSpecifier(data) => data.property_name == Some(node),
            _ => false,
        }
    }

    /// tsgo `IsLiteralComputedPropertyDeclarationName` (ast/utilities.go:1324-1328).
    fn is_literal_computed_property_declaration_name(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        node_util::is_string_or_numeric_literal_like(source, node)
            && self
                .parent_of(node)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ComputedPropertyName)
            && self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|grand_parent| self.is_declaration_like_tsgo(grand_parent))
    }

    /// tsgo `IsTypeDeclaration` (ast/utilities.go:3627-3638).
    pub(crate) fn is_type_declaration(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::TypeParameter
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration => true,
            SyntaxKind::ImportClause => {
                matches!(self.data_of(node), NodeData::ImportClause(data) if data.is_type_only)
            }
            SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier => self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|grand_parent| match self.data_of(grand_parent) {
                    NodeData::ImportClause(data) => data.is_type_only,
                    NodeData::ExportDeclaration(data) => data.is_type_only,
                    _ => false,
                }),
            _ => false,
        }
    }

    /// tsgo `IsTypeDeclarationName` (ast/utilities.go:3640-3644).
    fn is_type_declaration_name(&self, name: NodeId) -> bool {
        self.kind_of(name) == SyntaxKind::Identifier
            && self.parent_of(name).is_some_and(|parent| {
                self.is_type_declaration(parent)
                    && node_util::get_name_of_declaration(
                        self.binder.source_of_node(parent),
                        parent,
                    ) == Some(name)
            })
    }

    /// tsgo `TryGetClassImplementingOrExtendingHeritageClauseElement`
    /// (ast/utilities.go:1445-1451): the class and whether the clause is
    /// `implements`.
    pub(crate) fn try_get_class_implementing_or_extending_heritage_clause_element(
        &self,
        node: NodeId,
    ) -> Option<(NodeId, bool)> {
        if !matches!(
            self.kind_of(node),
            SyntaxKind::ExpressionWithTypeArguments | SyntaxKind::TypeReference
        ) {
            return None;
        }
        let clause = self.parent_of(node)?;
        let NodeData::HeritageClause(data) = self.data_of(clause) else {
            return None;
        };
        let token = data.token;
        let class = self.parent_of(clause)?;
        if !matches!(
            self.kind_of(class),
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
        ) {
            return None;
        }
        Some((class, token == SyntaxKind::ImplementsKeyword))
    }

    /// tsgo `IsNameOfHeritageClauseTypeReference` (ast/utilities.go:1766-1773).
    pub(crate) fn is_name_of_heritage_clause_type_reference(&self, node: NodeId) -> bool {
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            if self.kind_of(parent) != SyntaxKind::QualifiedName {
                break;
            }
            node = parent;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        matches!(self.data_of(parent), NodeData::TypeReference(data) if data.type_name == Some(node))
            && self
                .parent_of(parent)
                .is_some_and(|clause| self.kind_of(clause) == SyntaxKind::HeritageClause)
    }

    /// tsgo `isInNameOfExpressionWithTypeArgumentsOrHeritageTypeReference`
    /// (checker/utilities.go:1188-1195).
    fn is_in_name_of_expression_with_type_arguments_or_heritage_type_reference(
        &self,
        node: NodeId,
    ) -> bool {
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            if !matches!(
                self.kind_of(parent),
                SyntaxKind::PropertyAccessExpression | SyntaxKind::QualifiedName
            ) {
                break;
            }
            node = parent;
        }
        self.parent_of(node)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExpressionWithTypeArguments)
            || self.is_name_of_heritage_clause_type_reference(node)
    }

    /// tsgo `isImportTypeQualifierPart` (checker/utilities.go:1174-1186): the
    /// import type whose qualifier this entity name is part of.
    fn is_import_type_qualifier_part(&self, node: NodeId) -> Option<NodeId> {
        let mut node = node;
        let mut parent = self.parent_of(node)?;
        while self.kind_of(parent) == SyntaxKind::QualifiedName {
            node = parent;
            parent = self.parent_of(parent)?;
        }
        match self.data_of(parent) {
            NodeData::ImportType(data) if data.qualifier == Some(node) => Some(parent),
            _ => None,
        }
    }

    /// tsgo `IsEntityName`: an identifier or a qualified name.
    fn is_entity_name(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        )
    }

    /// tsgo `IsJsxTagName` (ast/utilities.go:1342-1349).
    pub(crate) fn is_jsx_tag_name(&self, node: NodeId) -> bool {
        self.parent_of(node)
            .is_some_and(|parent| match self.data_of(parent) {
                NodeData::JsxOpeningElement(data) => data.tag_name == Some(node),
                NodeData::JsxClosingElement(data) => data.tag_name == Some(node),
                NodeData::JsxSelfClosingElement(data) => data.tag_name == Some(node),
                _ => false,
            })
    }

    /// tsgo `IsJSDocNameReferenceContext`: inside a JSDoc name reference or
    /// link. The walker never visits JSDoc nodes, so this is reached only
    /// through other callers.
    fn is_jsdoc_name_reference_context(&self, node: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(node) = current {
            if matches!(
                self.kind_of(node),
                SyntaxKind::JSDocNameReference
                    | SyntaxKind::JSDocLink
                    | SyntaxKind::JSDocLinkCode
                    | SyntaxKind::JSDocLinkPlain
            ) {
                return true;
            }
            current = self.parent_of(node);
        }
        false
    }

    /// The string literal cases of getSymbolAtLocation that name a module:
    /// `import x = require("m")`, an import or export declaration's
    /// specifier, `require("m")` in JavaScript, `import("m")` and the
    /// argument of a literal import type.
    fn is_module_specifier_literal(
        &self,
        node: NodeId,
        parent: NodeId,
        grand_parent: Option<NodeId>,
    ) -> bool {
        if let Some(grand_parent) = grand_parent {
            if let NodeData::ImportEqualsDeclaration(data) = self.data_of(grand_parent) {
                if data.module_reference == Some(parent)
                    && matches!(self.data_of(parent), NodeData::ExternalModuleReference(reference) if reference.expression == Some(node))
                {
                    return true;
                }
            }
        }
        match self.data_of(parent) {
            NodeData::ImportDeclaration(data) if data.module_specifier == Some(node) => {
                return true
            }
            NodeData::ExportDeclaration(data) if data.module_specifier == Some(node) => {
                return true
            }
            _ => {}
        }
        if grand_parent.is_some_and(|grand_parent| {
            self.is_variable_declaration_initialized_to_require(grand_parent)
        }) || self.is_import_call(parent)
        {
            return true;
        }
        if self.kind_of(parent) == SyntaxKind::LiteralType {
            if let Some(grand_parent) = grand_parent {
                if let NodeData::ImportType(data) = self.data_of(grand_parent) {
                    return data.argument == Some(parent);
                }
            }
        }
        false
    }

    /// The numeric-literal (and fallthrough string-literal) arm of
    /// getSymbolAtLocation: an element access argument or an indexed
    /// access type's literal index names a property.
    fn get_index_access_literal_symbol(
        &mut self,
        node: NodeId,
        parent: NodeId,
        grand_parent: Option<NodeId>,
    ) -> CheckResult<Option<SymbolId>> {
        let object_type = match self.data_of(parent) {
            NodeData::ElementAccessExpression(data) => {
                if data.argument_expression == Some(node) {
                    let Some(expression) = data.expression else {
                        return Ok(None);
                    };
                    Some(self.get_type_of_expression(expression)?)
                } else {
                    None
                }
            }
            NodeData::LiteralType(_) => match grand_parent.map(|g| (g, self.data_of(g))) {
                Some((_, NodeData::IndexedAccessType(data))) => {
                    let Some(object_type) = data.object_type else {
                        return Ok(None);
                    };
                    Some(self.get_type_from_type_node(object_type)?)
                }
                _ => None,
            },
            _ => None,
        };
        let Some(object_type) = object_type else {
            return Ok(None);
        };
        let Some(text) = self.literal_like_text(node) else {
            return Ok(None);
        };
        self.get_property_of_type_full(object_type, text.as_str())
    }

    fn literal_import_type_literal(&self, node: NodeId) -> Option<NodeId> {
        let NodeData::ImportType(data) = self.data_of(node) else {
            return None;
        };
        let argument = data.argument?;
        match self.data_of(argument) {
            NodeData::LiteralType(literal) => literal.literal.filter(|&literal| {
                matches!(
                    self.kind_of(literal),
                    SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                )
            }),
            _ => None,
        }
    }

    fn call_argument(&self, call: NodeId, index: usize) -> Option<NodeId> {
        let NodeData::CallExpression(data) = self.data_of(call) else {
            return None;
        };
        let arguments = data.arguments?;
        self.binder
            .source_of_node(call)
            .arena
            .node_array(arguments)
            .nodes
            .get(index)
            .copied()
    }

    /// `node.Text()` of a string, numeric or template literal.
    fn literal_like_text(&self, node: NodeId) -> Option<String> {
        match self.data_of(node) {
            NodeData::StringLiteral(data) => Some(data.text.to_string_lossy().into_owned()),
            NodeData::NumericLiteral(data) => Some(data.text.clone()),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(data.text.to_string_lossy().into_owned())
            }
            _ => None,
        }
    }

    /// `IsTypeAny` (checker/utilities.go): the any-flagged types.
    pub(crate) fn is_type_any_flagged(&self, ty: TypeId) -> bool {
        self.tables.flags_of(ty).intersects(TypeFlags::ANY)
    }
}
