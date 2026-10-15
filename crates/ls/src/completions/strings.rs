//! tsgo `ls/string_completions.go`: completions inside a string literal,
//! from the literal types it may be, the properties it may name or the
//! paths and modules it may import (`paths.rs`).

use std::collections::HashSet;

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_emitter::LiteralQuote;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ContextFlags, LiteralValue, SymbolId, TypeData, TypeFlags, TypeId};

use crate::lsutil::{ScriptElementKind, ScriptElementKindModifier};
use crate::utilities::Touching;

use super::context::{is_string_literal_like, Completions};
use super::data::{CompletionDataData, JsxInitializer};
use super::paths::PathCompletions;
use super::{
    default_commit_characters, items, sort_text, ClientSupport, CompletionKind, CompletionList,
    KeywordCompletionFilters,
};

/// tsgo `completionsFromTypes`.
pub(super) struct CompletionsFromTypes {
    pub(super) types: Vec<TypeId>,
    is_new_identifier: bool,
}

/// tsgo `completionsFromProperties`.
pub(super) struct CompletionsFromProperties {
    pub(super) symbols: Vec<SymbolId>,
    has_index_signature: bool,
}

/// tsgo `stringLiteralCompletions`: the kind of it that is set.
pub(super) enum StringLiteralCompletions {
    Types(CompletionsFromTypes),
    Properties(CompletionsFromProperties),
    Paths(PathCompletions),
}

/// tsgo `toCompletionsFromTypes`: none for no types.
fn to_completions_from_types(types: Vec<TypeId>) -> Option<CompletionsFromTypes> {
    (!types.is_empty()).then_some(CompletionsFromTypes {
        types,
        is_new_identifier: false,
    })
}

