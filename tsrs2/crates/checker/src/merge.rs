//! Symbol merging + the initializeTypeChecker slice (M4 5.0).
//!
//! tsc merges every non-module file's locals into the checker-wide
//! `globals` table at init (88732); merge conflicts re-run the
//! declareSymbol-style duplicate reporting ACROSS files, with the
//! amalgamated cross-file grouping. Module augmentations and
//! jsGlobalAugmentations are 5.8 rows (ledger notes inline).

use indexmap::IndexMap;
use tsrs2_binder::node_util::get_name_of_declaration;
use tsrs2_binder::{SymbolId, SymbolTable};
use tsrs2_diags::{gen as diagnostics, RelatedInfo};
use tsrs2_syntax::{NodeId, SyntaxKind};
use tsrs2_types::{NodeFlags, SymbolFlags, TypeData, TypeFlags};

use crate::links::LinkSlot;
use crate::state::CheckerState;

/// tsc amalgamatedDuplicates value (47767): one entry per unordered
/// file pair, conflicting symbols keyed by display name in first-seen
/// order (tsc Map semantics — flush order is observable).
#[derive(Debug, Default)]
pub struct FilesDuplicates {
    pub conflicting_symbols: IndexMap<String, ConflictingSymbolInfo>,
}

#[derive(Debug, Default)]
pub struct ConflictingSymbolInfo {
    pub is_block_scoped: bool,
    pub first_file_locations: Vec<NodeId>,
    pub second_file_locations: Vec<NodeId>,
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
        self.binder.symbol(symbol).merged_into.unwrap_or(symbol)
    }

    /// tsc-port: recordMergedSymbol @6.0.3
    /// tsc-hash: 3d180845d677f642074afc2950c818459a777cf6c61eb91f477e0a105248c98e
    /// tsc-span: _tsc.js:47689-47695
    fn record_merged_symbol(&mut self, target: SymbolId, source: SymbolId) {
        self.binder.symbol_mut(source).merged_into = Some(target);
    }

    /// tsc-port: cloneSymbol @6.0.3
    /// tsc-hash: 1a41af611deac3728405e13030e086a1fbdd56ccbeb7e8aea75c5489897c283c
    /// tsc-span: _tsc.js:47696-47706
    fn clone_symbol(&mut self, symbol: SymbolId) -> SymbolId {
        let original = self.binder.symbol(symbol);
        let flags = original.flags;
        let escaped_name = original.escaped_name.clone();
        let declarations = original.declarations.clone();
        let parent = original.parent;
        let value_declaration = original.value_declaration;
        let const_enum_only_module = original.const_enum_only_module;
        let members = original.members.clone();
        let exports = original.exports.clone();
        let result = self.binder.create_symbol(flags, escaped_name);
        let cloned = self.binder.symbol_mut(result);
        cloned.declarations = declarations;
        cloned.parent = parent;
        cloned.value_declaration = value_declaration;
        if const_enum_only_module == Some(true) {
            cloned.const_enum_only_module = Some(true);
        }
        cloned.members = members;
        cloned.exports = exports;
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
            && (tsrs2_binder::declare::is_assignment_declaration(
                source_of(self, value_declaration),
                value_declaration,
            ) && !tsrs2_binder::declare::is_assignment_declaration(
                source_of(self, node),
                node,
            )))
            || (self.kind_of(value_declaration) != self.kind_of(node)
                && tsrs2_binder::declare::is_effective_module_declaration(
                    source_of(self, value_declaration),
                    value_declaration,
                ));
        if prefer_new {
            self.binder.symbol_mut(symbol).value_declaration = Some(node);
        }
    }

    fn is_in_js_file(&self, node: NodeId) -> bool {
        let name = &self.binder.source_of_node(node).file_name;
        [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|extension| name.ends_with(extension))
    }

    /// tsc resolveSymbol (49943) — the merge path's alias hop. Alias
    /// resolution is resolveAlias (M4 5.1); until it lands, an alias
    /// symbol resolves to itself. Constructible divergence: an
    /// `import x = require(...)` alias in a SCRIPT file colliding with
    /// another file's global — 5.1 replaces this identity hop.
    fn resolve_symbol_for_merge(&self, symbol: SymbolId) -> SymbolId {
        symbol
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
                && self.binder.symbol(target).const_enum_only_module == Some(true)
                && self.binder.symbol(source).const_enum_only_module != Some(true)
            {
                self.binder.symbol_mut(target).const_enum_only_module = Some(false);
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
            let source_members = self.binder.symbol(source).members.clone();
            if !source_members.is_empty() {
                let mut target_members =
                    std::mem::take(&mut self.binder.symbol_mut(target).members);
                self.merge_symbol_table(&mut target_members, &source_members, unidirectional, None);
                self.binder.symbol_mut(target).members = target_members;
            }
            let source_exports = self.binder.symbol(source).exports.clone();
            if !source_exports.is_empty() {
                let mut target_exports =
                    std::mem::take(&mut self.binder.symbol_mut(target).exports);
                self.merge_symbol_table(
                    &mut target_exports,
                    &source_exports,
                    unidirectional,
                    Some(target),
                );
                self.binder.symbol_mut(target).exports = target_exports;
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
                self.error_at(
                    error_node,
                    &diagnostics::Cannot_augment_module_0_with_value_exports_because_it_resolves_to_a_non_module_entity,
                    &[&name],
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
    pub fn symbol_display_name(&self, symbol: SymbolId) -> String {
        tsrs2_binder::unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name)
            .to_owned()
    }

    /// tsc reportMergeSymbolError (inside mergeSymbol, 47755-47775) +
    /// addDuplicateLocations (47776-47782).
    ///
    /// isPlainJsFile slice: any JS-extension file (checkJs is not yet a
    /// modeled option and checkJsDirective pragmas are not collected —
    /// both make files NON-plain, so this slice only ever SUPPRESSES
    /// more JS-side duplicate reports, matching the M2 plainJSErrors
    /// filtering posture).
    fn report_merge_symbol_error(&mut self, target: SymbolId, source: SymbolId) {
        // M2 3.4c residual guard: JS special-assignment binding
        // (bindPropertyAssignment's entity-name declarations) does not
        // yet stamp SymbolFlags::ASSIGNMENT, so a JS container merging
        // with a TS declaration (tsc's silent Assignment path at 47708)
        // is indistinguishable from a real duplicate here. Any JS-file
        // involvement suppresses the report entirely — JS gaps must
        // produce FNs, never FPs (tsc with checkJs off WOULD report the
        // TS side; that report returns when JS assignment binding
        // lands).
        let side_is_js = |state: &Self, symbol: SymbolId| {
            state
                .binder
                .symbol(symbol)
                .declarations
                .first()
                .is_some_and(|&declaration| {
                    is_js_file_name(&state.binder.source_of_node(declaration).file_name)
                })
        };
        if side_is_js(self, target) || side_is_js(self, source) {
            return;
        }
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
        let source_file = self
            .binder
            .symbol(source)
            .declarations
            .first()
            .map(|&declaration| self.binder.source_of_node(declaration).file_name.clone());
        let target_file = self
            .binder
            .symbol(target)
            .declarations
            .first()
            .map(|&declaration| self.binder.source_of_node(declaration).file_name.clone());
        let is_source_plain_js = source_file.as_deref().is_some_and(is_js_file_name);
        let is_target_plain_js = target_file.as_deref().is_some_and(is_js_file_name);
        let symbol_name = self.symbol_display_name(source);
        match (source_file, target_file) {
            (Some(source_file), Some(target_file))
                if !is_either_enum && source_file != target_file =>
            {
                // comparePaths slice: harness file names are flat
                // relative names, so plain string order decides the
                // (firstFile, secondFile) pair.
                let (first_file, second_file, source_is_first) = if source_file < target_file {
                    (source_file, target_file, true)
                } else {
                    (target_file, source_file, false)
                };
                let info = self
                    .amalgamated_duplicates
                    .entry((first_file, second_file))
                    .or_default()
                    .conflicting_symbols
                    .entry(symbol_name)
                    .or_insert_with(|| ConflictingSymbolInfo {
                        is_block_scoped: is_either_block_scoped,
                        ..Default::default()
                    });
                let source_declarations = self.binder.symbol(source).declarations.clone();
                let target_declarations = self.binder.symbol(target).declarations.clone();
                let (source_locations, target_locations) = if source_is_first {
                    (
                        &mut info.first_file_locations,
                        &mut info.second_file_locations,
                    )
                } else {
                    (
                        &mut info.second_file_locations,
                        &mut info.first_file_locations,
                    )
                };
                if !is_source_plain_js {
                    for declaration in source_declarations {
                        if !source_locations.contains(&declaration) {
                            source_locations.push(declaration);
                        }
                    }
                }
                if !is_target_plain_js {
                    for declaration in target_declarations {
                        if !target_locations.contains(&declaration) {
                            target_locations.push(declaration);
                        }
                    }
                }
            }
            _ => {
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
        }
    }

    /// tsc-port: addDuplicateDeclarationErrorsForSymbols @6.0.3
    /// tsc-hash: e718d927bbdd807670fe6c275346799c2546a4c3670890477379ca1685296aa3
    /// tsc-span: _tsc.js:47784-47788
    fn add_duplicate_declaration_errors_for_symbols(
        &mut self,
        target: SymbolId,
        message: &'static tsrs2_diags::DiagnosticMessage,
        symbol_name: &str,
        source: SymbolId,
    ) {
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
    fn add_duplicate_declaration_error(
        &mut self,
        node: NodeId,
        message: &'static tsrs2_diags::DiagnosticMessage,
        symbol_name: &str,
        related_nodes: &[NodeId],
    ) {
        let error_node =
            get_name_of_declaration(self.binder.source_of_node(node), node).unwrap_or(node);
        let index = self.lookup_or_issue_error(Some(error_node), message, &[symbol_name]);
        for &related_node in related_nodes {
            let adjusted =
                get_name_of_declaration(self.binder.source_of_node(related_node), related_node)
                    .unwrap_or(related_node);
            if adjusted == error_node {
                continue;
            }
            let leading = self.related_for_node(
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
            self.diagnostics[index].related.push(addition);
        }
    }

    fn related_for_node(
        &self,
        node: NodeId,
        message: &'static tsrs2_diags::DiagnosticMessage,
        args: &[&str],
    ) -> RelatedInfo {
        let diagnostic = self.diagnostic_for_node(node, message, args);
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
            target.insert(id.clone(), merged);
        }
    }

    /// tsc-port: addUndefinedToGlobalsOrErrorOnRedeclaration @6.0.3
    /// tsc-hash: 441bb0403861850ce1c4a8190e56d54ee70bfccfb47b247bd784ae08bc8af46c
    /// tsc-span: _tsc.js:47882-47894
    fn add_undefined_to_globals_or_error_on_redeclaration(&mut self) {
        let name = self
            .binder
            .symbol(self.undefined_symbol)
            .escaped_name
            .clone();
        match self.globals.get(&name).copied() {
            Some(target_symbol) => {
                let declarations = self.binder.symbol(target_symbol).declarations.clone();
                for declaration in declarations {
                    if !is_type_declaration(self, declaration) {
                        let diagnostic = self.diagnostic_for_node(
                            declaration,
                            &diagnostics::Declaration_name_conflicts_with_built_in_global_identifier_0,
                            &[tsrs2_binder::unescape_leading_underscores(&name)],
                        );
                        self.diagnostics.push(diagnostic);
                    }
                }
            }
            None => {
                self.globals.insert(name, self.undefined_symbol);
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
    /// 88874-88881 — module resolution, 5.8 rows),
    /// jsGlobalAugmentations (88751-88753, JS expandos), and the eager
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
                if let Some(locals) = self.binder.locals_of(source.root) {
                    if let Some(&file_global_this) = locals.get("globalThis") {
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
                    let locals = locals.clone();
                    let mut globals = std::mem::take(&mut self.globals);
                    self.merge_symbol_table(&mut globals, &locals, false, None);
                    self.globals = globals;
                }
            }
            // file.patternAmbientModules concatenation (88754-88756).
            let pattern_modules = self.binder.file(index).pattern_ambient_modules.clone();
            self.pattern_ambient_modules.extend(pattern_modules);
            // file.symbol.globalExports (88760-88767): only names not
            // already in globals join.
            if let Some(file_symbol) = self.binder.node_symbol(source.root) {
                let global_exports = self.binder.symbol(file_symbol).global_exports.clone();
                for (id, source_symbol) in global_exports {
                    if !self.globals.contains_key(&id) {
                        self.globals.insert(id, source_symbol);
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
        self.tables.type_mut(global_this_type).object_flags = tsrs2_types::ObjectFlags::ANONYMOUS;
        self.tables.type_mut(global_this_type).symbol = Some(self.global_this_symbol);
        self.links.set_symbol_type(
            self.speculation_depth,
            self.global_this_symbol,
            LinkSlot::Resolved(global_this_type),
        );
        self.flush_amalgamated_duplicates();
    }

    /// The amalgamatedDuplicates flush (88882-88905).
    fn flush_amalgamated_duplicates(&mut self) {
        let amalgamated = std::mem::take(&mut self.amalgamated_duplicates);
        for ((first_file, second_file), files_duplicates) in amalgamated {
            if files_duplicates.conflicting_symbols.len() < 8 {
                for (symbol_name, info) in files_duplicates.conflicting_symbols {
                    let message = if info.is_block_scoped {
                        &diagnostics::Cannot_redeclare_block_scoped_variable_0
                    } else {
                        &diagnostics::Duplicate_identifier_0
                    };
                    for &node in &info.first_file_locations {
                        self.add_duplicate_declaration_error(
                            node,
                            message,
                            &symbol_name,
                            &info.second_file_locations,
                        );
                    }
                    for &node in &info.second_file_locations {
                        self.add_duplicate_declaration_error(
                            node,
                            message,
                            &symbol_name,
                            &info.first_file_locations,
                        );
                    }
                }
            } else {
                let list = files_duplicates
                    .conflicting_symbols
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let first_root = self.root_of_file_named(&first_file);
                let second_root = self.root_of_file_named(&second_file);
                if let (Some(first_root), Some(second_root)) = (first_root, second_root) {
                    let mut first_diag = self.diagnostic_for_node(
                        first_root,
                        &diagnostics::Definitions_of_the_following_identifiers_conflict_with_those_in_another_file_0,
                        &[&list],
                    );
                    first_diag.related.push(self.related_for_node(
                        second_root,
                        &diagnostics::Conflicts_are_in_this_file,
                        &[],
                    ));
                    self.diagnostics.push(first_diag);
                    let mut second_diag = self.diagnostic_for_node(
                        second_root,
                        &diagnostics::Definitions_of_the_following_identifiers_conflict_with_those_in_another_file_0,
                        &[&list],
                    );
                    second_diag.related.push(self.related_for_node(
                        first_root,
                        &diagnostics::Conflicts_are_in_this_file,
                        &[],
                    ));
                    self.diagnostics.push(second_diag);
                }
            }
        }
    }

    fn root_of_file_named(&self, file_name: &str) -> Option<NodeId> {
        (0..self.binder.file_count())
            .map(|index| self.binder.source(index))
            .find(|source| source.file_name == file_name)
            .map(|source| source.root)
    }
}

fn is_js_file_name(name: &str) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// tsc-port: isTypeDeclaration @6.0.3
/// tsc-hash: b2274df074ed8639268970588736dbae37b3f9f0e10f20792b347297677273e1
/// tsc-span: _tsc.js:19262-19284
///
/// JSDoc tag arms elided (JSDoc declarations are not modeled — M2
/// residual); the import/export arms read the CLAUSE's type-only bit
/// through the parent chain, like tsc's phaseModifier checks.
fn is_type_declaration(state: &CheckerState, node: NodeId) -> bool {
    let source = state.binder.source_of_node(node);
    let arena = &source.arena;
    match arena.node(node).kind {
        SyntaxKind::TypeParameter
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::EnumDeclaration => true,
        SyntaxKind::ImportClause => matches!(
            &arena.node(node).data,
            tsrs2_syntax::NodeData::ImportClause(data) if data.is_type_only
        ),
        SyntaxKind::ImportSpecifier => grandparent(arena, node).is_some_and(|clause| {
            matches!(
                &arena.node(clause).data,
                tsrs2_syntax::NodeData::ImportClause(data) if data.is_type_only
            )
        }),
        SyntaxKind::ExportSpecifier => grandparent(arena, node).is_some_and(|declaration| {
            matches!(
                &arena.node(declaration).data,
                tsrs2_syntax::NodeData::ExportDeclaration(data) if data.is_type_only
            )
        }),
        _ => false,
    }
}

fn grandparent(arena: &tsrs2_syntax::NodeArena, node: NodeId) -> Option<NodeId> {
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
mod tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags};

    use crate::state::test_support::with_program_state;

    #[test]
    fn cross_file_duplicate_classes_report_2300_on_both_files() {
        with_program_state(
            &[("a.ts", "class C {}\n"), ("b.ts", "class C {}\n")],
            &CompilerOptions::default(),
            |state| {
                let mut pins: Vec<(u32, Option<String>)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.file_name.clone()))
                    .collect();
                pins.sort();
                assert_eq!(
                    pins,
                    [
                        (2300, Some("a.ts".to_owned())),
                        (2300, Some("b.ts".to_owned())),
                    ]
                );
                // Each report carries the "was also declared here"
                // related info pointing at the OTHER file.
                for diagnostic in &state.diagnostics {
                    assert_eq!(diagnostic.related.len(), 1);
                    assert_ne!(diagnostic.related[0].file_name, diagnostic.file_name);
                }
            },
        );
    }

    #[test]
    fn cross_file_let_redeclaration_reports_2451() {
        with_program_state(
            &[
                ("a.ts", "declare let x: number;\n"),
                ("b.ts", "declare let x: string;\n"),
            ],
            &CompilerOptions::default(),
            |state| {
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2451, 2451]);
            },
        );
    }

    #[test]
    fn cross_file_interfaces_merge_declarations_and_members() {
        with_program_state(
            &[
                ("a.ts", "interface I { a: number }\n"),
                ("b.ts", "interface I { b: string }\n"),
            ],
            &CompilerOptions::default(),
            |state| {
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
                let symbol = state
                    .resolve_file_scope_name("I", SymbolFlags::TYPE)
                    .expect("merged interface resolves");
                assert_eq!(state.binder.symbol(symbol).declarations.len(), 2);
                // The merged global is a checker-side clone (the file-a
                // original was not transient), and both originals chase
                // to it.
                assert!(state
                    .binder
                    .symbol(symbol)
                    .flags
                    .intersects(SymbolFlags::TRANSIENT));
                let declared = state
                    .get_declared_type_of_class_or_interface(symbol)
                    .expect("thisless non-generic interface");
                let members = state
                    .resolve_structured_type_members(declared)
                    .expect("members resolve");
                let names: Vec<String> = state
                    .members_of(members)
                    .properties
                    .iter()
                    .map(|&p| state.binder.symbol(p).escaped_name.clone())
                    .collect();
                assert_eq!(names, ["a", "b"]);
            },
        );
    }

    #[test]
    fn global_this_declaration_conflicts_with_builtin() {
        with_program_state(
            &[("a.ts", "var globalThis: number;\n")],
            &CompilerOptions::default(),
            |state| {
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2397]);
            },
        );
    }

    #[test]
    fn var_undefined_conflicts_with_builtin_but_type_undefined_does_not() {
        with_program_state(
            &[("a.ts", "var undefined: number;\n")],
            &CompilerOptions::default(),
            |state| {
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2397]);
            },
        );
        with_program_state(
            &[("a.ts", "interface undefined { a: number }\n")],
            &CompilerOptions::default(),
            |state| {
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn js_involvement_suppresses_duplicate_reporting() {
        // M2 3.4c residual guard: JS containers merging with TS
        // declarations must not produce FPs while JS Assignment-flag
        // binding is unported.
        let options = CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        };
        with_program_state(
            &[
                ("a.d.ts", "declare class A {}\n"),
                ("b.js", "const A = {};\n"),
            ],
            &options,
            |state| {
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }
}
