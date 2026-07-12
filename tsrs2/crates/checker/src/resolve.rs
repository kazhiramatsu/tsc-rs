//! resolveName + resolveEntityName — lexical name resolution (M4 5.1).
//!
//! The scope walk is tsc's createNameResolver closure (19516) with the
//! checker's callbacks (46504): the JSDoc arms are elided (JSDoc nodes
//! are not modeled — no JSDoc kind is constructible in the arena), and
//! the failure path emits the PLAIN nameNotFoundMessage form only —
//! spelling suggestions (getSuggestedSymbolForNonexistentSymbol) and
//! the checkAndReportErrorFor* alternates are M8 rows, ledgered at
//! on_failed_to_resolve_symbol.

use tsrs2_binder::node_util::{
    self, body_of, get_immediately_invoked_function_expression, has_syntactic_modifier,
    is_function_like_declaration_kind, is_function_like_kind, is_part_of_parameter_declaration,
};
use tsrs2_binder::{SymbolId, SymbolTable};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, ScriptTarget, SymbolFlags};

use crate::state::CheckerState;

/// tsc maximumSuggestionCount (47424).
const MAXIMUM_SUGGESTION_COUNT: u32 = 10;

/// lookup_probe's outcome: the suggestion snapshot defers the &mut
/// spelling pass so binder-borrowed tables drop their borrow first.
enum LookupProbe {
    Found(SymbolId),
    Miss,
    Suggest {
        values: Vec<SymbolId>,
        capitalized_primitives: Vec<&'static str>,
    },
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getSymbol @6.0.3 (the createNameResolver `lookup`)
    /// tsc-hash: bd2696712b634b49b85269b6fd5118efb5b99ad3e3986e2b7adc77ed494d4746
    /// tsc-span: _tsc.js:47904-47919
    ///
    /// The Alias arm chases the alias TARGET's flags (getSymbolFlags);
    /// resolveAlias is unported (import semantics, M4 5.8), so an
    /// alias whose own flags miss `meaning` does not match. FN-only:
    /// alias names in matching positions resolve to nothing until 5.8.
    pub fn get_symbol_in_table(
        &self,
        table: &SymbolTable,
        name: &str,
        meaning: SymbolFlags,
    ) -> Option<SymbolId> {
        if meaning.is_empty() {
            return None;
        }
        let &symbol = table.get(name)?;
        let symbol = self.get_merged_symbol(symbol);
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(meaning) {
            return Some(symbol);
        }
        // (M4 5.8) Alias target-flags chase elided — see doc above.
        None
    }

