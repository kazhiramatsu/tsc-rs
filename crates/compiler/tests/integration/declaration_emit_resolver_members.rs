//! Declaration-emit resolver members: global names, bind/check diagnostic
//! ownership and linked-alias collection through the checker's emit resolver.

use tsc_checker::emit::CheckerSession;
use tsc_checker::{
    check_program_with_authoritative_modules_at_for_emit, AuthoritativeModuleLookupFailure,
    AuthoritativeModuleProvider, AuthoritativeModuleRequest, AuthoritativeModuleResolution,
    AuthoritativeSourceMetadata, AuthoritativeSourceToken, InputFile, ProgramSnapshot,
};
use tsc_emitter::{EmitResolver, EmitResolverNode, SourceFileId};

struct NoModuleRequests;

impl AuthoritativeModuleProvider for NoModuleRequests {
    fn resolve_module(
        &self,
        _request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        Err(AuthoritativeModuleLookupFailure::Missing)
    }
}

fn with_focused_emit_resolver(
    files: &[(&str, &str)],
    options: &tsc_checker::CompilerOptions,
    mut operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>),
) {
    let inputs = files
        .iter()
        .map(|(name, text)| InputFile::new(*name, *text))
        .collect::<Vec<_>>();
    let metadata = files
        .iter()
        .enumerate()
        .map(|(index, (name, _))| AuthoritativeSourceMetadata {
            token: AuthoritativeSourceToken(index as u32),
            file_name: (*name).to_owned().into(),
            may_be_emitted: true,
            implied_node_format: None,
            implied_node_format_for_emit: None,
            project_reference: None,
        })
        .collect::<Vec<_>>();
    check_program_with_authoritative_modules_at_for_emit(
        &[],
        &inputs,
        &[],
        &metadata,
        options,
        "/project",
        &NoModuleRequests,
        |snapshot, resolver, checked| {
            assert!(checked.partial_checks.is_empty());
            operation(snapshot, resolver);
        },
    )
    .expect("focused checker session");
}

#[test]
fn declaration_emit_resolver_members_forward_values_and_source_ownership() {
    let files = [
        ("/project/global.ts", "var GlobalName: number;\n"),
        (
            "/project/aliases.ts",
            concat!(
                "namespace N { export class Value {} }\n",
                "import Alias = N.Value;\n",
                "export = Alias;\n",
            ),
        ),
        ("/project/plain.js", "module.exports = 1;\n"),
        ("/project/nocheck.ts", "// @ts-nocheck\nlet value = 1;\n"),
        ("/project/nocheck.js", "// @ts-nocheck\nlet value = 1;\n"),
    ];
    with_focused_emit_resolver(
        &files,
        &tsc_checker::CompilerOptions {
            allow_js: true,
            ..tsc_checker::CompilerOptions::default()
        },
        |snapshot, resolver| {
            assert!(resolver.has_global_name("GlobalName").unwrap());
            assert!(!resolver.has_global_name("MissingGlobal").unwrap());

            assert!(resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(0))
                .unwrap());
            assert!(resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(2))
                .unwrap());
            assert!(!resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(3))
                .unwrap());
            assert!(!resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(4))
                .unwrap());

            let alias_source = SourceFileId::from_raw(1);
            let (export_name, linked) = snapshot.documents()[1]
                .source()
                .arena
                .node_ids()
                .find_map(|node| {
                    resolver
                        .collect_linked_aliases(EmitResolverNode::new(alias_source, node), false)
                        .unwrap()
                        .filter(|linked| !linked.is_empty())
                        .map(|linked| (node, linked))
                })
                .expect("export assignment links its internal alias");
            assert!(linked.iter().all(|node| node.source() == alias_source));
            assert_eq!(
                resolver
                    .collect_linked_aliases(EmitResolverNode::new(alias_source, export_name), true,)
                    .unwrap(),
                None,
            );
        },
    );

    with_focused_emit_resolver(
        &[("/project/check-js.js", "let value = 1;\n")],
        &tsc_checker::CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..tsc_checker::CompilerOptions::default()
        },
        |_, resolver| {
            assert!(resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(0))
                .unwrap());
        },
    );
    with_focused_emit_resolver(
        &[
            ("/project/no-check-js.js", "let value = 1;\n"),
            ("/project/ts-check.js", "// @ts-check\nlet value = 1;\n"),
        ],
        &tsc_checker::CompilerOptions {
            allow_js: true,
            check_js: Some(false),
            ..tsc_checker::CompilerOptions::default()
        },
        |_, resolver| {
            assert!(!resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(0))
                .unwrap());
            assert!(resolver
                .can_include_bind_and_check_diagnostics(SourceFileId::from_raw(1))
                .unwrap());
        },
    );
}
