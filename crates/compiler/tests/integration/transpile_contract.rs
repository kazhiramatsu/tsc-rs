//! `transpile_module` / `transpile_declaration` pinned to TypeScript 7.1's
//! `transpile.TranspileModule` / `TranspileDeclaration`: the expected texts
//! are the outputs in tsgo's transpile baselines
//! (`testdata/baselines/reference/transpile`, vendored with the profile).

use tsc_compiler::transpile::{
    transpile_compiler_options, transpile_declaration, transpile_module, TranspileOptions,
};
use tsc_program::CompilerOptions;

fn options(file_name: &str, compiler_options: CompilerOptions) -> TranspileOptions {
    TranspileOptions {
        compiler_options: Some(compiler_options),
        file_name: Some(file_name.to_owned()),
        report_diagnostics: false,
    }
}

fn diagnostic_rows(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<(String, Option<u32>, u32)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic
                    .file_name
                    .as_ref()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                diagnostic.start,
                diagnostic.code(),
            )
        })
        .collect()
}

#[test]
fn the_worker_clears_and_forces_options() {
    let caller = CompilerOptions {
        declaration: Some(true),
        declaration_map: Some(true),
        emit_declaration_only: Some(true),
        no_emit: Some(true),
        incremental: Some(true),
        composite: Some(true),
        out_file: Some("out.js".into()),
        no_emit_on_error: Some(true),
        allow_importing_ts_extensions: Some(true),
        verbatim_module_syntax: Some(true),
        ..CompilerOptions::default()
    };
    let module = transpile_compiler_options(Some(&caller), false);
    assert_eq!(module.declaration, Some(false));
    assert_eq!(module.declaration_map, Some(false));
    assert_eq!(module.isolated_declarations, Some(false));
    assert_eq!(module.emit_declaration_only, None);
    assert_eq!(module.no_emit, None);
    assert_eq!(module.incremental, None);
    assert_eq!(module.composite, None);
    assert_eq!(module.out_file, None);
    assert_eq!(module.no_emit_on_error, None);
    assert_eq!(module.allow_importing_ts_extensions, None);
    // verbatimModuleSyntax makes isolatedModules redundant.
    assert_eq!(module.isolated_modules, None);
    assert_eq!(module.no_check, Some(true));
    assert_eq!(module.no_resolve, Some(true));
    assert_eq!(module.suppress_output_path_check, Some(true));
    assert_eq!(module.allow_non_ts_extensions, Some(true));

    let declaration = transpile_compiler_options(None, true);
    assert_eq!(declaration.declaration, Some(true));
    assert_eq!(declaration.emit_declaration_only, Some(true));
    assert_eq!(declaration.isolated_declarations, Some(true));
    assert_eq!(declaration.isolated_modules, Some(true));
}

// transpile/noModuleResolution.{js,d.ts}: no module or lib reference is
// resolved, and the declaration route reads only its barebones library.
#[test]
fn nothing_is_resolved() {
    let input = "/// <reference lib=\"dom\" />\n\nexport { x } from \"./does-not-exist\";\nexport const value: number = 1;\n";
    let compiler_options = CompilerOptions {
        declaration: Some(true),
        lib_replacement: Some(true),
        ..CompilerOptions::default()
    };
    let module = transpile_module(
        input,
        &options("noModuleResolution.ts", compiler_options.clone()),
    )
    .expect("transpileModule");
    assert_eq!(
        module.output_text,
        "/// <reference lib=\"dom\" />\nexport { x } from \"./does-not-exist\";\nexport const value = 1;\n"
    );
    assert!(module.diagnostics.is_empty());
    assert_eq!(module.source_map_text, None);
    let declaration =
        transpile_declaration(input, &options("noModuleResolution.ts", compiler_options))
            .expect("transpileDeclaration");
    assert_eq!(
        declaration.output_text,
        "export { x } from \"./does-not-exist\";\nexport declare const value: number;\n"
    );
    assert!(declaration.diagnostics.is_empty());
}

