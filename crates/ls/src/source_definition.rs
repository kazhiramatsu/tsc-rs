//! tsgo `ls/sourcedefinition.go`: go to a declaration's source — its
//! implementation (a package's JavaScript for its declaration files, a
//! declaration map's source) rather than the declaration file — resolving
//! modules without declaration files and reading files the program does not
//! have.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use tsc_diagnostics::spanmap::FEATURE_DEFINITION;

use tsc_binder::assignment::{get_assignment_declaration_kind, AssignmentDeclarationKind};
use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{DefinitionResponse, DocumentUri, Position};
use tsc_program::{ModuleResolver, ResolutionMode, ResolutionOutcome};
use tsc_syntax::{NodeData, NodeId, ParseOptions, ScriptKind, SourceFile, SyntaxKind};
use tsc_types::{JsStr, ModifierFlags};

use crate::astnav::Navigator;
use crate::definition::combine_definition_responses;
use crate::language_service::{LanguageService, LsError};
use crate::references::collect_external_module_references;
use crate::request::Request;
use crate::utilities::{
    is_require_call, range_of_node_in, reference_at_position, source_file, touching, Touching,
};

impl LanguageService<'_> {
    /// tsgo `ProvideSourceDefinition` (a file without a content mapper: one
    /// position).
    pub fn provide_source_definition(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<DefinitionResponse, LsError> {
        let links = self.capabilities.text_document.definition.link_support;
        let file = self.file(uri);
        let result = self.with_definitions(
            file,
            position,
            links,
            FEATURE_DEFINITION,
            |request, file, position| request.source_definition_at_position(file, position),
        )?;
        Ok(combine_definition_responses(
            result.into_iter().collect(),
            links,
        ))
    }
}

/// A declaration of a program file or of a file the program does not have.
#[derive(Clone)]
struct Declaration<'a> {
    file: DeclarationFile<'a>,
    node: NodeId,
}

#[derive(Clone)]
enum DeclarationFile<'a> {
    Program(&'a SourceFile),
    Parsed(Rc<SourceFile>),
}

impl DeclarationFile<'_> {
    fn source(&self) -> &SourceFile {
        match self {
            Self::Program(source) => source,
            Self::Parsed(source) => source,
        }
    }
}

impl<'a> Declaration<'a> {
    fn source(&self) -> &SourceFile {
        self.file.source()
    }
}

/// tsgo `sourceDefResolver`: resolves modules without declaration files
/// (`noDtsResolution`) from the request's file, and parses the files the
/// program does not have.
struct SourceDefResolver<'h> {
    resolver: Option<ModuleResolver<'h>>,
    options: &'h tsc_types::CompilerOptions,
    resolve_from: String,
    parsed: HashMap<String, Option<Rc<SourceFile>>>,
}

