//! tsgo `ls/codeactions_missingmemberfixer.go`: the class members made for
//! the members a class lacks — properties, accessors and methods typed by
//! the node builder, with stubbed bodies — and the imports their types
//! need.

use tsc_binder::node_util;
use tsc_checker::state::{CheckerState, SignatureId};
use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{
    CheckFlags, ModifierFlags, NodeFlags, SignatureFlags, SymbolFlags, SymbolId, TypeData,
    TypeFlags, TypeId,
};

use crate::autoimport::{try_get_auto_importable_reference_from_type_node, ImportAdder, View};
use crate::change::Tracker;
use crate::lsutil::{get_quote_preference, QuotePreference, UserPreferences};

/// tsgo `preserveOptionalFlags`.
pub(super) const PRESERVE_OPTIONAL_METHOD: u8 = 1;
pub(super) const PRESERVE_OPTIONAL_PROPERTY: u8 = 2;
pub(super) const PRESERVE_OPTIONAL_ALL: u8 = PRESERVE_OPTIONAL_METHOD | PRESERVE_OPTIONAL_PROPERTY;

/// A failure building a member (the node builder's or the factory's).
pub(super) type FixResult<T> = Result<T, String>;

fn transform_error(error: TransformError) -> String {
    error.to_string()
}

fn check_error(error: tsc_checker::state::CheckAbort) -> String {
    error.to_string()
}

/// tsgo `missingMemberFixer`.
pub(super) struct MissingMemberFixer<'f, 'a, 'r> {
    pub(super) tracker: &'f mut Tracker<'a>,
    /// The edited file and its copy in the tracker's arena.
    pub(super) file: &'a SourceFile,
    pub(super) target: TransformSourceId,
    pub(super) options: &'f tsc_types::CompilerOptions,
    pub(super) preferences: &'f UserPreferences,
    pub(super) import_adder: Option<(&'f mut ImportAdder<'a>, &'f mut View<'r>)>,
    /// tsgo `Method_not_implemented`, localized.
    pub(super) method_not_implemented: String,
}

