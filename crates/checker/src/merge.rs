//! Symbol merging + the initializeTypeChecker slice (M4 5.0).
//!
//! tsc merges every non-module file's locals into the checker-wide
//! `globals` table at init (88732); merge conflicts re-run the
//! declareSymbol-style duplicate reporting ACROSS files, with the
//! amalgamated cross-file grouping. Module augmentations and
//! jsGlobalAugmentations are 5.8 rows (ledger notes inline).

use tsc_binder::node_util::get_name_of_declaration;
use tsc_binder::{SymbolId, SymbolTable};
use tsc_diagnostics::{gen as diagnostics, RelatedInfo};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{EscapedName, JsStr, JsString, NodeFlags, SymbolFlags, TypeData, TypeFlags};

use crate::links::LinkSlot;
use crate::program::ProgramFileId;
use crate::state::{CheckResult, CheckerState};
use tsc_binder::NameKey;

/// tsc-port: escapeString @6.0.3 (doubleQuote flavor)
/// tsc-hash: a41f6d5932395df14118761cfc227d8ad3266e0e2f3133c4ec5857ff7e0b4d2d
/// tsc-span: _tsc.js:16311-16314
pub(crate) fn escape_double_quoted_symbol_name(text: JsStr<'_>) -> JsString {
    let mut out = JsString::new();
    let units = text.to_utf16();
    for (index, &unit) in units.iter().enumerate() {
        let replacement = match unit {
            0x5C => Some(r"\\"),
            0x22 => Some(r#"\""#),
            0 if units
                .get(index + 1)
                .is_some_and(|unit| (0x30..=0x39).contains(unit)) =>
            {
                Some(r"\x00")
            }
            0 => Some(r"\0"),
            9 => Some(r"\t"),
            11 => Some(r"\v"),
            12 => Some(r"\f"),
            8 => Some(r"\b"),
            13 => Some(r"\r"),
            10 => Some(r"\n"),
            0x2028 => Some(r"\u2028"),
            0x2029 => Some(r"\u2029"),
            0x85 => Some(r"\u0085"),
            _ => None,
        };
        if let Some(replacement) = replacement {
            out.push_str(replacement);
        } else if unit < 0x20 {
            out.push_str(&format!("\\u{unit:04X}"));
        } else {
            // escapeString's regex does not match unpaired surrogates.
            out.push_code_unit(unit);
        }
    }
    out
}

/// tsc-port: getExcludedSymbolFlags @6.0.3
/// tsc-hash: 44af025b45ba77e5268ef6b5eb0d490623d4607c7085362e5f1f0f0436f2da41
/// tsc-span: _tsc.js:47669-47688
pub fn get_excluded_symbol_flags(flags: SymbolFlags) -> SymbolFlags {
    let mut result = SymbolFlags::NONE;
    if flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
        result |= SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE) {
        result |= SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::PROPERTY) {
        result |= SymbolFlags::PROPERTY_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        result |= SymbolFlags::ENUM_MEMBER_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::FUNCTION) {
        result |= SymbolFlags::FUNCTION_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::CLASS) {
        result |= SymbolFlags::CLASS_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::INTERFACE) {
        result |= SymbolFlags::INTERFACE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::REGULAR_ENUM) {
        result |= SymbolFlags::REGULAR_ENUM_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::CONST_ENUM) {
        result |= SymbolFlags::CONST_ENUM_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::VALUE_MODULE) {
        result |= SymbolFlags::VALUE_MODULE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::METHOD) {
        result |= SymbolFlags::METHOD_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::GET_ACCESSOR) {
        result |= SymbolFlags::GET_ACCESSOR_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::SET_ACCESSOR) {
        result |= SymbolFlags::SET_ACCESSOR_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        result |= SymbolFlags::TYPE_PARAMETER_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        result |= SymbolFlags::TYPE_ALIAS_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::ALIAS) {
        result |= SymbolFlags::ALIAS_EXCLUDES;
    }
    result
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getMergedSymbol @6.0.3
    /// tsc-hash: d38909f3d76db468de6e0df4d57914bb34e5d0049669cfa184e092e62378ddf5
    /// tsc-span: _tsc.js:49932-49935
    pub fn get_merged_symbol(&self, symbol: SymbolId) -> SymbolId {
        self.merged_symbols.get(&symbol).copied().unwrap_or(symbol)
    }

    /// tsc-port: recordMergedSymbol @6.0.3
    /// tsc-hash: 3d180845d677f642074afc2950c818459a777cf6c61eb91f477e0a105248c98e
    /// tsc-span: _tsc.js:47689-47695
    /// tsc stamps source.mergeId and keys the per-checker
    /// mergedSymbols map with it; the map IS per-program state, so it
    /// lives on CheckerState — the source symbol (possibly a shared
    /// lib-binder symbol) is never mutated (lib-loading L3 invariant:
    /// file binders are read-only after bind).
    fn record_merged_symbol(&mut self, target: SymbolId, source: SymbolId) {
        self.merged_symbols.insert(source, target);
        self.merged_symbol_sources
            .entry(target)
            .or_default()
            .push(source);
    }

    /// tsc-port: cloneSymbol @6.0.3
    /// tsc-hash: 1a41af611deac3728405e13030e086a1fbdd56ccbeb7e8aea75c5489897c283c
    /// tsc-span: _tsc.js:47696-47706
    pub(crate) fn clone_symbol(&mut self, symbol: SymbolId) -> SymbolId {
        let original = self.binder.symbol(symbol);
        let flags = original.flags;
        let escaped_name = original.escaped_name;
        let declarations = original.declarations.clone();
        let parent = original.parent;
        let value_declaration = original.value_declaration;
        let const_enum_only_module = original.extras().const_enum_only_module;
        let members = original.members().clone();
        let exports = original.exports().clone();
        let result = self.binder.create_symbol(flags, escaped_name);
        let cloned = self.binder.symbol_mut(result);
        cloned.declarations = declarations;
        cloned.parent = parent;
        cloned.value_declaration = value_declaration;
        if const_enum_only_module == Some(true) {
            cloned.extras_mut().const_enum_only_module = Some(true);
        }
        *cloned.members_mut() = members;
        *cloned.exports_mut() = exports;
        self.record_merged_symbol(result, symbol);
        result
    }

    /// tsc-port: setValueDeclaration @6.0.3
    /// tsc-hash: a59d9538fb29e56c3a8225e23c78e2a2c0e3570f1bbc442be1dcc2ed93436dac
    /// tsc-span: _tsc.js:15190-15195
    fn set_value_declaration(&mut self, symbol: SymbolId, node: NodeId) {
        let Some(value_declaration) = self.binder.symbol(symbol).value_declaration else {
            self.binder.symbol_mut(symbol).value_declaration = Some(node);
            return;
        };
        let node_is_ambient_ts = self.binder.flags_of(node).intersects(NodeFlags::AMBIENT)
            && !self.is_in_js_file(node)
            && !self
                .binder
                .flags_of(value_declaration)
                .intersects(NodeFlags::AMBIENT);
        let source_of = |state: &Self, id: NodeId| state.binder.source_of_node(id);
        let prefer_new = (!node_is_ambient_ts
            && (tsc_binder::declare::is_assignment_declaration(
                source_of(self, value_declaration),
                value_declaration,
            ) && !tsc_binder::declare::is_assignment_declaration(source_of(self, node), node)))
            || (self.kind_of(value_declaration) != self.kind_of(node)
                && tsc_binder::declare::is_effective_module_declaration(
                    source_of(self, value_declaration),
                    value_declaration,
                ));
        if prefer_new {
            self.binder.symbol_mut(symbol).value_declaration = Some(node);
        }
    }

    /// tsc-port: isInJSFile @6.0.3
    /// tsc-hash: c5f0db66356c51537ce1e7c91692c0775f3db5764d39100f22ad85b89e0ec9a1
    /// tsc-span: _tsc.js:14886-14888
    pub(crate) fn is_in_js_file(&self, node: NodeId) -> bool {
        crate::is_js_file_name(&self.binder.source_of_node(node).file_name)
    }

    /// tsc resolveSymbol (49113) on mergeSymbol's immutable-target arm.
    ///
    /// Alias identity is not the merge meaning. For example an exported
    /// import-equals alias that resolves to a type alias must conflict with
    /// another type alias when two `declare global` namespaces are merged.
    /// Keep the Rust-only abort boundary here: mergeSymbol itself has no
    /// fallible counterpart in tsc, while alias resolution may cross one of
    /// this checker's typed oracle-crash boundaries.
    fn resolve_symbol_for_merge(&mut self, symbol: SymbolId) -> SymbolId {
        match self.resolve_symbol_ex(Some(symbol), /*dont_resolve_alias*/ false) {
            Ok(Some(resolved)) => resolved,
            Ok(None) => symbol,
            Err(abort) => {
                if let Some(&declaration) = self.binder.symbol(symbol).declarations.first() {
                    self.mark_oracle_crash_range(declaration, abort);
                }
                self.unknown_symbol
            }
        }
    }

    /// tsc-port: mergeSymbol @6.0.3
    /// tsc-hash: 1b2782c87ef6132c3927aeaa879b58dfaff7fcb1e192463b4d52ab115f868242
    /// tsc-span: _tsc.js:47707-47783
    pub fn merge_symbol(
        &mut self,
        mut target: SymbolId,
        source: SymbolId,
        unidirectional: bool,
    ) -> SymbolId {
        let source_flags = self.binder.symbol(source).flags;
        let target_flags = self.binder.symbol(target).flags;
        if !target_flags.intersects(get_excluded_symbol_flags(source_flags))
            || (source_flags | target_flags).intersects(SymbolFlags::ASSIGNMENT)
        {
            if source == target {
                return target;
            }
            if !target_flags.intersects(SymbolFlags::TRANSIENT) {
                let resolved_target = self.resolve_symbol_for_merge(target);
                if resolved_target == self.unknown_symbol {
                    return source;
                }
                let resolved_flags = self.binder.symbol(resolved_target).flags;
                if !resolved_flags.intersects(get_excluded_symbol_flags(source_flags))
                    || (source_flags | resolved_flags).intersects(SymbolFlags::ASSIGNMENT)
                {
                    target = self.clone_symbol(resolved_target);
                } else {
                    self.report_merge_symbol_error(target, source);
                    return source;
                }
            }
            if source_flags.intersects(SymbolFlags::VALUE_MODULE)
                && self
                    .binder
                    .symbol(target)
                    .flags
                    .intersects(SymbolFlags::VALUE_MODULE)
                && self.binder.symbol(target).extras().const_enum_only_module == Some(true)
                && self.binder.symbol(source).extras().const_enum_only_module != Some(true)
            {
                self.binder
                    .symbol_mut(target)
                    .extras_mut()
                    .const_enum_only_module = Some(false);
            }
            {
                let source_symbol = self.binder.symbol(source);
                let source_value_declaration = source_symbol.value_declaration;
                let source_declarations = source_symbol.declarations.clone();
                let target_symbol = self.binder.symbol_mut(target);
                target_symbol.flags |= source_flags;
                target_symbol.declarations.extend(source_declarations);
                if let Some(value_declaration) = source_value_declaration {
                    self.set_value_declaration(target, value_declaration);
                }
            }
            let source_members = self.binder.symbol(source).members().clone();
            if !source_members.is_empty() {
                let mut target_members =
                    std::mem::take(self.binder.symbol_mut(target).members_mut());
                self.merge_symbol_table(
                    std::sync::Arc::make_mut(&mut target_members),
                    &source_members,
                    unidirectional,
                    None,
                );
                *self.binder.symbol_mut(target).members_mut() = target_members;
            }
            let source_exports = self.binder.symbol(source).exports().clone();
            if !source_exports.is_empty() {
                let mut target_exports =
                    std::mem::take(self.binder.symbol_mut(target).exports_mut());
                self.merge_symbol_table(
                    std::sync::Arc::make_mut(&mut target_exports),
                    &source_exports,
                    unidirectional,
                    Some(target),
                );
                *self.binder.symbol_mut(target).exports_mut() = target_exports;
            }
            if !unidirectional {
                self.record_merged_symbol(target, source);
            }
            target
        } else if target_flags.intersects(SymbolFlags::NAMESPACE_MODULE) {
            if target != self.global_this_symbol {
                let error_node =
                    self.binder
                        .symbol(source)
                        .declarations
                        .first()
                        .and_then(|&declaration| {
                            get_name_of_declaration(
                                self.binder.source_of_node(declaration),
                                declaration,
                            )
                        });
                let name = self.symbol_display_name(target);
                self.error_at_js(
                    error_node,
                    &diagnostics::Cannot_augment_module_0_with_value_exports_because_it_resolves_to_a_non_module_entity,
                    &[name.as_js()],
                );
            }
            target
        } else {
            self.report_merge_symbol_error(target, source);
            target
        }
    }

    /// tsc symbolToString slice for the merge-error message args: the
    /// unescaped symbol name (the full display machinery is M8 tail).
    /// tsc-port: symbolToString @6.0.3
    /// tsc-hash: 483aaf1e4cc4280b31d8e18dab23b7c3bb1ed6966d92ff0e59a80ab3bdb157f5
    /// tsc-span: _tsc.js:50649-50682
    pub fn symbol_display_name(&self, symbol: SymbolId) -> JsString {
        tsc_binder::unescape_leading_underscores(self.binder.symbol(symbol).escaped_name).to_owned()
    }

    /// tsc-port: symbolName @6.0.3
    /// tsc-hash: 201131264fe4f6f45c2da6248df265ae411ebc24acc96f4b77c6b676a48ade0a
    /// tsc-span: _tsc.js:11452-11457
    ///
    /// A private class member renders its `#name` source text
    /// (isPrivateIdentifierClassElementDeclaration, 11944-11946); every other
    /// symbol unescapes its escaped name. Spelling-suggestion candidates and
    /// "Did you mean" values use this face, never the written face of
    /// getNameOfSymbolAsWritten.
    pub(crate) fn symbol_name(&self, symbol: SymbolId) -> JsString {
        if let Some(declaration) = self.binder.symbol(symbol).value_declaration {
            let source = self.binder.source_of_node(declaration);
            let is_class_element = matches!(
                source.arena.node(declaration).kind,
                SyntaxKind::PropertyDeclaration
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
            );
            if is_class_element {
                if let Some(name) = get_name_of_declaration(source, declaration) {
                    if let NodeData::PrivateIdentifier(data) = &source.arena.node(name).data {
                        return JsString::from(data.text());
                    }
                }
            }
        }
        self.symbol_display_name(symbol)
    }

    /// tsc-port: getNameOfSymbolAsWritten @6.0.3
    /// tsc-hash: 6202a5dabe4ef7e7d99294b9c5e97a88c6c3dc22be8f024346eb294dc50eae1c
    /// tsc-span: _tsc.js:55541-55575
    ///
    /// The default symbolToString face is declaration-backed, but an
    /// EARLY computed string/number property first renders its cooked
    /// nameType: identifier/numeric names are bare, other strings are
    /// double quoted, and negative numeric names are bracketed. Late
    /// computed names (`__@...`) deliberately keep the declaration's
    /// written `[expression]` face. Quoted and non-canonical numeric
    /// declaration names likewise retain their exact source spelling.
    pub(crate) fn symbol_name_as_written(&self, symbol: SymbolId) -> JsString {
        for &declaration in &self.binder.symbol(symbol).declarations {
            let source = self.binder.source_of_node(declaration);
            let Some(name_node) = get_name_of_declaration(source, declaration) else {
                continue;
            };
            let is_early_computed = matches!(
                source.arena.node(name_node).data,
                NodeData::ComputedPropertyName(_)
            ) && !self
                .links
                .read_symbol(symbol, |links| links.check_flags)
                .intersects(tsc_types::CheckFlags::LATE);
            if is_early_computed {
                if let Some(name_type) = self.links.symbol(symbol).name_type {
                    let flags = self.tables.flags_of(name_type);
                    if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL) {
                        let name = match &self.tables.type_of(name_type).data {
                            TypeData::Literal { value } => match value {
                                tsc_types::LiteralValue::String(text) => text.to_js_string(),
                                tsc_types::LiteralValue::Number(value) => {
                                    JsString::from(tsc_types::js_number_to_string(*value))
                                }
                                tsc_types::LiteralValue::BigInt(_) => {
                                    unreachable!(
                                        "string/number literal flags imply string/number value"
                                    )
                                }
                            },
                            _ => unreachable!("literal flags imply literal data"),
                        };
                        // A non-scalar value cannot be an identifier or numeric spelling.
                        let numeric = name
                            .as_str()
                            .is_some_and(crate::evaluate::is_numeric_literal_name);
                        if !name.as_str().is_some_and(tsc_syntax::is_identifier_text) && !numeric {
                            let mut quoted = JsString::from("\"");
                            quoted.push_js(escape_double_quoted_symbol_name(name.as_js()).as_js());
                            quoted.push('"');
                            return quoted;
                        }
                        if numeric && name.starts_with("-") {
                            let mut bracketed = JsString::from("[");
                            bracketed.push_js(name.as_js());
                            bracketed.push(']');
                            return bracketed;
                        }
                        return name;
                    }
                }
            }
            return tsc_binder::node_util::declaration_name_to_string(source, Some(name_node))
                .into();
        }
        // getNameOfSymbolAsWritten (_tsc.js:55586-55588): a symbol without a
        // named declaration (a mapped-type or otherwise synthesized property)
        // renders its nameType face before falling back to symbolName.
        if let Some(name) = self.symbol_name_from_name_type(symbol, false, false, None) {
            return name;
        }
        self.symbol_name(symbol)
    }

    /// tsc reportMergeSymbolError (inside mergeSymbol, 47755-47775) +
    /// addDuplicateLocations (47776-47782).
    ///
    /// isPlainJsFile is evaluated from the file directive plus the
    /// program checkJs option, matching the diagnostic collection
    /// layer. Checked JavaScript participates in duplicate reporting;
    /// only genuinely plain JavaScript omits its locations.
    fn report_merge_symbol_error(&mut self, target: SymbolId, source: SymbolId) {
        let target_flags = self.binder.symbol(target).flags;
        let source_flags = self.binder.symbol(source).flags;
        let is_either_enum = target_flags.intersects(SymbolFlags::ENUM)
            || source_flags.intersects(SymbolFlags::ENUM);
        let is_either_block_scoped = target_flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
            || source_flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE);
        let message = if is_either_enum {
            &diagnostics::Enum_declarations_can_only_merge_with_namespace_or_other_enum_declarations
        } else if is_either_block_scoped {
            &diagnostics::Cannot_redeclare_block_scoped_variable_0
        } else {
            &diagnostics::Duplicate_identifier_0
        };
        let is_plain_js_symbol = |state: &Self, symbol: SymbolId| {
            state
                .binder
                .symbol(symbol)
                .declarations
                .first()
                .is_some_and(|&declaration| {
                    let source = state.binder.source_of_node(declaration);
                    crate::is_plain_js_file(
                        is_js_file_name(&source.file_name),
                        crate::check_directive(source.text()),
                        state.options,
                    )
                })
        };
        let is_source_plain_js = is_plain_js_symbol(self, source);
        let is_target_plain_js = is_plain_js_symbol(self, target);
        // symbolToString(source) reaches getNameOfSymbolAsWritten for
        // declaration-backed symbols. In particular, a late-bound
        // computed class member that collides with the synthetic
        // `prototype` export keeps its written `[expr]` spelling.
        let symbol_name = self.symbol_name_as_written(source);
        // tsgo reportMergeSymbolError (checker.go:14437-14460) reports
        // cross-file conflicts immediately; it has no amalgamatedDuplicates
        // and no TS6200 summary.
        if !is_source_plain_js {
            self.add_duplicate_declaration_errors_for_symbols(
                source,
                message,
                &symbol_name,
                target,
            );
        }
        if !is_target_plain_js {
            self.add_duplicate_declaration_errors_for_symbols(
                target,
                message,
                &symbol_name,
                source,
            );
        }
    }

    /// tsc-port: addDuplicateDeclarationErrorsForSymbols @6.0.3
    /// tsc-hash: e718d927bbdd807670fe6c275346799c2546a4c3670890477379ca1685296aa3
    /// tsc-span: _tsc.js:47784-47788
    pub(crate) fn add_duplicate_declaration_errors_for_symbols(
        &mut self,
        target: SymbolId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        symbol_name: impl NameKey,
        source: SymbolId,
    ) {
        let symbol_name = symbol_name.name();
        let declarations = self.binder.symbol(target).declarations.clone();
        let related = self.binder.symbol(source).declarations.clone();
        for node in declarations {
            self.add_duplicate_declaration_error(node, message, symbol_name, &related);
        }
    }

    /// tsc-port: addDuplicateDeclarationError @6.0.3
    /// tsc-hash: fe4d031a3205590126c6e8e48b4d6d268ee133735d1411e1480f7935e0d20b52
    /// tsc-span: _tsc.js:47789-47809
    ///
    /// getExpandoInitializer arm (JS expando assignments) elided: those
    /// declarations only reach here from plain-JS files, which the
    /// plain-JS gate above already suppresses.
    fn add_duplicate_declaration_error<'n>(
        &mut self,
        node: NodeId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        symbol_name: impl Into<JsStr<'n>>,
        related_nodes: &[NodeId],
    ) {
        let symbol_name = symbol_name.into();
        let error_node =
            get_name_of_declaration(self.binder.source_of_node(node), node).unwrap_or(node);
        // A row in a file whose check is skipped (skipLibCheck on a
        // declaration file, skipDefaultLibCheck, noCheck, unchecked
        // JavaScript) is never published: tsgo builds it in every checker
        // and getSemanticDiagnosticsForFile drops the file. Not building it
        // keeps conflicting library copies out of every checker's memory
        // (zod's two @types/node versions: 2,142 rows, 4 MB per checker).
        let file = ProgramFileId::from_raw(
            u32::try_from(self.binder.file_index_of_node(error_node))
                .expect("Program file index overflow"),
        );
        if self.skip_type_checking_file(file) {
            return;
        }
        let index = self.lookup_or_issue_error_js(Some(error_node), message, &[symbol_name]);
        for &related_node in related_nodes {
            let adjusted =
                get_name_of_declaration(self.binder.source_of_node(related_node), related_node)
                    .unwrap_or(related_node);
            if adjusted == error_node {
                continue;
            }
            let leading = self.related_for_node_js(
                adjusted,
                &diagnostics::_0_was_also_declared_here,
                &[symbol_name],
            );
            let follow_on = self.related_for_node(adjusted, &diagnostics::and_here, &[]);
            let existing = &self.diagnostics[index].related;
            if existing.len() >= 5
                || existing
                    .iter()
                    .any(|r| related_equal(r, &follow_on) || related_equal(r, &leading))
            {
                continue;
            }
            let addition = if existing.is_empty() {
                leading
            } else {
                follow_on
            };
            self.diagnostics
                .update(index, |diagnostic| diagnostic.related.push(addition));
        }
    }

    /// tsrs-native: RelatedInfo adapter over the ledgered
    /// diagnostic_for_node path; tsc carries related information in
    /// structurally compatible object literals.
    pub(crate) fn related_for_node(
        &self,
        node: NodeId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[&str],
    ) -> RelatedInfo {
        let args = args.iter().map(|arg| JsStr::from(*arg)).collect::<Vec<_>>();
        self.related_for_node_js(node, message, &args)
    }

    /// tsrs-native: JS twin of related_for_node RelatedInfo adapter (sibling is tsrs-
    /// native)
    pub(crate) fn related_for_node_js(
        &self,
        node: NodeId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[JsStr<'_>],
    ) -> RelatedInfo {
        let diagnostic = self.diagnostic_for_node_js(node, message, args);
        RelatedInfo {
            file_name: diagnostic.file_name,
            start: diagnostic.start,
            length: diagnostic.length,
            message: diagnostic.message,
        }
    }

    /// tsc-port: mergeSymbolTable @6.0.3
    /// tsc-hash: 13b5cda9e1b64998a8d2d194060fb80dde2067f7a40aa2171b1f4065e67a71dc
    /// tsc-span: _tsc.js:47818-47829
    pub fn merge_symbol_table(
        &mut self,
        target: &mut SymbolTable,
        source: &SymbolTable,
        unidirectional: bool,
        merged_parent: Option<SymbolId>,
    ) {
        for (id, &source_symbol) in source {
            let target_symbol = target.get(id).copied();
            let merged = match target_symbol {
                Some(existing) => self.merge_symbol(existing, source_symbol, unidirectional),
                None => self.get_merged_symbol(source_symbol),
            };
            if let Some(parent) = merged_parent {
                if target_symbol.is_some()
                    && self
                        .binder
                        .symbol(merged)
                        .flags
                        .intersects(SymbolFlags::TRANSIENT)
                {
                    self.binder.symbol_mut(merged).parent = Some(parent);
                }
            }
            target.insert(*id, merged);
        }
    }

    /// M*erge into the checker-owned globals table without moving that table
    /// out of `self`. `mergeSymbol` may resolve an existing alias while it
    /// combines a colliding declaration; that resolution must observe every
    /// global published earlier in this same merge, just as it does through
    /// tsc's shared `Map` identity. Looking up and inserting one entry at a
    /// time keeps the borrow local while preserving that visibility.
    fn merge_into_globals(&mut self, source: &SymbolTable, unidirectional: bool) {
        for (id, &source_symbol) in source {
            let target_symbol = self.globals.get(id).copied();
            let merged = match target_symbol {
                Some(existing) => self.merge_symbol(existing, source_symbol, unidirectional),
                None => self.get_merged_symbol(source_symbol),
            };
            std::sync::Arc::make_mut(&mut self.globals).insert(*id, merged);
        }
    }

    /// `merge_into_globals` over borrowed ent*ries (a file's locals listed
    /// without cloning its table).
    fn merge_entries_into_globals(
        &mut self,
        source: &[(EscapedName, SymbolId)],
        unidirectional: bool,
    ) {
        for (id, source_symbol) in source {
            let source_symbol = *source_symbol;
            let target_symbol = self.globals.get(id).copied();
            let merged = match target_symbol {
                Some(existing) => self.merge_symbol(existing, source_symbol, unidirectional),
                None => self.get_merged_symbol(source_symbol),
            };
            std::sync::Arc::make_mut(&mut self.globals).insert(*id, merged);
        }
    }

    /// tsc-port: addUndefinedToGlobalsOrError*OnRedeclaration @6.0.3
    /// tsc-hash: 441bb0403861850ce1c4a8190e56d54ee70bfccfb47b247bd784ae08bc8af46c
    /// tsc-span: _tsc.js:47882-47894
    fn add_undefined_to_globals_or_error_on_redeclaration(&mut self) {
        let name = self.binder.symbol(self.undefined_symbol).escaped_name;
        match self.globals.get(name).copied() {
            Some(target_symbol) => {
                let declarations = self.binder.symbol(target_symbol).declarations.clone();
                for declaration in declarations {
                    if !is_type_declaration(self, declaration) {
                        let diagnostic = self.diagnostic_for_node_js(
                            declaration,
                            &diagnostics::Declaration_name_conflicts_with_built_in_global_identifier_0,
                            &[name.unescape()],
                        );
                        self.diagnostics.push(diagnostic);
                    }
                }
            }
            None => {
                std::sync::Arc::make_mut(&mut self.globals).insert(name, self.undefined_symbol);
            }
        }
    }

    /// tsc-port: initializeTypeChecker @6.0.3 (the M4 5.0 slice)
    /// tsc-hash: afc4ef8d42d94dcf56ac2a1db86715fecc06a4579e0e9718f662cb9919182276
    /// tsc-span: _tsc.js:88732-88906
    ///
    /// Ported here: per-file bind is done by the caller; the globals
    /// merge over non-module files (88738-88768), the globalThis
    /// redeclaration check (88743-88748), globalExports adoption
    /// (88760-88767), addUndefinedToGlobals (88777), the eager
    /// symbol-type seeds that need no globals lookup (88778, 88786,
    /// 88787), and the amalgamated-duplicates flush (88882-88905).
    /// Deliberately NOT here: module augmentations (88769-88776,
    /// 88874-88881 — module resolution, 5.8 rows) and the eager
    /// getGlobalType binding block (88779-88785, 88788-88873) — those
    /// globals stay LAZY accessors (globals.rs) per the M4 5.0 doc so
    /// they begin resolving when 5.1's declared types exist.
    pub(crate) fn initialize_program_globals(&mut self) {
        let file_count = self.binder.file_count();
        for index in 0..file_count {
            let source = self.binder.source(index);
            let is_external_module = source.external_module_indicator.is_some()
                || self.binder.file(index).common_js_module_indicator.is_some();
            if !is_external_module {
                // The binder table is immutable for the session; only its
                // entries are needed, so borrow them into a list instead of
                // cloning the whole map per file per checker.
                let locals = self.binder.locals_of(source.root).map(|locals| {
                    locals
                        .iter()
                        .map(|(name, &symbol)| (*name, symbol))
                        .collect::<Vec<_>>()
                });
                if let Some(locals) = locals {
                    if let Some(&(_, file_global_this)) =
                        locals.iter().find(|(name, _)| name.as_js() == "globalThis")
                    {
                        let declarations =
                            self.binder.symbol(file_global_this).declarations.clone();
                        for declaration in declarations {
                            let diagnostic = self.diagnostic_for_node(
                                declaration,
                                &diagnostics::Declaration_name_conflicts_with_built_in_global_identifier_0,
                                &["globalThis"],
                            );
                            self.diagnostics.push(diagnostic);
                        }
                    }
                    // tsgo initializeChecker (checker.go): "We defer merging
                    // of global ambient module declarations since they may
                    // require other global symbols and types to be
                    // resolved." Merging two declarations of an ambient
                    // module resolves an alias among their exports, which
                    // can read a global type that a `declare global`
                    // augmentation has yet to extend.
                    let (ambient_modules, others): (Vec<_>, Vec<_>) =
                        locals.into_iter().partition(|&(name, symbol)| {
                            self.binder
                                .symbol(symbol)
                                .flags
                                .intersects(SymbolFlags::MODULE)
                                && name.as_js().starts_with("\"")
                        });
                    self.merge_entries_into_globals(&others, false);
                    self.deferred_ambient_module_symbols.extend(ambient_modules);
                }
            }
            // file.patternAmbientModules concatenation (88754-88756).
            let pattern_modules = self.binder.file(index).pattern_ambient_modules.clone();
            self.pattern_ambient_modules.extend(pattern_modules);
            // file.symbol.globalExports (88760-88767): only names not
            // already in globals join.
            if let Some(file_symbol) = self.binder.node_symbol(source.root) {
                let global_exports = self
                    .binder
                    .symbol(file_symbol)
                    .extras()
                    .global_exports
                    .clone();
                for (id, &source_symbol) in global_exports.iter() {
                    if !self.globals.contains_key(id) {
                        std::sync::Arc::make_mut(&mut self.globals).insert(*id, source_symbol);
                    }
                }
            }
        }
        self.add_undefined_to_globals_or_error_on_redeclaration();
        // getSymbolLinks(undefinedSymbol).type = undefinedWideningType
        // (88778); unknownSymbol.type = errorType (88786);
        // globalThisSymbol.type = createObjectType(Anonymous,
        // globalThisSymbol) (88787) — members resolve at 5.3.
        // argumentsSymbol.type (88779-88785) is the LAZY IArguments
        // accessor in globals.rs.
        self.links.set_symbol_type(
            self.speculation_depth,
            self.undefined_symbol,
            LinkSlot::Resolved(self.tables.intrinsics.undefined_widening),
        );
        self.links.set_symbol_type(
            self.speculation_depth,
            self.unknown_symbol,
            LinkSlot::Resolved(self.tables.intrinsics.error),
        );
        let global_this_type = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(global_this_type).object_flags = tsc_types::ObjectFlags::ANONYMOUS;
        self.tables.type_mut(global_this_type).symbol = Some(self.global_this_symbol);
        self.links.set_symbol_type(
            self.speculation_depth,
            self.global_this_symbol,
            LinkSlot::Resolved(global_this_type),
        );
        // The amalgamated-duplicates flush happens AFTER the
        // augmentation passes (tsc 88882 follows 88874-88881) — see
        // merge_module_augmentations (A8).
    }

    /// tsgo initializeChecker: "Now merge global ambient module
    /// declarations" — the ambient module symbols of the script files,
    /// after the global-scope augmentations (and, in tsgo, the global
    /// types). A second call finds nothing left.
    pub(crate) fn merge_deferred_ambient_modules(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_ambient_module_symbols);
        self.merge_entries_into_globals(&deferred, false);
    }

    /// tsc-port: mergeModuleAugmentation @6.0.3
    /// tsc-hash: bc18945a0391c5bd0ccac3f41fd0c981d53173117b3a017c11a9d6eb8d6d9e61
    /// tsc-span: _tsc.js:47830-47881
    ///
    /// Both initializeTypeChecker augmentation passes (88769-88776
    /// global-scope first, 88874-88881 external-module second), driven
    /// from the program layer AFTER the resolver's host view exists —
    /// pass 2 resolves module names. The collector mirrors
    /// collectModuleReferences: top-level ambient declarations in a global
    /// script are traversed so their non-relative nested declarations join
    /// file.moduleAugmentations, while an external-module augmentation is a
    /// boundary and its body is not traversed. A per-augmentation CheckAbort
    /// is contained like the check_source_element boundary.
    pub fn merge_module_augmentations(&mut self) {
        self.merge_module_augmentations_around(|_| ());
    }

    /// `merge_module_augmentations` with `between` run where tsgo's
    /// initializeChecker (checker.go:1352-1382) constructs the global types:
    /// after the global-scope augmentations merge and before the deferred
    /// ambient module declarations do. The program driver constructs them
    /// there (materialize_init_global_diagnostics); the unit tests leave
    /// them lazy.
    pub fn merge_module_augmentations_around(&mut self, between: impl FnOnce(&mut Self)) {
        let file_count = self.binder.file_count();
        let mut global_augmentations: Vec<NodeId> = Vec::new();
        let mut module_augmentations: Vec<NodeId> = Vec::new();
        for index in 0..file_count {
            let source = self.binder.source(index);
            let root = source.root;
            let tsc_syntax::NodeData::SourceFile(data) = &source.arena.node(root).data else {
                continue;
            };
            let Some(statements) = data.statements else {
                continue;
            };
            self.collect_module_augmentations(
                source,
                source.arena.node_array(statements).nodes,
                /*in_ambient_module*/ false,
                &mut global_augmentations,
                &mut module_augmentations,
            );
        }
        // Pass 1: global-scope augmentations merge into globals
        // (mergeSymbolTable(globals, augmentation.symbol.exports)).
        for augmentation in global_augmentations {
            let Some(symbol) = self.binder.node_symbol(augmentation) else {
                continue;
            };
            // First-declaration guard (47833-47836): later merged
            // declarations ride the first one's merge.
            if self.binder.symbol(symbol).declarations.first().copied() != Some(augmentation) {
                continue;
            }
            let exports = self.binder.symbol(symbol).exports().clone();
            self.merge_into_globals(&exports, false);
        }
        // tsc and tsgo look the global types up after the global-scope
        // augmentations ("We won't have the correct global types until
        // global augmentations are merged"). A global with a single
        // declaration is replaced in the table by the clone its first
        // augmentation merges into, so a symbol found earlier is stale:
        // with `lib: ["es5"]` an augmented `Array<T>` had two declared
        // types and `string[]` did not see the augmentation.
        self.run_init_global_type_probes();
        // The global types are also CONSTRUCTED here, before the deferred
        // ambient module declarations merge (tsgo initializeChecker,
        // checker.go:1352-1382): getDeclaredTypeOfClassOrInterface of
        // `Function` resolves the base names of every `Function` declaration
        // (isThislessInterface), and a global augmentation that extends an
        // imported interface resolves its import then. An import of a name
        // that only an ambient module declaration provides is not found at
        // that point (the alias is cached unresolved, TS2307); DefinitelyTyped's
        // `ember/v2` depends on it.
        between(self);
        self.merge_deferred_ambient_modules();
        // tsgo (checker.go:1385): pattern modules merge before the
        // external-module augmentations resolve.
        if let Err(err) = self.merge_pattern_ambient_modules() {
            if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                eprintln!("contained mergePatternAmbientModules: {err}");
            }
        }
        // Pass 2: external-module augmentations resolve + merge.
        for augmentation in module_augmentations {
            if let Err(err) = self.merge_one_module_augmentation(augmentation) {
                self.mark_oracle_crash_range(augmentation, err);
                if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                    eprintln!("contained @{augmentation:?}: {err}");
                }
            }
        }
    }

    /// tsgo: mergePatternAmbientModules (checker.go:1408-1432). Pattern
    /// modules with the same pattern and identical import attributes types
    /// merge into one; globals follow the merged symbols.
    fn merge_pattern_ambient_modules(&mut self) -> CheckResult<()> {
        let modules = self.pattern_ambient_modules.clone();
        let mut grouped: Vec<(tsc_types::JsString, tsc_types::JsString, SymbolId)> =
            Vec::with_capacity(modules.len());
        let mut groups_by_pattern = rustc_hash::FxHashMap::<
            (tsc_types::JsString, tsc_types::JsString),
            Vec<usize>,
        >::default();
        for (prefix, suffix, symbol) in &modules {
            let attributes_type = self.get_type_of_module_import_attributes(*symbol)?;
            let key = (prefix.clone(), suffix.clone());
            let mut group_index = None;
            for &index in groups_by_pattern.get(&key).map_or(&[][..], Vec::as_slice) {
                let other = self.get_type_of_module_import_attributes(grouped[index].2)?;
                if self.is_type_identical_to(attributes_type, other)? {
                    group_index = Some(index);
                    break;
                }
            }
            match group_index {
                Some(index) => {
                    let target = grouped[index].2;
                    grouped[index].2 = self.merge_symbol(target, *symbol, false);
                }
                None => {
                    groups_by_pattern
                        .entry(key)
                        .or_default()
                        .push(grouped.len());
                    grouped.push((prefix.clone(), suffix.clone(), *symbol));
                }
            }
        }
        for (_, _, symbol) in &modules {
            let name = self.binder.symbol(*symbol).escaped_name;
            if self.globals.get(name).is_some() {
                let merged = self.get_merged_symbol(*symbol);
                std::sync::Arc::make_mut(&mut self.globals).insert(name, merged);
            }
        }
        self.pattern_ambient_modules = grouped;
        Ok(())
    }

    /// collectModuleReferences' ModuleDeclaration arm
    /// (_tsc.js:124139-124160), projected to the augmentation list consumed by
    /// initializeTypeChecker. This deliberately follows only module bodies;
    /// arbitrary namespace descendants are not part of that collector.
    fn collect_module_augmentations(
        &self,
        source: &tsc_syntax::SourceFile,
        statements: &[NodeId],
        in_ambient_module: bool,
        global_augmentations: &mut Vec<NodeId>,
        module_augmentations: &mut Vec<NodeId>,
    ) {
        let is_external_module_file = source.external_module_indicator.is_some();
        for &statement in statements {
            if self.kind_of(statement) != SyntaxKind::ModuleDeclaration
                || !tsc_binder::node_util::is_ambient_module(source, statement)
                || !(in_ambient_module
                    || tsc_binder::node_util::has_syntactic_modifier(
                        source,
                        statement,
                        tsc_types::ModifierFlags::AMBIENT,
                    )
                    || source.is_declaration_file)
            {
                continue;
            }
            let name = match self.data_of(statement) {
                NodeData::ModuleDeclaration(data) => data.name,
                _ => None,
            };
            let name_text = name
                .and_then(|name| {
                    tsc_binder::node_util::get_text_of_identifier_or_literal(source, name)
                })
                .unwrap_or_default();
            if is_external_module_file
                || (in_ambient_module && !Self::is_external_module_name_relative(&name_text))
            {
                if tsc_binder::node_util::is_global_scope_augmentation(source, statement) {
                    global_augmentations.push(statement);
                } else {
                    module_augmentations.push(statement);
                }
                continue;
            }
            if in_ambient_module {
                continue;
            }
            let body = match self.data_of(statement) {
                NodeData::ModuleDeclaration(data) => data.body,
                _ => None,
            };
            let Some(body) = body else {
                continue;
            };
            let Some(nested) = tsc_binder::node_util::statements_of(source, body) else {
                continue;
            };
            self.collect_module_augmentations(
                source,
                source.arena.node_array(nested).nodes,
                /*in_ambient_module*/ true,
                global_augmentations,
                module_augmentations,
            );
        }
    }

    fn merge_one_module_augmentation(&mut self, augmentation: NodeId) -> CheckResult<()> {
        let Some(augmentation_symbol) = self.binder.node_symbol(augmentation) else {
            return Ok(());
        };
        if self
            .binder
            .symbol(augmentation_symbol)
            .declarations
            .first()
            .copied()
            != Some(augmentation)
        {
            return Ok(());
        }
        let name = match self.data_of(augmentation) {
            tsc_syntax::NodeData::ModuleDeclaration(data) => data.name,
            _ => None,
        };
        let Some(name) = name else {
            return Ok(());
        };
        let name_text = match self.data_of(name) {
            tsc_syntax::NodeData::StringLiteral(data) => data.text.clone(),
            _ => return Ok(()),
        };
        // moduleName.parent.parent = the augmentation's container:
        // non-ambient containers report the not-found face.
        let container_ambient = self
            .binder
            .flags_of(augmentation)
            .intersects(tsc_types::NodeFlags::AMBIENT)
            && self.parent_of(augmentation).is_some_and(|parent| {
                self.binder
                    .flags_of(parent)
                    .intersects(tsc_types::NodeFlags::AMBIENT)
            });
        let module_not_found_error = if !container_ambient {
            Some(&diagnostics::Invalid_module_name_in_augmentation_module_0_cannot_be_found)
        } else {
            None
        };
        let main_module = self.resolve_external_module_name_worker(
            name,
            name,
            module_not_found_error,
            /*ignore_errors*/ false,
            /*is_for_augmentation*/ true,
        )?;
        let Some(main_module) = main_module else {
            return Ok(());
        };
        let main_module = self
            .resolve_external_module_symbol(Some(main_module), false)?
            .expect("resolveExternalModuleSymbol(Some) is Some");
        if self
            .binder
            .symbol(main_module)
            .flags
            .intersects(SymbolFlags::NAMESPACE)
        {
            let is_pattern = self
                .pattern_ambient_modules
                .iter()
                .any(|(_, _, symbol)| self.get_merged_symbol(*symbol) == main_module);
            if is_pattern {
                let merged = self.merge_symbol(
                    augmentation_symbol,
                    main_module,
                    /*unidirectional*/ true,
                );
                // tsgo (checker.go:1472-1473) also records the target, so
                // only resolutions to that pattern module take the
                // augmentation.
                self.pattern_ambient_module_augmentation_targets
                    .insert(name_text.clone(), main_module);
                self.pattern_ambient_module_augmentations
                    .insert(name_text, merged);
            } else {
                // Export-star pre-merge (47867-47874): augmentation
                // names that only exist through the main module's
                // export-star walk merge into the RESOLVED table.
                // m4-review B24 disposition: vendored 6.0.3 calls
                // mergeSymbol(resolved, value) with NO unidirectional
                // flag (default false) — upstream main passes
                // /*unidirectional*/ true here; re-audit on any tsc
                // upgrade, but 6.0.3 is the port authority and this
                // bidirectional call matches it.
                let has_export_star = self
                    .binder
                    .symbol(main_module)
                    .exports()
                    .contains_key(tsc_types::InternalSymbolName::EXPORT_STAR);
                let augmentation_exports =
                    self.binder.symbol(augmentation_symbol).exports().clone();
                if has_export_star && !augmentation_exports.is_empty() {
                    let resolved_exports = self.get_exports_of_module(main_module)?;
                    for (key, &value) in augmentation_exports.iter() {
                        if let Some(&resolved) = resolved_exports.get(key) {
                            if !self.binder.symbol(main_module).exports().contains_key(key) {
                                self.merge_symbol(resolved, value, false);
                            }
                        }
                    }
                }
                self.merge_symbol(main_module, augmentation_symbol, false);
            }
        } else {
            self.error_at_js(
                Some(name),
                &diagnostics::Cannot_augment_module_0_because_it_resolves_to_a_non_module_entity,
                &[name_text.as_js()],
            );
        }
        Ok(())
    }
}