impl<'a> Request<'_, 'a> {
    /// tsgo `provideSourceDefinitionAtPosition`.
    fn source_definition_at_position(
        &mut self,
        file: usize,
        position: u32,
    ) -> CheckResult<DefinitionResponse> {
        let source = self.state.binder.source(file);
        let host = self.host;
        let compiler_host = host.compiler_host();
        let mut options = self.state.options.clone();
        options.no_dts_resolution = Some(true);
        let program_options = self.prepared.program_options();
        let mut resolver = SourceDefResolver {
            resolver: compiler_host.as_deref().and_then(|host| {
                ModuleResolver::new_with_program_options(host, &options, program_options).ok()
            }),
            options: &options,
            resolve_from: source.file_name.to_string_lossy().into_owned(),
            parsed: HashMap::new(),
        };

        let navigator = Navigator::new(source);
        let found = navigator.touching_property_name(position);
        if navigator.kind(found) == SyntaxKind::SourceFile {
            // Triple-slash directives are comments, not AST nodes, so
            // GetTouchingPropertyName returns the SourceFile node.
            if let Some((declarations, reference)) =
                self.resolve_triple_slash_reference(&mut resolver, file, position)
            {
                if !declarations.is_empty() {
                    let origin = self.lsp_range(source, reference);
                    return Ok(self.source_definition_locations(origin, &declarations));
                }
            }
            return Ok(DefinitionResponse::Null);
        }
        let origin = self.lsp_range(
            source,
            navigator.start_of_node(found, false) as usize..navigator.end(found) as usize,
        );
        let Some(node) = touching(found) else {
            return self.definition_at_position(file, position);
        };

        let containing_module_specifier = self.find_containing_module_specifier(node);
        if let Some(specifier) = containing_module_specifier {
            let parent = self
                .parent_of(specifier)
                .expect("a module specifier's import");
            if tsc_syntax::utilities::is_source_phase_import(&source.arena, parent) {
                return self.definition_at_position(file, position);
            }
        }

        // If the cursor is directly on a module specifier string, resolve to the
        // implementation file's entry point.
        if let (Some(specifier), Touching::Node(id)) = (containing_module_specifier, node) {
            if id == specifier {
                let mode = self.usage_mode(specifier);
                let text = self.node_text(Touching::Node(specifier));
                if let Some(implementation) = resolver.resolve_implementation(&text, mode) {
                    if let Some(implementation) =
                        self.get_or_parse_source_file(&mut resolver, &implementation)
                    {
                        let declarations = entry_declarations(implementation);
                        return Ok(self.source_definition_locations(origin, &declarations));
                    }
                }
                return self.definition_at_position(file, position);
            }
        }

        // Phase 1: Syntactic fast path — when the cursor is inside an
        // import/require/export, forward-resolve the module specifier to an
        // implementation file and search it directly. This avoids acquiring
        // the type checker entirely when the fast path succeeds.
        let resolved_implementation_file = containing_module_specifier.and_then(|specifier| {
            let mode = self.usage_mode(specifier);
            let text = self.node_text(Touching::Node(specifier));
            resolver.resolve_implementation(&text, mode)
        });
        if let Some(implementation) = &resolved_implementation_file {
            let names = self.candidate_source_declaration_names(Some(node), None);
            if let Some(results) =
                self.search_implementation_file(&mut resolver, node, implementation, &names)
            {
                if !results.is_empty()
                    && (!self.is_part_of_type_node_or_type_only_import(node)
                        || has_concrete_source_declarations(&results))
                {
                    let results = unique_declaration_nodes(results);
                    return Ok(self.source_definition_locations(origin, &results));
                }
            }
        }

        // Phase 2: Type checker path — acquire the checker for the original file
        // and use its declarations and module specifier to map to source
        // implementations.
        let (checker_declarations, module_specifier) = self.source_definition_checker_info(node)?;

        // Phase 3: Map checker results to source definitions.
        let declarations = self.resolve_from_checker_info(
            &mut resolver,
            node,
            resolved_implementation_file.clone(),
            &checker_declarations,
            module_specifier,
        );
        if declarations.is_empty() {
            // If we resolved an implementation file from an import/export but
            // couldn't find specific declarations, fall back to the file entry
            // point rather than the standard definition provider — unless the
            // checker found declarations that are all type-only (e.g. interfaces),
            // in which case the .d.ts definition is more appropriate.
            let checker_declarations: Vec<Declaration<'a>> = checker_declarations
                .iter()
                .map(|&node| self.program_declaration(node))
                .collect();
            if containing_module_specifier.is_some()
                && !has_concrete_source_declarations(&checker_declarations)
            {
                if let Some(implementation) = &resolved_implementation_file {
                    if let Some(implementation) =
                        self.get_or_parse_source_file(&mut resolver, implementation)
                    {
                        let declarations = entry_declarations(implementation);
                        return Ok(self.source_definition_locations(origin, &declarations));
                    }
                }
            }
            return self.definition_at_position(file, position);
        }
        Ok(self.source_definition_locations(origin, &declarations))
    }

    /// tsgo `createDefinitionLocations` of source definitions.
    fn source_definition_locations(
        &self,
        origin: tsc_lsproto::Range,
        declarations: &[Declaration<'_>],
    ) -> DefinitionResponse {
        let declarations: Vec<(&SourceFile, NodeId)> = declarations
            .iter()
            .map(|declaration| (declaration.source(), declaration.node))
            .collect();
        self.definition_locations_in(origin, &declarations, None, FEATURE_DEFINITION)
    }

    /// A program declaration.
    fn program_declaration(&self, node: NodeId) -> Declaration<'a> {
        Declaration {
            file: DeclarationFile::Program(self.state.binder.source_of_node(node)),
            node,
        }
    }

    /// tsgo `Program.GetModeForUsageLocation` of a module specifier (none
    /// for tsgo's `ResolutionModeNone`).
    fn usage_mode(&self, specifier: NodeId) -> Option<ResolutionMode> {
        match self.state.api_mode_for_usage_location(specifier) {
            1 => Some(ResolutionMode::CommonJs),
            99 => Some(ResolutionMode::EsNext),
            _ => None,
        }
    }

    /// tsgo `ast.IsPartOfTypeNode(node) ||
    /// ast.IsPartOfTypeOnlyImportOrExportDeclaration(node)`.
    fn is_part_of_type_node_or_type_only_import(&self, node: Touching) -> bool {
        let Touching::Node(id) = node else {
            return false;
        };
        self.state.is_part_of_type_node(id)
            || self
                .state
                .is_part_of_type_only_import_or_export_declaration(id)
    }

    /// tsgo `getSourceDefCheckerInfo`: the definition declarations of a node
    /// (a property's of the left side's type when it has none; the called
    /// signature's) and the module specifier of the import that brought its
    /// (or its access chain's root's) symbol into scope.
    fn source_definition_checker_info(
        &mut self,
        node: Touching,
    ) -> CheckResult<(Vec<NodeId>, Option<String>)> {
        let mut declarations = self.declarations_from_location(node)?;
        let parent = self.parent(node);
        let access = parent.filter(|&parent| {
            matches!(
                &self.state.binder.node_record(parent).data,
                NodeData::PropertyAccessExpression(access)
                    if access.name == Some(match node { Touching::Node(id) => id, Touching::Token { parent, .. } => parent })
                        && matches!(node, Touching::Node(_))
            )
        });
        let left = access.and_then(|access| match &self.state.binder.node_record(access).data {
            NodeData::PropertyAccessExpression(data) => data.expression,
            _ => None,
        });
        if declarations.is_empty() {
            if let Some(left) = left {
                let ty = self.state.get_type_at_location(left)?;
                let name = self.node_text(node);
                let name = tsc_binder::escape_leading_underscores(JsStr::from_str(&name));
                if let Some(property) = self.state.get_property_of_type_full(ty, name)? {
                    declarations = self.declarations_of(property);
                }
            }
        }
        if let Some(called) = self.signature_declaration(node)? {
            declarations.retain(|&declaration| {
                !node_util::is_function_like_kind(self.node_kind(declaration))
            });
            declarations.push(called);
        }

        // Extract module specifier from the import that brought this symbol into
        // scope. For property access (obj.prop), walk up the access chain to the
        // root expression's symbol.
        let mut resolve_node = node;
        if let Some(mut expression) = left {
            while let Some(inner) = match &self.state.binder.node_record(expression).data {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                _ => None,
            } {
                expression = inner;
            }
            resolve_node = Touching::Node(expression);
        }
        let mut module_specifier = None;
        if let Some(symbol) = self.symbol_at(resolve_node)? {
            for declaration in self.declarations_of(symbol) {
                if !matches!(
                    self.node_kind(declaration),
                    SyntaxKind::ImportSpecifier
                        | SyntaxKind::ImportClause
                        | SyntaxKind::NamespaceImport
                        | SyntaxKind::ImportEqualsDeclaration
                ) {
                    continue;
                }
                if let Some(specifier) = self
                    .state
                    .try_get_module_specifier_from_declaration(declaration)
                {
                    module_specifier = Some(self.node_text(Touching::Node(specifier)));
                    break;
                }
            }
        }
        Ok((declarations, module_specifier))
    }

    /// tsgo `resolveFromCheckerInfo`: the checker's declarations mapped to
    /// their sources, if any is concrete.
    fn resolve_from_checker_info(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        node: Touching,
        resolved_implementation_file: Option<String>,
        checker_declarations: &[NodeId],
        module_specifier: Option<String>,
    ) -> Vec<Declaration<'a>> {
        // If we don't yet have a forward-resolved implementation file, try to
        // recover a module specifier from the checker (e.g. from the import that
        // brought the symbol into scope, or from the root of an access expression).
        let mut resolved_implementation_file = resolved_implementation_file;
        if resolved_implementation_file.is_none() {
            if let Some(specifier) = module_specifier.filter(|specifier| !specifier.is_empty()) {
                let from = resolver.resolve_from.clone();
                let mode = resolver.infer_implied_node_format(&from);
                resolved_implementation_file = resolver.resolve_implementation(&specifier, mode);
            }
        }

        // For property access where the checker found no declarations (e.g.
        // mapped types), search the implementation file for the property name.
        if checker_declarations.is_empty() {
            if let Some(implementation) = &resolved_implementation_file {
                let names = self.candidate_source_declaration_names(Some(node), None);
                if let Some(results) =
                    self.search_implementation_file(resolver, node, implementation, &names)
                {
                    return unique_declaration_nodes(results);
                }
            }
        }

        let mut declarations = Vec::new();
        for &declaration in checker_declarations {
            declarations.extend(self.map_declaration_to_source(
                resolver,
                node,
                declaration,
                resolved_implementation_file.as_deref(),
            ));
        }
        let declarations = unique_declaration_nodes(declarations);
        if has_concrete_source_declarations(&declarations) {
            declarations
        } else {
            Vec::new()
        }
    }

    /// tsgo `resolveTripleSlashReference`: a `/// <reference path/types>`'s
    /// implementation file's entry (a declaration file's through the
    /// declaration-less resolver), with the reference's range.
    fn resolve_triple_slash_reference(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        file: usize,
        position: u32,
    ) -> Option<(Vec<Declaration<'a>>, std::ops::Range<usize>)> {
        let reference = reference_at_position(self.state, self.prepared, file, position)?;
        let target = reference.file?;
        let range = reference.reference?;
        let target = self.state.binder.source(target);
        // If the referenced file is already an implementation file, return it directly.
        if !target.is_declaration_file {
            return Some((entry_declarations(DeclarationFile::Program(target)), range));
        }
        // The referenced file is a .d.ts. Try to find the implementation file
        // using the NoDts module resolver via findImplementationFileFromDtsFileName.
        let declaration_file_name = target.file_name.to_string_lossy().into_owned();
        let mode = resolver.infer_implied_node_format(&declaration_file_name);
        let implementation = self.find_implementation_file_from_declaration_file(
            resolver,
            &declaration_file_name,
            mode,
        )?;
        let implementation = self.get_or_parse_source_file(resolver, &implementation)?;
        Some((entry_declarations(implementation), range))
    }

    /// tsgo `searchImplementationFile`: the declarations of `names` in an
    /// implementation file (and the files it forwards to), preferred ones
    /// first; none when nothing matched.
    fn search_implementation_file(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        original_node: Touching,
        implementation_file: &str,
        names: &[String],
    ) -> Option<Vec<Declaration<'a>>> {
        if implementation_file.is_empty() {
            return None;
        }
        let source = self.get_or_parse_source_file(resolver, implementation_file)?;
        if self.is_default_import_name(original_node) {
            // For default imports, only search for "default" declarations to avoid
            // matching unrelated declarations with the same identifier name.
            let defaults = self.find_declarations_in_file(
                resolver,
                implementation_file,
                &["default".to_owned()],
                &mut HashSet::new(),
            );
            if !defaults.is_empty() {
                return Some(self.filter_preferred_source_declarations(original_node, defaults));
            }
            return Some(entry_declarations(source));
        }
        let declarations = self.find_declarations_in_file(
            resolver,
            implementation_file,
            names,
            &mut HashSet::new(),
        );
        if !declarations.is_empty() {
            return Some(self.filter_preferred_source_declarations(original_node, declarations));
        }
        None
    }

    /// tsgo `isDefaultImportName`: the name of a default import.
    fn is_default_import_name(&self, node: Touching) -> bool {
        let Touching::Node(id) = node else {
            return false;
        };
        let Some(clause) = self.parent_of(id) else {
            return false;
        };
        let NodeData::ImportClause(data) = &self.state.binder.node_record(clause).data else {
            return false;
        };
        data.name == Some(id)
            && self.parent_of(clause).is_some_and(|declaration| {
                self.node_kind(declaration) == SyntaxKind::ImportDeclaration
            })
    }

    /// tsgo `mapDeclarationToSource`: a declaration's source — through its
    /// declaration map, else the declaration itself in an implementation
    /// file, else the declarations named like it in its implementation file.
    fn map_declaration_to_source(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        original_node: Touching,
        declaration: NodeId,
        resolved_implementation_file: Option<&str>,
    ) -> Vec<Declaration<'a>> {
        let source = self.state.binder.source_of_node(declaration);
        let name = node_util::get_name_of_declaration(source, declaration).unwrap_or(declaration);
        let start = range_of_node_in(source, name).start;
        let file_name = source.file_name.to_string_lossy().into_owned();

        if let Some(mapped) = self.source_maps.try_get_source_position(&file_name, start) {
            if let Some(mapped_file) = self.get_or_parse_source_file(resolver, &mapped.file_name) {
                return vec![find_closest_declaration_node(mapped_file, mapped.pos)];
            }
        }

        if !tsc_program::is_declaration_file_name(&file_name) {
            return vec![self.program_declaration(declaration)];
        }

        let implementation_file = match resolved_implementation_file {
            Some(implementation) => Some(implementation.to_owned()),
            // Reverse-resolve .d.ts path to implementation file. This path is only
            // reached for declarations with no associated module specifier (e.g.
            // globals, ambient declarations, or when forward resolution failed).
            None => {
                let mode = resolver.infer_implied_node_format(&file_name);
                self.find_implementation_file_from_declaration_file(resolver, &file_name, mode)
            }
        };
        let names = self.candidate_source_declaration_names(Some(original_node), Some(declaration));
        self.search_implementation_file(
            resolver,
            original_node,
            implementation_file.as_deref().unwrap_or_default(),
            &names,
        )
        .unwrap_or_default()
    }

    /// tsgo `findImplementationFileFromDtsFileName`: the JavaScript file
    /// beside a declaration file, else (for a package's declaration file)
    /// the package's implementation of its path, else of the package.
    fn find_implementation_file_from_declaration_file(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        declaration_file_name: &str,
        preferred_mode: Option<ResolutionMode>,
    ) -> Option<String> {
        if let Some(js_extension) = tsc_program::try_get_js_extension_for_file(
            JsStr::from_str(declaration_file_name),
            resolver.options,
        ) {
            let candidate =
                tsc_program::change_extension(JsStr::from_str(declaration_file_name), js_extension)
                    .to_string_lossy()
                    .into_owned();
            if self.host.file_exists(&candidate) {
                return Some(candidate);
            }
        }

        let parts = tsc_checker::node_module_package_path(declaration_file_name)?;
        if parts.has_nested_node_modules || parts.is_direct_node_modules_file {
            return None;
        }
        let package_name = tsc_program::get_package_name_from_types_package_name(
            &tsc_program::unmangle_scoped_package_name(&parts.package_name),
        );
        if package_name.is_empty() {
            return None;
        }

        // Try resolving as a package subpath first (e.g. "pkg/dist/utils"), then
        // fall back to the bare package name (e.g. "pkg"). This covers both main
        // entrypoints and deep imports without needing to inspect package.json
        // entrypoints.
        if !parts.package_relative_path.is_empty() {
            let specifier = tsc_program::remove_file_extension(JsStr::from_str(&format!(
                "{package_name}/{}",
                parts.package_relative_path
            )))
            .to_string_lossy()
            .into_owned();
            if let Some(implementation) =
                resolver.resolve_implementation(&specifier, preferred_mode)
            {
                return Some(implementation);
            }
        }
        resolver.resolve_implementation(&package_name, preferred_mode)
    }

    /// tsgo `getOrParseSourceFile`: the program's file, else the file parsed
    /// (once) from the snapshot.
    fn get_or_parse_source_file(
        &self,
        resolver: &mut SourceDefResolver<'_>,
        file_name: &str,
    ) -> Option<DeclarationFile<'a>> {
        if let Some(file) = source_file(self.state, self.prepared, file_name) {
            return Some(DeclarationFile::Program(self.state.binder.source(file)));
        }
        if let Some(parsed) = resolver.parsed.get(file_name) {
            return parsed.clone().map(DeclarationFile::Parsed);
        }
        let parsed = self.host.read_file(file_name).map(|text| {
            // A declaration map's `sources` entries are arbitrary strings, so the
            // file name here may not have a recognized extension.
            let mut script_kind = ScriptKind::from_file_name(file_name);
            if script_kind == ScriptKind::Unknown {
                script_kind = ScriptKind::Ts;
            }
            Rc::new(tsc_syntax::parse_source_file(
                file_name,
                text,
                ParseOptions {
                    script_kind,
                    ..ParseOptions::default()
                },
                None,
            ))
        });
        resolver.parsed.insert(file_name.to_owned(), parsed.clone());
        parsed.map(DeclarationFile::Parsed)
    }

    /// tsgo `findDeclarationsInFile`: the shallowest declarations of `names`
    /// in a file, else (or besides nothing concrete) in the files it
    /// imports.
    fn find_declarations_in_file(
        &mut self,
        resolver: &mut SourceDefResolver<'_>,
        file_name: &str,
        names: &[String],
        seen: &mut HashSet<String>,
    ) -> Vec<Declaration<'a>> {
        if file_name.is_empty() || names.is_empty() || !seen.insert(file_name.to_owned()) {
            return Vec::new();
        }
        let Some(file) = self.get_or_parse_source_file(resolver, file_name) else {
            return Vec::new();
        };
        let declarations = find_declaration_nodes_by_name(&file, names);
        if !declarations.is_empty() && has_concrete_source_declarations(&declarations) {
            return declarations;
        }
        let mut forwarded = Vec::new();
        for forwarded_file in self.forwarded_implementation_files(resolver, file.source()) {
            forwarded.extend(self.find_declarations_in_file(
                resolver,
                &forwarded_file,
                names,
                seen,
            ));
        }
        if !forwarded.is_empty() {
            if has_concrete_source_declarations(&forwarded) {
                return unique_declaration_nodes(forwarded);
            }
            let mut all = declarations;
            all.extend(forwarded);
            return unique_declaration_nodes(all);
        }
        declarations
    }

    /// tsgo `getForwardedImplementationFiles`: the implementation files a
    /// file's imports resolve to, each once.
    fn forwarded_implementation_files(
        &self,
        resolver: &mut SourceDefResolver<'_>,
        source: &SourceFile,
    ) -> Vec<String> {
        let file_name = source.file_name.to_string_lossy().into_owned();
        let preferred_mode = resolver.infer_implied_node_format(&file_name);
        let mut files: Vec<String> = Vec::new();
        for import in collect_external_module_references(source).imports {
            let module_name = match &source.arena.node(import).data {
                NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
                NodeData::NoSubstitutionTemplateLiteral(data) => {
                    data.text.to_string_lossy().into_owned()
                }
                _ => continue,
            };
            if let Some(implementation) =
                resolver.resolve_implementation_from(&module_name, &file_name, preferred_mode)
            {
                if !files.contains(&implementation) {
                    files.push(implementation);
                }
            }
        }
        files
    }

    /// tsgo `getCandidateSourceDeclarationNames`: the names a source
    /// declaration of the node (and of its declaration) may have.
    fn candidate_source_declaration_names(
        &self,
        original_node: Option<Touching>,
        declaration: Option<NodeId>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        if let Some(declaration) = declaration {
            let source = self.state.binder.source_of_node(declaration);
            if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
                if let Some(text) = node_util::try_get_text_of_property_name(source, name)
                    .filter(|text| !text.is_empty())
                {
                    names.push(text);
                }
            }
            let kind = self.node_kind(declaration);
            if kind == SyntaxKind::ExportAssignment {
                names.push("default".to_owned());
            }
            if matches!(
                kind,
                SyntaxKind::FunctionDeclaration | SyntaxKind::ClassDeclaration
            ) && node_util::get_syntactic_modifier_flags(source, declaration)
                .contains(ModifierFlags::EXPORT_DEFAULT)
            {
                names.push("default".to_owned());
            }
            if let Some(property_name) = self.specifier_property_name(declaration) {
                names.push(property_name);
            }
        }
        if let Some(original_node) = original_node {
            if matches!(
                self.kind(original_node),
                SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier
            ) {
                names.push(self.node_text(original_node));
            }
            if self.is_default_import_name(original_node) {
                names.push("default".to_owned());
            }
            if let Some(parent) = self.parent(original_node) {
                if let Some(property_name) = self.specifier_property_name(parent) {
                    names.push(property_name);
                }
            }
        }
        names
    }

    /// The property name of an import or export specifier (`a` of `a as b`).
    fn specifier_property_name(&self, node: NodeId) -> Option<String> {
        let property_name = match &self.state.binder.node_record(node).data {
            NodeData::ImportSpecifier(data) => data.property_name,
            NodeData::ExportSpecifier(data) => data.property_name,
            _ => None,
        }?;
        Some(self.node_text(Touching::Node(property_name)))
    }

    /// tsgo `filterPreferredSourceDeclarations`: for a property access's
    /// name its property-like declarations, else the concrete ones, else
    /// all.
    fn filter_preferred_source_declarations(
        &self,
        original_node: Touching,
        declarations: Vec<Declaration<'a>>,
    ) -> Vec<Declaration<'a>> {
        if declarations.len() <= 1 {
            return declarations;
        }
        let preferred = self.property_like_source_declarations(original_node, &declarations);
        if !preferred.is_empty() {
            return preferred;
        }
        let concrete: Vec<Declaration<'a>> = declarations
            .iter()
            .filter(|declaration| is_concrete_source_declaration(declaration))
            .cloned()
            .collect();
        if !concrete.is_empty() {
            return concrete;
        }
        declarations
    }

    /// tsgo `getPropertyLikeSourceDeclarations`.
    fn property_like_source_declarations(
        &self,
        original_node: Touching,
        declarations: &[Declaration<'a>],
    ) -> Vec<Declaration<'a>> {
        let Touching::Node(id) = original_node else {
            return Vec::new();
        };
        let is_property_name = self.parent_of(id).is_some_and(|parent| {
            matches!(
                &self.state.binder.node_record(parent).data,
                NodeData::PropertyAccessExpression(access) if access.name == Some(id)
            )
        });
        if !is_property_name {
            return Vec::new();
        }
        declarations
            .iter()
            .filter(|declaration| {
                matches!(
                    declaration.source().arena.node(declaration.node).kind,
                    SyntaxKind::PropertyAssignment
                        | SyntaxKind::ShorthandPropertyAssignment
                        | SyntaxKind::PropertyDeclaration
                        | SyntaxKind::PropertySignature
                        | SyntaxKind::MethodDeclaration
                        | SyntaxKind::MethodSignature
                        | SyntaxKind::GetAccessor
                        | SyntaxKind::SetAccessor
                        | SyntaxKind::EnumMember
                )
            })
            .cloned()
            .collect()
    }

    /// tsgo `findContainingModuleSpecifier`: the string module specifier of
    /// the import, export, `require` or `import()` around a node.
    fn find_containing_module_specifier(&self, node: Touching) -> Option<NodeId> {
        let mut current = match node {
            Touching::Node(id) => Some(id),
            Touching::Token { parent, .. } => Some(parent),
        };
        while let Some(id) = current {
            let source = self.state.binder.source_of_node(id);
            let module_name = match &source.arena.node(id).data {
                NodeData::ImportDeclaration(data) => data.module_specifier,
                NodeData::ExportDeclaration(data) => data.module_specifier,
                NodeData::ImportEqualsDeclaration(data) => {
                    data.module_reference.and_then(|reference| {
                        match &source.arena.node(reference).data {
                            NodeData::ExternalModuleReference(reference) => reference.expression,
                            _ => None,
                        }
                    })
                }
                NodeData::CallExpression(call)
                    if is_require_call(source, id, true)
                        || tsc_syntax::utilities::is_import_call(&source.arena, id) =>
                {
                    call.arguments.and_then(|arguments| {
                        source.arena.node_array(arguments).nodes.first().copied()
                    })
                }
                _ => None,
            };
            if let Some(module_name) = module_name.filter(|&name| {
                matches!(
                    source.arena.node(name).kind,
                    SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                )
            }) {
                return Some(module_name);
            }
            current = source.arena.node(id).parent;
        }
        None
    }
}

