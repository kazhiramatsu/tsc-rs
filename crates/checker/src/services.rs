//! tsgo `checker/services.go` and its exports for the language service:
//! the symbols a property or alias comes from, the declarations of a
//! contextual type's properties and of the index signatures at a location,
//! and the type argument of a known generic type.

use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckFlags, ContextFlags, LiteralValue, ObjectFlags, SymbolFlags, SymbolId, TypeData,
    TypeFlags, TypeId,
};

use crate::links::LinkSlot;
use crate::state::{CheckResult, CheckerState};

/// tsgo `knownGenericTypeNames`.
const KNOWN_GENERIC_TYPE_NAMES: [&str; 20] = [
    "Array",
    "ArrayLike",
    "ReadonlyArray",
    "Promise",
    "PromiseLike",
    "Iterable",
    "IterableIterator",
    "AsyncIterable",
    "Set",
    "WeakSet",
    "ReadonlySet",
    "Map",
    "WeakMap",
    "ReadonlyMap",
    "Partial",
    "Required",
    "Readonly",
    "Pick",
    "Omit",
    "NonNullable",
];

impl CheckerState<'_> {
    /// tsgo `GetContextualType`, the language service's entry: with
    /// `IgnoreNodeInferences`, the contextual type with inference from the
    /// node (and its ancestors up to the containing call) blocked.
    pub fn language_service_contextual_type(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult<Option<TypeId>> {
        if context_flags.intersects(ContextFlags::IGNORE_NODE_INFERENCES) {
            return self.run_with_inference_blocked_from_source_node(Some(node), node, |state| {
                state.get_contextual_type(node, context_flags)
            });
        }
        self.get_contextual_type(node, context_flags)
    }

    /// tsgo `GetContextualType` of a token tsc-rs's tree does not keep, by
    /// its parent.
    pub fn language_service_contextual_type_of_token(
        &mut self,
        parent: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult<Option<TypeId>> {
        if context_flags.intersects(ContextFlags::IGNORE_NODE_INFERENCES) {
            return self.run_with_inference_blocked_from_source_node(None, parent, |state| {
                state.get_contextual_type_of_token(parent, context_flags)
            });
        }
        self.get_contextual_type_of_token(parent, context_flags)
    }

    /// tsgo `runWithInferenceBlockedFromSourceNode` from `node`, or from a
    /// token (`None`) of `start`. tsgo marks the token too, but no inference
    /// reads a token.
    fn run_with_inference_blocked_from_source_node<T>(
        &mut self,
        node: Option<NodeId>,
        start: NodeId,
        run: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let mut containing_call = Some(start);
        while let Some(current) = containing_call {
            if self.is_call_like_expression(current) {
                break;
            }
            containing_call = self.parent_of(current);
        }
        if let Some(containing_call) = containing_call {
            let mut to_mark_skip = match node {
                Some(node) => Some(node),
                None => (start != containing_call).then_some(start),
            };
            while let Some(current) = to_mark_skip {
                self.skip_direct_inference_nodes.insert(current);
                to_mark_skip = self
                    .parent_of(current)
                    .filter(|&parent| parent != containing_call);
            }
        }
        self.is_inference_partially_blocked = true;
        let result = self.run_without_resolved_signature_caching(start, run);
        self.is_inference_partially_blocked = false;
        self.skip_direct_inference_nodes.clear();
        result
    }

    /// tsgo `runWithoutResolvedSignatureCaching`: `run` with the resolved
    /// signatures of the enclosing calls and functions, and the types of the
    /// enclosing function expressions, forgotten, then restored.
    fn run_without_resolved_signature_caching<T>(
        &mut self,
        node: NodeId,
        run: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let mut signatures = Vec::new();
        let mut types = Vec::new();
        let mut ancestor = self.call_like_or_function_like_ancestor(Some(node));
        while let Some(current) = ancestor {
            let previous = self.links.swap_node_resolved_signature_iife(
                self.speculation_depth,
                current,
                LinkSlot::Vacant,
            );
            signatures.push((current, previous));
            if self.is_function_expression_or_arrow_function(current) {
                let symbol = self.get_symbol_of_declaration(current)?;
                let previous = self
                    .links
                    .read_symbol(symbol, |links| links.type_of_symbol.get());
                self.links
                    .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Vacant);
                types.push((symbol, previous));
            }
            ancestor = self.call_like_or_function_like_ancestor(self.parent_of(current));
        }
        let result = run(self);
        for (current, previous) in signatures {
            self.links
                .swap_node_resolved_signature_iife(self.speculation_depth, current, previous);
        }
        for (symbol, previous) in types {
            self.links
                .set_symbol_type(self.speculation_depth, symbol, previous);
        }
        result
    }

    /// tsgo `ast.FindAncestor(node, ast.IsCallLikeOrFunctionLikeExpression)`.
    fn call_like_or_function_like_ancestor(&self, mut node: Option<NodeId>) -> Option<NodeId> {
        while let Some(current) = node {
            if self.is_call_like_expression(current)
                || self.is_function_expression_or_arrow_function(current)
            {
                return Some(current);
            }
            node = self.parent_of(current);
        }
        None
    }

    /// tsgo `GetRootSymbols`: the symbols a synthetic, spread, mapped or
    /// instantiated symbol comes from, the symbol itself when none.
    pub fn get_root_symbols(&mut self, symbol: SymbolId) -> CheckResult<Vec<SymbolId>> {
        let roots = self.get_immediate_root_symbols(symbol)?;
        if roots.is_empty() {
            return Ok(vec![symbol]);
        }
        let mut result = Vec::new();
        for root in roots {
            result.extend(self.get_root_symbols(root)?);
        }
        Ok(result)
    }

    /// tsgo `getImmediateRootSymbols`.
    fn get_immediate_root_symbols(&mut self, symbol: SymbolId) -> CheckResult<Vec<SymbolId>> {
        let check_flags = self.links.read_symbol(symbol, |links| links.check_flags);
        if check_flags.intersects(CheckFlags::SYNTHETIC) {
            let Some(containing_type) = *self.links.symbol_cold().containing_type.get(symbol)
            else {
                return Ok(Vec::new());
            };
            let name = self.binder.symbol(symbol).escaped_name;
            let mut roots = Vec::new();
            for ty in self.get_types_of_type(containing_type) {
                if let Some(property) = self.get_property_of_type_full(ty, name)? {
                    roots.push(property);
                }
            }
            return Ok(roots);
        }
        if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::TRANSIENT)
        {
            let cold = self.links.symbol_cold();
            if let Some(left) = *cold.left_spread.get(symbol) {
                let right = cold.right_spread.get(symbol).expect("a spread pair");
                return Ok(vec![left, right]);
            }
            if let Some(origin) = *cold.synthetic_origin.get(symbol) {
                return Ok(vec![origin]);
            }
            if let Some(target) = self.try_get_target(symbol) {
                return Ok(vec![target]);
            }
        }
        Ok(Vec::new())
    }

    /// tsgo `tryGetTarget`: the end of the symbol's chain of targets.
    fn try_get_target(&self, symbol: SymbolId) -> Option<SymbolId> {
        let mut target = None;
        let mut next = symbol;
        while let Some(found) = self.links.read_symbol(next, |links| links.target) {
            target = Some(found);
            next = found;
        }
        target
    }

    /// tsgo `GetFirstTypeArgumentFromKnownType`: the first type argument of
    /// a reference to (or an alias of) a known global generic type.
    pub fn get_first_type_argument_from_known_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let record = self.tables.type_of(ty);
        let (object_flags, symbol, alias_symbol) =
            (record.object_flags, record.symbol, record.alias_symbol);
        let alias_arguments = record.alias_type_arguments.clone();
        if object_flags.intersects(ObjectFlags::REFERENCE) {
            if let Some(symbol) = symbol {
                let name = self
                    .symbol_display_name(symbol)
                    .to_string_lossy()
                    .into_owned();
                if KNOWN_GENERIC_TYPE_NAMES.contains(&name.as_str()) {
                    let global = self.get_global_symbol(&name, SymbolFlags::TYPE, None)?;
                    let target = self.tables.reference_target(ty);
                    if global.is_some() && global == self.tables.type_of(target).symbol {
                        return Ok(self.get_type_arguments(ty)?.first().copied());
                    }
                }
            }
        }
        if let Some(alias_symbol) = alias_symbol {
            let name = self
                .symbol_display_name(alias_symbol)
                .to_string_lossy()
                .into_owned();
            if KNOWN_GENERIC_TYPE_NAMES.contains(&name.as_str()) {
                let global = self.get_global_symbol(&name, SymbolFlags::TYPE, None)?;
                if global == Some(alias_symbol) {
                    return Ok(alias_arguments.and_then(|arguments| arguments.first().copied()));
                }
            }
        }
        Ok(None)
    }

    /// tsgo `GetPropertySymbolsFromContextualType`: the symbols of one
    /// property of an object literal element's contextual type.
    pub fn get_property_symbols_from_contextual_type(
        &mut self,
        node: NodeId,
        contextual_type: TypeId,
        union_symbol_ok: bool,
    ) -> CheckResult<Vec<SymbolId>> {
        let Some(name) = self
            .name_of_node(node)
            .and_then(|name| self.try_get_text_of_property_name(name))
        else {
            return Ok(Vec::new());
        };
        if !self
            .tables
            .flags_of(contextual_type)
            .intersects(TypeFlags::UNION)
        {
            return Ok(self
                .get_property_of_type_full(contextual_type, name)?
                .into_iter()
                .collect());
        }
        let types = self.get_types_of_type(contextual_type);
        let parent = self.parent_of(node);
        let mut filtered = types.clone();
        if let Some(parent) = parent.filter(|&parent| {
            matches!(
                self.kind_of(parent),
                SyntaxKind::ObjectLiteralExpression | SyntaxKind::JsxAttributes
            )
        }) {
            let mut kept = Vec::new();
            for ty in filtered {
                if !self.is_type_invalid_due_to_union_discriminant(ty, parent)? {
                    kept.push(ty);
                }
            }
            filtered = kept;
        }
        let mut discriminated = Vec::new();
        for &ty in &filtered {
            if let Some(property) = self.get_property_of_type_full(ty, name)? {
                discriminated.push(property);
            }
        }
        if union_symbol_ok && (discriminated.is_empty() || discriminated.len() == types.len()) {
            if let Some(symbol) = self.get_property_of_type_full(contextual_type, name)? {
                return Ok(vec![symbol]);
            }
        }
        if filtered.is_empty() && discriminated.is_empty() {
            // A bad discriminant: again without discriminating.
            let mut properties = Vec::new();
            for ty in types {
                if let Some(property) = self.get_property_of_type_full(ty, name)? {
                    properties.push(property);
                }
            }
            return Ok(properties);
        }
        let mut unique = Vec::new();
        for symbol in discriminated {
            if !unique.contains(&symbol) {
                unique.push(symbol);
            }
        }
        Ok(unique)
    }

    /// tsgo `IsTypeInvalidDueToUnionDiscriminant`: whether a literal-typed
    /// property of the object literal rules the union member out.
    pub fn is_type_invalid_due_to_union_discriminant(
        &mut self,
        contextual_type: TypeId,
        object: NodeId,
    ) -> CheckResult<bool> {
        let properties = match self.data_of(object) {
            NodeData::ObjectLiteralExpression(data) => self.nodes_of(data.properties),
            NodeData::JsxAttributes(data) => self.nodes_of(data.properties),
            _ => Vec::new(),
        };
        for property in properties {
            let Some(name) = self.name_of_node(property) else {
                continue;
            };
            let name_type = if self.kind_of(name) == SyntaxKind::JsxNamespacedName {
                let text = tsc_binder::node_util::get_text_of_jsx_namespaced_name(
                    self.binder.source_of_node(name),
                    name,
                )
                .unwrap_or_default();
                self.tables.get_string_literal_type(text.as_str())
            } else {
                self.get_literal_type_from_property_name(name)?
            };
            if !self.is_type_usable_as_property_name(name_type) {
                continue;
            }
            let Some(property_name) = self.get_property_name_from_type(name_type) else {
                continue;
            };
            let Some(expected) =
                self.get_type_of_property_of_type_full(contextual_type, property_name)?
            else {
                continue;
            };
            if self.is_literal_type(expected) {
                let actual = self.get_type_of_node(property)?;
                if !self.is_type_assignable_to(actual, expected)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsgo `GetIndexSignaturesAtLocation`: the declarations of the index
    /// signatures a property access's name reads.
    pub fn get_index_signatures_at_location(&mut self, node: NodeId) -> CheckResult<Vec<NodeId>> {
        let mut signatures = Vec::new();
        let Some(parent) = self.parent_of(node) else {
            return Ok(signatures);
        };
        let NodeData::PropertyAccessExpression(access) = self.data_of(parent) else {
            return Ok(signatures);
        };
        if self.kind_of(node) != SyntaxKind::Identifier || access.name != Some(node) {
            return Ok(signatures);
        }
        let Some(expression) = access.expression else {
            return Ok(signatures);
        };
        let key_type = self.get_literal_type_from_property_name(node)?;
        let object_type = self.get_type_of_expression(expression)?;
        let distributed = if self
            .tables
            .flags_of(object_type)
            .intersects(TypeFlags::UNION)
        {
            self.get_types_of_type(object_type)
        } else {
            vec![object_type]
        };
        for ty in distributed {
            for info in self.get_applicable_index_infos(ty, key_type)? {
                if let Some(declaration) = info.declaration {
                    if !signatures.contains(&declaration) {
                        signatures.push(declaration);
                    }
                }
            }
        }
        Ok(signatures)
    }

    /// The check flags of a (transient) symbol (tsgo `Symbol.CheckFlags`).
    pub fn symbol_check_flags(&self, symbol: SymbolId) -> CheckFlags {
        self.links.read_symbol(symbol, |links| links.check_flags)
    }

    /// tsgo `IsLibTypeForHoverVerbosity`: a tuple, or a type (a reference's
    /// target) declared in a default library, which hover never expands.
    pub fn is_lib_type_for_hover_verbosity(&self, ty: TypeId) -> bool {
        crate::node_builder::is_lib_type(self, ty)
    }

    /// tsgo `GetTypeAliasTypeParameters`: the type alias's local type
    /// parameters.
    pub fn get_type_alias_type_parameters(&mut self, symbol: SymbolId) -> CheckResult<Vec<TypeId>> {
        self.get_declared_type_of_symbol(symbol)?;
        Ok(self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol))
    }

    /// tsgo `RemoveMissingOrUndefinedType`.
    pub fn remove_missing_or_undefined_type_public(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        self.remove_missing_or_undefined_type(ty)
    }

    /// tsgo `LiteralType.String()` (`ValueToString`): a string literal
    /// quoted, a number as JavaScript writes it, a bigint with its `n`.
    pub fn literal_type_value_text(&self, ty: TypeId) -> String {
        match &self.tables.type_of(ty).data {
            TypeData::Literal { value } => match value {
                LiteralValue::String(text) => format!(
                    "\"{}\"",
                    tsc_emitter::escape_double_quoted_string(text.units())
                ),
                LiteralValue::Number(value) => tsc_types::js_number_to_string(*value),
                LiteralValue::BigInt(value) => format!("{}n", value.to_base10_string()),
            },
            _ if ty == self.tables.intrinsics.true_fresh
                || ty == self.tables.intrinsics.true_regular =>
            {
                "true".to_owned()
            }
            _ if ty == self.tables.intrinsics.false_fresh
                || ty == self.tables.intrinsics.false_regular =>
            {
                "false".to_owned()
            }
            _ => String::new(),
        }
    }

    /// tsgo `GetBaseConstructorTypeOfClass`.
    pub fn get_base_constructor_type_of_class_public(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        self.get_base_constructor_type_of_class(ty)
    }

    /// tsgo `CheckFlagsIndexSymbol`: the property symbol
    /// `getApplicableIndexSymbol` makes for the index signatures a property
    /// access reads (named `__index`, unlike their declarations' signature
    /// symbol).
    pub fn is_index_symbol(&self, symbol: SymbolId) -> bool {
        let symbol = self.binder.symbol(symbol);
        symbol.flags.intersects(SymbolFlags::PROPERTY)
            && symbol.escaped_name.as_str() == Some(tsc_types::InternalSymbolName::INDEX)
    }
}

impl CheckerState<'_> {
    /// tsgo `IsUndefinedSymbol`.
    pub fn is_undefined_symbol(&self, symbol: SymbolId) -> bool {
        symbol == self.undefined_symbol
    }

    /// tsgo `IsArgumentsSymbol`.
    pub fn is_arguments_symbol(&self, symbol: SymbolId) -> bool {
        symbol == self.arguments_symbol
    }

    /// tsgo `IsUnknownSymbol`.
    pub fn is_unknown_symbol(&self, symbol: SymbolId) -> bool {
        symbol == self.unknown_symbol
    }
}