// transpile/declarationSingleFileHasErrors{,Reported}.{js,d.ts}: the emit's
// isolated-declaration error is always reported, the syntax error only on
// request, located in the input rooted at `/`.
#[test]
fn emit_diagnostics_are_always_reported() {
    let compiler_options = CompilerOptions {
        target: Some(2),
        module: Some(1),
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    let input = "export const a number = \"missing colon\";";
    let file_name = "declarationSingleFileHasErrors.ts";
    let declaration = transpile_declaration(input, &options(file_name, compiler_options.clone()))
        .expect("transpileDeclaration");
    assert_eq!(
        declaration.output_text,
        "export declare const a: any, number = \"missing colon\";\n"
    );
    assert_eq!(
        diagnostic_rows(&declaration.diagnostics),
        vec![(
            "/declarationSingleFileHasErrors.ts".to_owned(),
            Some(13),
            9010
        )]
    );
    let module = transpile_module(input, &options(file_name, compiler_options.clone()))
        .expect("transpileModule");
    assert!(module.diagnostics.is_empty());

    let reported = TranspileOptions {
        report_diagnostics: true,
        ..options(file_name, compiler_options)
    };
    let module = transpile_module(input, &reported).expect("transpileModule");
    assert_eq!(
        module.output_text,
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports.number = exports.a = void 0;\nexports.number = \"missing colon\";\n"
    );
    assert_eq!(
        diagnostic_rows(&module.diagnostics),
        vec![(
            "/declarationSingleFileHasErrors.ts".to_owned(),
            Some(15),
            1005
        )]
    );
}

// Without a file name the input is `/module.ts`, or `/module.tsx` under jsx.
#[test]
fn the_default_file_name_follows_jsx() {
    let input = "export const a number = 1;";
    let reported = |jsx| TranspileOptions {
        compiler_options: Some(CompilerOptions {
            jsx,
            ..CompilerOptions::default()
        }),
        file_name: None,
        report_diagnostics: true,
    };
    let plain = transpile_module(input, &reported(None)).expect("transpileModule");
    assert_eq!(diagnostic_rows(&plain.diagnostics)[0].0, "/module.ts");
    let jsx = transpile_module(input, &reported(Some(2))).expect("transpileModule");
    assert_eq!(diagnostic_rows(&jsx.diagnostics)[0].0, "/module.tsx");
}

// transpile/jsWithSourceMapBasic(sourceMap=true).js, namespace.ts: an
// exported import alias in a namespace maps its member name and the whole
// declaration (runtimesyntax.go createExportStatement).
#[test]
fn namespace_import_alias_maps_like_tsgo() {
    let input = "export namespace ns {\n    namespace internal {\n        export class Foo {}\n    }\n    export namespace nested {\n        export import inner = internal;\n    }\n}";
    let output = transpile_module(
        input,
        &options(
            "namespace.ts",
            CompilerOptions {
                target: Some(2),
                source_map: Some(true),
                ..CompilerOptions::default()
            },
        ),
    )
    .expect("transpileModule");
    assert_eq!(
        output.output_text,
        "export var ns;\n(function (ns) {\n    let internal;\n    (function (internal) {\n        class Foo {\n        }\n        internal.Foo = Foo;\n    })(internal || (internal = {}));\n    let nested;\n    (function (nested) {\n        nested.inner = internal;\n    })(nested = ns.nested || (ns.nested = {}));\n})(ns || (ns = {}));\n//# sourceMappingURL=namespace.js.map"
    );
    assert_eq!(
        output.source_map_text.as_deref(),
        Some("{\"version\":3,\"file\":\"namespace.js\",\"sourceRoot\":\"\",\"sources\":[\"namespace.ts\"],\"names\":[],\"mappings\":\"AAAA,MAAM,KAAW,EAAE,CAOlB;AAPD,WAAiB,EAAE;IACf,IAAU,QAAQ,CAEjB;IAFD,WAAU,QAAQ;QACd,MAAa,GAAG;SAAG;QAAN,SAAA,GAAG,MAAG,CAAA;IACvB,CAAC,EAFS,QAAQ,KAAR,QAAQ,QAEjB;IACD,IAAiB,MAAM,CAEtB;IAFD,WAAiB,MAAM;QACL,OAAA,KAAK,GAAG,QAAS,CAAA;IACnC,CAAC,EAFgB,MAAM,GAAN,GAAA,MAAM,KAAN,GAAA,MAAM,QAEtB;AACL,CAAC,EAPgB,EAAE,KAAF,EAAE,QAOlB\"}")
    );
}

// transpile/jsWithSourceMapBasic(sourceMap=true).js, variables.ts: the
// assignment of a hoisted exported `using` maps the binding name's own span
// (using.go hoistInitializedVariable clones the name with its location).
#[test]
fn hoisted_using_binding_maps_like_tsgo() {
    let input = "export const a = 1;\nexport let b = 2;\nexport var c = 3;\nusing d = undefined;\nexport { d };\nawait using e = undefined;\nexport { e };";
    let output = transpile_module(
        input,
        &options(
            "variables.ts",
            CompilerOptions {
                target: Some(2),
                source_map: Some(true),
                ..CompilerOptions::default()
            },
        ),
    )
    .expect("transpileModule");
    assert_eq!(
        output.source_map_text.as_deref(),
        Some("{\"version\":3,\"file\":\"variables.js\",\"sourceRoot\":\"\",\"sources\":[\"variables.ts\"],\"names\":[],\"mappings\":\";;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;AAAA,MAAM,CAAC,MAAM,CAAC,GAAG,CAAC,CAAC;AACnB,MAAM,CAAC,IAAI,CAAC,GAAG,CAAC,CAAC;AACjB,MAAM,CAAC,IAAI,CAAC,GAAG,CAAC,CAAC;AAEjB,OAAO,EAAE,CAAC,EAAE,CAAC;AAEb,OAAO,EAAE,CAAC,EAAE,CAAC;;;;IAHP,CAAC,kCAAG,SAAS,QAAA,CAAC;IAER,CAAC,kCAAG,SAAS,OAAA,CAAC\"}")
    );
}
