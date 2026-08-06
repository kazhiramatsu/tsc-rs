use tsc_checker::{
    check_program_with_authoritative_modules_at, AuthoritativeModuleLookupFailure,
    AuthoritativeModuleProvider, AuthoritativeModuleRequest, AuthoritativeModuleResolution,
    AuthoritativeResolutionMode, AuthoritativeResolvedModule, AuthoritativeSourceMetadata,
    AuthoritativeSourceToken, CompilerOptions, InputFile,
};

const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

struct Provider {
    is_external_library_import: bool,
}

impl AuthoritativeModuleProvider for Provider {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        assert_eq!(request.source_token, AuthoritativeSourceToken(2));
        assert_eq!(request.containing_file, "/main.js");
        assert_eq!(request.specifier, "pkg");
        assert_eq!(request.mode, AuthoritativeResolutionMode::Unspecified);
        Ok(AuthoritativeModuleResolution::Resolved(
            AuthoritativeResolvedModule {
                target_token: AuthoritativeSourceToken(1),
                resolved_file_name: "/node_modules/pkg/index.js".to_owned(),
                resolved_using_ts_extension: false,
                is_tsx: false,
                is_arbitrary_extension: false,
                is_external_library_import: self.is_external_library_import,
                package_id: None,
                alternate_result: None,
                types_package_exists: false,
                package_bundles_types: false,
            },
        ))
    }
}

fn metadata(
    token: u32,
    file_name: &str,
    may_be_emitted: bool,
    implied_node_format: Option<AuthoritativeResolutionMode>,
) -> AuthoritativeSourceMetadata {
    AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(token),
        file_name: file_name.to_owned(),
        may_be_emitted,
        implied_node_format,
        implied_node_format_for_emit: implied_node_format,
    }
}

#[test]
fn authoritative_external_fact_alone_controls_checked_js_implicit_any_suggestion() {
    let libs = [InputFile::new(
        "/lib.d.ts".to_owned(),
        MINIMAL_GLOBALS.to_owned(),
    )];
    let lib_metadata = [metadata(0, "/lib.d.ts", false, None)];
    let files = [
        InputFile::new(
            "/node_modules/pkg/index.js".to_owned(),
            "module.exports = {};\n".to_owned(),
        ),
        InputFile::new(
            "/main.js".to_owned(),
            "const value = require('pkg');\nvalue.answer = 42;\n".to_owned(),
        ),
    ];
    let file_metadata = [
        metadata(
            1,
            "/node_modules/pkg/index.js",
            true,
            Some(AuthoritativeResolutionMode::CommonJs),
        ),
        metadata(
            2,
            "/main.js",
            true,
            Some(AuthoritativeResolutionMode::CommonJs),
        ),
    ];
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };

    for (is_external_library_import, expected_7016) in [(false, 0), (true, 1)] {
        let result = check_program_with_authoritative_modules_at(
            &libs,
            &files,
            &lib_metadata,
            &file_metadata,
            &options,
            "/",
            &Provider {
                is_external_library_import,
            },
        )
        .expect("run authoritative checked-JS program");
        assert_eq!(
            result
                .suggestion_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 7016)
                .count(),
            expected_7016,
            "the node_modules spelling must not override the authoritative external fact"
        );
    }
}