impl SourceDefResolver<'_> {
    /// tsgo `resolveImplementation`: from the request's file.
    fn resolve_implementation(
        &mut self,
        module_name: &str,
        preferred_mode: Option<ResolutionMode>,
    ) -> Option<String> {
        let from = self.resolve_from.clone();
        self.resolve_implementation_from(module_name, &from, preferred_mode)
    }

    /// tsgo `resolveImplementationFrom`: the first resolution, in the
    /// preferred mode then ESM then CommonJS, to a file that is not a
    /// declaration file.
    fn resolve_implementation_from(
        &mut self,
        module_name: &str,
        resolve_from_file: &str,
        preferred_mode: Option<ResolutionMode>,
    ) -> Option<String> {
        let resolver = self.resolver.as_mut()?;
        let mut modes = vec![preferred_mode.unwrap_or(ResolutionMode::Unspecified)];
        if preferred_mode != Some(ResolutionMode::EsNext) {
            modes.push(ResolutionMode::EsNext);
        }
        if preferred_mode != Some(ResolutionMode::CommonJs) {
            modes.push(ResolutionMode::CommonJs);
        }
        for mode in modes {
            let Ok(ResolutionOutcome::Resolved(module)) = resolver.resolve(
                JsStr::from_str(resolve_from_file),
                JsStr::from_str(module_name),
                mode,
            ) else {
                continue;
            };
            let file_name = module
                .resolved_file()
                .display()
                .to_string_lossy()
                .into_owned();
            if !tsc_program::is_declaration_file_name(&file_name) {
                return Some(file_name);
            }
        }
        None
    }

    /// tsgo `inferImpliedNodeFormat`: a file's module format by its
    /// extension and its package scope's `type` (for a file the program
    /// may not have).
    fn infer_implied_node_format(&mut self, file_name: &str) -> Option<ResolutionMode> {
        let package_type = self
            .resolver
            .as_mut()
            .and_then(|resolver| {
                resolver
                    .package_scope_for_file(JsStr::from_str(file_name))
                    .ok()
            })
            .flatten()
            .map(|scope| scope.module_type());
        tsc_program::get_implied_node_format_for_file(JsStr::from_str(file_name), || package_type)
    }
}

