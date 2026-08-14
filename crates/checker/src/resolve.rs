//! resolveName + resolveEntityName — lexical name resolution (M4 5.1).
//!
//! The scope walk is tsc's createNameResolver closure (19516) with the
//! checker's callbacks (46504). The failure path emits the PLAIN
//! nameNotFoundMessage form only — spelling suggestions
//! (getSuggestedSymbolForNonexistentSymbol) and the
//! checkAndReportErrorFor* alternates are M8 rows, ledgered at
//! on_failed_to_resolve_symbol.

use tsc_binder::node_util::{
    self, body_of, get_immediately_invoked_function_expression, has_syntactic_modifier,
    is_function_like_declaration_kind, is_function_like_kind, is_part_of_parameter_declaration,
};
use tsc_binder::{SymbolId, SymbolTable};
use tsc_diagnostics::{gen as diagnostics, DiagnosticCategory, DiagnosticMessage};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, ScriptTarget, SymbolFlags, TypeFlags};

use crate::state::{CheckResult, CheckerState};

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
    /// The Alias arm chases the alias TARGET's flags (getSymbolFlags,
    /// M4 5.8d): an alias whose own flags miss `meaning` matches when
    /// its resolved chain carries it.
    pub fn get_symbol_in_table(
        &mut self,
        table: &SymbolTable,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult<Option<SymbolId>> {
        if meaning.is_empty() {
            return Ok(None);
        }
        let Some(&symbol) = table.get(name) else {
            return Ok(None);
        };
        self.get_symbol_with_meaning(symbol, meaning)
    }

    /// tsrs-native: borrow-splitting worker for the pinned `getSymbol` port;
    /// it accepts an already retrieved symbol so Rust table borrows end before
    /// fallible alias resolution.
    ///
    /// The symbol half of `getSymbol`: merge first, then test the symbol's
    /// own flags and finally the resolved alias chain. Keeping this separate
    /// from the table borrow lets globals and lexical tables share the exact
    /// same meaning filter without cloning either table.
    pub(crate) fn get_symbol_with_meaning(
        &mut self,
        symbol: SymbolId,
        meaning: SymbolFlags,
    ) -> CheckResult<Option<SymbolId>> {
        if meaning.is_empty() {
            return Ok(None);
        }
        let symbol = self.get_merged_symbol(symbol);
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(meaning) {
            return Ok(Some(symbol));
        }
        if flags.intersects(SymbolFlags::ALIAS) {
            let target_flags = self.get_symbol_flags_of(symbol)?;
            if target_flags.intersects(meaning) {
                return Ok(Some(symbol));
            }
        }
        Ok(None)
    }

    /// The parameterized lookup's probe half (&self): exact match or,
    /// in suggestion mode, a table snapshot for the spelling pass.
    /// tsc getSuggestionForSymbolNameLookup (75522-75535) — the
    /// capitalized-primitive synthetics exist only at the GLOBALS
    /// level.
    fn lookup_probe(
        &mut self,
        table: &SymbolTable,
        name: &str,
        meaning: SymbolFlags,
        suggestion: bool,
        is_globals: bool,
    ) -> CheckResult<LookupProbe> {
        if let Some(found) = self.get_symbol_in_table(table, name, meaning)? {
            return Ok(LookupProbe::Found(found));
        }
        if !suggestion {
            return Ok(LookupProbe::Miss);
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
        Ok(LookupProbe::Suggest {
            values: table.values().copied().collect(),
            capitalized_primitives,
        })
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
                    tsc_binder::unescape_leading_underscores(name),
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
    ) -> CheckResult<Option<SymbolId>> {
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
    ) -> CheckResult<Option<SymbolId>> {
        self.resolve_name_full(
            location, name, meaning, /*name_not_found_message*/ None, /*is_use*/ false,
            /*exclude_globals*/ false, /*suggestion*/ true,
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
    ) -> CheckResult<Option<SymbolId>> {
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
                return Ok(None);
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
                    let locals = locals.clone();
                    let probe = self.lookup_probe(&locals, name, meaning, suggestion, false)?;
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
                                && last_location
                                    .is_none_or(|last| self.kind_of(last) != SyntaxKind::JSDoc)
                            {
                                use_result = if result_flags.intersects(SymbolFlags::TYPE_PARAMETER)
                                {
                                    last_location == self.type_annotation_of(loc)
                                        || last_location.is_some_and(|l| {
                                            matches!(
                                                self.kind_of(l),
                                                SyntaxKind::Parameter
                                                    | SyntaxKind::JSDocParameterTag
                                                    | SyntaxKind::JSDocReturnTag
                                                    | SyntaxKind::TypeParameter
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
                                module_exports.get(tsc_types::InternalSymbolName::DEFAULT)
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
                        if name != tsc_types::InternalSymbolName::DEFAULT {
                            let masked = meaning & SymbolFlags::MODULE_MEMBER;
                            let probe = self.lookup_probe(
                                &module_exports,
                                name,
                                masked,
                                suggestion,
                                false,
                            )?;
                            if let Some(found) = self.finish_lookup(probe, name, masked) {
                                let is_cjs = is_source_file
                                    && self
                                        .binder
                                        .file(self.binder.file_index_of_node(loc))
                                        .common_js_module_indicator
                                        .is_some();
                                let is_jsdoc_type_alias = self
                                    .binder
                                    .symbol(found)
                                    .declarations
                                    .iter()
                                    .copied()
                                    .any(|declaration| self.is_jsdoc_type_alias(declaration));
                                if !is_cjs || is_jsdoc_type_alias {
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
                    let probe = self.lookup_probe(&exports, name, masked, suggestion, false)?;
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
                                    let ctor_locals = ctor_locals.clone();
                                    let masked = meaning & SymbolFlags::VALUE;
                                    let probe = self.lookup_probe(
                                        &ctor_locals,
                                        name,
                                        masked,
                                        suggestion,
                                        false,
                                    )?;
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
                    let probe = self.lookup_probe(&members, name, masked, suggestion, false)?;
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
                                return Ok(None);
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
                                    self.lookup_probe(&members, name, masked, suggestion, false)?;
                                if self.finish_lookup(probe, name, masked).is_some() {
                                    if name_not_found_message.is_some() {
                                        self.error_at(
                                            original_location,
                                            &diagnostics::Base_class_expressions_cannot_reference_class_type_parameters,
                                            &[],
                                        );
                                    }
                                    return Ok(None);
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
                                self.lookup_probe(&members, name, masked, suggestion, false)?;
                            if self.finish_lookup(probe, name, masked).is_some() {
                                if name_not_found_message.is_some() {
                                    self.error_at(
                                        original_location,
                                        &diagnostics::A_computed_property_name_cannot_reference_a_type_parameter_from_its_containing_type,
                                        &[],
                                    );
                                }
                                return Ok(None);
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
                SyntaxKind::JSDocTypedefTag
                | SyntaxKind::JSDocCallbackTag
                | SyntaxKind::JSDocEnumTag
                | SyntaxKind::JSDocImportTag => {
                    if let Some(hop) = self
                        .get_jsdoc_root(loc)
                        .and_then(|document| self.parent_of(document))
                    {
                        location = self.advance_walk(
                            &mut last_location,
                            &mut last_self_reference_location,
                            hop,
                        );
                        continue 'walk;
                    }
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

        // tsc 19767-19769: `result.isReferenced |= meaning` for uses
        // outside the self-reference location — BEFORE the globals
        // fallback, so a globals-only hit is never marked.
        if is_use {
            if let Some(found) = result {
                let is_self_reference = last_self_reference_location
                    .is_some_and(|loc| self.binder.node_symbol(loc) == Some(found));
                if !is_self_reference {
                    self.links
                        .add_symbol_reference_meaning(self.speculation_depth, found, meaning);
                }
            }
        }

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
                            return Ok(Some(file_symbol));
                        }
                    }
                }
            }
            if !exclude_globals {
                let globals = self.globals.clone();
                let probe = self.lookup_probe(&globals, name, meaning, suggestion, true)?;
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
                    result,
                ) {
                    return Ok(None);
                }
            }
            match result {
                None => self.on_failed_to_resolve_symbol(original_location, name, meaning, message),
                Some(found) => {
                    self.on_successfully_resolved_symbol(
                        original_location,
                        found,
                        meaning,
                        associated_declaration_for_containing_initializer,
                        within_deferred_context,
                    )?;
                }
            }
        }
        Ok(result)
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
        match self.kind_of(loc) {
            SyntaxKind::JSDocTemplateTag => self
                .effective_container_for_jsdoc_template_tag(loc)
                .or_else(|| self.parent_of(loc)),
            SyntaxKind::JSDocParameterTag | SyntaxKind::JSDocReturnTag => self
                .get_host_signature_from_jsdoc(loc)
                .or_else(|| self.parent_of(loc)),
            _ => self.parent_of(loc),
        }
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
                    return !self.options.emit_standard_class_fields();
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
                if self.kind_of(declaration) != SyntaxKind::TypeParameter {
                    return false;
                }
                let Some(declaration_parent) = self.parent_of(declaration) else {
                    return false;
                };
                let parent = if self.kind_of(declaration_parent) == SyntaxKind::JSDocTemplateTag {
                    self.get_jsdoc_host(declaration_parent)
                } else {
                    Some(declaration_parent)
                };
                if parent != Some(container) {
                    return false;
                }
                !(self.kind_of(declaration_parent) == SyntaxKind::JSDocTemplateTag
                    && self.parent_of(declaration_parent).is_some_and(|document| {
                        matches!(
                            self.data_of(document),
                            NodeData::JSDoc(data)
                                if self.nodes_of(data.tags)
                                    .into_iter()
                                    .any(|tag| self.is_jsdoc_type_alias(tag))
                        )
                    }))
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
    /// suggestion-family row (M8). H2.4b admits an explicit
    /// `useDefineForClassFields=false` at ESNext, so this must use the exact
    /// computed emitStandardClassFields predicate rather than target alone.
    fn check_and_report_error_for_invalid_initializer(
        &mut self,
        error_location: Option<NodeId>,
        name: &str,
        property: NodeId,
        result: Option<SymbolId>,
    ) -> bool {
        if self.options.emit_standard_class_fields() {
            return false;
        }
        // 48098: an UNRESOLVED name first probes the missing-prefix
        // alternate (2662/2663) — only a miss falls to the 2301/2844
        // rows; a probe that unwinds suppresses both (FN-safe: this
        // helper has no Err channel).
        if result.is_none() {
            match self.check_and_report_error_for_missing_prefix(error_location, name) {
                Ok(true) | Err(_) => return true,
                Ok(false) => {}
            }
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
                tsc_binder::unescape_leading_underscores(name),
            ],
        );
        true
    }

    /// tsc-port: isUncheckedJSSuggestion @6.0.3
    /// tsc-hash: f77f37511d0a62ad1d4853cc1874d267075f8b5965fd3a52399e704cfed3f61b
    /// tsc-span: _tsc.js:75323-75338
    pub(crate) fn is_unchecked_js_suggestion(
        &self,
        node: Option<NodeId>,
        suggestion: Option<SymbolId>,
        exclude_classes: bool,
    ) -> bool {
        let Some(node) = node else {
            return false;
        };
        let source = self.binder.source_of_node(node);
        if !crate::is_plain_js_file(
            crate::is_js_file_name(&source.file_name),
            crate::check_directive(source.text()),
            self.options,
        ) {
            return false;
        }

        let Some(suggestion_id) = suggestion else {
            return true;
        };
        let suggestion = self.binder.symbol(suggestion_id);
        let declaration_file = suggestion
            .declarations
            .first()
            .map(|&declaration| self.binder.source_of_node(declaration));
        if declaration_file.is_some_and(|declaration_file| {
            source.root != declaration_file.root
                && !self
                    .binder
                    .is_external_or_common_js_module_of_node(declaration_file.root)
        }) {
            return false;
        }

        let suggestion_has_no_extends_or_decorators = match suggestion.value_declaration {
            None => true,
            Some(declaration) => match self.data_of(declaration) {
                NodeData::ClassDeclaration(data) => {
                    !self.nodes_of(data.heritage_clauses).is_empty()
                        || self.class_has_decorator(declaration)
                }
                NodeData::ClassExpression(data) => {
                    !self.nodes_of(data.heritage_clauses).is_empty()
                        || self.class_has_decorator(declaration)
                }
                _ => true,
            },
        };
        if exclude_classes
            && suggestion.flags.intersects(SymbolFlags::CLASS)
            && suggestion_has_no_extends_or_decorators
        {
            return false;
        }
        if exclude_classes
            && matches!(
                self.data_of(node),
                NodeData::PropertyAccessExpression(data)
                    if data.expression.is_some_and(|expression| {
                        self.kind_of(expression) == SyntaxKind::ThisKeyword
                    })
            )
            && suggestion_has_no_extends_or_decorators
        {
            return false;
        }
        true
    }

    fn class_has_decorator(&self, declaration: NodeId) -> bool {
        node_util::modifiers_of(self.binder.source_of_node(declaration), declaration).is_some_and(
            |modifiers| {
                self.nodes_of(Some(modifiers))
                    .into_iter()
                    .any(|modifier| self.kind_of(modifier) == SyntaxKind::Decorator)
            },
        )
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
        // in tsc's chain. A CheckAbort unwind inside its member
        // probes makes the alternate-vs-plain choice undecidable:
        // suppress the whole report (honest FN).
        match self.check_and_report_error_for_missing_prefix(error_location, name) {
            Ok(true) => return,
            Ok(false) => {}
            Err(_) => return,
        }
        // checkAndReportErrorForExtendingInterface is SECOND (48114
        // chain order): `class C extends I` over a type-only I reports
        // 2689 instead of the plain form. Same Err disposition as the
        // prefix probe.
        if let Some(error_location) = error_location {
            match self.check_and_report_error_for_extending_interface(error_location) {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
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
        if let Some(error_location) = error_location {
            match self.check_and_report_error_for_using_type_as_namespace(
                error_location,
                name,
                meaning,
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
            match self.check_and_report_error_for_using_namespace_as_type_or_value(
                error_location,
                name,
                meaning,
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
            match self.check_and_report_error_for_using_type_as_value(error_location, name, meaning)
            {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
        }
        // checkAndReportErrorForUsingValueAsType is the final alternate
        // in tsc's chain. A name that resolves only in the value-only
        // meaning reports 2749 instead of the plain missing-type
        // diagnostic. Namespace-bearing values remain valid type
        // qualification roots and are deliberately excluded.
        if let Some(error_location) = error_location {
            match self.check_and_report_error_for_using_value_as_type(error_location, name, meaning)
            {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
        }
        // The alternate ladder above now owns typed symbols that exist
        // under a different meaning. Falling through is tsc's ordinary
        // missing-name/namespace tail even when an unrelated-meaning
        // symbol exists (for example `import v = V` where V is a
        // value). JavaScript still has unmaterialized value/namespace
        // merges; preserve the old all-meanings shield there only.
        if error_location.is_some_and(|location| self.is_in_js_file(location)) {
            match self.resolve_name(error_location, name, SymbolFlags::ALL, None, false, false) {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => {}
            }
        }
        // The remaining JavaScript-only unresolved-name exemptions are
        // value constructs: prototype assignment roots and implicit
        // `require`. JSDoc aliases are ordinary binder symbols.
        if error_location
            .is_some_and(|location| self.is_js_prototype_assignment_declaration_root(location))
            || name == "require"
                && error_location.is_some_and(|location| self.is_in_js_file(location))
        {
            return;
        }
        // getSuggestedLibForNonExistentName is static lib metadata, so
        // the 2583/2584-family lib arm is exact. The SPELLING branch
        // (48123-48151) is budget-gated: suggestionCount < 10, where
        // the noLib bootstrap burns all 10 (run_init_global_type_probes)
        // — oracle-pinned via strictBindCallApply:false. Every failure
        // reaching this tail consumes one slot, suggestion or not.
        let display = error_location
            .filter(|&location| {
                self.kind_of(location) == SyntaxKind::Identifier
                    && self.identifier_text_of(location) == Some(name)
            })
            .map(|location| {
                node_util::declaration_name_to_string(
                    self.binder.source_of_node(location),
                    Some(location),
                )
            })
            .unwrap_or_else(|| tsc_binder::unescape_leading_underscores(name).to_owned());
        let suggested_lib = get_suggested_lib_for_non_existent_name(name);
        if let Some(lib) = suggested_lib {
            self.error_at(error_location, message, &[display.as_str(), lib]);
        } else {
            let mut suggestion: Option<SymbolId> = None;
            if self.suggestion_count < MAXIMUM_SUGGESTION_COUNT {
                // A CheckAbort unwind makes plain-vs-suggested
                // undecidable — skip the report (the failure-band
                // discipline above).
                suggestion =
                    match self.resolve_name_for_symbol_suggestion(error_location, name, meaning) {
                        Ok(suggestion) => suggestion,
                        Err(_) => return,
                    };
                // The isGlobalScopeAugmentationDeclaration filter
                // (48126-48129) — LIVE since 5.8d retired the
                // global-augmentation failure gate: a `declare global`
                // container symbol never suggests.
                if let Some(candidate) = suggestion {
                    let is_global_scope_augmentation_declaration =
                        self.binder.symbol(candidate).value_declaration.is_some_and(
                            |declaration| {
                                let source = self.binder.source_of_node(declaration);
                                node_util::is_ambient_module(source, declaration)
                                    && node_util::is_global_scope_augmentation(source, declaration)
                            },
                        );
                    if is_global_scope_augmentation_declaration {
                        suggestion = None;
                    }
                }
                if let Some(suggested) = suggestion {
                    let suggestion_name = self.symbol_display_name(suggested);
                    // Namespace meaning selects the 2833 flavor; plain
                    // unchecked JS uses the suggestion-category 2570
                    // flavor where checked JS/TS uses 2552.
                    let is_unchecked_js =
                        self.is_unchecked_js_suggestion(error_location, Some(suggested), false);
                    let did_you_mean = if meaning == SymbolFlags::NAMESPACE {
                        &diagnostics::Cannot_find_namespace_0_Did_you_mean_1
                    } else if is_unchecked_js {
                        &diagnostics::Could_not_find_name_0_Did_you_mean_1
                    } else {
                        &diagnostics::Cannot_find_name_0_Did_you_mean_1
                    };
                    let mut diagnostic = self.diagnostic_for_node_or_compiler(
                        error_location,
                        did_you_mean,
                        &[display.as_str(), &suggestion_name],
                    );
                    // getCanonicalDiagnostic(nameNotFoundMessage, name):
                    // sort/dedupe compare through the PLAIN form.
                    diagnostic.canonical_head = Some(tsc_diagnostics::CanonicalHead {
                        code: message.code,
                        text: tsc_diagnostics::MessageChain::new(
                            message,
                            std::slice::from_ref(&display),
                        )
                        .text,
                    });
                    if is_unchecked_js {
                        diagnostic.message.category = DiagnosticCategory::Suggestion;
                    }
                    // addErrorOrSuggestion clones an unchecked-JS
                    // suggestion into suggestionDiagnostics before
                    // tsc appends related information to the original
                    // diagnostic, so the published 2570 has no related
                    // row.
                    if !is_unchecked_js {
                        if let Some(value_declaration) =
                            self.binder.symbol(suggested).value_declaration
                        {
                            diagnostic.related.push(self.related_info_for_node(
                                value_declaration,
                                &diagnostics::_0_is_declared_here,
                                &[&suggestion_name],
                            ));
                        }
                    }
                    self.push_error_diagnostic(diagnostic);
                }
            }
            if suggestion.is_none() {
                self.error_at(error_location, message, &[display.as_str()]);
            }
        }
        self.suggestion_count += 1;
    }

    /// tsc-port: checkAndReportErrorForUsingTypeAsNamespace @6.0.3
    /// tsc-hash: 369552ff361606da3e4a270b246db042db3a5d20aac49b4c0302d3a227ac56d8
    /// tsc-span: _tsc.js:48282-48315
    fn check_and_report_error_for_using_type_as_namespace(
        &mut self,
        error_location: NodeId,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        let namespace_meaning = if self.is_in_js_file(error_location) {
            SymbolFlags::NAMESPACE | SymbolFlags::VALUE
        } else {
            SymbolFlags::NAMESPACE
        };
        if meaning != namespace_meaning {
            return Ok(false);
        }
        let type_only =
            SymbolFlags::from_bits(SymbolFlags::TYPE.bits() & !namespace_meaning.bits());
        let symbol =
            self.resolve_name(Some(error_location), name, type_only, None, false, false)?;
        let symbol = self.resolve_symbol_ex(symbol, false)?;
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        let display = tsc_binder::unescape_leading_underscores(name);
        let mut reported = false;
        if let Some(parent) = self.parent_of(error_location) {
            if let NodeData::QualifiedName(data) = self.data_of(parent) {
                if data.left == Some(error_location) {
                    if let Some(right) = data.right {
                        if let Some(property_name) =
                            self.identifier_text_of(right).map(str::to_owned)
                        {
                            let declared = self.get_declared_type_of_symbol_slice(symbol)?;
                            if self
                                .get_property_of_type_full(declared, &property_name)?
                                .is_some()
                            {
                                self.error_at(
                                    Some(parent),
                                    &diagnostics::Cannot_access_0_1_because_0_is_a_type_but_not_a_namespace_Did_you_mean_to_retrieve_the_type_of_the_property_1_in_0_with_0_1,
                                    &[display, &property_name],
                                );
                                reported = true;
                            }
                        }
                    }
                }
            }
        }
        if !reported {
            self.error_at(
                Some(error_location),
                &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_namespace_here,
                &[display],
            );
        }
        Ok(true)
    }

    /// tsc-port: checkAndReportErrorForUsingNamespaceAsTypeOrValue @6.0.3
    /// tsc-hash: 8e92afc3394351510ffdd3494015a3467fc26145c2f6015bc451387fc58f7a12
    /// tsc-span: _tsc.js:48412-48447
    fn check_and_report_error_for_using_namespace_as_type_or_value(
        &mut self,
        error_location: NodeId,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        let value_only =
            SymbolFlags::from_bits(SymbolFlags::VALUE.bits() & !SymbolFlags::TYPE.bits());
        let type_only =
            SymbolFlags::from_bits(SymbolFlags::TYPE.bits() & !SymbolFlags::VALUE.bits());
        let (lookup_meaning, message) = if meaning.intersects(value_only) {
            (
                SymbolFlags::NAMESPACE_MODULE,
                &diagnostics::Cannot_use_namespace_0_as_a_value,
            )
        } else if meaning.intersects(type_only) {
            (
                SymbolFlags::MODULE,
                &diagnostics::Cannot_use_namespace_0_as_a_type,
            )
        } else {
            return Ok(false);
        };
        if self.is_js_property_assignment_declaration_root(error_location) {
            return Ok(false);
        }
        let symbol = self.resolve_name(
            Some(error_location),
            name,
            lookup_meaning,
            None,
            false,
            false,
        )?;
        if self.resolve_symbol_ex(symbol, false)?.is_none() {
            return Ok(false);
        }
        let display = tsc_binder::unescape_leading_underscores(name);
        self.error_at(Some(error_location), message, &[display]);
        Ok(true)
    }

    /// A checked-JS property assignment can supply the root's value
    /// face even when the assignment-declaration binder has not
    /// materialized that face yet.
    fn is_js_property_assignment_declaration_root(&self, location: NodeId) -> bool {
        if !self.is_in_js_file(location) || self.kind_of(location) != SyntaxKind::Identifier {
            return false;
        }
        let mut current = location;
        while let Some(parent) = self.parent_of(current) {
            let NodeData::PropertyAccessExpression(data) = self.data_of(parent) else {
                break;
            };
            if data.expression != Some(current) {
                break;
            }
            current = parent;
        }
        current != location && self.get_assignment_target(current).is_some()
    }

    /// tsc-port: checkAndReportErrorForUsingTypeAsValue @6.0.3
    /// tsc-hash: 707908002b66e08db226aaa2a289d7a671dd2176022f0ec6ff5f22da16bd406a
    /// tsc-span: _tsc.js:48344-48386
    ///
    /// The primitive-name arm is implemented by
    /// report_primitive_type_name_used_as_value immediately before
    /// this symbol arm in the caller.
    fn check_and_report_error_for_using_type_as_value(
        &mut self,
        error_location: NodeId,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        if !meaning.intersects(SymbolFlags::VALUE) {
            return Ok(false);
        }
        let type_only =
            SymbolFlags::from_bits(SymbolFlags::TYPE.bits() & !SymbolFlags::VALUE.bits());
        let symbol =
            self.resolve_name(Some(error_location), name, type_only, None, false, false)?;
        let symbol = self.resolve_symbol_ex(symbol, false)?;
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        if self
            .get_symbol_flags_of(symbol)?
            .intersects(SymbolFlags::VALUE)
        {
            return Ok(false);
        }
        let display = tsc_binder::unescape_leading_underscores(name);
        if is_es2015_or_later_constructor_name(name) {
            self.error_at(
                Some(error_location),
                &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_es2015_or_later,
                &[display],
            );
        } else if self.maybe_mapped_type_for_value_error(error_location, symbol)? {
            let replacement = if display == "K" { "P" } else { "K" };
            self.error_at(
                Some(error_location),
                &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here_Did_you_mean_to_use_1_in_0,
                &[display, replacement],
            );
        } else {
            self.error_at(
                Some(error_location),
                &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here,
                &[display],
            );
        }
        Ok(true)
    }

    /// tsc-port: maybeMappedType @6.0.3
    /// tsc-hash: f9721e06b1e4eca553aa6d59a0dd9b6969398abd308f2d7efb4d3761c4d0a898
    /// tsc-span: _tsc.js:48387-48399
    fn maybe_mapped_type_for_value_error(
        &mut self,
        node: NodeId,
        symbol: SymbolId,
    ) -> CheckResult<bool> {
        let mut current = self.parent_of(node);
        let container = loop {
            let Some(candidate) = current else {
                break None;
            };
            match self.kind_of(candidate) {
                SyntaxKind::ComputedPropertyName | SyntaxKind::PropertySignature => {
                    current = self.parent_of(candidate);
                }
                SyntaxKind::TypeLiteral => break Some(candidate),
                _ => break None,
            }
        };
        let Some(container) = container else {
            return Ok(false);
        };
        let members = match self.data_of(container) {
            NodeData::TypeLiteral(data) => data.members,
            _ => None,
        };
        if self.nodes_of(members).len() != 1 {
            return Ok(false);
        }
        let ty = self.get_declared_type_of_symbol_slice(symbol)?;
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return Ok(false);
        }
        let constituents = match &self.tables.type_of(ty).data {
            tsc_types::TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies union data"),
        };
        for constituent in constituents {
            if !self.is_type_assignable_to_kind(
                constituent,
                TypeFlags::STRING_OR_NUMBER_LITERAL,
                true,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: checkAndReportErrorForUsingValueAsType @6.0.3
    /// tsc-hash: 0e523a9d04c65234f9615c68bb47989bffc9d7b5379f68d822bb396529d149e5
    /// tsc-span: _tsc.js:48316-48333
    fn check_and_report_error_for_using_value_as_type(
        &mut self,
        error_location: NodeId,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        let non_namespace_type =
            SymbolFlags::from_bits(SymbolFlags::TYPE.bits() & !SymbolFlags::NAMESPACE.bits());
        if !meaning.intersects(non_namespace_type) {
            return Ok(false);
        }
        let value_only =
            SymbolFlags::from_bits(SymbolFlags::VALUE.bits() & !SymbolFlags::TYPE.bits());
        let symbol =
            self.resolve_name(Some(error_location), name, value_only, None, false, false)?;
        let symbol = self.resolve_symbol_ex(symbol, false)?;
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::NAMESPACE)
        {
            return Ok(false);
        }
        let display = tsc_binder::unescape_leading_underscores(name);
        self.error_at(
            Some(error_location),
            &diagnostics::_0_refers_to_a_value_but_is_being_used_as_a_type_here_Did_you_mean_typeof_0,
            &[display],
        );
        Ok(true)
    }

    /// A JS `C.prototype... =` assignment can synthesize the root
    /// value declaration even when the binder has not materialized it.
    fn is_js_prototype_assignment_declaration_root(&self, location: NodeId) -> bool {
        if !self.is_in_js_file(location) || self.kind_of(location) != SyntaxKind::Identifier {
            return false;
        }
        let mut current = location;
        let mut saw_prototype = false;
        while let Some(parent) = self.parent_of(current) {
            let NodeData::PropertyAccessExpression(data) = self.data_of(parent) else {
                break;
            };
            if data.expression != Some(current) {
                break;
            }
            if data
                .name
                .is_some_and(|name| self.identifier_text_of(name) == Some("prototype"))
            {
                saw_prototype = true;
            }
            current = parent;
        }
        saw_prototype && self.get_assignment_target(current).is_some()
    }

    /// createError: node-anchored when a location exists, compiler-
    /// global otherwise (the locationless bootstrap flavors).
    fn diagnostic_for_node_or_compiler(
        &self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> tsc_diagnostics::Diagnostic {
        match location {
            Some(node) => self.diagnostic_for_node(node, message, args),
            None => {
                let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
                tsc_diagnostics::Diagnostic::new(
                    None,
                    None,
                    None,
                    tsc_diagnostics::MessageChain::new(message, &args),
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
    ) -> crate::state::CheckResult<bool> {
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
        let container = tsc_binder::node_util::get_this_container(
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
                            &[tsc_binder::unescape_leading_underscores(name), &class_name],
                        );
                        return Ok(true);
                    }
                    if current == container && !self.is_static_node(current) {
                        let declared =
                            self.get_declared_type_of_class_or_interface(class_symbol)?;
                        let this_type = match &self.tables.type_of(declared).data {
                            tsc_types::TypeData::GenericType { this_type, .. } => Some(*this_type),
                            _ => None,
                        };
                        if let Some(this_type) = this_type {
                            if self.get_property_of_type_full(this_type, name)?.is_some() {
                                self.error_at(
                                    Some(error_location),
                                    &diagnostics::Cannot_find_name_0_Did_you_mean_the_instance_member_this_0,
                                    &[tsc_binder::unescape_leading_underscores(name)],
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
                if container_kind == Some(SyntaxKind::InterfaceDeclaration) && heritage_is_extends {
                    self.error_at(
                        Some(error_location),
                        &diagnostics::An_interface_cannot_extend_a_primitive_type_like_0_It_can_only_extend_other_named_object_types,
                        &[tsc_binder::unescape_leading_underscores(name)],
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
                        &[tsc_binder::unescape_leading_underscores(name)],
                    );
                    return;
                }
                if container_is_class_like && !heritage_is_extends {
                    // ImplementsKeyword is the only other heritage
                    // token.
                    self.error_at(
                        Some(error_location),
                        &diagnostics::A_class_cannot_implement_a_primitive_type_like_0_It_can_only_implement_other_named_object_types,
                        &[tsc_binder::unescape_leading_underscores(name)],
                    );
                    return;
                }
            }
        }
        self.error_at(
            Some(error_location),
            &diagnostics::_0_only_refers_to_a_type_but_is_being_used_as_a_value_here,
            &[tsc_binder::unescape_leading_underscores(name)],
        );
    }

    /// tsc-port: isValidTypeOnlyAliasUseSite @6.0.3
    /// tsc-hash: 3b58ba6af9dbe593bc4519ffebbef0c9eeea8004014ecc966f9f7287a85f7471
    /// tsc-span: _tsc.js:18991-19024
    fn is_valid_type_only_alias_use_site(&self, use_site: NodeId) -> bool {
        if self.node_flags(use_site) & NodeFlags::AMBIENT.bits() != 0
            || self.node_flags(use_site) & NodeFlags::JS_DOC.bits() != 0
            || self.is_in_type_query(use_site)
        {
            return true;
        }

        if self.kind_of(use_site) == SyntaxKind::Identifier {
            let mut current = self.parent_of(use_site);
            while let Some(node) = current {
                match self.kind_of(node) {
                    SyntaxKind::HeritageClause => {
                        let implements = matches!(
                            self.data_of(node),
                            NodeData::HeritageClause(data)
                                if data.token == SyntaxKind::ImplementsKeyword
                        );
                        let interface_extends = self.parent_of(node).is_some_and(|parent| {
                            self.kind_of(parent) == SyntaxKind::InterfaceDeclaration
                        });
                        if implements || interface_extends {
                            return true;
                        }
                        break;
                    }
                    SyntaxKind::PropertyAccessExpression
                    | SyntaxKind::ExpressionWithTypeArguments => {
                        current = self.parent_of(node);
                    }
                    _ => break,
                }
            }
        }

        let mut computed_ancestor = use_site;
        while matches!(
            self.kind_of(computed_ancestor),
            SyntaxKind::Identifier | SyntaxKind::PropertyAccessExpression
        ) {
            let Some(parent) = self.parent_of(computed_ancestor) else {
                break;
            };
            computed_ancestor = parent;
        }
        if self.kind_of(computed_ancestor) == SyntaxKind::ComputedPropertyName {
            if let Some(member) = self.parent_of(computed_ancestor) {
                if has_syntactic_modifier(
                    self.binder.source_of_node(member),
                    member,
                    ModifierFlags::ABSTRACT,
                ) {
                    return true;
                }
                if self.parent_of(member).is_some_and(|container| {
                    matches!(
                        self.kind_of(container),
                        SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeLiteral
                    )
                }) {
                    return true;
                }
            }
        }

        let shorthand_property_name = self.kind_of(use_site) == SyntaxKind::Identifier
            && self.parent_of(use_site).is_some_and(|parent| {
                matches!(
                    self.data_of(parent),
                    NodeData::ShorthandPropertyAssignment(data) if data.name == Some(use_site)
                )
            });
        !node_util::is_expression_node(self.binder.source_of_node(use_site), use_site)
            && !shorthand_property_name
    }

    /// tsc-port: onSuccessfullyResolvedSymbolCallback @6.0.3
    /// tsc-hash: 6211274979d7706fc68ba0495d89289b5d23019693acc5ac2a0bdd82c7949dbb
    /// tsc-span: _tsc.js:48157-48204
    /// d2: d2:7727a05897150ebcee38a45f37636edcf5fcf3863bc5672843cca9ae0f4bc5c9
    ///
    /// Live arms: checkResolvedBlockScopedVariable (2448/2449/2450)
    /// parameter-initializer ordering checks (2372/2373), UMD-global
    /// access (2686), and type-only alias value uses (1361/1362).
    /// addLazyDiagnostic is eager by the checker-wide identity
    /// decision. The isolatedModules tail remains owned by its option
    /// prerequisite.
    fn on_successfully_resolved_symbol(
        &mut self,
        error_location: Option<NodeId>,
        result: SymbolId,
        meaning: SymbolFlags,
        associated_declaration: Option<NodeId>,
        within_deferred_context: bool,
    ) -> CheckResult<()> {
        let Some(error_location) = error_location else {
            return Ok(());
        };
        if meaning.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
            || (meaning.intersects(SymbolFlags::CLASS | SymbolFlags::ENUM)
                && meaning.contains(SymbolFlags::VALUE))
        {
            let export_or_local = self.get_export_symbol_of_value_symbol_if_exported(result);
            if self.binder.symbol(export_or_local).flags.intersects(
                SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS | SymbolFlags::ENUM,
            ) {
                // CheckAbort unwinds inside the declared-before-use
                // walk (property/parameter arms 5.5d) drop the TDZ
                // report for that reference — an honest FN; tsc has no
                // failure channel here.
                let _ = self.check_resolved_block_scoped_variable(export_or_local, error_location);
            }
        }
        if self
            .binder
            .is_external_or_common_js_module_of_node(error_location)
            && meaning.contains(SymbolFlags::VALUE)
        {
            let merged = self.get_merged_symbol(result);
            let declarations = self.binder.symbol(merged).declarations.clone();
            let name = self.binder.symbol(merged).escaped_name.clone();
            let is_umd_global = !declarations.is_empty()
                && declarations.iter().all(|&declaration| {
                    if self.kind_of(declaration) == SyntaxKind::NamespaceExportDeclaration {
                        return true;
                    }
                    if self.kind_of(declaration) != SyntaxKind::SourceFile {
                        return false;
                    }
                    self.binder
                        .node_symbol(declaration)
                        .is_some_and(|file_symbol| {
                            self.binder
                                .symbol(file_symbol)
                                .global_exports
                                .contains_key(&name)
                        })
                });
            if is_umd_global {
                let display = tsc_binder::unescape_leading_underscores(&name);
                let mut diagnostic = self.diagnostic_for_node(
                    error_location,
                    &diagnostics::_0_refers_to_a_UMD_global_but_the_current_file_is_a_module_Consider_adding_an_import_instead,
                    &[display],
                );
                if self.options.allow_umd_global_access == Some(true) {
                    diagnostic.message.category = DiagnosticCategory::Suggestion;
                }
                self.push_error_diagnostic(diagnostic);
            }
        }
        if let Some(associated_declaration) = associated_declaration
            .filter(|_| !within_deferred_context && meaning.contains(SymbolFlags::VALUE))
        {
            let candidate = self.get_late_bound_symbol(result)?;
            let candidate = self.get_merged_symbol(candidate);
            let associated_symbol = self.get_symbol_of_declaration(associated_declaration)?;
            let associated_name = self
                .name_of_node(associated_declaration)
                .map(|name| {
                    node_util::declaration_name_to_string(
                        self.binder.source_of_node(name),
                        Some(name),
                    )
                })
                .unwrap_or_default();
            if candidate == associated_symbol {
                self.error_at(
                    Some(error_location),
                    &diagnostics::Parameter_0_cannot_reference_itself,
                    &[&associated_name],
                );
            } else {
                let candidate_data = self.binder.symbol(candidate);
                let value_declaration = candidate_data.value_declaration;
                let candidate_name = candidate_data.escaped_name.clone();
                let root = node_util::get_root_declaration(
                    self.binder.source_of_node(associated_declaration),
                    associated_declaration,
                );
                let root_parent = self.parent_of(root);
                let root_local = root_parent
                    .and_then(|parent| self.binder.locals_of(parent))
                    .cloned()
                    .map(|locals| (locals, candidate_name));
                let declared_after = value_declaration.is_some_and(|declaration| {
                    self.pos_of(declaration) > self.pos_of(associated_declaration)
                });
                if declared_after {
                    if let Some((locals, candidate_name)) = root_local {
                        if self.get_symbol_in_table(&locals, &candidate_name, meaning)?
                            == Some(candidate)
                        {
                            let referenced_name = node_util::declaration_name_to_string(
                                self.binder.source_of_node(error_location),
                                Some(error_location),
                            );
                            self.error_at(
                                Some(error_location),
                                &diagnostics::Parameter_0_cannot_reference_identifier_1_declared_after_it,
                                &[&associated_name, &referenced_name],
                            );
                        }
                    }
                }
            }
        }
        let result_flags = self.binder.symbol(result).flags;
        if meaning.intersects(SymbolFlags::VALUE)
            && result_flags.intersects(SymbolFlags::ALIAS)
            && !result_flags.intersects(SymbolFlags::VALUE)
            && !self.is_valid_type_only_alias_use_site(error_location)
        {
            if let Some(type_only_declaration) =
                self.get_type_only_alias_declaration_ex(result, Some(SymbolFlags::VALUE))?
            {
                let exported = matches!(
                    self.kind_of(type_only_declaration),
                    SyntaxKind::ExportSpecifier
                        | SyntaxKind::ExportDeclaration
                        | SyntaxKind::NamespaceExport
                );
                let message = if exported {
                    &diagnostics::_0_cannot_be_used_as_a_value_because_it_was_exported_using_export_type
                } else {
                    &diagnostics::_0_cannot_be_used_as_a_value_because_it_was_imported_using_import_type
                };
                let related_message = if exported {
                    &diagnostics::_0_was_exported_here
                } else {
                    &diagnostics::_0_was_imported_here
                };
                let name = self.binder.symbol(result).escaped_name.clone();
                let display = tsc_binder::unescape_leading_underscores(&name);
                let related =
                    self.related_info_for_node(type_only_declaration, related_message, &[display]);
                self.error_at_with_related(
                    Some(error_location),
                    message,
                    &[display],
                    vec![related],
                );
            }
        }
        Ok(())
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
    ) -> crate::state::CheckResult<()> {
        let flags = self.binder.symbol(result).flags;
        debug_assert!(flags.intersects(
            SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS | SymbolFlags::ENUM
        ));
        if flags.intersects(
            SymbolFlags::FUNCTION | SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::ASSIGNMENT,
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
        if self.node_flags(declaration) & tsc_types::NodeFlags::AMBIENT.bits() != 0 {
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
                vec![tsc_diagnostics::RelatedInfo {
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

    /// tsrs-native: the dontResolveAlias=false default flavor of
    /// resolve_entity_name_ex (tsc's optional-parameter default).
    pub fn resolve_entity_name(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
        ignore_errors: bool,
        location: Option<NodeId>,
    ) -> CheckResult<Option<SymbolId>> {
        self.resolve_entity_name_ex(name, meaning, ignore_errors, location, false)
    }

    /// tsc-port: resolveEntityName @6.0.3
    /// tsc-hash: 0c5ce0e5980d5548db101cd9240b04944dea6e35cde2b0b3416210816fdb85b9
    /// tsc-span: _tsc.js:49292-49393
    ///
    /// Slices, each ledgered: the CJS-require namespace
    /// re-resolution (JS), and the type-not-namespace alternate in the
    /// missing-export path. The qualified-name typeof suggestion, the
    /// module-member spelling suggestion, the type-only alias marking,
    /// and the final resolveAlias hop are LIVE (M4 5.8d).
    pub fn resolve_entity_name_ex(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
        ignore_errors: bool,
        location: Option<NodeId>,
        dont_resolve_alias: bool,
    ) -> CheckResult<Option<SymbolId>> {
        if node_util::node_is_missing(self.binder.source_of_node(name), Some(name)) {
            return Ok(None);
        }
        let namespace_meaning = SymbolFlags::NAMESPACE
            | if self.is_in_js_file(name) {
                meaning & SymbolFlags::VALUE
            } else {
                SymbolFlags::NONE
            };
        let symbol = match self.kind_of(name) {
            SyntaxKind::Identifier => {
                let Some(text) = self.identifier_text_of(name).map(str::to_owned) else {
                    return Ok(None);
                };
                let synthesized = self.node_flags(name) & NodeFlags::SYNTHESIZED.bits() != 0;
                let message = if meaning == namespace_meaning || synthesized {
                    &diagnostics::Cannot_find_namespace_0
                } else {
                    self.cannot_find_name_diagnostic_for_name(name)
                };
                let symbol_from_js_prototype = if self.is_in_js_file(name) && !synthesized {
                    self.resolve_entity_name_from_assignment_declaration(name, meaning)?
                } else {
                    None
                };
                let symbol = self.resolve_name(
                    location.or(Some(name)),
                    &text,
                    meaning,
                    (!ignore_errors && symbol_from_js_prototype.is_none()).then_some(message),
                    true,
                    false,
                )?;
                let Some(symbol) = symbol.or(symbol_from_js_prototype) else {
                    return Ok(None);
                };
                self.get_merged_symbol(symbol)
            }
            SyntaxKind::QualifiedName | SyntaxKind::PropertyAccessExpression => {
                let (left, right) = match self.data_of(name) {
                    NodeData::QualifiedName(data) => (data.left, data.right),
                    NodeData::PropertyAccessExpression(data) => (data.expression, data.name),
                    _ => unreachable!("kind implies payload"),
                };
                let (Some(left), Some(right)) = (left, right) else {
                    return Ok(None);
                };
                let namespace = self.resolve_entity_name_ex(
                    left,
                    namespace_meaning,
                    ignore_errors,
                    location,
                    /*dont_resolve_alias*/ false,
                )?;
                let Some(namespace) = namespace else {
                    return Ok(None);
                };
                if node_util::node_is_missing(self.binder.source_of_node(right), Some(right)) {
                    return Ok(None);
                }
                if namespace == self.unknown_symbol {
                    return Ok(Some(namespace));
                }
                let Some(right_text) = self.identifier_text_of(right).map(str::to_owned) else {
                    return Ok(None);
                };
                // getExportsOfSymbol's globalThis special case (47710):
                // globalThisSymbol's exports ARE the merged globals
                // table (initializeTypeChecker 46492 aliases them).
                let exports = if namespace == self.global_this_symbol {
                    self.globals.clone()
                } else {
                    self.get_exports_of_symbol(namespace)?
                };
                let mut symbol = self
                    .get_symbol_in_table(&exports, &right_text, meaning)?
                    .map(|s| self.get_merged_symbol(s));
                if symbol.is_none()
                    && self
                        .binder
                        .symbol(namespace)
                        .flags
                        .intersects(SymbolFlags::ALIAS)
                {
                    let resolved_namespace = self.resolve_alias(namespace)?;
                    let alias_exports = if resolved_namespace == self.global_this_symbol {
                        self.globals.clone()
                    } else {
                        self.get_exports_of_symbol(resolved_namespace)?
                    };
                    symbol = self
                        .get_symbol_in_table(&alias_exports, &right_text, meaning)?
                        .map(|s| self.get_merged_symbol(s));
                }
                let Some(symbol) = symbol else {
                    if !ignore_errors {
                        let namespace_name = self.get_fully_qualified_name(namespace);
                        let declaration_name = node_util::declaration_name_to_string(
                            self.binder.source_of_node(right),
                            Some(right),
                        );
                        let suggestion =
                            self.get_suggested_symbol_for_nonexistent_module(right, namespace)?;
                        if let Some(suggestion) = suggestion {
                            let suggestion_name = self.symbol_display_name(suggestion);
                            self.error_at(
                                Some(right),
                                &diagnostics::_0_has_no_exported_member_named_1_Did_you_mean_2,
                                &[&namespace_name, &declaration_name, &suggestion_name],
                            );
                            return Ok(None);
                        }
                        // The qualified-name typeof alternate
                        // (49353-49364) is a value-property traversal,
                        // not a second entity-name/export lookup. That
                        // distinction is observable for chains such as
                        // `Color.Red.toString`: `toString` belongs to the
                        // enum member's value type rather than to the enum
                        // member symbol's exports.
                        let containing =
                            (self.kind_of(name) == SyntaxKind::QualifiedName).then(|| {
                                let mut containing = name;
                                while let Some(parent) = self.parent_of(containing) {
                                    let NodeData::QualifiedName(parent_data) = self.data_of(parent)
                                    else {
                                        break;
                                    };
                                    if parent_data.left != Some(containing) {
                                        break;
                                    }
                                    containing = parent;
                                }
                                containing
                            });
                        let in_type_query = containing.is_some_and(|containing| {
                            self.parent_of(containing)
                                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::TypeQuery)
                        });
                        let can_suggest_typeof = if meaning.intersects(SymbolFlags::TYPE)
                            && !in_type_query
                            && self.globals.get("Object").is_some()
                        {
                            match containing {
                                Some(containing) => {
                                    self.try_get_qualified_name_as_value(containing)?.is_some()
                                }
                                None => false,
                            }
                        } else {
                            false
                        };
                        if can_suggest_typeof {
                            let containing = containing.expect("guarded qualified name");
                            let display = self.entity_name_to_string(containing)?;
                            self.error_at(
                                Some(containing),
                                &diagnostics::_0_refers_to_a_value_but_is_being_used_as_a_type_here_Did_you_mean_typeof_0,
                                &[&display],
                            );
                            return Ok(None);
                        }
                        if meaning.intersects(SymbolFlags::NAMESPACE) {
                            if let Some(parent) = self.parent_of(name) {
                                if let NodeData::QualifiedName(parent_data) = self.data_of(parent) {
                                    if parent_data.left == Some(name) {
                                        let exported_type = self
                                            .get_symbol_in_table(
                                                &exports,
                                                &right_text,
                                                SymbolFlags::TYPE,
                                            )?
                                            .map(|symbol| self.get_merged_symbol(symbol));
                                        if let (Some(exported_type), Some(property)) =
                                            (exported_type, parent_data.right)
                                        {
                                            let symbol_name =
                                                self.symbol_display_name(exported_type);
                                            let property_name =
                                                node_util::declaration_name_to_string(
                                                    self.binder.source_of_node(property),
                                                    Some(property),
                                                );
                                            self.error_at(
                                                Some(property),
                                                &diagnostics::Cannot_access_0_1_because_0_is_a_type_but_not_a_namespace_Did_you_mean_to_retrieve_the_type_of_the_property_1_in_0_with_0_1,
                                                &[&symbol_name, &property_name],
                                            );
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                        self.error_at(
                            Some(right),
                            &diagnostics::Namespace_0_has_no_exported_member_1,
                            &[&namespace_name, &declaration_name],
                        );
                    }
                    return Ok(None);
                };
                symbol
            }
            _ => unreachable!("Unknown entity name kind."),
        };
        // The type-only alias marking on entity names (49380-49391;
        // nodeIsSynthesized is always false — no synthesis).
        if matches!(
            self.kind_of(name),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        ) && (self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
            || self
                .parent_of(name)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExportAssignment))
        {
            let alias_declaration = self.get_alias_declaration_from_name(name);
            self.mark_symbol_of_alias_declaration_if_type_only(
                alias_declaration,
                Some(symbol),
                /*final_target*/ None,
                /*overwrite_empty*/ true,
                None,
                None,
            )?;
        }
        // The resolveAlias tail (49392).
        if self.binder.symbol(symbol).flags.intersects(meaning) || dont_resolve_alias {
            Ok(Some(symbol))
        } else {
            Ok(Some(self.resolve_alias(symbol)?))
        }
    }

    /// tsc-port: tryGetQualifiedNameAsValue @6.0.3
    /// tsc-hash: 16e585859fc851283219148364337b294d658ebb9a6b38c828b8feb8d15daa4c
    /// tsc-span: _tsc.js:49268-49291
    ///
    /// Resolve the root under value meaning, then follow each qualified
    /// segment through the preceding symbol's value type. Entity-name
    /// resolution cannot substitute for this walk because ordinary value
    /// properties are not namespace exports.
    fn try_get_qualified_name_as_value(&mut self, node: NodeId) -> CheckResult<Option<SymbolId>> {
        let mut left = node;
        while let NodeData::QualifiedName(data) = self.data_of(left) {
            let Some(next) = data.left else {
                return Ok(None);
            };
            left = next;
        }
        let Some(root_name) = self.identifier_text_of(left).map(str::to_owned) else {
            return Ok(None);
        };
        let Some(mut symbol) = self.resolve_name(
            Some(left),
            &root_name,
            SymbolFlags::VALUE,
            None,
            true,
            false,
        )?
        else {
            return Ok(None);
        };

        while let Some(parent) = self.parent_of(left) {
            let NodeData::QualifiedName(data) = self.data_of(parent) else {
                break;
            };
            if data.left != Some(left) {
                break;
            }
            let Some(right) = data.right else {
                return Ok(None);
            };
            let Some(property_name) = self.identifier_text_of(right).map(str::to_owned) else {
                return Ok(None);
            };
            let ty = self.get_type_of_symbol(symbol)?;
            let Some(property) = self.get_property_of_type_full(ty, &property_name)? else {
                return Ok(None);
            };
            symbol = property;
            left = parent;
        }
        Ok(Some(symbol))
    }

    /// tsc-port: resolveEntityNameFromAssignmentDeclaration @6.0.3
    /// tsc-hash: 9027817e17cf0a40985354d59fa9d1536a8a5cb5b99c3acf96437e45ea60a4fc
    /// tsc-span: _tsc.js:49394-49409
    fn resolve_entity_name_from_assignment_declaration(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
    ) -> CheckResult<Option<SymbolId>> {
        let Some(type_reference) = self.parent_of(name) else {
            return Ok(None);
        };
        if !self.is_jsdoc_type_reference(type_reference) {
            return Ok(None);
        }
        let Some(secondary_location) = self.get_assignment_declaration_location(type_reference)
        else {
            return Ok(None);
        };
        let Some(text) = self.identifier_text_of(name).map(str::to_owned) else {
            return Ok(None);
        };
        self.resolve_name(
            Some(secondary_location),
            &text,
            meaning,
            None,
            /*is_use*/ true,
            /*exclude_globals*/ false,
        )
    }

    /// tsc-port: getAssignmentDeclarationLocation @6.0.3
    /// tsc-hash: 391cc3534ccd3032d777e02e77625e7c5ac73b1294ac52327636c568ca32e977
    /// tsc-span: _tsc.js:49410-49439
    fn get_assignment_declaration_location(&self, node: NodeId) -> Option<NodeId> {
        let mut ancestor = Some(node);
        while let Some(current) = ancestor {
            let kind = self.kind_of(current);
            let in_jsdoc = (SyntaxKind::FirstJSDocNode <= kind
                && kind <= SyntaxKind::LastJSDocNode)
                || self.node_flags(current) & NodeFlags::JS_DOC.bits() != 0;
            if !in_jsdoc {
                break;
            }
            if self.is_jsdoc_type_alias(current) {
                return None;
            }
            ancestor = self.parent_of(current);
        }

        let host = self.get_jsdoc_host(node);
        if let Some(host) = host {
            if let NodeData::ExpressionStatement(data) = self.data_of(host) {
                if let Some(expression) = data.expression {
                    if tsc_binder::get_assignment_declaration_kind(
                        self.binder.source_of_node(expression),
                        expression,
                    ) == tsc_binder::AssignmentDeclarationKind::PrototypeProperty
                    {
                        if let NodeData::BinaryExpression(data) = self.data_of(expression) {
                            if let Some(location) =
                                data.left.and_then(|left| self.node_symbol(left)).and_then(
                                    |symbol| self.declaration_of_js_prototype_container(symbol),
                                )
                            {
                                return Some(location);
                            }
                        }
                    }
                }
            }
            if self.kind_of(host) == SyntaxKind::FunctionExpression {
                if let Some(assignment) = self.parent_of(host) {
                    if tsc_binder::get_assignment_declaration_kind(
                        self.binder.source_of_node(assignment),
                        assignment,
                    ) == tsc_binder::AssignmentDeclarationKind::PrototypeProperty
                        && self.parent_of(assignment).is_some_and(|statement| {
                            self.kind_of(statement) == SyntaxKind::ExpressionStatement
                        })
                    {
                        if let NodeData::BinaryExpression(data) = self.data_of(assignment) {
                            if let Some(location) =
                                data.left.and_then(|left| self.node_symbol(left)).and_then(
                                    |symbol| self.declaration_of_js_prototype_container(symbol),
                                )
                            {
                                return Some(location);
                            }
                        }
                    }
                }
            }
            if matches!(
                self.kind_of(host),
                SyntaxKind::MethodDeclaration | SyntaxKind::PropertyAssignment
            ) {
                if let Some(assignment) = self
                    .parent_of(host)
                    .and_then(|parent| self.parent_of(parent))
                {
                    if self.kind_of(assignment) == SyntaxKind::BinaryExpression
                        && tsc_binder::get_assignment_declaration_kind(
                            self.binder.source_of_node(assignment),
                            assignment,
                        ) == tsc_binder::AssignmentDeclarationKind::Prototype
                    {
                        if let NodeData::BinaryExpression(data) = self.data_of(assignment) {
                            if let Some(location) =
                                data.left.and_then(|left| self.node_symbol(left)).and_then(
                                    |symbol| self.declaration_of_js_prototype_container(symbol),
                                )
                            {
                                return Some(location);
                            }
                        }
                    }
                }
            }
        }
        let signature = self.get_effective_jsdoc_host(node)?;
        if node_util::is_function_like_kind(self.kind_of(signature)) {
            let symbol = self.node_symbol(signature)?;
            return self.binder.symbol(symbol).value_declaration;
        }
        None
    }

    /// tsc-port: getDeclarationOfJSPrototypeContainer @6.0.3
    /// tsc-hash: fea290776e329848fc5b82dda21205c6927386b7b8c915ee8fced6fd1ae9282a
    /// tsc-span: _tsc.js:49440-49447
    fn declaration_of_js_prototype_container(&self, symbol: SymbolId) -> Option<NodeId> {
        let declaration = self
            .binder
            .symbol(symbol)
            .parent
            .and_then(|parent| self.binder.symbol(parent).value_declaration)?;
        let source = self.binder.source_of_node(declaration);
        let initializer = if tsc_binder::declare::is_assignment_declaration(source, declaration) {
            tsc_binder::assignment::get_assigned_expando_initializer(source, declaration)
        } else {
            self.initializer_of(declaration).and_then(|initializer| {
                let name = self.name_of_node(declaration);
                let is_prototype = name
                    .is_some_and(|name| tsc_binder::assignment::is_prototype_access(source, name));
                tsc_binder::assignment::get_expando_initializer(source, initializer, is_prototype)
            })
        };
        initializer.or(Some(declaration))
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

    /// tsrs-native: typed AST projection for tsc's direct
    /// `node.name` property access.
    pub(crate) fn name_of_node(&self, node: NodeId) -> Option<NodeId> {
        node_util::name_field_of(self.binder.source_of_node(node), node)
    }

    /// tsrs-native: typed Identifier/PrivateIdentifier projection for
    /// tsc idText/direct escapedText access.
    pub(crate) fn identifier_text_of(&self, node: NodeId) -> Option<&'a str> {
        match self.data_of(node) {
            NodeData::Identifier(data) => Some(&data.escaped_text),
            // tsc idText serves Identifier AND PrivateIdentifier — the
            // private text keeps its `#` (getSymbolNameForPrivateIdentifier
            // suffixes exactly this form).
            NodeData::PrivateIdentifier(data) => Some(&data.escaped_text),
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
    pub(crate) fn get_resolved_symbol(&mut self, node: NodeId) -> CheckResult<Option<SymbolId>> {
        if let Some(cached) = self.links.node(node).resolved_symbol.resolved() {
            return Ok((cached != self.unknown_symbol).then_some(cached));
        }
        let resolved = if node_util::node_is_missing(self.binder.source_of_node(node), Some(node)) {
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
            )?
        };
        let cached = resolved.unwrap_or(self.unknown_symbol);
        if self.speculation_depth == 0 {
            self.links
                .set_node_resolved_symbol(self.speculation_depth, node, cached);
        }
        Ok(resolved)
    }

    fn is_static_node(&self, node: NodeId) -> bool {
        has_syntactic_modifier(
            self.binder.source_of_node(node),
            node,
            ModifierFlags::STATIC,
        )
    }

    /// tsc findConstructorDeclaration: the constructor WITH a body.
    /// tsc-port: findConstructorDeclaration @6.0.3
    /// tsc-hash: 39fbf7ea056a909825faed464e1bb64ae7a1d51960d447e54ac1cc9b3780bc72
    /// tsc-span: _tsc.js:19508-19515
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

    /// tsrs-native: typed HeritageClause projection for tsc's direct
    /// token comparison.
    pub(crate) fn heritage_clause_is_extends(&self, clause: NodeId) -> bool {
        matches!(
            self.data_of(clause),
            NodeData::HeritageClause(data) if data.token == SyntaxKind::ExtendsKeyword
        )
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

    /// tsrs-native: typed AST projection for tsc's direct declaration
    /// `type` property access.
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

    fn nodes_of_array(&self, array: Option<tsc_syntax::NodeArrayId>) -> Vec<NodeId> {
        match array {
            Some(array) => self.binder.node_array(array).nodes.clone(),
            None => Vec::new(),
        }
    }

    fn child_nodes_of(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
    }

    /// tsc-port: isTypeNodeKind @6.0.3
    /// tsc-hash: 9848a49e4c3c1c37141e5b8133408d45e2b07b5a04ca4d2a6b33befcee7dc766
    /// tsc-span: _tsc.js:17579-17581
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
    /// tsrs-native: fixed-predicate Rust adapter around the syntax
    /// ancestor walk; tsc passes a JavaScript predicate closure.
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
    /// tsc-port: getDeclarationOfKind @6.0.3
    /// tsc-hash: d34933434824a0ff76b3eb034566feb42e6054c05caf812831c45ba8aed59e3c
    /// tsc-span: _tsc.js:12642-12652
    pub(crate) fn declaration_of_kind(&self, symbol: SymbolId, kind: SyntaxKind) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == kind)
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

/// tsc-port: isES2015OrLaterConstructorName @6.0.3
/// tsc-hash: a4d548ad3ba4c80e8c8d1b5e08e91b627c64f32f7fb4aa2ae8ea38f05c466ac5
/// tsc-span: _tsc.js:48400-48411
fn is_es2015_or_later_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "Promise" | "Symbol" | "Map" | "WeakMap" | "Set" | "WeakSet"
    )
}

#[cfg(test)]
#[path = "../tests/unit/resolve/tests.rs"]
mod tests;
