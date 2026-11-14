use super::*;
use std::{cell::Cell, rc::Rc};
use tsc_diagnostics::JsString;

struct Host<'a> {
    options: &'a tsc_types::CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    ids: [SourceFileId; 1],
}
impl EmitHost for Host<'_> {
    fn compiler_options(&self) -> &tsc_types::CompilerOptions {
        self.options
    }
    fn current_directory(&self) -> JsStr<'_> {
        "/".into()
    }
    fn common_source_directory(&self) -> JsStr<'_> {
        "/".into()
    }
    fn config_file_path(&self) -> Option<JsStr<'_>> {
        None
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }
    fn source_file(&self, id: SourceFileId) -> Option<crate::EmitSource<'_>> {
        (id == self.ids[0]).then(|| {
            crate::EmitSource::new(
                id,
                "/fixture.ts".into(),
                "/fixture.ts".into(),
                true,
                None,
                Some(self.syntax),
            )
        })
    }
}

// This resolver intentionally cannot be sent to a printer thread.
struct OwnerResolver<'a> {
    syntax: &'a tsc_syntax::SourceFile,
    owner: std::thread::ThreadId,
    calls: Rc<Cell<usize>>,
}
impl OwnerResolver<'_> {
    fn called(&self) {
        assert_eq!(std::thread::current().id(), self.owner);
        self.calls.set(self.calls.get() + 1);
    }
}
impl EmitResolver for OwnerResolver<'_> {
    fn is_optional_parameter(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(false)
    }
    fn precalculate_declaration_emit_visibility(
        &self,
        _node: EmitResolverNode,
    ) -> Result<(), crate::EmitResolverError> {
        self.called();
        Ok(())
    }
    fn is_literal_const_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(false)
    }
    fn requires_adding_implicit_undefined(
        &self,
        _parameter: EmitResolverNode,
        _enclosing: Option<EmitResolverNode>,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(false)
    }
    fn create_type_of_declaration(
        &self,
        arena: &mut TransformArena,
        target: crate::TransformSourceId,
        declaration: EmitResolverNode,
        _enclosing: EmitResolverNode,
        _flags: crate::EmitNodeBuilderFlags,
        _internal: crate::EmitInternalNodeBuilderFlags,
        _tracker: &mut dyn crate::EmitSymbolTracker,
    ) -> Result<Option<crate::TransformNode>, crate::EmitResolverError> {
        self.called();
        let kind = if self.syntax.arena.node(declaration.node()).kind == SyntaxKind::Parameter {
            SyntaxKind::StringKeyword
        } else {
            SyntaxKind::NumberKeyword
        };
        arena
            .factory()
            .create_keyword_type_node(target, kind)
            .map(Some)
            .map_err(|error| crate::EmitResolverError::Factory {
                method: crate::EmitResolverMethod::CreateTypeOfDeclaration,
                error: Box::new(error),
            })
    }
    fn create_return_type_of_signature_declaration(
        &self,
        arena: &mut TransformArena,
        target: crate::TransformSourceId,
        _declaration: EmitResolverNode,
        _enclosing: EmitResolverNode,
        _flags: crate::EmitNodeBuilderFlags,
        _internal: crate::EmitInternalNodeBuilderFlags,
        _tracker: &mut dyn crate::EmitSymbolTracker,
    ) -> Result<Option<crate::TransformNode>, crate::EmitResolverError> {
        self.called();
        arena
            .factory()
            .create_keyword_type_node(target, SyntaxKind::BooleanKeyword)
            .map(Some)
            .map_err(|error| crate::EmitResolverError::Factory {
                method: crate::EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
                error: Box::new(error),
            })
    }
    fn can_include_bind_and_check_diagnostics(
        &self,
        _source: SourceFileId,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(true)
    }
    fn is_declaration_visible(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(true)
    }
    fn is_first_declaration_of_symbol(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(true)
    }
    fn is_implementation_of_overload(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(false)
    }
    fn has_global_name(&self, _name: &str) -> Result<bool, crate::EmitResolverError> {
        self.called();
        Ok(false)
    }
}

#[test]
fn declaration_text_and_maps_cross_without_their_owner_resolver() {
    for maps in [false, true] {
        let (expected, work, pool, calls, release, blocked) = {
            let syntax = tsc_syntax::parse_source_file(
                "/fixture.ts",
                "interface Shape { value: number; method(x: string): boolean; }",
                Default::default(),
                None,
            );
            let options = tsc_types::CompilerOptions {
                target: Some(9),
                module: Some(99),
                declaration: Some(true),
                declaration_map: Some(maps),
                emit_bom: Some(true),
                source_root: maps.then(|| JsString::from("/sources")),
                map_root: maps.then(|| JsString::from("/maps")),
                out_dir: Some("/out".into()),
                ..Default::default()
            };
            let host = Host {
                options: &options,
                syntax: &syntax,
                ids: [SourceFileId::from_raw(0)],
            };
            let preflight =
                crate::preflight_emit(&host, crate::EmitSelection::WholeProgram).unwrap();
            let paths = crate::PlanDeclarationPaths::new(&host, &preflight);
            let unit = &preflight.plan().units()[0];
            let resolver = OwnerResolver {
                syntax: &syntax,
                owner: std::thread::current().id(),
                calls: Rc::new(Cell::new(0)),
            };
            let expected = emit_declaration_unit(
                &resolver,
                &host,
                &preflight,
                &paths,
                unit.root(),
                unit.paths().declaration_path().unwrap(),
                unit.paths().declaration_map_path(),
                false,
            )
            .unwrap();
            // Occupy the only worker until all borrowed preparation inputs have
            // left this scope. The declaration must keep its own syntax alive.
            let pool = crate::EmitPrintPool::for_budget(2, 1, 16 * 1024 * 1024).unwrap();
            let (release, wait) = std::sync::mpsc::channel();
            let blocked = pool.submit(pool.acquire(), move |_| wait.recv().unwrap());
            let work = prepare_declaration_unit(
                &resolver,
                &host,
                &preflight,
                &paths,
                unit.root(),
                unit.paths().declaration_path().unwrap(),
                unit.paths().declaration_map_path(),
                false,
                Some(&pool),
                Some(pool.acquire()),
                None,
            )
            .unwrap();
            (
                expected,
                work,
                pool,
                Rc::clone(&resolver.calls),
                release,
                blocked,
            )
        };
        assert!(matches!(work, DeclarationWork::Pending(_)));
        release.send(()).unwrap();
        blocked.finish();
        let actual = work.finish().unwrap();
        assert_eq!(actual.artifact, expected.artifact);
        assert_eq!(actual.map_artifact, expected.map_artifact);
        assert_eq!(actual.map_observation, expected.map_observation);
        assert_eq!(actual.diagnostics, expected.diagnostics);
        assert_eq!(actual.decl_blocked, expected.decl_blocked);
        assert!(calls.get() > 0);
        assert_eq!(pool.peak().submitted, 2);
        assert_eq!(pool.peak().workers, 1);
    }
}
