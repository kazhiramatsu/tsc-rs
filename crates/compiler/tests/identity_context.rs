//! A cache-backed Program keeps its registry's capabilities, even when an
//! embedding client hands it a Program prepared by a finite command.
use std::{path::PathBuf, sync::Arc};
use tsc_checker::{
    check_program_with_authoritative_modules_at_for_emit_with_workers,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeResolutionMode, AuthoritativeSourceMetadata,
    AuthoritativeSourceToken, BoundDocument, IncrementalRequest, InputFile,
    SyntacticDiagnosticsGate,
};
use tsc_compiler::{DocumentRegistry, LiveProgram};
use tsc_host::MemoryCompilerHost;
use tsc_program::WorkerBudget;
use tsc_program::{
    load_no_lib_program, CompilerOptions, PreparedProgram, ProgramLoadLimits, ProgramOptions,
};
use tsc_types::IdentityDomain;

fn load(domain: Option<IdentityDomain>) -> PreparedProgram {
    let host = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"export const value = 1;".to_vec())
        .build()
        .unwrap();
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let program_options = match domain {
        Some(domain) => program_options.with_identity_domain(domain),
        None => program_options,
    };
    load_no_lib_program(
        &host,
        &[PathBuf::from("/p/a.ts")],
        options,
        program_options,
        ProgramLoadLimits::new(10, 100, 10, 1 << 20, 1 << 20),
    )
    .unwrap()
}

#[test]
fn cached_programs_keep_their_registry_domain_and_unchanged_document() {
    let command = IdentityDomain::new();
    let prepared = load(Some(command.clone()));
    let placements = Arc::clone(
        prepared.source_files()[0]
            .preparsed_syntax()
            .direct_layout()
            .unwrap(),
    );
    assert_eq!(placements.len(), 1);
    let registry = DocumentRegistry::new();
    assert!(!command.same_domain(registry.identity_domain()));
    let first = LiveProgram::with_documents(prepared, &registry).unwrap();
    let document = first.document(0).unwrap();
    assert!(document
        .source()
        .identity_owned_by(registry.identity_domain()));
    assert!(!document.source().identity_owned_by(&command));
    assert!(
        !placements[0].matches(document.source()),
        "foreign capability takes exact adoption"
    );

    let ordinary = load(None);
    assert!(ordinary.program_options().identity_domain().is_none());
    assert!(ordinary.source_files()[0]
        .preparsed_syntax()
        .identity_domain()
        .is_none());
    let second = LiveProgram::with_documents(ordinary, &registry).unwrap();
    assert!(
        Arc::ptr_eq(document, second.document(0).unwrap()),
        "API/project/LSP cache-backed Programs still reuse an unchanged document"
    );
}

#[test]
fn finite_programs_adopt_only_their_own_placed_capabilities() {
    struct NeverProvider(IdentityDomain);
    impl AuthoritativeModuleProvider for NeverProvider {
        fn identity_domain(&self) -> Option<&IdentityDomain> {
            Some(&self.0)
        }
        fn resolve_module(
            &self,
            _: AuthoritativeModuleRequest<'_>,
        ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
            unreachable!("these sources have no imports")
        }
    }
    let host = tsc_host::MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"export const value = 1;".to_vec())
        .build()
        .unwrap();
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    };
    let domains: [IdentityDomain; 4] = std::array::from_fn(|_| IdentityDomain::new());
    let receiving = [0, 1, 3];
    assert!(!domains[0].same_domain(&domains[1]));
    let registry = DocumentRegistry::new();
    assert!(domains
        .iter()
        .all(|domain| !domain.same_domain(registry.identity_domain())));
    let programs: Vec<_> = std::thread::scope(|scope| {
        let host = &host;
        let options = &options;
        let loads: Vec<_> = domains[..3]
            .iter()
            .map(|domain| {
                scope.spawn(move || {
                    tsc_program::load_no_lib_program(
                        host,
                        &[std::path::PathBuf::from("/p/a.ts")],
                        (*options).clone(),
                        tsc_program::ProgramOptions::default()
                            .with_no_lib(true)
                            .with_types(Vec::new())
                            .with_identity_domain(domain.clone()),
                        tsc_program::ProgramLoadLimits::new(10, 100, 10, 1 << 20, 1 << 20),
                    )
                    .unwrap()
                })
            })
            .collect();
        loads.into_iter().map(|load| load.join().unwrap()).collect()
    });
    let mut kept: Vec<Arc<BoundDocument>> = Vec::new();
    for (index, program) in programs.into_iter().enumerate() {
        let receiver = receiving[index];
        let provider = NeverProvider(domains[receiver].clone());
        let prepared = &program.source_files()[0];
        let syntax = prepared.preparsed_syntax();
        assert!(syntax
            .identity_domain()
            .unwrap()
            .same_domain(&domains[index]));
        let layout = Arc::clone(syntax.direct_layout().unwrap());
        assert_eq!(layout.len(), 1);
        let files = [InputFile::from_snapshot(
            prepared.path().display().to_owned(),
            Arc::clone(prepared.snapshot()),
        )
        .with_preparsed_syntax(syntax.clone())
        .with_js_doc_parsing_mode(Some(tsc_program::default_js_doc_parsing_mode()))];
        let convert = |mode| match mode {
            tsc_program::ResolutionMode::CommonJs => AuthoritativeResolutionMode::CommonJs,
            tsc_program::ResolutionMode::EsNext => AuthoritativeResolutionMode::EsNext,
            tsc_program::ResolutionMode::Unspecified => AuthoritativeResolutionMode::Unspecified,
        };
        let metadata = [AuthoritativeSourceMetadata {
            token: AuthoritativeSourceToken(1),
            file_name: files[0].name.clone(),
            may_be_emitted: true,
            implied_node_format: prepared.implied_node_format().map(convert),
            implied_node_format_for_emit: prepared.implied_node_format_for_emit().map(convert),
            project_reference: None,
        }];
        let mut observations = 0;
        check_program_with_authoritative_modules_at_for_emit_with_workers(
            &[],
            &files,
            &[],
            &metadata,
            &options,
            "/p",
            &provider,
            WorkerBudget::serial(),
            IncrementalRequest::NONE,
            false,
            SyntacticDiagnosticsGate::CheckEverySource,
            |snapshot, _, _| {
                observations += 1;
                let document = snapshot.document(0);
                assert!(document.source().identity_owned_by(&domains[receiver]));
                assert_eq!(
                    layout[0].matches(document.source()),
                    receiver == index,
                    "only a session of the placed domain may adopt Ready capabilities"
                );
                if receiver != index {
                    assert!(!document.source().identity_owned_by(&domains[index]));
                }
                kept.push(Arc::clone(document));
            },
        )
        .unwrap();
        assert_eq!(observations, 1);
        assert!(!syntax.is_available());
        assert!(
            syntax
                .identity_domain()
                .unwrap()
                .same_domain(&domains[index]),
            "consuming the tree must not erase the immutable handle's owner"
        );
    }
    for (index, document) in kept.iter().enumerate() {
        assert!(
            document
                .source()
                .identity_owned_by(&domains[receiving[index]]),
            "documents retain the receiving owner after prepared Programs have dropped"
        );
    }
}
