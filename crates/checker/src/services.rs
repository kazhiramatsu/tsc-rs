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
            return self.run_with_inference_blocked_from_source_node(node, |state| {
                state.get_contextual_type(node, context_flags)
            });
        }
        self.get_contextual_type(node, context_flags)
    }

    /// tsgo `runWithInferenceBlockedFromSourceNode`.
    fn run_with_inference_blocked_from_source_node<T>(
        &mut self,
        node: NodeId,
        run: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let mut containing_call = Some(node);
        while let Some(current) = containing_call {
            if self.is_call_like_expression(current) {
                break;
            }
            containing_call = self.parent_of(current);
        }
        if let Some(containing_call) = containing_call {
            let mut to_mark_skip = node;
            loop {
                self.skip_direct_inference_nodes.insert(to_mark_skip);
                match self.parent_of(to_mark_skip) {
                    Some(parent) if parent != containing_call => to_mark_skip = parent,
                    _ => break,
                }
            }
        }
        self.is_inference_partially_blocked = true;
        let result = self.run_without_resolved_signature_caching(node, run);
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
}