/// tsgo `getSourceDefinitionEntryDeclarations`: a file's first statement,
/// else the file.
fn entry_declarations(file: DeclarationFile<'_>) -> Vec<Declaration<'_>> {
    let source = file.source();
    let node = match &source.arena.node(source.root).data {
        NodeData::SourceFile(data) => data
            .statements
            .and_then(|statements| source.arena.node_array(statements).nodes.first().copied()),
        _ => None,
    }
    .unwrap_or(source.root);
    vec![Declaration { file, node }]
}

/// tsgo `findDeclarationNodesByName`: the declarations named one of
/// `names` (a `default` export for `default`) at the shallowest container
/// depth.
fn find_declaration_nodes_by_name<'a>(
    file: &DeclarationFile<'a>,
    names: &[String],
) -> Vec<Declaration<'a>> {
    let mut wanted: HashSet<&str> = HashSet::new();
    let mut want_default = false;
    for name in names.iter().filter(|name| !name.is_empty()) {
        if name == "default" {
            want_default = true;
        } else {
            wanted.insert(name.as_str());
        }
    }
    if wanted.is_empty() && !want_default {
        return Vec::new();
    }
    let source = file.source();
    let mut candidates: Vec<(NodeId, usize)> = Vec::new();
    let mut stack: Vec<NodeId> = Vec::new();
    let children = |node: NodeId, stack: &mut Vec<NodeId>| {
        let mut nodes = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            nodes.push(child);
            false
        });
        stack.extend(nodes.into_iter().rev());
    };
    children(source.root, &mut stack);
    while let Some(node) = stack.pop() {
        let mut matched = node_util::get_name_of_declaration(source, node)
            .and_then(|name| node_util::try_get_text_of_property_name(source, name))
            .is_some_and(|text| !text.is_empty() && wanted.contains(text.as_str()));
        let kind = source.arena.node(node).kind;
        if want_default && kind == SyntaxKind::ExportAssignment {
            matched = true;
        }
        if want_default
            && matches!(
                kind,
                SyntaxKind::FunctionDeclaration | SyntaxKind::ClassDeclaration
            )
            && node_util::get_syntactic_modifier_flags(source, node)
                .contains(ModifierFlags::EXPORT_DEFAULT)
        {
            matched = true;
        }
        if matched {
            candidates.push((node, container_depth(source, node)));
        }
        children(node, &mut stack);
    }
    // Only keep declarations at the shallowest depth, like getTopMostDeclarationNamesInFile.
    let Some(min_depth) = candidates.iter().map(|&(_, depth)| depth).min() else {
        return Vec::new();
    };
    unique_declaration_nodes(
        candidates
            .into_iter()
            .filter(|&(_, depth)| depth == min_depth)
            .map(|(node, _)| Declaration {
                file: file.clone(),
                node,
            })
            .collect(),
    )
}