/// The services completions ask (tsgo `services.go`).
impl CheckerState<'_> {
    /// tsgo `SkipAlias` (utilities.go:1700-1705): an alias's target.
    pub fn skip_alias(&mut self, symbol: SymbolId) -> CheckResult<SymbolId> {
        if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            return self.get_aliased_symbol(symbol);
        }
        Ok(symbol)
    }

    /// tsgo `IsValidPropertyAccess` (services.go:164-178): whether the
    /// property `name` (escaped) of the accessed expression's type, of a
    /// qualified name's left side or of an import type is accessible there.
    pub fn is_valid_property_access(
        &mut self,
        node: NodeId,
        name: tsc_types::EscapedName,
    ) -> CheckResult<bool> {
        let (is_super, ty) = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => {
                let expression = data.expression.expect("property access expression");
                let is_super = self.kind_of(expression) == SyntaxKind::SuperKeyword;
                let ty = self.check_expression(expression, tsc_types::CheckMode::NORMAL)?;
                (is_super, self.get_widened_type(ty)?)
            }
            NodeData::QualifiedName(data) => {
                let left = data.left.expect("qualified name");
                let ty = self.check_expression(left, tsc_types::CheckMode::NORMAL)?;
                (false, self.get_widened_type(ty)?)
            }
            NodeData::ImportType(_) => (false, self.get_type_from_type_node(node)?),
            _ => panic!(
                "Unexpected node kind in isValidPropertyAccess: {:?}",
                self.kind_of(node)
            ),
        };
        // tsgo isValidPropertyAccessWithType: any is short-circuited.
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(true);
        }
        let Some(property) = self.get_property_of_type_full(ty, name)? else {
            return Ok(false);
        };
        self.is_property_accessible(node, is_super, false, ty, property)
    }

    /// tsgo `GetAllPossiblePropertiesOfTypes` (services.go:209-229): the
    /// properties of each type of a union, as properties of the union.
    pub fn get_all_possible_properties_of_types(
        &mut self,
        types: &[TypeId],
    ) -> CheckResult<Vec<SymbolId>> {
        let union_type = self.get_union_type_ex(types, tsc_types::UnionReduction::Literal)?;
        if !self
            .tables
            .flags_of(union_type)
            .intersects(TypeFlags::UNION)
        {
            return self.get_apparent_properties(union_type);
        }
        let mut names = std::collections::HashSet::new();
        let mut properties = Vec::new();
        for &member_type in types {
            for property in self.get_apparent_properties(member_type)? {
                let name = self.binder.symbol(property).escaped_name;
                if names.contains(&name) {
                    continue;
                }
                // None when the property is private.
                if let Some(property) =
                    self.create_union_or_intersection_property(union_type, name, false)?
                {
                    names.insert(name);
                    properties.push(property);
                }
            }
        }
        Ok(properties)
    }

    /// tsgo `GetNonOptionalType` (services.go:244-246).
    pub fn get_non_optional_type(&mut self, ty: TypeId) -> TypeId {
        self.remove_optional_type_marker(ty)
    }

    /// tsgo `GetStringIndexType` (services.go:248-250).
    pub fn get_string_index_type(&mut self, ty: TypeId) -> CheckResult<Option<TypeId>> {
        let string_type = self.tables.intrinsics.string;
        self.get_index_type_of_type(ty, string_type)
    }

    /// tsgo `GetNumberIndexType` (services.go:252-254).
    pub fn get_number_index_type(&mut self, ty: TypeId) -> CheckResult<Option<TypeId>> {
        let number_type = self.tables.intrinsics.number;
        self.get_index_type_of_type(ty, number_type)
    }

    /// tsgo `TryGetMemberInModuleExportsAndProperties` (services.go:
    /// 295-312): a module's export, or a property of its `export =` value.
    pub fn try_get_member_in_module_exports_and_properties(
        &mut self,
        member_name: &str,
        module_symbol: SymbolId,
    ) -> CheckResult<Option<SymbolId>> {
        if let Some(symbol) = self.try_get_member_in_module_exports(member_name, module_symbol)? {
            return Ok(Some(symbol));
        }
        let export_equals = self
            .resolve_external_module_symbol(Some(module_symbol), false)?
            .expect("resolved module symbol");
        if export_equals == module_symbol {
            return Ok(None);
        }
        let ty = self.get_type_of_symbol(export_equals)?;
        if self.should_treat_properties_of_external_module_as_exports(ty)? {
            return self.get_property_of_type_full(
                ty,
                tsc_binder::escape_leading_underscores(member_name),
            );
        }
        Ok(None)
    }

    /// tsgo `GetExportsAndPropertiesOfModule` (services.go:840-851): unlike
    /// `GetExportsOfModule`, the properties of an `export =` value too.
    pub fn get_exports_and_properties_of_module(
        &mut self,
        module_symbol: SymbolId,
    ) -> CheckResult<Vec<SymbolId>> {
        let mut exports = self.get_exports_of_module_symbols(module_symbol)?;
        let export_equals = self
            .resolve_external_module_symbol(Some(module_symbol), false)?
            .expect("resolved module symbol");
        if export_equals != module_symbol {
            let ty = self.get_type_of_symbol(export_equals)?;
            if self.should_treat_properties_of_external_module_as_exports(ty)? {
                exports.extend(self.get_properties_of_type(ty)?);
            }
        }
        Ok(exports)
    }

    /// tsgo `shouldTreatPropertiesOfExternalModuleAsExports`
    /// (services.go:318-324).
    fn should_treat_properties_of_external_module_as_exports(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<bool> {
        Ok(!self.tables.flags_of(ty).intersects(TypeFlags::PRIMITIVE)
            || self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CLASS)
            || self.is_array_type(ty)?
            || self.tables.is_tuple_type(ty))
    }

    /// tsgo `GetJsxIntrinsicTagNamesAt` (services.go:857-864): the
    /// properties of `JSX.IntrinsicElements`.
    pub fn get_jsx_intrinsic_tag_names_at(
        &mut self,
        location: NodeId,
    ) -> CheckResult<Vec<SymbolId>> {
        let intrinsics = self.get_jsx_type(crate::jsx::JSX_INTRINSIC_ELEMENTS, location)?;
        self.get_properties_of_type(intrinsics)
    }

    /// tsgo `GetContextualTypeForJsxAttribute` (services.go:865-867).
    pub fn get_contextual_type_for_jsx_attribute_like(
        &mut self,
        attribute: NodeId,
    ) -> CheckResult<Option<TypeId>> {
        self.get_contextual_type_for_jsx_attribute(attribute, ContextFlags::NONE)
    }

    /// tsgo `GetTypeOfPropertyOfContextualType` (exports.go:175-177), by
    /// the property's escaped name.
    pub fn get_type_of_property_of_contextual_type_by_name(
        &mut self,
        ty: TypeId,
        name: tsc_types::EscapedName,
    ) -> CheckResult<Option<TypeId>> {
        self.get_type_of_property_of_contextual_type(ty, name, None)
    }

    /// tsgo `GetElementTypeOfArrayType` (services.go:256-258).
    pub fn element_type_of_array_type(&mut self, ty: TypeId) -> CheckResult<Option<TypeId>> {
        self.get_element_type_of_array_type(ty)
    }

    /// tsgo `GetContextualTypeForArrayLiteralAtPosition` (services.go:
    /// 951-976): the contextual type of the element at a position of an
    /// array literal that may be unfinished, so of unknown length.
    pub fn get_contextual_type_for_array_literal_at_position(
        &mut self,
        contextual_array_type: TypeId,
        array_literal: NodeId,
        position: u32,
    ) -> CheckResult<Option<TypeId>> {
        let elements = match self.data_of(array_literal) {
            NodeData::ArrayLiteralExpression(data) => data.elements,
            _ => None,
        };
        let elements = match elements {
            Some(elements) => self.binder.node_array(elements).nodes.to_vec(),
            None => Vec::new(),
        };
        let mut first_spread_index = None;
        let mut last_spread_index = None;
        let mut element_index = 0;
        for (index, &element) in elements.iter().enumerate() {
            if self.binder.node_record(element).pos < position {
                element_index += 1;
            }
            if self.kind_of(element) == SyntaxKind::SpreadElement {
                if first_spread_index.is_none() {
                    first_spread_index = Some(index as u32);
                }
                last_spread_index = Some(index as u32);
            }
        }
        self.get_contextual_type_for_element_expression(
            Some(contextual_array_type),
            element_index,
            None,
            first_spread_index,
            last_spread_index,
        )
    }

    /// tsgo `GetTypeArgumentConstraint` (services.go:699-704, 741-811): the
    /// constraint a type argument must meet, across the signatures of a
    /// call-like expression, of an instantiation expression, or of a type
    /// reference's type parameters.
    pub fn get_type_argument_constraint(&mut self, node: NodeId) -> CheckResult<Option<TypeId>> {
        if !self.is_type_node_kind(self.kind_of(node)) {
            return Ok(None);
        }
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        let type_arguments = match self.data_of(parent) {
            NodeData::CallExpression(data) => data.type_arguments,
            NodeData::NewExpression(data) => data.type_arguments,
            NodeData::TaggedTemplateExpression(data) => data.type_arguments,
            NodeData::TypeReference(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(_) => self.heritage_type_arguments(parent),
            NodeData::ImportType(data) => data.type_arguments,
            NodeData::TypeQuery(data) => data.type_arguments,
            NodeData::JsxOpeningElement(data) => data.type_arguments,
            NodeData::JsxSelfClosingElement(data) => data.type_arguments,
            _ => None,
        };
        let Some(position) = type_arguments.and_then(|list| {
            self.binder
                .node_array(list)
                .nodes
                .iter()
                .position(|&argument| argument == node)
        }) else {
            return Ok(None);
        };
        // A type argument of a call, a `new`, a decorator, an instantiation
        // expression, or a generic type's instantiation.
        if self.is_call_like_expression(parent) {
            let signatures = self.uninstantiated_signatures(parent)?;
            return Ok(Some(
                self.type_parameter_constraint_for_position_across_signatures(
                    &signatures,
                    position,
                )?,
            ));
        }
        let grandparent = self.parent_of(parent);
        if let Some(decorator) =
            grandparent.filter(|&grandparent| self.kind_of(grandparent) == SyntaxKind::Decorator)
        {
            let signatures = self.uninstantiated_signatures(decorator)?;
            return Ok(Some(
                self.type_parameter_constraint_for_position_across_signatures(
                    &signatures,
                    position,
                )?,
            ));
        }
        if self.kind_of(parent) == SyntaxKind::ExpressionWithTypeArguments
            && grandparent.is_some_and(|grandparent| {
                self.kind_of(grandparent) == SyntaxKind::ExpressionStatement
            })
        {
            let NodeData::ExpressionWithTypeArguments(data) = self.data_of(parent) else {
                unreachable!("an expression with type arguments");
            };
            let expression = data
                .expression
                .expect("an instantiation expression's expression");
            let uninstantiated_type =
                self.check_expression(expression, tsc_types::CheckMode::NORMAL)?;
            let call_signatures = self
                .get_signatures_of_type(uninstantiated_type, crate::state::SignatureKind::Call)?;
            let call_constraint = self.type_parameter_constraint_for_position_across_signatures(
                &call_signatures,
                position,
            )?;
            let construct_signatures = self.get_signatures_of_type(
                uninstantiated_type,
                crate::state::SignatureKind::Construct,
            )?;
            let construct_constraint = self
                .type_parameter_constraint_for_position_across_signatures(
                    &construct_signatures,
                    position,
                )?;
            // An instantiation expression instantiates both kinds of
            // signature, so the arguments meet both constraints.
            if self
                .tables
                .flags_of(construct_constraint)
                .intersects(TypeFlags::NEVER)
            {
                return Ok(Some(call_constraint));
            }
            if self
                .tables
                .flags_of(call_constraint)
                .intersects(TypeFlags::NEVER)
            {
                return Ok(Some(construct_constraint));
            }
            return Ok(Some(self.get_intersection_type(
                &[call_constraint, construct_constraint],
                tsc_types::IntersectionFlags::NONE,
            )?));
        }
        if matches!(
            self.kind_of(parent),
            SyntaxKind::TypeReference | SyntaxKind::ExpressionWithTypeArguments
        ) {
            let Some(type_parameters) =
                self.get_type_parameters_for_type_reference_or_import(parent)?
            else {
                return Ok(None);
            };
            if position >= type_parameters.len() {
                return Ok(None);
            }
            if let Some(constraint) =
                self.get_constraint_of_type_parameter(type_parameters[position])?
            {
                let arguments = self.get_effective_type_arguments(parent, &type_parameters)?;
                let mapper = self.create_type_mapper(type_parameters, Some(arguments));
                return Ok(Some(self.instantiate_type(constraint, Some(mapper))?));
            }
        }
        Ok(None)
    }

    /// tsgo `getTypeParameterConstraintForPositionAcrossSignatures`: the
    /// union of the constraints of each signature's type parameter there.
    fn type_parameter_constraint_for_position_across_signatures(
        &mut self,
        signatures: &[crate::state::SignatureId],
        position: usize,
    ) -> CheckResult<TypeId> {
        let mut constraints = Vec::new();
        for &signature in signatures {
            let Some(type_parameter) = self
                .signature_of(signature)
                .type_parameters
                .as_ref()
                .and_then(|parameters| parameters.get(position).copied())
            else {
                continue;
            };
            if let Some(constraint) = self.get_constraint_of_type_parameter(type_parameter)? {
                constraints.push(constraint);
            }
        }
        self.get_union_type_ex(&constraints, tsc_types::UnionReduction::Literal)
    }

    /// tsgo `getUninstantiatedSignatures`: the generic signatures of a
    /// call-like expression's callee.
    fn uninstantiated_signatures(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Vec<crate::state::SignatureId>> {
        use crate::state::SignatureKind;
        let (target, kind) = match self.data_of(node) {
            NodeData::CallExpression(data) => (data.expression, SignatureKind::Call),
            NodeData::Decorator(data) => (data.expression, SignatureKind::Call),
            NodeData::NewExpression(data) => (data.expression, SignatureKind::Construct),
            NodeData::JsxOpeningElement(data) => {
                let tag_name = data.tag_name.expect("a tag name");
                if self.is_jsx_intrinsic_tag_name(tag_name) {
                    return Ok(Vec::new());
                }
                (Some(tag_name), SignatureKind::Call)
            }
            NodeData::JsxSelfClosingElement(data) => {
                let tag_name = data.tag_name.expect("a tag name");
                if self.is_jsx_intrinsic_tag_name(tag_name) {
                    return Ok(Vec::new());
                }
                (Some(tag_name), SignatureKind::Call)
            }
            NodeData::TaggedTemplateExpression(data) => (data.tag, SignatureKind::Call),
            _ => return Ok(Vec::new()),
        };
        let Some(target) = target else {
            return Ok(Vec::new());
        };
        let ty = self.get_type_of_expression(target)?;
        self.get_signatures_of_type(ty, kind)
    }

    /// tsgo `GetAccessibleSymbolChain` (symbolaccessibility.go:444-458).
    pub fn get_accessible_symbol_chain(
        &mut self,
        symbol: SymbolId,
        enclosing_declaration: Option<NodeId>,
        meaning: SymbolFlags,
        use_only_external_aliasing: bool,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        self.accessible_symbol_chain_at(
            symbol,
            meaning,
            enclosing_declaration,
            use_only_external_aliasing,
        )
    }

    /// tsgo `Node.Type`: the type annotation, else in JavaScript the type a
    /// JSDoc tag gave the declaration (`@type`, `@param` or `@return`).
    pub fn type_node_of(&self, node: NodeId) -> Option<NodeId> {
        self.type_annotation_of(node)
            .or_else(|| self.reparsed_type_node(node))
    }

    /// tsgo `TryGetThisTypeAtEx` (checker.go:12378-12384): the type of
    /// `this` at `node`, in `container` or the node's own `this` container.
    pub fn try_get_this_type_at_ex(
        &mut self,
        node: NodeId,
        include_global_this: bool,
        container: Option<NodeId>,
    ) -> CheckResult<Option<TypeId>> {
        let Some(container) =
            container.or_else(|| crate::expr::get_this_container_full(self, node, false, false))
        else {
            return Ok(None);
        };
        self.try_get_this_type_at(node, include_global_this, container)
    }
}