impl MissingMemberFixer<'_, '_, '_> {
    /// tsgo `createMemberFromSymbol`: the members declaring `symbol` in
    /// `enclosing_declaration` (a class), with `body` or a stub.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_member_from_symbol(
        &mut self,
        checker: &mut CheckerState<'_>,
        symbol: SymbolId,
        enclosing_declaration: NodeId,
        body: Option<TransformNode>,
        preserve_optional: u8,
        is_abstract: bool,
    ) -> FixResult<Vec<TransformNode>> {
        let declarations = checker.binder.symbol(symbol).declarations.to_vec();
        let declaration = declarations.first().copied();
        let quote_preference = get_quote_preference(self.file, self.preferences);
        let ambient = NodeFlags::from_bits(self.file.arena.node(enclosing_declaration).flags)
            .intersects(NodeFlags::AMBIENT);
        let signature_only = ambient || is_abstract;
        let optional = checker
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::OPTIONAL);
        let kind = declaration.map_or(SyntaxKind::PropertySignature, |declaration| {
            checker
                .binder
                .source_of_node(declaration)
                .arena
                .node(declaration)
                .kind
        });
        let declaration_name = self.create_declaration_name(checker, symbol, declaration)?;
        let modifiers = self.create_modifiers(checker, symbol, declaration)?;
        let mut flags = EmitNodeBuilderFlags::NO_TRUNCATION.0;
        if quote_preference == QuotePreference::SINGLE {
            flags |= EmitNodeBuilderFlags::USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE.0;
        }
        let ty = checker
            .get_type_of_symbol_at_location(symbol, Some(enclosing_declaration))
            .map_err(check_error)?;
        let ty = checker.get_widened_type(ty).map_err(check_error)?;
        let mut nodes = Vec::new();
        match kind {
            SyntaxKind::PropertySignature | SyntaxKind::PropertyDeclaration => {
                let type_node = self.create_type_node(checker, ty, enclosing_declaration, flags)?;
                let question_token = (optional
                    && preserve_optional & PRESERVE_OPTIONAL_PROPERTY != 0)
                    .then(|| self.token(SyntaxKind::QuestionToken))
                    .transpose()?;
                let name = self.create_property_name(declaration_name, quote_preference)?;
                nodes.push(
                    self.tracker
                        .arena()
                        .factory()
                        .create_property_declaration(
                            self.target,
                            modifiers,
                            name,
                            question_token,
                            type_node,
                            None,
                        )
                        .map_err(transform_error)?,
                );
                Ok(nodes)
            }
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                let declaration = declaration.expect("an accessor declaration");
                let (first, second) =
                    get_all_accessor_declarations(checker, &declarations, declaration);
                for accessor in std::iter::once(first).chain(second) {
                    let accessor_source = checker.binder.source_of_node(accessor);
                    match accessor_source.arena.node(accessor).kind {
                        SyntaxKind::GetAccessor => {
                            let name =
                                self.create_property_name(declaration_name, quote_preference)?;
                            let type_node =
                                self.create_type_node(checker, ty, enclosing_declaration, flags)?;
                            let body = self.create_body(body, quote_preference, signature_only)?;
                            let parameters = self.node_array(Vec::new())?;
                            nodes.push(
                                self.tracker
                                    .arena()
                                    .factory()
                                    .create_get_accessor_declaration(
                                        self.target,
                                        modifiers,
                                        name,
                                        parameters,
                                        type_node,
                                        body,
                                    )
                                    .map_err(transform_error)?,
                            );
                        }
                        SyntaxKind::SetAccessor => {
                            let parameter = node_util::get_set_accessor_value_parameter(
                                accessor_source,
                                accessor,
                            )
                            .expect("Expected set accessor to have a parameter.");
                            let parameter_name = parameter_name_text(accessor_source, parameter);
                            let name =
                                self.create_property_name(declaration_name, quote_preference)?;
                            let type_node =
                                self.create_type_node(checker, ty, enclosing_declaration, flags)?;
                            let parameters = self.create_dummy_parameters(
                                1,
                                &[parameter_name],
                                &[type_node],
                                1,
                                is_in_js_file(self.file),
                            )?;
                            let body = self.create_body(body, quote_preference, signature_only)?;
                            nodes.push(
                                self.tracker
                                    .arena()
                                    .factory()
                                    .create_set_accessor_declaration(
                                        self.target,
                                        modifiers,
                                        name,
                                        parameters,
                                        body,
                                    )
                                    .map_err(transform_error)?,
                            );
                        }
                        _ => {}
                    }
                }
                Ok(nodes)
            }
            SyntaxKind::MethodSignature | SyntaxKind::MethodDeclaration => {
                let signatures = self.get_call_signatures(checker, ty)?;
                let preserve_optional =
                    optional && preserve_optional & PRESERVE_OPTIONAL_METHOD != 0;
                if signatures.is_empty() {
                    return Ok(Vec::new());
                }
                if declarations.len() == 1 {
                    let body = self.create_body(body, quote_preference, signature_only)?;
                    if let Some(method) = self.create_signature_declaration_from_signature(
                        checker,
                        signatures[0],
                        SyntaxKind::MethodDeclaration,
                        enclosing_declaration,
                        body,
                        modifiers,
                        Some(declaration_name),
                        preserve_optional,
                    )? {
                        nodes.push(method);
                    }
                    return Ok(nodes);
                }
                for &signature in &signatures {
                    let ambient_declaration = checker
                        .signature_of(signature)
                        .declaration
                        .is_some_and(|declaration| {
                            NodeFlags::from_bits(
                                checker
                                    .binder
                                    .source_of_node(declaration)
                                    .arena
                                    .node(declaration)
                                    .flags,
                            )
                            .intersects(NodeFlags::AMBIENT)
                        });
                    if ambient_declaration {
                        continue;
                    }
                    if let Some(method) = self.create_signature_declaration_from_signature(
                        checker,
                        signature,
                        SyntaxKind::MethodDeclaration,
                        enclosing_declaration,
                        None,
                        modifiers,
                        Some(declaration_name),
                        preserve_optional,
                    )? {
                        nodes.push(method);
                    }
                }
                if signature_only {
                    return Ok(nodes);
                }
                if declarations.len() > signatures.len() {
                    let last = *declarations.last().expect("a declaration");
                    let signature = checker
                        .get_signature_from_declaration(last)
                        .map_err(check_error)?;
                    let body = self.create_body(body, quote_preference, false)?;
                    if let Some(method) = self.create_signature_declaration_from_signature(
                        checker,
                        signature,
                        SyntaxKind::MethodDeclaration,
                        enclosing_declaration,
                        body,
                        modifiers,
                        Some(declaration_name),
                        preserve_optional,
                    )? {
                        nodes.push(method);
                    }
                } else if let Some(method) = self.create_signature_declaration_from_signatures(
                    checker,
                    &signatures,
                    declaration_name,
                    preserve_optional,
                    modifiers,
                    quote_preference,
                    body,
                    enclosing_declaration,
                )? {
                    nodes.push(method);
                }
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// tsgo `getCallSignatures`: a union's members' call signatures.
    fn get_call_signatures(
        &self,
        checker: &mut CheckerState<'_>,
        ty: TypeId,
    ) -> FixResult<Vec<SignatureId>> {
        let members = match &checker.tables.type_of(ty).data {
            TypeData::Union { types, .. }
                if checker.tables.flags_of(ty).intersects(TypeFlags::UNION) =>
            {
                types.to_vec()
            }
            _ => vec![ty],
        };
        let mut signatures = Vec::new();
        for member in members {
            signatures.extend(
                checker
                    .get_signatures_of_type(member, tsc_checker::state::SignatureKind::Call)
                    .map_err(check_error)?,
            );
        }
        Ok(signatures)
    }

    /// tsgo `createTypeNode`: the type's node with the imports it needs.
    fn create_type_node(
        &mut self,
        checker: &mut CheckerState<'_>,
        ty: TypeId,
        enclosing_declaration: NodeId,
        flags: u32,
    ) -> FixResult<Option<TransformNode>> {
        let built = checker.type_to_type_node_in_arena(
            self.tracker.arena(),
            self.target,
            ty,
            Some(enclosing_declaration),
            flags,
            EmitInternalNodeBuilderFlags::NONE.0,
        )?;
        match built {
            Some((node, id_to_symbol)) => self
                .import_type_node(checker, node, &id_to_symbol)
                .map(Some),
            None => Ok(None),
        }
    }

    /// tsgo `createModifiers`: the static, public or protected, accessor and
    /// override modifiers the member takes.
    fn create_modifiers(
        &mut self,
        checker: &CheckerState<'_>,
        symbol: SymbolId,
        declaration: Option<NodeId>,
    ) -> FixResult<Option<TransformNodeArray>> {
        let mut modifier_flags = ModifierFlags::NONE;
        if let Some(declaration) = declaration {
            let effective = checker.get_declaration_modifier_flags_from_symbol(symbol);
            modifier_flags = effective & ModifierFlags::STATIC;
            if effective.intersects(ModifierFlags::PUBLIC) {
                modifier_flags |= ModifierFlags::PUBLIC;
            } else if effective.intersects(ModifierFlags::PROTECTED) {
                modifier_flags |= ModifierFlags::PROTECTED;
            }
            let source = checker.binder.source_of_node(declaration);
            if node_util::is_auto_accessor_property_declaration(source, declaration) {
                modifier_flags |= ModifierFlags::ACCESSOR;
            }
        }
        if self.should_add_override_keyword(checker, declaration) {
            modifier_flags |= ModifierFlags::OVERRIDE;
        }
        if modifier_flags == ModifierFlags::NONE {
            return Ok(None);
        }
        self.tracker
            .arena()
            .factory()
            .create_modifiers_from_modifier_flags(self.target, modifier_flags)
            .map_err(transform_error)
    }

    /// tsgo `shouldAddOverrideKeyword`.
    fn should_add_override_keyword(
        &self,
        checker: &CheckerState<'_>,
        declaration: Option<NodeId>,
    ) -> bool {
        declaration.is_some_and(|declaration| {
            self.options.no_implicit_override == Some(true)
                && node_util::has_syntactic_modifier(
                    checker.binder.source_of_node(declaration),
                    declaration,
                    ModifierFlags::ABSTRACT,
                )
        })
    }

    /// tsgo `createSignatureDeclarationFromSignature`.
    #[allow(clippy::too_many_arguments)]
    fn create_signature_declaration_from_signature(
        &mut self,
        checker: &mut CheckerState<'_>,
        signature: SignatureId,
        kind: SyntaxKind,
        enclosing_declaration: NodeId,
        body: Option<TransformNode>,
        modifiers: Option<TransformNodeArray>,
        name: Option<TransformNode>,
        optional: bool,
    ) -> FixResult<Option<TransformNode>> {
        let quote_preference = get_quote_preference(self.file, self.preferences);
        let mut flags = EmitNodeBuilderFlags::NO_TRUNCATION.0
            | EmitNodeBuilderFlags::SUPPRESS_ANY_RETURN_TYPE.0
            | EmitNodeBuilderFlags::ALLOW_EMPTY_TUPLE.0;
        if quote_preference == QuotePreference::SINGLE {
            flags |= EmitNodeBuilderFlags::USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE.0;
        }
        let Some((declaration, id_to_symbol)) = checker
            .signature_to_signature_declaration_in_arena(
                self.tracker.arena(),
                self.target,
                signature,
                kind,
                Some(enclosing_declaration),
                flags,
                EmitInternalNodeBuilderFlags::ALLOW_UNRESOLVED_NAMES.0,
            )?
        else {
            return Ok(None);
        };
        let is_js = is_in_js_file(self.file);
        let data = self
            .tracker
            .arena()
            .node(declaration)
            .map_err(transform_error)?
            .data
            .clone();
        let NodeData::MethodDeclaration(method) = data else {
            return Ok(None);
        };
        let type_parameters = if is_js { None } else { method.type_parameters };
        let type_parameters = match type_parameters {
            Some(list) => {
                let nodes = self.list_nodes(list)?;
                if nodes.is_empty() {
                    Some(list)
                } else {
                    let mut updated = Vec::with_capacity(nodes.len());
                    for type_parameter in nodes {
                        let NodeData::TypeParameter(data) = self.data(type_parameter)? else {
                            updated.push(type_parameter);
                            continue;
                        };
                        let constraint = match data.constraint {
                            Some(constraint) => {
                                let constraint = self.node(constraint)?;
                                Some(self.import_type_node(checker, constraint, &id_to_symbol)?)
                            }
                            None => None,
                        };
                        let default_type = match data.default {
                            Some(default_type) => {
                                let default_type = self.node(default_type)?;
                                Some(self.import_type_node(checker, default_type, &id_to_symbol)?)
                            }
                            None => None,
                        };
                        let modifiers = data
                            .modifiers
                            .map(|modifiers| self.array(modifiers))
                            .transpose()?;
                        let name = self.node(data.name.expect("a type parameter name"))?;
                        updated.push(
                            self.tracker
                                .arena()
                                .factory()
                                .update_type_parameter_declaration(
                                    type_parameter,
                                    modifiers,
                                    name,
                                    constraint,
                                    default_type,
                                )
                                .map_err(transform_error)?,
                        );
                    }
                    Some(self.node_array(updated)?.array())
                }
            }
            None => None,
        };
        let parameters = match method.parameters {
            Some(list) => {
                let nodes = self.list_nodes(list)?;
                let mut updated = Vec::with_capacity(nodes.len());
                for parameter in nodes {
                    let NodeData::Parameter(data) = self.data(parameter)? else {
                        continue;
                    };
                    let parameter_type = match data.r#type {
                        Some(parameter_type) => {
                            let parameter_type = self.node(parameter_type)?;
                            Some(self.import_type_node(checker, parameter_type, &id_to_symbol)?)
                        }
                        None => None,
                    };
                    let modifiers = data
                        .modifiers
                        .map(|modifiers| self.array(modifiers))
                        .transpose()?;
                    let dot_dot_dot_token = data
                        .dot_dot_dot_token
                        .map(|token| self.node(token))
                        .transpose()?;
                    let name = self.node(data.name.expect("a parameter name"))?;
                    let question_token = if is_js {
                        None
                    } else {
                        data.question_token
                            .map(|token| self.node(token))
                            .transpose()?
                    };
                    let initializer = data.initializer.map(|node| self.node(node)).transpose()?;
                    updated.push(
                        self.tracker
                            .arena()
                            .factory()
                            .update_parameter_declaration(
                                parameter,
                                modifiers,
                                dot_dot_dot_token,
                                name,
                                question_token,
                                parameter_type,
                                initializer,
                            )
                            .map_err(transform_error)?,
                    );
                }
                Some(self.node_array(updated)?.array())
            }
            None => None,
        };
        let type_node = if is_js { None } else { method.r#type };
        let type_node = match type_node {
            Some(type_node) => {
                let type_node = self.node(type_node)?;
                Some(
                    self.import_type_node(checker, type_node, &id_to_symbol)?
                        .node(),
                )
            }
            None => None,
        };
        let question_token = optional
            .then(|| self.token(SyntaxKind::QuestionToken))
            .transpose()?
            .map(|token| token.node());
        let method_name = match name {
            None => self.identifier("")?,
            Some(name) => self.create_property_name(name, quote_preference)?,
        };
        let data =
            NodeData::MethodDeclaration(Box::new(tsc_syntax::nodes::MethodDeclarationData {
                name: Some(method_name.node()),
                type_parameters,
                parameters,
                r#type: type_node,
                asterisk_token: method.asterisk_token,
                question_token,
                exclamation_token: None,
                body: body.map(|body| body.node()),
                modifiers: modifiers.map(|modifiers| modifiers.array()),
            }));
        self.tracker
            .arena()
            .factory()
            .update_node(declaration, data, TransformFlags::NONE)
            .map(Some)
            .map_err(transform_error)
    }

    /// tsgo `createSignatureDeclarationFromSignatures`: one method for
    /// several signatures, with parameters enough for each.
    #[allow(clippy::too_many_arguments)]
    fn create_signature_declaration_from_signatures(
        &mut self,
        checker: &mut CheckerState<'_>,
        signatures: &[SignatureId],
        name: TransformNode,
        optional: bool,
        modifiers: Option<TransformNodeArray>,
        quote_preference: QuotePreference,
        body: Option<TransformNode>,
        enclosing_declaration: NodeId,
    ) -> FixResult<Option<TransformNode>> {
        let Some(&first) = signatures.first() else {
            return Ok(None);
        };
        let mut max_args_signature = first;
        let mut min_argument_count = checker.signature_of(first).min_argument_count;
        let mut has_rest_parameter = false;
        let has_rest = |checker: &CheckerState<'_>, signature: SignatureId| {
            checker
                .signature_of(signature)
                .flags
                .intersects(SignatureFlags::HAS_REST_PARAMETER)
        };
        for &signature in signatures {
            min_argument_count =
                min_argument_count.min(checker.signature_of(signature).min_argument_count);
            if has_rest(checker, signature) {
                has_rest_parameter = true;
            }
            if checker.signature_of(signature).parameters.len()
                >= checker.signature_of(max_args_signature).parameters.len()
                && (!has_rest(checker, signature) || has_rest(checker, max_args_signature))
            {
                max_args_signature = signature;
            }
        }
        let max_parameters = checker.signature_of(max_args_signature).parameters.clone();
        let max_non_rest_args =
            max_parameters.len() - usize::from(has_rest(checker, max_args_signature));
        let parameter_names: Vec<String> = max_parameters
            .iter()
            .map(|&parameter| {
                checker
                    .binder
                    .symbol(parameter)
                    .escaped_name
                    .unescape()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let mut parameters = self.create_dummy_parameters_nodes(
            max_non_rest_args,
            &parameter_names,
            &[],
            min_argument_count as usize,
            is_in_js_file(self.file),
        )?;
        if has_rest_parameter {
            let rest_parameter_name = parameter_names
                .get(max_non_rest_args)
                .filter(|name| !name.is_empty())
                .cloned()
                .unwrap_or_else(|| "rest".to_owned());
            let question_token = (max_non_rest_args >= min_argument_count as usize)
                .then(|| self.token(SyntaxKind::QuestionToken))
                .transpose()?;
            let dot_dot_dot = self.token(SyntaxKind::DotDotDotToken)?;
            let rest_name = self.identifier(&rest_parameter_name)?;
            let unknown = self.keyword_type(SyntaxKind::UnknownKeyword)?;
            let array_type = self
                .tracker
                .arena()
                .factory()
                .create_array_type_node(self.target, unknown)
                .map_err(transform_error)?;
            parameters.push(
                self.tracker
                    .arena()
                    .factory()
                    .create_parameter_declaration(
                        self.target,
                        None,
                        Some(dot_dot_dot),
                        rest_name,
                        question_token,
                        Some(array_type),
                        None,
                    )
                    .map_err(transform_error)?,
            );
        }
        let parameters = self.node_array(parameters)?;
        let method_name = self.create_property_name(name, quote_preference)?;
        let question_token = optional
            .then(|| self.token(SyntaxKind::QuestionToken))
            .transpose()?;
        let return_type =
            self.get_return_type_from_signatures(checker, signatures, enclosing_declaration)?;
        let body = self.create_body(body, quote_preference, false)?;
        self.tracker
            .arena()
            .factory()
            .create_method_declaration(
                self.target,
                modifiers,
                None,
                method_name,
                question_token,
                None,
                parameters,
                return_type,
                body,
            )
            .map(Some)
            .map_err(transform_error)
    }

    /// tsgo `getReturnTypeFromSignatures`: the union of the signatures'
    /// return types.
    fn get_return_type_from_signatures(
        &mut self,
        checker: &mut CheckerState<'_>,
        signatures: &[SignatureId],
        enclosing_declaration: NodeId,
    ) -> FixResult<Option<TransformNode>> {
        if signatures.is_empty() {
            return Ok(None);
        }
        let mut return_types = Vec::with_capacity(signatures.len());
        for &signature in signatures {
            return_types.push(
                checker
                    .get_return_type_of_signature(signature)
                    .map_err(check_error)?,
            );
        }
        let union_type = checker.get_union_type(&return_types).map_err(check_error)?;
        let built = checker.type_to_type_node_in_arena(
            self.tracker.arena(),
            self.target,
            union_type,
            Some(enclosing_declaration),
            EmitNodeBuilderFlags::NO_TRUNCATION.0,
            EmitInternalNodeBuilderFlags::ALLOW_UNRESOLVED_NAMES.0,
        )?;
        match built {
            Some((node, id_to_symbol)) => self
                .import_type_node(checker, node, &id_to_symbol)
                .map(Some),
            None => Ok(None),
        }
    }

    /// tsgo `importTypeNode`: the type node with the import types the node
    /// builder wrote for symbols replaced by references to them, imported.
    fn import_type_node(
        &mut self,
        checker: &mut CheckerState<'_>,
        type_node: TransformNode,
        id_to_symbol: &[(TransformNode, SymbolId)],
    ) -> FixResult<TransformNode> {
        if self.import_adder.is_none() {
            return Ok(type_node);
        }
        let (imported_type_node, symbols) = try_get_auto_importable_reference_from_type_node(
            checker,
            self.tracker.arena(),
            type_node,
            id_to_symbol,
        )
        .map_err(transform_error)?;
        let (import_adder, view) = self.import_adder.as_mut().expect("an import adder");
        for symbol in symbols {
            let Some(export_symbol) = get_exported_symbol(checker, symbol) else {
                continue;
            };
            import_adder
                .add_import_from_exported_symbol(checker, view, export_symbol, true)
                .map_err(check_error)?;
        }
        Ok(imported_type_node)
    }

    /// tsgo `createIndexSignatureDeclarationFromType`: the index signature of
    /// the implemented type for the key type, if it has one.
    pub(super) fn create_index_signature_declaration_from_type(
        &mut self,
        checker: &mut CheckerState<'_>,
        class_declaration: NodeId,
        implemented_type: TypeId,
        key_type: TypeId,
    ) -> FixResult<Option<TransformNode>> {
        let Some(index_info) = checker
            .get_index_info_of_type(implemented_type, key_type)
            .map_err(check_error)?
        else {
            return Ok(None);
        };
        checker.index_info_to_index_signature_declaration_in_arena(
            self.tracker.arena(),
            self.target,
            &index_info,
            Some(class_declaration),
            EmitNodeBuilderFlags::NONE.0,
        )
    }

    /// tsgo `createBody`: the given body cloned, else a stub; none for a
    /// signature only.
    fn create_body(
        &mut self,
        body: Option<TransformNode>,
        quote_preference: QuotePreference,
        signature_only: bool,
    ) -> FixResult<Option<TransformNode>> {
        if signature_only {
            return Ok(None);
        }
        match body {
            Some(body) => self
                .tracker
                .arena()
                .factory()
                .deep_clone_node(body)
                .map(Some)
                .map_err(transform_error),
            None => self.create_stubbed_method_body(quote_preference).map(Some),
        }
    }

    /// tsgo `createStubbedMethodBody`: `{ throw new Error("Method not
    /// implemented."); }`.
    fn create_stubbed_method_body(
        &mut self,
        quote_preference: QuotePreference,
    ) -> FixResult<TransformNode> {
        let message = self.method_not_implemented.clone();
        let target = self.target;
        let mut factory = self.tracker.arena().factory();
        let error = factory
            .create_identifier(target, "Error")
            .map_err(transform_error)?;
        let literal = factory
            .create_string_literal(
                target,
                message.as_str(),
                quote_preference == QuotePreference::SINGLE,
            )
            .map_err(transform_error)?;
        let arguments = factory
            .create_node_array(target, vec![literal])
            .map_err(transform_error)?;
        let new_expression = factory
            .create_node(
                target,
                NodeData::NewExpression(tsc_syntax::nodes::NewExpressionData {
                    expression: Some(error.node()),
                    type_arguments: None,
                    arguments: Some(arguments.array()),
                    question_dot_token: None,
                }),
                TransformFlags::NONE,
            )
            .map_err(transform_error)?;
        let throw_statement = factory
            .create_node(
                target,
                NodeData::ThrowStatement(tsc_syntax::nodes::ThrowStatementData {
                    expression: Some(new_expression.node()),
                }),
                TransformFlags::NONE,
            )
            .map_err(transform_error)?;
        let statements = factory
            .create_node_array(target, vec![throw_statement])
            .map_err(transform_error)?;
        factory
            .create_block(target, statements, true)
            .map_err(transform_error)
    }

    /// tsgo `createDummyParameters`.
    fn create_dummy_parameters(
        &mut self,
        argument_count: usize,
        names: &[String],
        types: &[Option<TransformNode>],
        min_argument_count: usize,
        in_js: bool,
    ) -> FixResult<TransformNodeArray> {
        let parameters = self.create_dummy_parameters_nodes(
            argument_count,
            names,
            types,
            min_argument_count,
            in_js,
        )?;
        self.node_array(parameters)
    }

    fn create_dummy_parameters_nodes(
        &mut self,
        argument_count: usize,
        names: &[String],
        types: &[Option<TransformNode>],
        min_argument_count: usize,
        in_js: bool,
    ) -> FixResult<Vec<TransformNode>> {
        let mut parameters = Vec::with_capacity(argument_count);
        let mut parameter_name_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for index in 0..argument_count {
            let mut parameter_name = names
                .get(index)
                .filter(|name| !name.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("arg{index}"));
            let count = parameter_name_counts
                .entry(parameter_name.clone())
                .or_insert(0);
            let previous = *count;
            *count += 1;
            if previous > 0 {
                parameter_name.push_str(&previous.to_string());
            }
            let question_token = (index >= min_argument_count)
                .then(|| self.token(SyntaxKind::QuestionToken))
                .transpose()?;
            let type_node = if in_js {
                None
            } else if let Some(Some(type_node)) = types.get(index) {
                Some(*type_node)
            } else {
                Some(self.keyword_type(SyntaxKind::UnknownKeyword)?)
            };
            let name = self.identifier(&parameter_name)?;
            parameters.push(
                self.tracker
                    .arena()
                    .factory()
                    .create_parameter_declaration(
                        self.target,
                        None,
                        None,
                        name,
                        question_token,
                        type_node,
                        None,
                    )
                    .map_err(transform_error)?,
            );
        }
        Ok(parameters)
    }

    /// tsgo `createDeclarationName`: a mapped symbol's name from its name
    /// type, else its declaration's name (cloned), else its own name.
    fn create_declaration_name(
        &mut self,
        checker: &CheckerState<'_>,
        symbol: SymbolId,
        declaration: Option<NodeId>,
    ) -> FixResult<TransformNode> {
        if checker
            .get_check_flags(symbol)
            .intersects(CheckFlags::MAPPED)
        {
            if let Some(name_type) = checker
                .get_name_type_of_symbol(symbol)
                .filter(|&name_type| checker.is_type_usable_as_property_name(name_type))
            {
                let name = checker
                    .get_property_name_from_type(name_type)
                    .map(|name| name.unescape().to_string_lossy().into_owned())
                    .unwrap_or_default();
                return self.identifier(&name);
            }
        }
        if let Some(name) = declaration.and_then(|declaration| {
            node_util::get_name_of_declaration(
                checker.binder.source_of_node(declaration),
                declaration,
            )
        }) {
            let Some(original) = checker.parse_tree_node_in_arena(self.tracker.arena(), name)?
            else {
                return Err("a declaration name outside the program".to_owned());
            };
            return self
                .tracker
                .arena()
                .factory()
                .clone_node_to_source(original, self.target)
                .map_err(transform_error);
        }
        let name = checker
            .binder
            .symbol(symbol)
            .escaped_name
            .unescape()
            .to_string_lossy()
            .into_owned();
        self.identifier(&name)
    }

    /// tsgo `createPropertyName`: `constructor` as a computed string name,
    /// any other name deep cloned.
    fn create_property_name(
        &mut self,
        node: TransformNode,
        quote_preference: QuotePreference,
    ) -> FixResult<TransformNode> {
        let data = self
            .tracker
            .arena()
            .node(node)
            .map_err(transform_error)?
            .data
            .clone();
        if let NodeData::Identifier(identifier) = &data {
            if identifier.text() == "constructor" {
                let literal = self
                    .tracker
                    .arena()
                    .factory()
                    .create_string_literal(
                        self.target,
                        "constructor",
                        quote_preference == QuotePreference::SINGLE,
                    )
                    .map_err(transform_error)?;
                return self
                    .tracker
                    .arena()
                    .factory()
                    .create_computed_property_name(self.target, literal)
                    .map_err(transform_error);
            }
        }
        self.tracker
            .arena()
            .factory()
            .deep_clone_node(node)
            .map_err(transform_error)
    }

    fn token(&mut self, kind: SyntaxKind) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_token(self.target, kind, TransformFlags::NONE)
            .map_err(transform_error)
    }

    fn identifier(&mut self, text: &str) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_identifier(self.target, text)
            .map_err(transform_error)
    }

    fn keyword_type(&mut self, kind: SyntaxKind) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_keyword_type_node(self.target, kind)
            .map_err(transform_error)
    }

    fn node_array(&mut self, nodes: Vec<TransformNode>) -> FixResult<TransformNodeArray> {
        self.tracker
            .arena()
            .factory()
            .create_node_array(self.target, nodes)
            .map_err(transform_error)
    }

    fn data(&mut self, node: TransformNode) -> FixResult<NodeData> {
        Ok(self
            .tracker
            .arena()
            .node(node)
            .map_err(transform_error)?
            .data
            .clone())
    }

    fn node(&mut self, id: NodeId) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .node_ref(self.target, id)
            .ok_or_else(|| "a node outside the target".to_owned())
    }

    fn array(&mut self, id: tsc_syntax::NodeArrayId) -> FixResult<TransformNodeArray> {
        self.tracker
            .arena()
            .node_array_ref(self.target, id)
            .ok_or_else(|| "a node array outside the target".to_owned())
    }

    fn list_nodes(&mut self, id: tsc_syntax::NodeArrayId) -> FixResult<Vec<TransformNode>> {
        let array = self.array(id)?;
        let ids = self
            .tracker
            .arena()
            .node_array(array)
            .map_err(transform_error)?
            .nodes
            .to_vec();
        ids.into_iter().map(|id| self.node(id)).collect()
    }
}