/// tsgo `getContainerDepth`: the containers above a declaration, plus one.
fn container_depth(source: &SourceFile, node: NodeId) -> usize {
    let mut depth = 0;
    let mut current = Some(node);
    while let Some(node) = current {
        current = node_util::get_container_node(source, node);
        depth += 1;
    }
    depth
}

/// tsgo `hasConcreteSourceDeclarations`.
fn has_concrete_source_declarations(declarations: &[Declaration<'_>]) -> bool {
    declarations.iter().any(is_concrete_source_declaration)
}

/// tsgo `isConcreteSourceDeclaration`: a declaration that is not an export
/// assignment, a JavaScript assignment, a parameter, a binding or an
/// import or export binding.
fn is_concrete_source_declaration(declaration: &Declaration<'_>) -> bool {
    let source = declaration.source();
    let node = declaration.node;
    let kind = source.arena.node(node).kind;
    if !node_util::is_declaration(source, node) || kind == SyntaxKind::ExportAssignment {
        return false;
    }
    if matches!(
        kind,
        SyntaxKind::BinaryExpression | SyntaxKind::CallExpression
    ) && get_assignment_declaration_kind(source, node) != AssignmentDeclarationKind::None
    {
        return false;
    }
    !matches!(
        kind,
        SyntaxKind::Parameter
            | SyntaxKind::TypeParameter
            | SyntaxKind::BindingElement
            | SyntaxKind::ImportClause
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
    )
}

/// tsgo `uniqueDeclarationNodes`: each file name and node range once.
fn unique_declaration_nodes(declarations: Vec<Declaration<'_>>) -> Vec<Declaration<'_>> {
    let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
    declarations
        .into_iter()
        .filter(|declaration| {
            let source = declaration.source();
            let record = source.arena.node(declaration.node);
            seen.insert((
                source.file_name.to_string_lossy().into_owned(),
                record.pos,
                record.end,
            ))
        })
        .collect()
}

/// tsgo `findClosestDeclarationNode`: the declaration (or export
/// assignment) around a position, else the file's entry.
fn find_closest_declaration_node(file: DeclarationFile<'_>, position: usize) -> Declaration<'_> {
    let source = file.source();
    let found = Navigator::new(source).touching_property_name(position as u32);
    let mut current = touching(found).map(|node| match node {
        Touching::Node(id) => id,
        Touching::Token { parent, .. } => parent,
    });
    while let Some(node) = current {
        if node_util::is_declaration(source, node)
            || source.arena.node(node).kind == SyntaxKind::ExportAssignment
        {
            return Declaration { file, node };
        }
        current = source.arena.node(node).parent;
    }
    entry_declarations(file).remove(0)
}