fn is_js_file_name(name: &tsc_types::JsString) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// tsc-port: isTypeDeclaration @6.0.3
/// tsc-hash: b2274df074ed8639268970588736dbae37b3f9f0e10f20792b347297677273e1
/// tsc-span: _tsc.js:19262-19284
///
/// Import/export arms read the clause's type-only bit through the
/// parent chain, like tsc's phaseModifier checks.
fn is_type_declaration(state: &CheckerState, node: NodeId) -> bool {
    let source = state.binder.source_of_node(node);
    let arena = &source.arena;
    match arena.node(node).kind {
        SyntaxKind::TypeParameter
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::JSDocTypedefTag
        | SyntaxKind::JSDocCallbackTag => true,
        SyntaxKind::ImportClause => matches!(
            &arena.node(node).data,
            tsc_syntax::NodeData::ImportClause(data) if data.is_type_only
        ),
        SyntaxKind::ImportSpecifier => grandparent(arena, node).is_some_and(|clause| {
            matches!(
                &arena.node(clause).data,
                tsc_syntax::NodeData::ImportClause(data) if data.is_type_only
            )
        }),
        SyntaxKind::ExportSpecifier => grandparent(arena, node).is_some_and(|declaration| {
            matches!(
                &arena.node(declaration).data,
                tsc_syntax::NodeData::ExportDeclaration(data) if data.is_type_only
            )
        }),
        _ => false,
    }
}

fn grandparent(arena: &tsc_syntax::NodeArena, node: NodeId) -> Option<NodeId> {
    arena.node(arena.node(node).parent?).parent
}

/// tsc DiagnosticCollection equality for relatedInformation dedup
/// (compareDiagnostics === EqualTo on the related candidates).
fn related_equal(left: &RelatedInfo, right: &RelatedInfo) -> bool {
    left.file_name == right.file_name
        && left.start == right.start
        && left.length == right.length
        && left.message.code == right.message.code
        && left.message.text == right.message.text
}

#[cfg(test)]
#[path = "../tests/unit/merge/tests.rs"]
mod tests;