impl Completions<'_, '_, '_> {
    /// tsgo `getStringLiteralCompletions`: the completions in the string
    /// literal the position is in.
    pub(super) fn string_literal_completions(
        &mut self,
        context_token: Option<Touching>,
    ) -> CheckResult<Option<CompletionList>> {
        if self.is_in_reference_comment() {
            let completion = self.triple_slash_reference_completions();
            return Ok(self.convert_path_completions(completion));
        }
        if !self.is_in_string(self.position, context_token) {
            return Ok(None);
        }
        let Some(Touching::Node(token)) =
            context_token.filter(|&token| is_string_literal_like(self.kind(token)))
        else {
            return Ok(None);
        };
        let completion = self.string_literal_completion_entries(token)?;
        self.convert_string_literal_completions(completion, token)
    }

    /// tsgo `convertStringLiteralCompletions`.
    fn convert_string_literal_completions(
        &mut self,
        completion: Option<StringLiteralCompletions>,
        token: NodeId,
    ) -> CheckResult<Option<CompletionList>> {
        let Some(completion) = completion else {
            return Ok(None);
        };
        let optional_replacement_range = self.range_from_string_literal_like_content(token);
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        match completion {
            StringLiteralCompletions::Paths(completion) => {
                Ok(self.convert_path_completions(Some(completion)))
            }
            StringLiteralCompletions::Properties(completion) => {
                let data = CompletionDataData {
                    symbols: completion.symbols,
                    completion_kind: CompletionKind::String,
                    property_access_to_convert: None,
                    is_new_identifier_location: completion.has_index_signature,
                    location: Touching::Node(self.source.root),
                    keyword_filters: KeywordCompletionFilters::None,
                    literals: Vec::new(),
                    symbol_to_origin_info_map: Default::default(),
                    symbol_to_sort_text_map: Default::default(),
                    recommended_completion: None,
                    context_token: Some(Touching::Node(token)),
                    jsx_initializer: JsxInitializer::default(),
                    inside_js_doc_tag_type_expression: false,
                    is_type_only_location: false,
                    is_jsx_identifier_expected: false,
                    is_right_of_open_tag: false,
                    is_right_of_dot_or_question_dot: false,
                    has_unresolved_auto_imports: false,
                    default_commit_characters: Vec::new(),
                    auto_imports: Vec::new(),
                    import_statement_completion: None,
                };
                let (_, mut items, symbols) =
                    self.completion_entries_from_symbols(&data, Some(Touching::Node(token)))?;
                let item_defaults = items::set_item_defaults(
                    ClientSupport(self.request.capabilities),
                    position,
                    &mut items,
                    Some(default_commit_characters(completion.has_index_signature)),
                    optional_replacement_range,
                );
                Ok(Some(CompletionList {
                    is_incomplete: false,
                    item_defaults,
                    items,
                    symbols,
                }))
            }
            StringLiteralCompletions::Types(completion) => {
                // tsgo reads the quote from the literal's value, not its
                // text.
                let quote = match self.data(token) {
                    NodeData::NoSubstitutionTemplateLiteral(_) => LiteralQuote::Backtick,
                    NodeData::StringLiteral(data)
                        if data.text.code_units().next() == Some(b'\'' as u16) =>
                    {
                        LiteralQuote::Single
                    }
                    _ => LiteralQuote::Double,
                };
                let replacement_span =
                    self.replacement_range_for_context_token(Some(Touching::Node(token)));
                let file_name = self.file_name();
                let mut items = Vec::with_capacity(completion.types.len());
                for ty in completion.types {
                    let TypeData::Literal {
                        value: LiteralValue::String(text),
                    } = &self.request.state.tables.type_of(ty).data
                    else {
                        continue;
                    };
                    items.push(items::create_lsp_completion_item(
                        self.text(),
                        &file_name,
                        self.position,
                        items::ItemSpec {
                            name: tsc_emitter::escape_string(text.units(), quote),
                            sort_text: sort_text::LOCATION_PRIORITY.to_owned(),
                            element_kind: Some(ScriptElementKind::String),
                            kind_modifiers: ScriptElementKindModifier::NONE,
                            replacement_span: replacement_span.clone(),
                            ..items::ItemSpec::default()
                        },
                    ));
                }
                let item_defaults = items::set_item_defaults(
                    ClientSupport(self.request.capabilities),
                    position,
                    &mut items,
                    Some(default_commit_characters(completion.is_new_identifier)),
                    None,
                );
                Ok(Some(CompletionList {
                    is_incomplete: false,
                    item_defaults,
                    items,
                    symbols: Vec::new(),
                }))
            }
        }
    }

    /// tsgo `getStringLiteralCompletionEntries`.
    pub(super) fn string_literal_completion_entries(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<StringLiteralCompletions>> {
        let parent = self.walk_up_parentheses(self.parent_of(node).expect("a literal's parent"));
        match self.data(parent) {
            NodeData::LiteralType(_) => {
                let grandparent = self
                    .walk_up_parentheses(self.parent_of(parent).expect("a literal type's parent"));
                if self.node_kind(grandparent) == SyntaxKind::ImportType {
                    return Ok(Some(StringLiteralCompletions::Paths(
                        self.string_literal_completions_from_module_names(node)?,
                    )));
                }
                self.unionable_literal_type_completions(grandparent, parent)
            }
            NodeData::PropertyAssignment(_) => {
                let object = self.parent_of(parent);
                if object.is_some_and(|object| {
                    self.node_kind(object) == SyntaxKind::ObjectLiteralExpression
                }) && self.name_of(parent) == Some(node)
                {
                    // The quoted names of the object literal's properties.
                    return Ok(self
                        .string_literal_completions_for_object_literal(object.expect("tested"))?
                        .map(StringLiteralCompletions::Properties));
                }
                if self
                    .find_ancestor(object, |this, node| {
                        this.request.is_call_like_expression(node)
                    })
                    .is_some()
                {
                    let mut uniques = HashSet::new();
                    let contextual = self
                        .request
                        .state
                        .language_service_contextual_type(node, ContextFlags::NONE)?;
                    let mut types = self.string_literal_types(contextual, &mut uniques)?;
                    let contextual = self.request.state.language_service_contextual_type(
                        node,
                        ContextFlags::IGNORE_NODE_INFERENCES,
                    )?;
                    types.extend(self.string_literal_types(contextual, &mut uniques)?);
                    return Ok(
                        to_completions_from_types(types).map(StringLiteralCompletions::Types)
                    );
                }
                Ok(self
                    .contextual_string_literals(ContextFlags::NONE, node)?
                    .map(StringLiteralCompletions::Types))
            }
            NodeData::ElementAccessExpression(access) => {
                let argument = access
                    .argument_expression
                    .map(|argument| node_util::skip_parentheses_pub(self.source, argument));
                if argument != Some(node) {
                    return Ok(None);
                }
                // The names of the expression's properties.
                let expression = access.expression.expect("an element access's expression");
                let ty = self.request.state.get_type_at_location(expression)?;
                Ok(Some(StringLiteralCompletions::Properties(
                    self.string_literal_completions_from_properties(ty)?,
                )))
            }
            NodeData::CallExpression(_)
            | NodeData::NewExpression(_)
            | NodeData::JsxAttribute(_)
                if !self.is_require_call_argument(node) && !self.is_import_call(parent) =>
            {
                let argument_node = if self.node_kind(parent) == SyntaxKind::JsxAttribute {
                    self.parent_of(parent).expect("a JSX attribute's parent")
                } else {
                    node
                };
                // The literal types of the parameters of the call's
                // signatures.
                let Some(info) = self
                    .argument_info_for_completions(Touching::Node(argument_node), self.position)?
                else {
                    return Ok(None);
                };
                if let Some(result) = self.string_literal_completions_from_signature(
                    info.invocation,
                    node,
                    info.argument_index,
                    info.argument_count,
                )? {
                    return Ok(Some(StringLiteralCompletions::Types(result)));
                }
                Ok(self
                    .contextual_string_literals(ContextFlags::NONE, node)?
                    .map(StringLiteralCompletions::Types))
            }
            // The known module names or a path to a module: an import, an
            // export, `require(...)` or `import(...)`.
            NodeData::CallExpression(_)
            | NodeData::NewExpression(_)
            | NodeData::JsxAttribute(_)
            | NodeData::ImportDeclaration(_)
            | NodeData::ExportDeclaration(_)
            | NodeData::ExternalModuleReference(_)
            | NodeData::JSDocImportTag(_) => Ok(Some(StringLiteralCompletions::Paths(
                self.string_literal_completions_from_module_names(node)?,
            ))),
            NodeData::CaseClause(_) => {
                let case_block = self.parent_of(parent).expect("a case clause's block");
                let tracker = self.case_clause_tracker(case_block)?;
                let Some(contextual) =
                    self.contextual_string_literals(ContextFlags::IGNORE_NODE_INFERENCES, node)?
                else {
                    return Ok(None);
                };
                let literals = contextual
                    .types
                    .into_iter()
                    .filter(|&ty| match &self.request.state.tables.type_of(ty).data {
                        TypeData::Literal { value } => !tracker.has_value(value),
                        _ => true,
                    })
                    .collect();
                Ok(Some(StringLiteralCompletions::Types(
                    CompletionsFromTypes {
                        types: literals,
                        is_new_identifier: false,
                    },
                )))
            }
            NodeData::ImportSpecifier(_) | NodeData::ExportSpecifier(_) => {
                self.import_or_export_specifier_completions(parent, node)
            }
            NodeData::BinaryExpression(binary)
                if binary
                    .operator_token
                    .is_some_and(|operator| self.node_kind(operator) == SyntaxKind::InKeyword) =>
            {
                let right = binary.right.expect("a binary expression's right");
                let ty = self.request.state.get_type_at_location(right)?;
                let properties = self.properties_for_completion(ty)?;
                let symbols = properties
                    .into_iter()
                    .filter(|&symbol| !self.is_private_identifier_class_element_symbol(symbol))
                    .collect();
                Ok(Some(StringLiteralCompletions::Properties(
                    CompletionsFromProperties {
                        symbols,
                        has_index_signature: false,
                    },
                )))
            }
            NodeData::BinaryExpression(_) => Ok(self
                .contextual_string_literals(ContextFlags::NONE, node)?
                .map(StringLiteralCompletions::Types)),
            _ => {
                if let Some(result) =
                    self.contextual_string_literals(ContextFlags::IGNORE_NODE_INFERENCES, node)?
                {
                    return Ok(Some(StringLiteralCompletions::Types(result)));
                }
                Ok(self
                    .contextual_string_literals(ContextFlags::NONE, node)?
                    .map(StringLiteralCompletions::Types))
            }
        }
    }

    /// The `import { "|" } from` and `export { "|" } from` case: the
    /// module's exports not imported or exported yet. tsgo looks for the
    /// symbol of the declaration itself, not of its module specifier, so
    /// this finds none.
    fn import_or_export_specifier_completions(
        &mut self,
        specifier: NodeId,
        node: NodeId,
    ) -> CheckResult<Option<StringLiteralCompletions>> {
        let property_name = match self.data(specifier) {
            NodeData::ImportSpecifier(data) => data.property_name,
            NodeData::ExportSpecifier(data) => data.property_name,
            _ => None,
        };
        // Not in `export { "..." as "|" } from`.
        if property_name.is_some_and(|property_name| property_name != node) {
            return Ok(None);
        }
        let named_imports_or_exports = self.parent_of(specifier).expect("a specifier's list");
        let declaration = if self.node_kind(named_imports_or_exports) == SyntaxKind::NamedImports {
            self.parent_of(named_imports_or_exports)
                .and_then(|clause| self.parent_of(clause))
        } else {
            self.parent_of(named_imports_or_exports)
        };
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        let Some(module_symbol) = self.request.state.get_symbol_at_location(declaration)? else {
            return Ok(None);
        };
        let exports = self
            .request
            .state
            .get_exports_and_properties_of_module(module_symbol)?;
        let elements = match self.data(named_imports_or_exports) {
            NodeData::NamedImports(data) => self.node_list(data.elements),
            NodeData::NamedExports(data) => self.node_list(data.elements),
            _ => &[],
        };
        let existing: HashSet<String> = elements
            .iter()
            .filter_map(|&element| {
                let (property_name, name) = match self.data(element) {
                    NodeData::ImportSpecifier(data) => (data.property_name, data.name),
                    NodeData::ExportSpecifier(data) => (data.property_name, data.name),
                    _ => (None, None),
                };
                property_name
                    .or(name)
                    .map(|name| self.module_export_name_text(name))
            })
            .collect();
        let symbols = exports
            .into_iter()
            .filter(|&symbol| {
                let name = self.raw_symbol_name(symbol);
                name != "default" && !existing.contains(&name)
            })
            .collect();
        Ok(Some(StringLiteralCompletions::Properties(
            CompletionsFromProperties {
                symbols,
                has_index_signature: false,
            },
        )))
    }

    /// tsgo `fromContextualType`: the string literal types of the
    /// contextual type.
    fn contextual_string_literals(
        &mut self,
        flags: ContextFlags,
        node: NodeId,
    ) -> CheckResult<Option<CompletionsFromTypes>> {
        let contextual = self.request.contextual_type_from_parent(node, flags)?;
        let types = self.string_literal_types(contextual, &mut HashSet::new())?;
        Ok(to_completions_from_types(types))
    }

    /// tsgo `fromUnionableLiteralType`: a literal type in a type argument,
    /// an indexed access type, a union or a property signature.
    fn unionable_literal_type_completions(
        &mut self,
        grandparent: NodeId,
        parent: NodeId,
    ) -> CheckResult<Option<StringLiteralCompletions>> {
        match self.node_kind(grandparent) {
            SyntaxKind::CallExpression
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::JsxOpeningElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::NewExpression
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::TypeReference => {
                let Some(type_argument) = self.find_ancestor(Some(parent), |this, node| {
                    this.parent_of(node) == Some(grandparent)
                }) else {
                    return Ok(None);
                };
                let constraint = self
                    .request
                    .state
                    .get_type_argument_constraint(type_argument)?;
                let types = self.string_literal_types(constraint, &mut HashSet::new())?;
                Ok(Some(StringLiteralCompletions::Types(
                    CompletionsFromTypes {
                        types,
                        is_new_identifier: false,
                    },
                )))
            }
            SyntaxKind::IndexedAccessType => {
                // The apparent property names of the object type.
                let NodeData::IndexedAccessType(data) = self.data(grandparent) else {
                    unreachable!("an indexed access type");
                };
                let index_type = data.index_type.expect("an indexed access's index type");
                let object_type = data.object_type.expect("an indexed access's object type");
                let index_record = self.record(index_type);
                if !(index_record.pos <= self.position && self.position <= index_record.end) {
                    return Ok(None);
                }
                let ty = self.request.state.get_type_from_type_node(object_type)?;
                Ok(Some(StringLiteralCompletions::Properties(
                    self.string_literal_completions_from_properties(ty)?,
                )))
            }
            SyntaxKind::UnionType => {
                let outer = self.walk_up_parentheses(
                    self.parent_of(grandparent).expect("a union type's parent"),
                );
                let Some(result) = self.unionable_literal_type_completions(outer, parent)? else {
                    return Ok(None);
                };
                let already_used =
                    self.already_used_types_in_string_literal_union(grandparent, parent);
                Ok(Some(match result {
                    StringLiteralCompletions::Paths(_) => return Ok(None),
                    StringLiteralCompletions::Properties(result) => {
                        StringLiteralCompletions::Properties(CompletionsFromProperties {
                            symbols: result
                                .symbols
                                .into_iter()
                                .filter(|&symbol| {
                                    let name = self.raw_symbol_name(symbol);
                                    !already_used.contains(&name.encode_utf16().collect())
                                })
                                .collect(),
                            has_index_signature: result.has_index_signature,
                        })
                    }
                    StringLiteralCompletions::Types(result) => {
                        StringLiteralCompletions::Types(CompletionsFromTypes {
                            types: result
                                .types
                                .into_iter()
                                .filter(|&ty| match &self.request.state.tables.type_of(ty).data {
                                    TypeData::Literal {
                                        value: LiteralValue::String(text),
                                    } => !already_used.contains(&text.units().to_vec()),
                                    _ => true,
                                })
                                .collect(),
                            is_new_identifier: false,
                        })
                    }
                }))
            }
            SyntaxKind::PropertySignature => {
                let constraint =
                    self.constraint_of_type_argument_property(Touching::Node(grandparent))?;
                let types = self.string_literal_types(constraint, &mut HashSet::new())?;
                Ok(Some(StringLiteralCompletions::Types(
                    CompletionsFromTypes {
                        types,
                        is_new_identifier: false,
                    },
                )))
            }
            _ => Ok(None),
        }
    }

    /// tsgo `stringLiteralCompletionsForObjectLiteral`: the properties of
    /// the object literal's contextual type.
    fn string_literal_completions_for_object_literal(
        &mut self,
        object: NodeId,
    ) -> CheckResult<Option<CompletionsFromProperties>> {
        let Some(contextual_type) = self
            .request
            .state
            .language_service_contextual_type(object, ContextFlags::NONE)?
        else {
            return Ok(None);
        };
        let completions_type = self
            .request
            .state
            .language_service_contextual_type(object, ContextFlags::IGNORE_NODE_INFERENCES)?;
        let symbols =
            self.properties_for_object_expression(contextual_type, completions_type, object)?;
        Ok(Some(CompletionsFromProperties {
            symbols,
            has_index_signature: self.has_index_signature(contextual_type)?,
        }))
    }

    /// tsgo `stringLiteralCompletionsFromProperties`: the apparent
    /// properties of a type but its private names.
    fn string_literal_completions_from_properties(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<CompletionsFromProperties> {
        let symbols = self
            .request
            .state
            .get_apparent_properties(ty)?
            .into_iter()
            .filter(|&symbol| !self.is_private_identifier_class_element_symbol(symbol))
            .collect();
        Ok(CompletionsFromProperties {
            symbols,
            has_index_signature: self.has_index_signature(ty)?,
        })
    }

    /// tsgo `getStringLiteralCompletionsFromSignature`: the string literal
    /// types of the argument's parameter in the call's candidates.
    fn string_literal_completions_from_signature(
        &mut self,
        call: NodeId,
        argument: NodeId,
        argument_index: usize,
        argument_count: usize,
    ) -> CheckResult<Option<CompletionsFromTypes>> {
        let is_jsx = matches!(
            self.node_kind(call),
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        );
        let editing_argument = if is_jsx {
            self.find_ancestor(self.parent_of(argument), |this, node| {
                this.node_kind(node) == SyntaxKind::JsxAttribute
            })
            .expect("a JSX attribute around the literal")
        } else {
            argument
        };
        let candidates = self
            .request
            .state
            .get_candidate_signatures_for_string_literal_completions(call, editing_argument)?;
        let mut is_new_identifier = false;
        let mut uniques = HashSet::new();
        let mut types = Vec::new();
        for candidate in candidates {
            let signature = self.request.state.signature_of(candidate);
            if !signature
                .flags
                .intersects(tsc_types::SignatureFlags::HAS_REST_PARAMETER)
                && argument_count > signature.parameters.len()
            {
                continue;
            }
            let mut ty = self
                .request
                .state
                .get_type_parameter_at_position(candidate, argument_index)?;
            if is_jsx {
                let name = match self.data(editing_argument) {
                    NodeData::JsxAttribute(data) => data.name,
                    _ => None,
                };
                if let Some(name) = name {
                    let name = self.jsx_attribute_name_text(name);
                    if let Some(property) = self
                        .request
                        .state
                        .get_type_of_property_of_type(ty, name.as_str())?
                    {
                        ty = property;
                    }
                }
            }
            is_new_identifier = is_new_identifier
                || self
                    .request
                    .state
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::STRING);
            types.extend(self.string_literal_types(Some(ty), &mut uniques)?);
        }
        Ok((!types.is_empty()).then_some(CompletionsFromTypes {
            types,
            is_new_identifier,
        }))
    }

    /// tsgo `getStringLiteralTypes`: the string literal types of a type, a
    /// type parameter's constraint, or a union's members, each value once.
    fn string_literal_types(
        &mut self,
        ty: Option<TypeId>,
        uniques: &mut HashSet<Vec<u16>>,
    ) -> CheckResult<Vec<TypeId>> {
        let Some(ty) = ty else {
            return Ok(Vec::new());
        };
        let ty = self.skip_constraint(ty)?;
        let flags = self.request.state.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION) {
            let mut types = Vec::new();
            for member in self.union_types(ty) {
                types.extend(self.string_literal_types(Some(member), uniques)?);
            }
            return Ok(types);
        }
        if flags.intersects(TypeFlags::STRING_LITERAL) && !flags.intersects(TypeFlags::ENUM_LITERAL)
        {
            if let TypeData::Literal {
                value: LiteralValue::String(text),
            } = &self.request.state.tables.type_of(ty).data
            {
                if uniques.insert(text.units().to_vec()) {
                    return Ok(vec![ty]);
                }
            }
        }
        Ok(Vec::new())
    }

    /// tsgo `skipConstraint`: a type parameter's base constraint.
    fn skip_constraint(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        if self
            .request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::TYPE_PARAMETER)
        {
            if let Some(constraint) = self.request.state.get_base_constraint_of_type(ty)? {
                return Ok(constraint);
            }
        }
        Ok(ty)
    }

    /// tsgo `getAlreadyUsedTypesInStringLiteralUnion`: the string literals
    /// of the union's other members.
    fn already_used_types_in_string_literal_union(
        &self,
        union: NodeId,
        current: NodeId,
    ) -> Vec<Vec<u16>> {
        let NodeData::UnionType(data) = self.data(union) else {
            return Vec::new();
        };
        self.node_list(data.types)
            .iter()
            .filter(|&&type_node| type_node != current)
            .filter_map(|&type_node| match self.data(type_node) {
                NodeData::LiteralType(literal) => match literal.literal.map(|l| self.data(l)) {
                    Some(NodeData::StringLiteral(text)) => Some(text.text.code_units().collect()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// tsgo `hasIndexSignature`.
    fn has_index_signature(&mut self, ty: TypeId) -> CheckResult<bool> {
        Ok(self.request.state.get_string_index_type(ty)?.is_some()
            || self.request.state.get_number_index_type(ty)?.is_some())
    }

    /// tsgo `isRequireCallArgument`: the first argument of `require(...)`.
    fn is_require_call_argument(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        let NodeData::CallExpression(call) = self.data(parent) else {
            return false;
        };
        self.node_list(call.arguments).first() == Some(&node)
            && call.expression.is_some_and(|expression| {
                self.node_kind(expression) == SyntaxKind::Identifier
                    && node_util::id_text(self.source_of(expression), expression) == Some("require")
            })
    }

    /// tsgo `ast.IsImportCall`.
    fn is_import_call(&self, node: NodeId) -> bool {
        matches!(self.data(node), NodeData::CallExpression(call)
        if call.expression.is_some_and(|expression| {
            self.node_kind(expression) == SyntaxKind::ImportKeyword
        }))
    }

    /// tsgo `ast.IsPrivateIdentifierClassElementDeclaration` of a symbol's
    /// value declaration.
    fn is_private_identifier_class_element_symbol(&self, symbol: SymbolId) -> bool {
        let Some(declaration) = self.request.state.binder.symbol(symbol).value_declaration else {
            return false;
        };
        let source = self.source_of(declaration);
        matches!(
            source.arena.node(declaration).kind,
            SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        ) && node_util::get_name_of_declaration(source, declaration)
            .is_some_and(|name| source.arena.node(name).kind == SyntaxKind::PrivateIdentifier)
    }

    /// tsgo `walkUpParentheses`: the node around a parenthesized type or
    /// expression.
    fn walk_up_parentheses(&self, node: NodeId) -> NodeId {
        let kind = self.node_kind(node);
        if !matches!(
            kind,
            SyntaxKind::ParenthesizedType | SyntaxKind::ParenthesizedExpression
        ) {
            return node;
        }
        let mut current = node;
        while self.node_kind(current) == kind {
            match self.parent_of(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        current
    }
}