    /// The parameterized lookup's probe half (&self): exact match or,
    /// in suggestion mode, a table snapshot for the spelling pass.
    /// tsc getSuggestionForSymbolNameLookup (75522-75535) — the
    /// capitalized-primitive synthetics exist only at the GLOBALS
    /// level.
    fn lookup_probe(
        &self,
        table: &SymbolTable,
        name: &str,
        meaning: SymbolFlags,
        suggestion: bool,
        is_globals: bool,
    ) -> LookupProbe {
        if let Some(found) = self.get_symbol_in_table(table, name, meaning) {
            return LookupProbe::Found(found);
        }
        if !suggestion {
            return LookupProbe::Miss;
        }
        let capitalized_primitives: Vec<&'static str> = if is_globals {
            ["string", "number", "boolean", "object", "bigint", "symbol"]
                .iter()
                .copied()
                .filter(|primitive| {
                    let mut capitalized = String::with_capacity(primitive.len());
                    capitalized.push(primitive.as_bytes()[0].to_ascii_uppercase() as char);
                    capitalized.push_str(&primitive[1..]);
                    table.contains_key(&capitalized)
                })
                .collect()
        } else {
            Vec::new()
        };
        LookupProbe::Suggest {
            values: table.values().copied().collect(),
            capitalized_primitives,
        }
    }

    /// The suggestion half (&mut self): synthetic lowercase-primitive
    /// TypeAlias candidates PREPEND (concatenate order), then the
    /// spelling core over the table values in insertion order.
    fn finish_lookup(
        &mut self,
        probe: LookupProbe,
        name: &str,
        meaning: SymbolFlags,
    ) -> Option<SymbolId> {
        match probe {
            LookupProbe::Found(found) => Some(found),
            LookupProbe::Miss => None,
            LookupProbe::Suggest {
                values,
                capitalized_primitives,
            } => {
                let mut candidates =
                    Vec::with_capacity(values.len() + capitalized_primitives.len());
                for primitive in capitalized_primitives {
                    candidates.push(
                        self.binder
                            .create_symbol(SymbolFlags::TYPE_ALIAS, primitive.to_owned()),
                    );
                }
                candidates.extend(values);
                self.get_spelling_suggestion_for_name(
                    tsrs2_binder::unescape_leading_underscores(name),
                    &candidates,
                    meaning,
                )
            }
        }
    }

    /// tsc-port: resolveNameHelper @6.0.3
    /// tsc-hash: 2a965808b21b9b6059de120cec14ef8ce90bb976242d6b8d5c29553b09d3de56
    /// tsc-span: _tsc.js:19534-19803
    ///
    /// Elisions, each FN-only and owned by a later stage:
    /// - JSDoc tag/template/parameter arms (JSDoc nodes unmodeled).
    /// - `result.isReferenced |= meaning` (M7 unused-diagnostics).
    /// - the JS `require` fallback (requireSymbol — M2 3.4c residual).
    /// - the EnumDeclaration isolatedModules qualification error
    ///   (isolatedModules option unmodeled).
    pub fn resolve_name(
        &mut self,
        location: Option<NodeId>,
        name: &str,
        meaning: SymbolFlags,
        name_not_found_message: Option<&'static DiagnosticMessage>,
        is_use: bool,
        exclude_globals: bool,
    ) -> Option<SymbolId> {
        self.resolve_name_full(
            location,
            name,
            meaning,
            name_not_found_message,
            is_use,
            exclude_globals,
            /*suggestion*/ false,
        )
    }

    /// tsc-port: resolveNameForSymbolSuggestion @6.0.3
    /// tsc-hash: add6fe8076fd4f769f84d1d9af8a2d1468945c362e2b78486e074e8ae1d5598a
    /// tsc-span: _tsc.js:75536-75550
    ///
    /// createNameResolver with lookup = getSuggestionForSymbolNameLookup
    /// (75522-75535): the SAME scope walk, each table answering
    /// exact-match-else-spelling — an inner near-miss legitimately
    /// shadows an outer exact match, like tsc.
    pub(crate) fn resolve_name_for_symbol_suggestion(
        &mut self,
        location: Option<NodeId>,
        name: &str,
        meaning: SymbolFlags,
    ) -> Option<SymbolId> {
        self.resolve_name_full(
            location,
            name,
            meaning,
            /*name_not_found_message*/ None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
            /*suggestion*/ true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_name_full(
        &mut self,
        location: Option<NodeId>,
        name: &str,
        meaning: SymbolFlags,
        name_not_found_message: Option<&'static DiagnosticMessage>,
        is_use: bool,
        exclude_globals: bool,
        suggestion: bool,
    ) -> Option<SymbolId> {
        let original_location = location;
        let mut location = location;
        let mut result: Option<SymbolId> = None;
        let mut last_location: Option<NodeId> = None;
        let mut last_self_reference_location: Option<NodeId> = None;
        let mut property_with_invalid_initializer: Option<NodeId> = None;
        let mut associated_declaration_for_containing_initializer: Option<NodeId> = None;
        let mut within_deferred_context = false;

        'walk: while let Some(loc) = location {
            if name == "const" && self.is_const_assertion(loc) {
                return None;
            }
            if matches!(
                self.kind_of(loc),
                SyntaxKind::ModuleDeclaration | SyntaxKind::EnumDeclaration
            ) && last_location.is_some()
                && self.name_of_node(loc) == last_location
            {
                last_location = Some(loc);
                location = self.parent_of(loc);
                continue;
            }
            let loc_is_global_source_file = self.kind_of(loc) == SyntaxKind::SourceFile
                && !self.binder.is_external_or_common_js_module_of_node(loc);
            if !loc_is_global_source_file {
                if let Some(locals) = self.binder.locals_of(loc) {
                    let probe = self.lookup_probe(locals, name, meaning, suggestion, false);
                    if let Some(found) = self.finish_lookup(probe, name, meaning) {
                        let mut use_result = true;
                        let result_flags = self.binder.symbol(found).flags;
                        if is_function_like_kind(self.kind_of(loc))
                            && last_location.is_some()
                            && last_location != body_of(self.binder.source_of_node(loc), loc)
                        {
                            // Type parameters of a function are in scope
                            // only in the return type and parameter list
                            // (Synthesized fake scopes are a services
                            // construct — no synthesized nodes exist).
                            if meaning.intersects(result_flags)
                                && (meaning & result_flags).intersects(SymbolFlags::TYPE)
                            {
                                use_result = if result_flags.intersects(SymbolFlags::TYPE_PARAMETER)
                                {
                                    last_location == self.type_annotation_of(loc)
                                        || last_location.is_some_and(|l| {
                                            matches!(
                                                self.kind_of(l),
                                                SyntaxKind::Parameter | SyntaxKind::TypeParameter
                                            )
                                        })
                                } else {
                                    false
                                };
                            }
                            if (meaning & result_flags).intersects(SymbolFlags::VARIABLE) {
                                if self.use_outer_variable_scope_in_parameter(
                                    found,
                                    loc,
                                    last_location,
                                ) {
                                    use_result = false;
                                } else if result_flags
                                    .intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
                                {
                                    use_result = last_location
                                        .is_some_and(|l| self.kind_of(l) == SyntaxKind::Parameter)
                                        || (last_location == self.type_annotation_of(loc)
                                            && self
                                                .binder
                                                .symbol(found)
                                                .value_declaration
                                                .is_some_and(|d| {
                                                    self.find_ancestor_of_kind(
                                                        d,
                                                        SyntaxKind::Parameter,
                                                    )
                                                    .is_some()
                                                }));
                                }
                            }
                        } else if self.kind_of(loc) == SyntaxKind::ConditionalType {
                            // Type parameters declared in an infer are
                            // in scope in the TRUE branch only.
                            let NodeData::ConditionalType(data) = self.data_of(loc) else {
                                unreachable!("ConditionalType kind implies payload");
                            };
                            use_result = last_location == data.true_type;
                        }
                        if use_result {
                            result = Some(found);
                            break 'walk;
                        }
                    }
                }
            }
            within_deferred_context =
                within_deferred_context || self.get_is_deferred_context(loc, last_location);
            match self.kind_of(loc) {
                SyntaxKind::SourceFile | SyntaxKind::ModuleDeclaration => {
                    let is_source_file = self.kind_of(loc) == SyntaxKind::SourceFile;
                    if is_source_file && loc_is_global_source_file {
                        // falls out of the switch (globals handled at
                        // the walk's end).
                    } else {
                        // getSymbolOfDeclaration (19586): merged
                        // namespaces expose the UNION exports table.
                        let module_symbol = self
                            .binder
                            .node_symbol(loc)
                            .map(|s| self.get_merged_symbol(s));
                        let module_exports: SymbolTable = module_symbol
                            .map(|s| self.binder.symbol(s).exports.clone())
                            .unwrap_or_default();
                        if is_source_file
                            || (self.kind_of(loc) == SyntaxKind::ModuleDeclaration
                                && self.node_flags(loc) & NodeFlags::AMBIENT.bits() != 0
                                && !node_util::is_global_scope_augmentation(
                                    self.binder.source_of_node(loc),
                                    loc,
                                ))
                        {
                            // Default exports are not looked up by
                            // local name...
                            if let Some(&default_export) =
                                module_exports.get(tsrs2_types::InternalSymbolName::DEFAULT)
                            {
                                let local = self.local_symbol_for_export_default(default_export);
                                if let Some(local) = local {
                                    if self.binder.symbol(default_export).flags.intersects(meaning)
                                        && self.binder.symbol(local).escaped_name == name
                                    {
                                        result = Some(default_export);
                                        break 'walk;
                                    }
                                }
                            }
                            // ...and export specifiers/namespace
                            // exports of the name are alias-only: skip
                            // the module-exports lookup for them.
                            if let Some(&module_export) = module_exports.get(name) {
                                let export_symbol = self.binder.symbol(module_export);
                                if export_symbol.flags == SymbolFlags::ALIAS
                                    && (self
                                        .declaration_of_kind(
                                            module_export,
                                            SyntaxKind::ExportSpecifier,
                                        )
                                        .is_some()
                                        || self
                                            .declaration_of_kind(
                                                module_export,
                                                SyntaxKind::NamespaceExport,
                                            )
                                            .is_some())
                                {
                                    // break out of the switch only
                                    location = self.advance_walk(
                                        &mut last_location,
                                        &mut last_self_reference_location,
                                        loc,
                                    );
                                    continue 'walk;
                                }
                            }
                        }
                        if name != tsrs2_types::InternalSymbolName::DEFAULT {
                            let masked = meaning & SymbolFlags::MODULE_MEMBER;
                            let probe =
                                self.lookup_probe(&module_exports, name, masked, suggestion, false);
                            if let Some(found) = self.finish_lookup(probe, name, masked) {
                                // commonJsModuleIndicator + JSDoc type
                                // alias exception: JSDoc unmodeled, so
                                // a CJS file's exports never match.
                                let is_cjs = is_source_file
                                    && self
                                        .binder
                                        .file(self.binder.file_index_of_node(loc))
                                        .common_js_module_indicator
                                        .is_some();
                                if !is_cjs {
                                    result = Some(found);
                                    break 'walk;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::EnumDeclaration => {
                    // getSymbolOfDeclaration (19609).
                    let exports: SymbolTable = self
                        .binder
                        .node_symbol(loc)
                        .map(|s| self.get_merged_symbol(s))
                        .map(|s| self.binder.symbol(s).exports.clone())
                        .unwrap_or_default();
                    let masked = meaning & SymbolFlags::ENUM_MEMBER;
                    let probe = self.lookup_probe(&exports, name, masked, suggestion, false);
                    if let Some(found) = self.finish_lookup(probe, name, masked) {
                        // (isolatedModules cross-file qualification
                        // error elided — option unmodeled.)
                        result = Some(found);
                        break 'walk;
                    }
                }
                SyntaxKind::PropertyDeclaration => {
                    if !self.is_static_node(loc) {
                        if let Some(class) = self.parent_of(loc) {
                            if let Some(ctor) = self.find_constructor_declaration(class) {
                                if let Some(ctor_locals) = self.binder.locals_of(ctor) {
                                    let masked = meaning & SymbolFlags::VALUE;
                                    let probe = self
                                        .lookup_probe(ctor_locals, name, masked, suggestion, false);
                                    if self.finish_lookup(probe, name, masked).is_some() {
                                        property_with_invalid_initializer = Some(loc);
                                    }
                                }
                            }
                        }
                    }
                }
                SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::InterfaceDeclaration => {
                    // getSymbolOfDeclaration (19636): merged interface
                    // declarations see type parameters/members from
                    // EVERY declaration (lib interfaces merge).
                    let members: SymbolTable = self
                        .binder
                        .node_symbol(loc)
                        .map(|s| self.get_merged_symbol(s))
                        .map(|s| self.binder.symbol(s).members.clone())
                        .unwrap_or_default();
                    let masked = meaning & SymbolFlags::TYPE;
                    let probe = self.lookup_probe(&members, name, masked, suggestion, false);
                    if let Some(found) = self.finish_lookup(probe, name, masked) {
                        if self.is_type_parameter_symbol_declared_in_container(found, loc) {
                            if last_location.is_some_and(|l| self.is_static_node(l)) {
                                if name_not_found_message.is_some() {
                                    self.error_at(
                                        original_location,
                                        &diagnostics::Static_members_cannot_reference_class_type_parameters,
                                        &[],
                                    );
                                }
                                return None;
                            }
                            result = Some(found);
                            break 'walk;
                        }
                    }
                    if self.kind_of(loc) == SyntaxKind::ClassExpression
                        && meaning.intersects(SymbolFlags::CLASS)
                    {
                        let NodeData::ClassExpression(data) = self.data_of(loc) else {
                            unreachable!("ClassExpression kind implies payload");
                        };
                        if let Some(class_name) = data.name {
                            if self.identifier_text_of(class_name) == Some(name) {
                                result = self.binder.node_symbol(loc);
                                if result.is_some() {
                                    break 'walk;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::ExpressionWithTypeArguments => {
                    let NodeData::ExpressionWithTypeArguments(data) = self.data_of(loc) else {
                        unreachable!("kind implies payload");
                    };
                    if last_location == data.expression
                        && self
                            .parent_of(loc)
                            .is_some_and(|clause| self.heritage_clause_is_extends(clause))
                    {
                        let container = self
                            .parent_of(loc)
                            .and_then(|clause| self.parent_of(clause));
                        if let Some(container) = container {
                            if matches!(
                                self.kind_of(container),
                                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                            ) {
                                let members: SymbolTable = self
                                    .binder
                                    .node_symbol(container)
                                    // getSymbolOfDeclaration (19660).
                                    .map(|s| self.get_merged_symbol(s))
                                    .map(|s| self.binder.symbol(s).members.clone())
                                    .unwrap_or_default();
                                let masked = meaning & SymbolFlags::TYPE;
                                let probe =
                                    self.lookup_probe(&members, name, masked, suggestion, false);
                                if self.finish_lookup(probe, name, masked).is_some() {
                                    if name_not_found_message.is_some() {
                                        self.error_at(
                                            original_location,
                                            &diagnostics::Base_class_expressions_cannot_reference_class_type_parameters,
                                            &[],
                                        );
                                    }
                                    return None;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::ComputedPropertyName => {
                    let grandparent = self
                        .parent_of(loc)
                        .and_then(|parent| self.parent_of(parent));
                    if let Some(grandparent) = grandparent {
                        if matches!(
                            self.kind_of(grandparent),
                            SyntaxKind::ClassDeclaration
                                | SyntaxKind::ClassExpression
                                | SyntaxKind::InterfaceDeclaration
                        ) {
                            let members: SymbolTable = self
                                .binder
                                .node_symbol(grandparent)
                                // getSymbolOfDeclaration (19679).
                                .map(|s| self.get_merged_symbol(s))
                                .map(|s| self.binder.symbol(s).members.clone())
                                .unwrap_or_default();
                            let masked = meaning & SymbolFlags::TYPE;
                            let probe =
                                self.lookup_probe(&members, name, masked, suggestion, false);
                            if self.finish_lookup(probe, name, masked).is_some() {
                                if name_not_found_message.is_some() {
                                    self.error_at(
                                        original_location,
                                        &diagnostics::A_computed_property_name_cannot_reference_a_type_parameter_from_its_containing_type,
                                        &[],
                                    );
                                }
                                return None;
                            }
                        }
                    }
                }
                SyntaxKind::ArrowFunction
                    if self.options.emit_script_target() >= ScriptTarget::ES2015 => {}
                SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::FunctionDeclaration => {
                    if meaning.intersects(SymbolFlags::VARIABLE) && name == "arguments" {
                        result = Some(self.arguments_symbol);
                        break 'walk;
                    }
                }
                SyntaxKind::FunctionExpression => {
                    if meaning.intersects(SymbolFlags::VARIABLE) && name == "arguments" {
                        result = Some(self.arguments_symbol);
                        break 'walk;
                    }
                    if meaning.intersects(SymbolFlags::FUNCTION) {
                        let NodeData::FunctionExpression(data) = self.data_of(loc) else {
                            unreachable!("kind implies payload");
                        };
                        if let Some(function_name) = data.name {
                            if self.identifier_text_of(function_name) == Some(name) {
                                result = self.binder.node_symbol(loc);
                                if result.is_some() {
                                    break 'walk;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::Decorator => {
                    // Decorators are resolved outside the parameter/
                    // class-element they annotate.
                    let mut hop = loc;
                    if let Some(parent) = self.parent_of(hop) {
                        if self.kind_of(parent) == SyntaxKind::Parameter {
                            hop = parent;
                        }
                    }
                    if let Some(parent) = self.parent_of(hop) {
                        if self.is_class_element_kind(parent)
                            || self.kind_of(parent) == SyntaxKind::ClassDeclaration
                        {
                            hop = parent;
                        }
                    }
                    location = self.advance_walk(
                        &mut last_location,
                        &mut last_self_reference_location,
                        hop,
                    );
                    continue 'walk;
                }
                SyntaxKind::Parameter => {
                    let NodeData::Parameter(data) = self.data_of(loc) else {
                        unreachable!("kind implies payload");
                    };
                    if last_location.is_some()
                        && (last_location == data.initializer
                            || (last_location == data.name
                                && last_location.is_some_and(|l| {
                                    node_util::is_binding_pattern(self.binder.source_of_node(l), l)
                                })))
                        && associated_declaration_for_containing_initializer.is_none()
                    {
                        associated_declaration_for_containing_initializer = Some(loc);
                    }
                }
                SyntaxKind::BindingElement => {
                    let NodeData::BindingElement(data) = self.data_of(loc) else {
                        unreachable!("kind implies payload");
                    };
                    if last_location.is_some()
                        && (last_location == data.initializer
                            || (last_location == data.name
                                && last_location.is_some_and(|l| {
                                    node_util::is_binding_pattern(self.binder.source_of_node(l), l)
                                })))
                        && is_part_of_parameter_declaration(self.binder.source_of_node(loc), loc)
                        && associated_declaration_for_containing_initializer.is_none()
                    {
                        associated_declaration_for_containing_initializer = Some(loc);
                    }
                }
                SyntaxKind::InferType => {
                    if meaning.intersects(SymbolFlags::TYPE_PARAMETER) {
                        let NodeData::InferType(data) = self.data_of(loc) else {
                            unreachable!("kind implies payload");
                        };
                        if let Some(type_parameter) = data.type_parameter {
                            let NodeData::TypeParameter(tp) = self.data_of(type_parameter) else {
                                unreachable!("TypeParameter kind implies payload");
                            };
                            if let Some(tp_name) = tp.name {
                                if self.identifier_text_of(tp_name) == Some(name) {
                                    result = self.binder.node_symbol(type_parameter);
                                    if result.is_some() {
                                        break 'walk;
                                    }
                                }
                            }
                        }
                    }
                }
                SyntaxKind::ExportSpecifier => {
                    let NodeData::ExportSpecifier(data) = self.data_of(loc) else {
                        unreachable!("kind implies payload");
                    };
                    // Re-exports (`export { a as b } from "m"`) resolve
                    // the property name in the TARGET module, not here.
                    if last_location.is_some()
                        && last_location == data.property_name
                        && self.export_declaration_of_specifier_has_module_specifier(loc)
                    {
                        if let Some(hop) = self
                            .parent_of(loc)
                            .and_then(|named| self.parent_of(named))
                            .and_then(|declaration| self.parent_of(declaration))
                        {
                            location = self.advance_walk(
                                &mut last_location,
                                &mut last_self_reference_location,
                                hop,
                            );
                            continue 'walk;
                        }
                    }
                }
                _ => {}
            }
            location =
                self.advance_walk(&mut last_location, &mut last_self_reference_location, loc);
        }

        // tsc: `result.isReferenced |= meaning` for uses outside the
        // self-reference location — M7's unused-diagnostics consumer;
        // the flags are not stored yet.
        let _ = (is_use, &last_self_reference_location);

        if result.is_none() {
            if let Some(last) = last_location {
                debug_assert_eq!(self.kind_of(last), SyntaxKind::SourceFile);
                let file_index = self.binder.file_index_of_node(last);
                if self
                    .binder
                    .file(file_index)
                    .common_js_module_indicator
                    .is_some()
                    && name == "exports"
                {
                    if let Some(file_symbol) = self.binder.node_symbol(last) {
                        if self.binder.symbol(file_symbol).flags.intersects(meaning) {
                            return Some(file_symbol);
                        }
                    }
                }
            }
            if !exclude_globals {
                let globals = self.globals.clone();
                let probe = self.lookup_probe(&globals, name, meaning, suggestion, true);
                result = self.finish_lookup(probe, name, meaning);
            }
        }
        // (JS `require` fallback elided — requireSymbol, M2 3.4c
        // residual; plain-JS diagnostics are allowlist-filtered.)

        if let Some(message) = name_not_found_message {
            if let Some(property) = property_with_invalid_initializer {
                if self.check_and_report_error_for_invalid_initializer(
                    original_location,
                    name,
                    property,
                ) {
                    return None;
                }
            }
            match result {
                None => self.on_failed_to_resolve_symbol(original_location, name, meaning, message),
                Some(found) => self.on_successfully_resolved_symbol(
                    original_location,
                    found,
                    meaning,
                    associated_declaration_for_containing_initializer,
                    within_deferred_context,
                ),
            }
        }
        result
    }

    fn advance_walk(
        &self,
        last_location: &mut Option<NodeId>,
        last_self_reference_location: &mut Option<NodeId>,
        loc: NodeId,
    ) -> Option<NodeId> {
        if self.is_self_reference_location(loc, *last_location) {
            *last_self_reference_location = Some(loc);
        }
        *last_location = Some(loc);
        // JSDoc template/parameter/return-tag re-routing elided (JSDoc
        // nodes unmodeled).
        self.parent_of(loc)
    }

    /// tsc-port: useOuterVariableScopeInParameter @6.0.3
    /// tsc-hash: 0a66813bef44f5421005e88434c0208291c927c87282f05fdecf37e4c199058b
    /// tsc-span: _tsc.js:19804-19849
    fn use_outer_variable_scope_in_parameter(
        &self,
        result: SymbolId,
        location: NodeId,
        last_location: Option<NodeId>,
    ) -> bool {
        let Some(last) = last_location else {
            return false;
        };
        if self.kind_of(last) != SyntaxKind::Parameter {
            return false;
        }
        let source = self.binder.source_of_node(location);
        let Some(body) = body_of(source, location) else {
            return false;
        };
        let Some(value_declaration) = self.binder.symbol(result).value_declaration else {
            return false;
        };
        let body_node = source.arena.node(body);
        let decl_node = source.arena.node(value_declaration);
        if !(decl_node.pos >= body_node.pos && decl_node.end <= body_node.end) {
            return false;
        }
        if self.options.emit_script_target() >= ScriptTarget::ES2015 {
            // requiresScopeChange: any parameter whose emit needs a
            // scope change keeps the parameter scope. The worker walks
            // parameter names + initializers for downlevel constructs;
            // at target >= ES2015 the only sub-ES2015 constructs are
            // optional chains/nullish (ES2020) and static class fields
            // — recurse per tsc.
            let parameters = self.parameters_of(location);
            let requires_change = parameters.iter().any(|&parameter| {
                let NodeData::Parameter(data) = self.data_of(parameter) else {
                    return false;
                };
                data.name
                    .is_some_and(|n| self.requires_scope_change_worker(n))
                    || data
                        .initializer
                        .is_some_and(|n| self.requires_scope_change_worker(n))
            });
            return !requires_change;
        }
        false
    }

    fn requires_scope_change_worker(&self, node: NodeId) -> bool {
        let target = self.options.emit_script_target();
        match self.kind_of(node) {
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor => false,
            SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::PropertyAssignment => self
                .name_of_node(node)
                .is_some_and(|n| self.requires_scope_change_worker(n)),
            SyntaxKind::PropertyDeclaration => {
                if has_syntactic_modifier(
                    self.binder.source_of_node(node),
                    node,
                    ModifierFlags::STATIC,
                ) {
                    // getEmitStandardClassFields: useDefineForClassFields
                    // is unmodeled (defaults by target).
                    return target < ScriptTarget::ES2022;
                }
                self.name_of_node(node)
                    .is_some_and(|n| self.requires_scope_change_worker(n))
            }
            _ => {
                let source = self.binder.source_of_node(node);
                if node_util::is_nullish_coalesce(source, node)
                    || node_util::is_optional_chain(source, node)
                {
                    return target < ScriptTarget::ES2020;
                }
                if let NodeData::BindingElement(data) = self.data_of(node) {
                    if data.dot_dot_dot_token.is_some()
                        && self
                            .parent_of(node)
                            .is_some_and(|p| self.kind_of(p) == SyntaxKind::ObjectBindingPattern)
                    {
                        return target < ScriptTarget::ES2017;
                    }
                }
                if self.is_type_node_kind(self.kind_of(node)) {
                    return false;
                }
                self.child_nodes_of(node)
                    .iter()
                    .any(|&child| self.requires_scope_change_worker(child))
            }
        }
    }

    /// tsc-port: getIsDeferredContext @6.0.3
    /// tsc-hash: 27070614d3f5101b64a27b7dd0c8bb6afbf88e06259b35113ab9adf642d2b5f7
    /// tsc-span: _tsc.js:19850-19861
    fn get_is_deferred_context(&self, location: NodeId, last_location: Option<NodeId>) -> bool {
        let kind = self.kind_of(location);
        let source = self.binder.source_of_node(location);
        if kind != SyntaxKind::ArrowFunction && kind != SyntaxKind::FunctionExpression {
            return node_util::is_part_of_type_query(source, location)
                && self.kind_of(location) == SyntaxKind::TypeQuery
                || (is_function_like_declaration_kind(kind)
                    || (kind == SyntaxKind::PropertyDeclaration
                        && !self.is_static_node(location)))
                    && (last_location.is_none() || last_location != self.name_of_node(location));
        }
        if last_location.is_some() && last_location == self.name_of_node(location) {
            return false;
        }
        if node_util::asterisk_token_of(source, location).is_some()
            || has_syntactic_modifier(source, location, ModifierFlags::ASYNC)
        {
            return true;
        }
        get_immediately_invoked_function_expression(source, location).is_none()
    }

    /// tsc-port: isSelfReferenceLocation @6.0.3
    /// tsc-hash: 5ad18c433fa49d3ac4b297f5e4590e8e7914461403f401572284d9a9c8e79ded
    /// tsc-span: _tsc.js:19862-19876
    fn is_self_reference_location(&self, node: NodeId, last_location: Option<NodeId>) -> bool {
        match self.kind_of(node) {
            SyntaxKind::Parameter => {
                last_location.is_some() && last_location == self.name_of_node(node)
            }
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::ModuleDeclaration => true,
            _ => false,
        }
    }

    /// tsc-port: isTypeParameterSymbolDeclaredInContainer @6.0.3
    /// tsc-hash: 784e4b5b0f8c0ac88e84fdeba8c8c81f77fc90280d4c90e9348e5b479ab5b503
    /// tsc-span: _tsc.js:19877-19890
    ///
    /// JSDoc template-tag hosts elided.
    fn is_type_parameter_symbol_declared_in_container(
        &self,
        symbol: SymbolId,
        container: NodeId,
    ) -> bool {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| {
                self.kind_of(declaration) == SyntaxKind::TypeParameter
                    && self.parent_of(declaration) == Some(container)
            })
    }

    /// tsc-port: getLocalSymbolForExportDefault @6.0.3
    /// tsc-hash: db0d13354a1e29a6237f541673b56fdd5e3a4e228e358b7fce92b8bcd09258bd
    /// tsc-span: _tsc.js:17195-17198
    fn local_symbol_for_export_default(&self, symbol: SymbolId) -> Option<SymbolId> {
        let data = self.binder.symbol(symbol);
        let first = *data.declarations.first()?;
        if !has_syntactic_modifier(
            self.binder.source_of_node(first),
            first,
            ModifierFlags::DEFAULT,
        ) {
            return None;
        }
        data.declarations.iter().find_map(|&declaration| {
            let binder = self
                .binder
                .file(self.binder.file_index_of_node(declaration));
            binder.node_local_symbol.get(&declaration).copied()
        })
    }

    /// tsc-port: checkAndReportErrorForInvalidInitializer @6.0.3
    /// tsc-hash: 83a792236d78e75da06ed44973e4ce9875dcf7cf07b558b8116cbe7719c3a8a7
    /// tsc-span: _tsc.js:48096-48110
    ///
    /// The checkAndReportErrorForMissingPrefix alternate is a
    /// suggestion-family row (M8). getEmitStandardClassFields:
    /// useDefineForClassFields unmodeled — target >= ES2022.
    fn check_and_report_error_for_invalid_initializer(
        &mut self,
        error_location: Option<NodeId>,
        name: &str,
        property: NodeId,
    ) -> bool {
        if self.options.emit_script_target() >= ScriptTarget::ES2022 {
            return false;
        }
        let NodeData::PropertyDeclaration(data) = self.data_of(property) else {
            unreachable!("PropertyDeclaration kind implies payload");
        };
        let in_type = match (error_location, data.r#type) {
            (Some(error_node), Some(type_node)) => {
                let source = self.binder.source_of_node(property);
                let error_pos = self
                    .binder
                    .source_of_node(error_node)
                    .arena
                    .node(error_node)
                    .pos;
                let type_range = source.arena.node(type_node);
                type_range.pos <= error_pos && error_pos <= type_range.end
            }
            _ => false,
        };
        let message = if in_type {
            &diagnostics::Type_of_instance_member_variable_0_cannot_reference_identifier_1_declared_in_the_constructor
        } else {
            &diagnostics::Initializer_of_instance_member_variable_0_cannot_reference_identifier_1_declared_in_the_constructor
        };
        let property_name = data
            .name
            .map(|n| {
                node_util::declaration_name_to_string(self.binder.source_of_node(property), Some(n))
            })
            .unwrap_or_default();
        self.error_at(
            error_location,
            message,
            &[
                &property_name,
                tsrs2_binder::unescape_leading_underscores(name),
            ],
        );
        true
    }

    /// tsc-port: onFailedToResolveSymbol @6.0.3 (PARTIAL)
    /// tsc-hash: 26a00d2e7d55d3d390e91be33ad3fa83b5e644a07fd724974bd352b3133829c5
    /// tsc-span: _tsc.js:48111-48155
    ///
    /// Plain-form slice per the M4 5.1 doc, widened at 5.5a with the
    /// two chain members expression forcing made reachable:
    /// checkAndReportErrorForMissingPrefix (2662/2663, full port) and
    /// the keyword-based primitive-name arms (2661/2693 + the heritage
    /// flavors — no symbol exists for `string` et al., so the
    /// all-meanings re-probe gate below cannot stand in for them). The
    /// remaining checkAndReportErrorFor* alternates and the spelling
    /// suggestion stay M8 rows behind the re-probe gate; NB the
    /// name-side 2552 Did-you-mean arm is oracle-unobserved on plain
    /// near-miss fixtures (probed 2026-07-12: cet/cat, greeet/greet,
    /// myVariabel/myVariable all emit plain 2304). tsc defers via
    /// addLazyDiagnostic; emission is eager here and the driver's
    /// final sort canonicalizes order.
    fn on_failed_to_resolve_symbol(
        &mut self,
        error_location: Option<NodeId>,
        name: &str,
        meaning: SymbolFlags,
        message: &'static DiagnosticMessage,
    ) {
        // checkAndReportErrorForMissingPrefix (48220-48249) runs FIRST
        // in tsc's chain. An Unsupported unwind inside its member
        // probes makes the alternate-vs-plain choice undecidable:
        // suppress the whole report (honest FN).
        match self.check_and_report_error_for_missing_prefix(error_location, name) {
            Ok(true) => return,
            Ok(false) => {}
            Err(_) => return,
        }
        // The primitive-name slice of the chain (isPrimitiveTypeName
        // 48334): checkAndReportErrorForExportingPrimitiveType (48337)
        // + checkAndReportErrorForUsingTypeAsValue's keyword arm
        // (48344-48362). The interleaved ExtendingInterface /
        // UsingTypeAsNamespace members and UsingTypeAsValue's symbol
        // arm never fire for these names (nothing resolves under any
        // meaning), so the slice is exact.
        if meaning.intersects(SymbolFlags::VALUE) && is_primitive_type_name(name) {
            if let Some(error_location) = error_location {
                self.report_primitive_type_name_used_as_value(error_location, name);
                return;
            }
        }
        // Failure-band gates, each an honest FN escape (no emission)
        // for a case where the plain form would be an un-tsc-like
        // diagnostic (the third 5.4-era gate — the lib_globals name
        // table — RETIRED with lib loading: conformance programs carry
        // their lib set, so a default-lib name either resolves or is
        // genuinely missing, where tsc reports too):
        // 1. tsc's remaining checkAndReportErrorFor* alternates
        //    (value-as-type, type-as-value symbol arm,
        //    namespace-as-type, 2749/2693/2503-family) and the
        //    alias-resolving meaning criteria both key on a symbol
        //    EXISTING under a different meaning; those arms are
        //    unported (alias resolution 5.8, alternates M8), so a
        //    quiet all-meanings re-probe finding anything means the
        //    plain form would be the wrong code.
        if self
            .resolve_name(error_location, name, SymbolFlags::ALL, None, false, false)
            .is_some()
        {
            return;
        }
        // 2. `declare global` blocks can inject arbitrary names into
        //    the global scope; global-augmentation binding is
        //    unported (M2 3.4c-adjacent / 5.8), so failures in such
        //    programs are undecidable.
        if self.program_has_global_augmentation() {
            return;
        }
        // getSuggestedLibForNonExistentName is static lib metadata, so
        // the 2583/2584-family lib arm is exact. The SPELLING branch
        // (48123-48151) is budget-gated: suggestionCount < 10, where
        // the noLib bootstrap burns all 10 (run_init_global_type_probes)
        // — oracle-pinned via strictBindCallApply:false. Every failure
        // reaching this tail consumes one slot, suggestion or not.
        let display = tsrs2_binder::unescape_leading_underscores(name);
        let suggested_lib = get_suggested_lib_for_non_existent_name(name);
        if let Some(lib) = suggested_lib {
            self.error_at(error_location, message, &[display, lib]);
        } else {
            let mut suggestion: Option<SymbolId> = None;
            if self.suggestion_count < MAXIMUM_SUGGESTION_COUNT {
                suggestion =
                    self.resolve_name_for_symbol_suggestion(error_location, name, meaning);
                // The isGlobalScopeAugmentationDeclaration filter
                // (48126-48129) is provably dead here: the
                // program_has_global_augmentation gate above already
                // returned for any program that could produce one.
                if let Some(suggested) = suggestion {
                    let suggestion_name = self.symbol_display_name(suggested);
                    // Namespace meaning selects the 2833 flavor; the
                    // unchecked-JS 2570 flavor is JS-band (TS: 2552).
                    let did_you_mean = if meaning == SymbolFlags::NAMESPACE {
                        &diagnostics::Cannot_find_namespace_0_Did_you_mean_1
                    } else {
                        &diagnostics::Cannot_find_name_0_Did_you_mean_1
                    };
                    let mut diagnostic = self.diagnostic_for_node_or_compiler(
                        error_location,
                        did_you_mean,
                        &[display, &suggestion_name],
                    );
                    // getCanonicalDiagnostic(nameNotFoundMessage, name):
                    // sort/dedupe compare through the PLAIN form.
                    diagnostic.canonical_head = Some(tsrs2_diags::CanonicalHead {
                        code: message.code,
                        text: tsrs2_diags::MessageChain::new(
                            message,
                            &[display.to_owned()],
                        )
                        .text,
                    });
                    if let Some(value_declaration) =
                        self.binder.symbol(suggested).value_declaration
                    {
                        diagnostic.related.push(self.related_info_for_node(
                            value_declaration,
                            &diagnostics::_0_is_declared_here,
                            &[&suggestion_name],
                        ));
                    }
                    self.push_error_diagnostic(diagnostic);
                }
            }
            if suggestion.is_none() {
                self.error_at(error_location, message, &[display]);
            }
        }
        self.suggestion_count += 1;
    }

    /// createError: node-anchored when a location exists, compiler-
    /// global otherwise (the locationless bootstrap flavors).
    fn diagnostic_for_node_or_compiler(
        &self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> tsrs2_diags::Diagnostic {
        match location {
            Some(node) => self.diagnostic_for_node(node, message, args),
            None => {
                let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
                tsrs2_diags::Diagnostic::new(
                    None,
                    None,
                    None,
                    tsrs2_diags::MessageChain::new(message, &args),
                )
            }
        }
    }

    /// tsc-port: checkAndReportErrorForMissingPrefix @6.0.3
    /// tsc-hash: 25a622a6654f76867484ca5da07f19a0eb74567e557d5a04a62422c98036c732
    /// tsc-span: _tsc.js:48220-48249
    ///
    /// The static probe reads the class VALUE side (5.3e) and the
    /// instance probe the declared thisType's apparent members (5.3b)
    /// — both live; nameArg display = unescapeLeadingUnderscores.
    fn check_and_report_error_for_missing_prefix(
        &mut self,
        error_location: Option<NodeId>,
        name: &str,
    ) -> crate::state::CheckResult2<bool> {
        let Some(error_location) = error_location else {
            return Ok(false);
        };
        if self.kind_of(error_location) != SyntaxKind::Identifier
            || self.identifier_text_of(error_location) != Some(name)
            || self.is_type_reference_identifier(error_location)
            || self.is_in_type_query(error_location)
        {
            return Ok(false);
        }
        let container = tsrs2_binder::node_util::get_this_container(
            self.binder.source_of_node(error_location),
            error_location,
            /*include_arrow_functions*/ false,
        );
        let Some(container) = container else {
            return Ok(false);
        };
        let mut location = Some(container);
        while let Some(current) = location {
            let parent = self.parent_of(current);
            if let Some(parent) = parent {
                if matches!(
                    self.kind_of(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) {
                    let Some(class_symbol) = self.node_symbol(parent) else {
                        break;
                    };
                    let class_symbol = self.get_merged_symbol(class_symbol);
                    let constructor_type = self.get_type_of_symbol(class_symbol)?;
                    if self
                        .get_property_of_type_full(constructor_type, name)?
                        .is_some()
                    {
                        let class_name = self.symbol_display_name(class_symbol);
                        self.error_at(
                            Some(error_location),
                            &diagnostics::Cannot_find_name_0_Did_you_mean_the_static_member_1_0,
                            &[
                                tsrs2_binder::unescape_leading_underscores(name),
                                &class_name,
                            ],
                        );
                        return Ok(true);
                    }
                    if current == container && !self.is_static_node(current) {
                        let declared =
                            self.get_declared_type_of_class_or_interface(class_symbol)?;
                        let this_type = match &self.tables.type_of(declared).data {
                            tsrs2_types::TypeData::GenericType { this_type, .. } => {
                                Some(*this_type)
                            }
                            _ => None,
                        };
                        if let Some(this_type) = this_type {
                            if self.get_property_of_type_full(this_type, name)?.is_some() {
                                self.error_at(
                                    Some(error_location),
                                    &diagnostics::Cannot_find_name_0_Did_you_mean_the_instance_member_this_0,
                                    &[tsrs2_binder::unescape_leading_underscores(name)],
                                );
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            location = parent;
        }
        Ok(false)
    }

    /// tsc isTypeReferenceIdentifier (87218).
    fn is_type_reference_identifier(&self, node: NodeId) -> bool {
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            if self.kind_of(parent) != SyntaxKind::QualifiedName {
                break;
            }
            node = parent;
        }
        self.parent_of(node)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::TypeReference)
    }

    /// The primitive-name arm of checkAndReportErrorForUsingTypeAsValue
    /// (48344-48362) plus checkAndReportErrorForExportingPrimitiveType
    /// (48337-48343), which precedes it in the chain.
    fn report_primitive_type_name_used_as_value(&mut self, error_location: NodeId, name: &str) {
        let parent = self.parent_of(error_location);
        if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExportSpecifier) {
            self.error_at(
                Some(error_location),
                &diagnostics::Cannot_export_0_Only_local_declarations_can_be_exported_from_a_module,
                &[name],
            );
            return;
        }
        let grandparent = parent.and_then(|parent| self.parent_of(parent));
        if let Some(grandparent) = grandparent {
            if let NodeData::HeritageClause(_) = self.data_of(grandparent) {
                let heritage_is_extends = self.heritage_clause_is_extends(grandparent);
                let container = self.parent_of(grandparent);
                let container_kind = container.map(|container| self.kind_of(container));
                if container_kind == Some(SyntaxKind::InterfaceDeclaration) && heritage_is_extends
                {
                    self.error_at(
                        Some(error_location),
                        &diagnostics::An_interface_cannot_extend_a_primitive_type_like_0_It_can_only_extend_other_named_object_types,
                        &[tsrs2_binder::unescape_leading_underscores(name)],
                    );
                    return;
                }
                let container_is_class_like = matches!(
                    container_kind,
                    Some(SyntaxKind::ClassDeclaration) | Some(SyntaxKind::ClassExpression)
                );
                if container_is_class_like && heritage_is_extends {
                    self.error_at(
                        Some(error_location),
                        &diagnostics::A_class_cannot_extend_a_primitive_type_like_0_Classes_can_only_extend_constructable_values,
                        &[tsrs2_binder::unescape_leading_underscores(name)],
                    );
                    return;
                }
                if container_is_class_like && !heritage_is_extends {
                    // ImplementsKeyword is the only other heritage
                    // token.
                    self.error_at(
                        Some(error_location),
                        &diagnostics::A_class_cannot_implement_a_primitive_type_like_0_It_can_only_implement_other_named_object_types,
                        &[tsrs2_binder::unescape_leading_underscores(name)],
                    );
                    return;
                }
            }
        }
        self.error_at(
            Some(error_location),
            &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here,
            &[tsrs2_binder::unescape_leading_underscores(name)],
        );
    }

    /// tsc-port: onSuccessfullyResolvedSymbol @6.0.3 (5.5a slice)
    /// tsc-hash: acc9b965f1efc2a1b2b86a42dfcd0415aa683933fb79d87a0a4a9db0c34ec7af
    /// tsc-span: _tsc.js:48156-48205
    ///
    /// Live from 5.5a: the checkResolvedBlockScopedVariable arm (the
    /// TDZ 2448/2449/2450 band; addLazyDiagnostic = eager identity).
    /// Still stubbed, each with its owner: the UMD-global module check
    /// (module resolution, 5.8), parameter self-reference 2372/2373
    /// (initializer forcing, 5.6), type-only alias use (alias
    /// resolution, 5.8), and the isolatedModules import conflict
    /// (option unmodeled).
    fn on_successfully_resolved_symbol(
        &mut self,
        error_location: Option<NodeId>,
        result: SymbolId,
        meaning: SymbolFlags,
        _associated_declaration: Option<NodeId>,
        _within_deferred_context: bool,
    ) {
        let Some(error_location) = error_location else {
            return;
        };
        if meaning.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
            || (meaning.intersects(SymbolFlags::CLASS | SymbolFlags::ENUM)
                && meaning.contains(SymbolFlags::VALUE))
        {
            let export_or_local = self.get_export_symbol_of_value_symbol_if_exported(result);
            if self.binder.symbol(export_or_local).flags.intersects(
                SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS | SymbolFlags::ENUM,
            ) {
                // Unsupported unwinds inside the declared-before-use
                // walk (property/parameter arms 5.5d) drop the TDZ
                // report for that reference — an honest FN; tsc has no
                // failure channel here.
                let _ = self.check_resolved_block_scoped_variable(export_or_local, error_location);
            }
        }
    }

    /// tsc-port: checkResolvedBlockScopedVariable @6.0.3
    /// tsc-hash: d342f112df1bf209ef6fd3dbd51e9d27223e206563f3a0f0af10153c496fef71
    /// tsc-span: _tsc.js:48448-48477
    ///
    /// The ConstEnum tail is gated on getIsolatedModules — the option
    /// is absent from CompilerOptions, so const-enum TDZ uses stay
    /// silent exactly like tsc-without-isolatedModules (no
    /// diagnosticMessage ⇒ no related info either).
    fn check_resolved_block_scoped_variable(
        &mut self,
        result: SymbolId,
        error_location: NodeId,
    ) -> crate::state::CheckResult2<()> {
        let flags = self.binder.symbol(result).flags;
        debug_assert!(flags.intersects(
            SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS | SymbolFlags::ENUM
        ));
        if flags.intersects(
            SymbolFlags::FUNCTION
                | SymbolFlags::FUNCTION_SCOPED_VARIABLE
                | SymbolFlags::ASSIGNMENT,
        ) && flags.intersects(SymbolFlags::CLASS)
        {
            return Ok(());
        }
        let declarations = self.binder.symbol(result).declarations.clone();
        let declaration = declarations.into_iter().find(|&declaration| {
            let source = self.binder.source_of_node(declaration);
            node_util::is_block_or_catch_scoped(source, declaration)
                || matches!(
                    self.kind_of(declaration),
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::ClassExpression
                        | SyntaxKind::EnumDeclaration
                )
        });
        let declaration = declaration
            .expect("checkResolvedBlockScopedVariable could not find block-scoped declaration");
        if self.node_flags(declaration) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0 {
            return Ok(());
        }
        if self.is_block_scoped_name_declared_before_use(declaration, error_location)? {
            return Ok(());
        }
        let declaration_source = self.binder.source_of_node(declaration);
        let declaration_name = node_util::declaration_name_to_string(
            declaration_source,
            node_util::get_name_of_declaration(declaration_source, declaration),
        );
        let message = if flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
            Some(&diagnostics::Block_scoped_variable_0_used_before_its_declaration)
        } else if flags.intersects(SymbolFlags::CLASS) {
            Some(&diagnostics::Class_0_used_before_its_declaration)
        } else if flags.intersects(SymbolFlags::REGULAR_ENUM) {
            Some(&diagnostics::Enum_0_used_before_its_declaration)
        } else {
            debug_assert!(flags.intersects(SymbolFlags::CONST_ENUM));
            // getIsolatedModules(compilerOptions): option unmodeled ⇒
            // false ⇒ no message.
            None
        };
        if let Some(message) = message {
            let related = self.create_error(
                Some(declaration),
                &diagnostics::_0_is_declared_here,
                &[&declaration_name],
            );
            self.error_at_with_related(
                Some(error_location),
                message,
                &[&declaration_name],
                vec![tsrs2_diags::RelatedInfo {
                    file_name: related.file_name,
                    start: related.start,
                    length: related.length,
                    message: related.message,
                }],
            );
        }
        Ok(())
    }

    // ---- resolveEntityName ----

    /// tsc-port: resolveEntityName @6.0.3
    /// tsc-hash: 0c5ce0e5980d5548db101cd9240b04944dea6e35cde2b0b3416210816fdb85b9
    /// tsc-span: _tsc.js:49292-49393
    ///
    /// Slices, each ledgered: the JS prototype-assignment secondary
    /// lookup (JSDoc/expando — unmodeled), the CJS-require namespace
    /// re-resolution, the three suggestion alternates in the
    /// missing-export path (M8; the plain 2694 form is emitted), the
    /// type-only alias marking, and the final resolveAlias hop (import
    /// semantics — an alias symbol returns unchanged; 5.8 replaces).
    /// getExportsOfSymbol is the plain exports table until 5.3's
    /// late-binding.
    pub fn resolve_entity_name(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
        ignore_errors: bool,
        location: Option<NodeId>,
    ) -> Option<SymbolId> {
        if node_util::node_is_missing(self.binder.source_of_node(name), Some(name)) {
            return None;
        }
        let namespace_meaning = SymbolFlags::NAMESPACE;
        match self.kind_of(name) {
            SyntaxKind::Identifier => {
                let text = self.identifier_text_of(name)?.to_owned();
                let message = if meaning == namespace_meaning {
                    &diagnostics::Cannot_find_namespace_0
                } else {
                    self.cannot_find_name_diagnostic_for_name(name)
                };
                let symbol = self.resolve_name(
                    location.or(Some(name)),
                    &text,
                    meaning,
                    (!ignore_errors).then_some(message),
                    true,
                    false,
                )?;
                let symbol = self.get_merged_symbol(symbol);
                self.finish_resolve_entity_name(symbol, meaning)
            }
            SyntaxKind::QualifiedName | SyntaxKind::PropertyAccessExpression => {
                let (left, right) = match self.data_of(name) {
                    NodeData::QualifiedName(data) => (data.left, data.right),
                    NodeData::PropertyAccessExpression(data) => (data.expression, data.name),
                    _ => unreachable!("kind implies payload"),
                };
                let left = left?;
                let right = right?;
                let namespace =
                    self.resolve_entity_name(left, namespace_meaning, ignore_errors, location)?;
                if node_util::node_is_missing(self.binder.source_of_node(right), Some(right)) {
                    return None;
                }
                if namespace == self.unknown_symbol {
                    return Some(namespace);
                }
                let right_text = self.identifier_text_of(right)?.to_owned();
                // getExportsOfSymbol's globalThis special case (47710):
                // globalThisSymbol's exports ARE the merged globals
                // table (initializeTypeChecker 46492 aliases them).
                let exports = if namespace == self.global_this_symbol {
                    self.globals.clone()
                } else {
                    self.binder.symbol(namespace).exports.clone()
                };
                let symbol = self
                    .get_symbol_in_table(&exports, &right_text, meaning)
                    .map(|s| self.get_merged_symbol(s));
                let Some(symbol) = symbol else {
                    if !ignore_errors {
                        let namespace_name = self.fully_qualified_name(namespace);
                        let declaration_name = node_util::declaration_name_to_string(
                            self.binder.source_of_node(right),
                            Some(right),
                        );
                        // (M8) getSuggestedSymbolForNonexistentModule /
                        // typeof-suggestion / type-not-namespace
                        // alternates elided — plain form only.
                        self.error_at(
                            Some(right),
                            &diagnostics::Namespace_0_has_no_exported_member_1,
                            &[&namespace_name, &declaration_name],
                        );
                    }
                    return None;
                };
                self.finish_resolve_entity_name(symbol, meaning)
            }
            _ => unreachable!("Unknown entity name kind."),
        }
    }

    /// The resolveEntityName tail: `symbol.flags & meaning ||
    /// dontResolveAlias ? symbol : resolveAlias(symbol)` — resolveAlias
    /// is unported (M4 5.8): alias symbols return unchanged, ledgered.
    fn finish_resolve_entity_name(
        &mut self,
        symbol: SymbolId,
        meaning: SymbolFlags,
    ) -> Option<SymbolId> {
        let _ = meaning;
        Some(symbol)
    }

    /// tsc-port: getCannotFindNameDiagnosticForName @6.0.3
    /// tsc-hash: 734be6af4f9e91c907d939e00c3f811d94394f8be1634532f714c456a345cc6e
    /// tsc-span: _tsc.js:69324-69376
    ///
    /// usesWildcardTypes(compilerOptions): the `types` option is
    /// unmodeled and absent in harness programs — the `some(types,
    /// "*")` test is false, selecting the long-form messages.
    pub(crate) fn cannot_find_name_diagnostic_for_name(
        &self,
        node: NodeId,
    ) -> &'static DiagnosticMessage {
        let first = self.first_identifier(node);
        let text = self.identifier_text_of(first).unwrap_or_default();
        match text {
            "document" | "console" => &diagnostics::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_include_dom,
            "$" => &diagnostics::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_jQuery_Try_npm_i_save_dev_types_jquery_and_then_add_jquery_to_the_types_field_in_your_tsconfig,
            "beforeEach" | "describe" | "suite" | "it" | "test" => &diagnostics::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_a_test_runner_Try_npm_i_save_dev_types_jest_or_npm_i_save_dev_types_mocha_and_then_add_jest_or_mocha_to_the_types_field_in_your_tsconfig,
            "process" | "require" | "Buffer" | "module" | "NodeJS" => &diagnostics::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_node_Try_npm_i_save_dev_types_node_and_then_add_node_to_the_types_field_in_your_tsconfig,
            "Bun" => &diagnostics::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_Bun_Try_npm_i_save_dev_types_bun_and_then_add_bun_to_the_types_field_in_your_tsconfig,
            "Map" | "Set" | "Promise" | "Symbol" | "WeakMap" | "WeakSet" | "Iterator"
            | "AsyncIterator" | "SharedArrayBuffer" | "Atomics" | "AsyncIterable"
            | "AsyncIterableIterator" | "AsyncGenerator" | "AsyncGeneratorFunction" | "BigInt"
            | "Reflect" | "BigInt64Array" | "BigUint64Array" => &diagnostics::Cannot_find_name_0_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_1_or_later,
            "await" if self
                .parent_of(node)
                .is_some_and(|p| self.kind_of(p) == SyntaxKind::CallExpression) => {
                &diagnostics::Cannot_find_name_0_Did_you_mean_to_write_this_in_an_async_function
            }
            _ => {
                if self
                    .parent_of(node)
                    .is_some_and(|p| self.kind_of(p) == SyntaxKind::ShorthandPropertyAssignment)
                {
                    &diagnostics::No_value_exists_in_scope_for_the_shorthand_property_0_Either_declare_one_or_provide_an_initializer
                } else {
                    &diagnostics::Cannot_find_name_0
                }
            }
        }
    }

    /// tsc-port: getFirstIdentifier @6.0.3
    /// tsc-hash: 7e4c88a83ebe44c7df44adf8d76fc1302c392d22d382068bd0d5f85a7feea3f1
    /// tsc-span: _tsc.js:17131-17144
    pub(crate) fn first_identifier(&self, node: NodeId) -> NodeId {
        let mut current = node;
        loop {
            match self.data_of(current) {
                NodeData::QualifiedName(data) => match data.left {
                    Some(left) => current = left,
                    None => return current,
                },
                NodeData::PropertyAccessExpression(data) => match data.expression {
                    Some(expression) => current = expression,
                    None => return current,
                },
                _ => return current,
            }
        }
    }

    /// tsc getFullyQualifiedName slice for the 2694 message arg: the
    /// parent-chain dotted name (the full symbolToString display walk
    /// is M8 tail).
    fn fully_qualified_name(&self, symbol: SymbolId) -> String {
        let mut parts = vec![self.symbol_display_name(symbol)];
        let mut current = self.binder.symbol(symbol).parent;
        while let Some(parent) = current {
            let data = self.binder.symbol(parent);
            if data.escaped_name.starts_with("__") {
                break;
            }
            parts.push(self.symbol_display_name(parent));
            current = data.parent;
        }
        parts.reverse();
        parts.join(".")
    }

    // ---- small structural predicates ----

    /// tsc isConstAssertion: (as/angle-bracket assertion) whose type is
    /// the `const` type reference.
    fn is_const_assertion(&self, node: NodeId) -> bool {
        let type_node = match self.data_of(node) {
            NodeData::AsExpression(data) => data.r#type,
            NodeData::TypeAssertionExpression(data) => data.r#type,
            _ => return None::<()>.is_some(),
        };
        let Some(type_node) = type_node else {
            return false;
        };
        let NodeData::TypeReference(data) = self.data_of(type_node) else {
            return false;
        };
        data.type_arguments.is_none()
            && data
                .type_name
                .is_some_and(|n| self.identifier_text_of(n) == Some("const"))
    }

    pub(crate) fn name_of_node(&self, node: NodeId) -> Option<NodeId> {
        node_util::name_field_of(self.binder.source_of_node(node), node)
    }

    pub(crate) fn identifier_text_of(&self, node: NodeId) -> Option<&'a str> {
        match self.data_of(node) {
            NodeData::Identifier(data) => Some(&data.escaped_text),
            _ => None,
        }
    }

    /// tsc-port: getResolvedSymbol @6.0.3
    /// tsc-hash: a2e483d12e4f94f17a890574405568a03060cad9c38b5df18836ef794ae69532
    /// tsc-span: _tsc.js:69389-69403
    ///
    /// isUse = !isWriteOnlyAccess(node) (accessKind port, expr.rs —
    /// live from 5.5a). Failure caches unknownSymbol (returned as None
    /// here) after the resolveName error path has fired, exactly once
    /// per node.
    pub(crate) fn get_resolved_symbol(&mut self, node: NodeId) -> Option<SymbolId> {
        if let Some(cached) = self.links.node(node).resolved_symbol.resolved() {
            return (cached != self.unknown_symbol).then_some(cached);
        }
        let resolved = if node_util::node_is_missing(self.binder.source_of_node(node), Some(node))
        {
            None
        } else {
            let name = self.identifier_text_of(node).unwrap_or_default().to_owned();
            let message = self.cannot_find_name_diagnostic_for_name(node);
            let is_use = !self.is_write_only_access(node);
            self.resolve_name(
                Some(node),
                &name,
                SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                Some(message),
                is_use,
                /*exclude_globals*/ false,
            )
        };
        let cached = resolved.unwrap_or(self.unknown_symbol);
        self.links.set_node_resolved_symbol(self.speculation_depth, node, cached);
        resolved
    }

    fn is_static_node(&self, node: NodeId) -> bool {
        has_syntactic_modifier(
            self.binder.source_of_node(node),
            node,
            ModifierFlags::STATIC,
        )
    }

    /// tsc findConstructorDeclaration: the constructor WITH a body.
    pub(crate) fn find_constructor_declaration(&self, class: NodeId) -> Option<NodeId> {
        let members = match self.data_of(class) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => return None,
        };
        self.nodes_of_array(members).into_iter().find(|&member| {
            self.kind_of(member) == SyntaxKind::Constructor
                && body_of(self.binder.source_of_node(member), member).is_some()
        })
    }

    pub(crate) fn heritage_clause_is_extends(&self, clause: NodeId) -> bool {
        if self.kind_of(clause) != SyntaxKind::HeritageClause {
            return false;
        }
        // The clause keyword is recoverable from the source range
        // (HeritageClauseData stores no token — parser note at
        // parse_heritage_clause).
        let source = self.binder.source_of_node(clause);
        let node = source.arena.node(clause);
        let start = tsrs2_syntax::skip_trivia(&source.text, node.pos as usize);
        source.text[start..].starts_with("extends")
    }

    fn is_class_element_kind(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::Constructor
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::IndexSignature
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::SemicolonClassElement
        )
    }

    fn export_declaration_of_specifier_has_module_specifier(&self, specifier: NodeId) -> bool {
        let Some(declaration) = self
            .parent_of(specifier)
            .and_then(|named| self.parent_of(named))
        else {
            return false;
        };
        matches!(
            self.data_of(declaration),
            NodeData::ExportDeclaration(data) if data.module_specifier.is_some()
        )
    }

    pub(crate) fn type_annotation_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::PropertyDeclaration(data) => data.r#type,
            NodeData::PropertySignature(data) => data.r#type,
            NodeData::Parameter(data) => data.r#type,
            NodeData::VariableDeclaration(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            NodeData::TypeAssertionExpression(data) => data.r#type,
            _ => None,
        }
    }

    fn parameters_of(&self, node: NodeId) -> Vec<NodeId> {
        let parameters = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        self.nodes_of_array(parameters)
    }

    fn nodes_of_array(&self, array: Option<tsrs2_syntax::NodeArrayId>) -> Vec<NodeId> {
        match array {
            Some(array) => self.binder.node_array(array).nodes.clone(),
            None => Vec::new(),
        }
    }

    fn child_nodes_of(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
    }

    pub(crate) fn is_type_node_kind(&self, kind: SyntaxKind) -> bool {
        (kind >= SyntaxKind::TypePredicate && kind <= SyntaxKind::ImportType)
            || matches!(
                kind,
                SyntaxKind::AnyKeyword
                    | SyntaxKind::UnknownKeyword
                    | SyntaxKind::NumberKeyword
                    | SyntaxKind::BigIntKeyword
                    | SyntaxKind::ObjectKeyword
                    | SyntaxKind::BooleanKeyword
                    | SyntaxKind::StringKeyword
                    | SyntaxKind::SymbolKeyword
                    | SyntaxKind::VoidKeyword
                    | SyntaxKind::UndefinedKeyword
                    | SyntaxKind::NeverKeyword
                    | SyntaxKind::IntrinsicKeyword
                    | SyntaxKind::ExpressionWithTypeArguments
            )
    }
}

impl<'a> CheckerState<'a> {
    /// tsc findAncestor(node, predicate) for a single kind.
    pub(crate) fn find_ancestor_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(node) = current {
            if self.kind_of(node) == kind {
                return Some(node);
            }
            current = self.parent_of(node);
        }
        None
    }

    /// tsc getDeclarationOfKind.
    pub(crate) fn declaration_of_kind(&self, symbol: SymbolId, kind: SyntaxKind) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == kind)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeFlags};

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// First Identifier node whose text is `text`, in allocation order.
    fn identifier_named(state: &CheckerState, text: &str) -> NodeId {
        let source = state.binder.source(0);
        source
            .arena
            .node_ids()
            .find(|&id| {
                matches!(
                    &source.arena.node(id).data,
                    NodeData::Identifier(data) if data.escaped_text == text
                )
            })
            .expect("identifier present")
    }

    fn annotation_of_var(state: &CheckerState, name: &str) -> NodeId {
        crate::relpin::find_probe_annotation(state.binder.source(0), name)
            .expect("declared var with annotation")
    }

    #[test]
    fn qualified_name_resolves_through_namespace_exports() {
        with_program_state(
            &[(
                "a.ts",
                "namespace N { export interface I { a: number } }\ndeclare var v: N.I;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("qualified interface reference resolves");
                assert!(state
                    .tables
                    .flags_of(resolved)
                    .intersects(TypeFlags::OBJECT));
                let symbol = state
                    .tables
                    .type_of(resolved)
                    .symbol
                    .expect("interface symbol");
                assert_eq!(state.binder.symbol(symbol).escaped_name, "I");
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn inner_scope_shadows_outer() {
        with_program_state(
            &[(
                "a.ts",
                "interface I { a: number }\nfunction f() { interface I { b: string } var v: I; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // Resolve "I" from the annotation inside f: the inner
                // interface wins.
                let annotation = annotation_of_var(state, "v");
                let symbol = state
                    .resolve_name(Some(annotation), "I", SymbolFlags::TYPE, None, false, false)
                    .expect("inner interface resolves");
                let declaration = state.binder.symbol(symbol).declarations[0];
                let outer = state
                    .resolve_name(
                        Some(state.binder.source(0).root),
                        "I",
                        SymbolFlags::TYPE,
                        None,
                        false,
                        false,
                    )
                    .expect("outer interface resolves");
                assert_ne!(symbol, outer);
                // The inner declaration sits inside f's body.
                assert!(state
                    .find_ancestor_of_kind(declaration, SyntaxKind::FunctionDeclaration)
                    .is_some());
            },
        );
    }

    #[test]
    fn arguments_resolves_inside_functions_only() {
        with_program_state(
            &[(
                "a.ts",
                "function f() { var n: number; }\nvar outer: string;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let inner = identifier_named(state, "n");
                let resolved = state
                    .resolve_name(
                        Some(inner),
                        "arguments",
                        SymbolFlags::VARIABLE,
                        None,
                        false,
                        false,
                    )
                    .expect("arguments resolves inside f");
                assert_eq!(resolved, state.arguments_symbol);
                let outer = identifier_named(state, "outer");
                assert_eq!(
                    state.resolve_name(
                        Some(outer),
                        "arguments",
                        SymbolFlags::VARIABLE,
                        None,
                        false,
                        false,
                    ),
                    None
                );
            },
        );
    }

    #[test]
    fn class_type_parameter_resolves_in_members() {
        with_program_state(
            &[("a.ts", "class C<T> { m(v: T): void {} }\n")],
            &CompilerOptions::default(),
            |state| {
                // From the parameter annotation inside m, T resolves to
                // the class's type parameter through the class-members
                // arm.
                let v = identifier_named(state, "v");
                let symbol = state
                    .resolve_name(
                        Some(v),
                        "T",
                        SymbolFlags::TYPE_PARAMETER,
                        None,
                        false,
                        false,
                    )
                    .expect("class type parameter resolves");
                assert_eq!(
                    state.kind_of(state.binder.symbol(symbol).declarations[0]),
                    SyntaxKind::TypeParameter
                );
            },
        );
    }

    #[test]
    fn const_resolution_inside_const_assertion_returns_none() {
        with_program_state(
            &[("a.ts", "var v = 1 as const;\n")],
            &CompilerOptions::default(),
            |state| {
                let source = state.binder.source(0);
                let as_expression = source
                    .arena
                    .node_ids()
                    .find(|&id| source.arena.node(id).kind == SyntaxKind::AsExpression)
                    .expect("as expression");
                assert_eq!(
                    state.resolve_name(
                        Some(as_expression),
                        "const",
                        SymbolFlags::TYPE,
                        None,
                        false,
                        false
                    ),
                    None
                );
            },
        );
    }

    #[test]
    fn missing_name_with_message_emits_plain_2304() {
        with_program_state(
            &[("a.ts", "var v: number;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = identifier_named(state, "v");
                let message = state.cannot_find_name_diagnostic_for_name(v);
                let resolved = state.resolve_name(
                    Some(v),
                    "nope",
                    SymbolFlags::VALUE,
                    Some(message),
                    true,
                    false,
                );
                assert_eq!(resolved, None);
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2304]);
            },
        );
    }
}

/// tsc-port: getScriptTargetFeatures @6.0.3 (keys-only slice)
/// tsc-hash: 4caf0dbfd5f82ff6f32731df602469bcbc345272d4a232aef289c77293d3f659
/// tsc-span: _tsc.js:13062-13646
///
/// getSuggestedLibForNonExistentName consumes only each type entry's
/// FIRST lib key (firstIterator(typeFeatures.keys())); the per-lib
/// member lists feed getSuggestedLibForNonExistentProperty (2550-family,
/// 5.5) and stay unported until then. Pairs extracted mechanically from
/// the table in source order.
static SCRIPT_TARGET_FEATURE_FIRST_LIB: &[(&str, &str)] = &[
    ("Array", "es2015"),
    ("Iterator", "es2015"),
    ("AsyncIterator", "es2015"),
    ("ArrayBuffer", "es2024"),
    ("Atomics", "es2017"),
    ("SharedArrayBuffer", "es2017"),
    ("AsyncIterable", "es2018"),
    ("AsyncIterableIterator", "es2018"),
    ("AsyncGenerator", "es2018"),
    ("AsyncGeneratorFunction", "es2018"),
    ("RegExp", "es2015"),
    ("RegExpConstructor", "es2025"),
    ("Reflect", "es2015"),
    ("ArrayConstructor", "es2015"),
    ("ObjectConstructor", "es2015"),
    ("NumberConstructor", "es2015"),
    ("Math", "es2015"),
    ("Map", "es2015"),
    ("MapConstructor", "es2024"),
    ("Set", "es2015"),
    ("PromiseConstructor", "es2015"),
    ("Symbol", "es2015"),
    ("WeakMap", "es2015"),
    ("WeakSet", "es2015"),
    ("String", "es2015"),
    ("StringConstructor", "es2015"),
    ("DateTimeFormat", "es2017"),
    ("Promise", "es2015"),
    ("RegExpMatchArray", "es2018"),
    ("RegExpExecArray", "es2018"),
    ("Intl", "es2018"),
    ("NumberFormat", "es2018"),
    ("SymbolConstructor", "es2020"),
    ("DataView", "es2020"),
    ("BigInt", "es2020"),
    ("RelativeTimeFormat", "es2020"),
    ("Int8Array", "es2022"),
    ("Uint8Array", "es2022"),
    ("Uint8ClampedArray", "es2022"),
    ("Int16Array", "es2022"),
    ("Uint16Array", "es2022"),
    ("Int32Array", "es2022"),
    ("Uint32Array", "es2022"),
    ("Float16Array", "es2025"),
    ("Float32Array", "es2022"),
    ("Float64Array", "es2022"),
    ("BigInt64Array", "es2020"),
    ("BigUint64Array", "es2020"),
    ("Error", "es2022"),
    ("ErrorConstructor", "esnext"),
    ("Uint8ArrayConstructor", "esnext"),
    ("Date", "esnext"),
    ("DisposableStack", "esnext"),
    ("AsyncDisposableStack", "esnext"),
];

/// tsc-port: getSuggestedLibForNonExistentName @6.0.3
/// tsc-hash: b60265e4566246083d64e6d4fc258cac9ecc7c3e156ac3218c439b565375f46b
/// tsc-span: _tsc.js:75476-75481
pub(crate) fn get_suggested_lib_for_non_existent_name(name: &str) -> Option<&'static str> {
    SCRIPT_TARGET_FEATURE_FIRST_LIB
        .iter()
        .find(|(type_name, _)| *type_name == name)
        .map(|(_, lib)| *lib)
}

/// tsc-port: isPrimitiveTypeName @6.0.3
/// tsc-hash: bca4359c333883add2e1cf67e043cd84811da2258483f0e2ab470b6b7f9d6bda
/// tsc-span: _tsc.js:48334-48336
fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name,
        "any" | "string" | "number" | "boolean" | "never" | "unknown"
    )
}