/// tsgo `getExportedSymbol`: a symbol's export symbol, none without a
/// parent.
fn get_exported_symbol(checker: &CheckerState<'_>, symbol: SymbolId) -> Option<SymbolId> {
    let symbol = checker.get_export_symbol_of_symbol(symbol);
    checker.binder.symbol(symbol).parent.map(|_| symbol)
}

/// tsgo `ast.GetAllAccessorDeclarations(symbol.Declarations(), declaration)`:
/// the accessor and its counterpart of the other kind with the same name
/// and staticness, first and second in source order.
fn get_all_accessor_declarations(
    checker: &CheckerState<'_>,
    declarations: &[NodeId],
    accessor: NodeId,
) -> (NodeId, Option<NodeId>) {
    let source = checker.binder.source_of_node(accessor);
    let kind = source.arena.node(accessor).kind;
    let other_kind = if kind == SyntaxKind::SetAccessor {
        SyntaxKind::GetAccessor
    } else {
        SyntaxKind::SetAccessor
    };
    let is_static = |declaration: NodeId| {
        node_util::has_syntactic_modifier(
            checker.binder.source_of_node(declaration),
            declaration,
            ModifierFlags::STATIC,
        )
    };
    let property_name = |declaration: NodeId| {
        let source = checker.binder.source_of_node(declaration);
        node_util::get_name_of_declaration(source, declaration)
            .and_then(|name| checker.property_name_for_property_name_node(name))
    };
    let matches: Vec<NodeId> = if node_util::has_dynamic_name(source, accessor) {
        // dynamic names can only be match up via checker symbol lookup, just return an object with just this accessor
        vec![accessor]
    } else {
        let accessor_name = property_name(accessor);
        let accessor_static = is_static(accessor);
        declarations
            .iter()
            .copied()
            .filter(|&member| {
                let member_kind = checker
                    .binder
                    .source_of_node(member)
                    .arena
                    .node(member)
                    .kind;
                matches!(
                    member_kind,
                    SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
                ) && is_static(member) == accessor_static
                    && property_name(member) == accessor_name
            })
            .collect()
    };
    let other = matches.into_iter().find(|&declaration| {
        checker
            .binder
            .source_of_node(declaration)
            .arena
            .node(declaration)
            .kind
            == other_kind
    });
    match other {
        Some(other)
            if checker.binder.source_of_node(other).arena.node(other).pos
                < source.arena.node(accessor).pos =>
        {
            (other, Some(accessor))
        }
        _ => (accessor, other),
    }
}

/// The text of a parameter's name.
fn parameter_name_text(source: &SourceFile, parameter: NodeId) -> String {
    let NodeData::Parameter(data) = &source.arena.node(parameter).data else {
        return String::new();
    };
    match data.name.map(|name| &source.arena.node(name).data) {
        Some(NodeData::Identifier(identifier)) => identifier.text().to_owned(),
        _ => String::new(),
    }
}

/// tsgo `ast.IsInJSFile`.
fn is_in_js_file(file: &SourceFile) -> bool {
    NodeFlags::from_bits(file.arena.node(file.root).flags).intersects(NodeFlags::JAVA_SCRIPT_FILE)
}
