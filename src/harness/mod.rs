//! Fixture-directive parsing (`// @name: value`), mirrored by tools/directives.mjs.
//! Shared conformance data: tests/directive_cases.json.

use crate::options::CompilerOptions;

pub struct Fixture {
    pub files: Vec<(String, String)>,
    /// Raw (name, value) option directives, lowercase names.
    pub options: Vec<(String, String)>,
    /// `@extraRootFiles:` — root names with no materialized content.
    pub extra_root_files: Vec<String>,
    /// `@cliArgs:` — raw command-line tokens; the fixture exercises the
    /// arg-parsing layer instead of the normal option directives.
    pub cli_args: Option<Vec<String>>,
}

pub fn parse_fixture(source: &str) -> Result<Fixture, String> {
    let mut options: Vec<(String, String)> = Vec::new();
    let mut extra_root_files: Vec<String> = Vec::new();
    let mut cli_args: Option<Vec<String>> = None;
    let mut files: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    let mut in_header = true;
    let mut default_lines: Vec<&str> = Vec::new();

    for raw_line in source.split('\n') {
        // Match the Python oracle harness, whose text reads use universal
        // newlines before fixture sections are materialized. Keeping CR bytes
        // here shifts absolute tsc offsets while leaving line/column unchanged.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some((name, value)) = parse_directive_line(line) {
            if name == "filename" {
                if let Some(cur) = current.take() {
                    files.push(cur);
                }
                current = Some((value.to_string(), Vec::new()));
                continue;
            }
            if in_header && current.is_none() && name == "extrarootfiles" {
                extra_root_files.extend(
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
                continue;
            }
            if in_header && current.is_none() && name == "cliargs" {
                cli_args = Some(value.split_whitespace().map(|s| s.to_string()).collect());
                continue;
            }
            if in_header && current.is_none() {
                options.push((name, value.to_string()));
                continue;
            }
            // Option-looking comment inside content is plain content.
        }
        if let Some(cur) = current.as_mut() {
            cur.1.push(line);
        } else {
            if !line.trim().is_empty() {
                in_header = false;
            }
            if !in_header {
                default_lines.push(line);
            }
        }
    }
    if let Some(cur) = current.take() {
        files.push(cur);
    }
    if files.is_empty() {
        files.push(("main.ts".to_string(), default_lines));
    }
    Ok(Fixture {
        files: files
            .into_iter()
            .map(|(n, lines)| (n, lines.join("\n")))
            .collect(),
        options,
        extra_root_files,
        cli_args,
    })
}

fn parse_directive_line(line: &str) -> Option<(String, &str)> {
    let rest = line.strip_prefix("//")?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('@')?;
    let name_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if name_end == 0 || !rest.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let name = &rest[..name_end];
    let after = rest[name_end..].trim_start_matches([' ', '\t']);
    let value = after.strip_prefix(':')?;
    Some((name.to_ascii_lowercase(), value.trim()))
}

/// Apply parsed directives onto options. Unknown directive -> Err (fixtures are
/// strictly validated).
pub fn apply_directives(
    opts: &mut CompilerOptions,
    dirs: &[(String, String)],
) -> Result<(), String> {
    for (name, value) in dirs {
        let b = || -> Result<bool, String> {
            let first = value.split(',').next().unwrap_or(value).trim();
            match first {
                "true" | "*" => Ok(true),
                "false" => Ok(false),
                _ => Err(format!("bad bool for @{name}: {value}")),
            }
        };
        match name.as_str() {
            "strict" => opts.strict = Some(b()?),
            "strictnullchecks" => opts.strict_null_checks = Some(b()?),
            "strictfunctiontypes" => opts.strict_function_types = Some(b()?),
            "strictpropertyinitialization" => opts.strict_property_initialization = Some(b()?),
            "strictbindcallapply" => opts.strict_bind_call_apply = Some(b()?),
            "noimplicitany" => opts.no_implicit_any = Some(b()?),
            "noimplicitthis" => opts.no_implicit_this = Some(b()?),
            "useunknownincatchvariables" => opts.use_unknown_in_catch_variables = Some(b()?),
            "nounusedlocals" => opts.no_unused_locals = b()?,
            "nounusedparameters" => opts.no_unused_parameters = b()?,
            "noimplicitreturns" => opts.no_implicit_returns = b()?,
            "nofallthroughcasesinswitch" => opts.no_fallthrough_cases_in_switch = b()?,
            "exactoptionalpropertytypes" => opts.exact_optional_property_types = b()?,
            "nouncheckedindexedaccess" => opts.no_unchecked_indexed_access = b()?,
            "noimplicitoverride" => opts.no_implicit_override = b()?,
            "erasablesyntaxonly" => opts.erasable_syntax_only = b()?,
            "usedefineforclassfields" => opts.use_define_for_class_fields = Some(b()?),
            "allowunreachablecode" => opts.allow_unreachable_code = Some(b()?),
            "allowunusedlabels" => opts.allow_unused_labels = Some(b()?),
            "experimentaldecorators" => opts.experimental_decorators = b()?,
            "emitdecoratormetadata" => opts.emit_decorator_metadata = b()?,
            "sourcemap" => opts.source_map = b()?,
            "inlinesourcemap" => opts.inline_source_map = b()?,
            "inlinesources" => opts.inline_sources = b()?,
            "declaration" => opts.declaration = b()?,
            "declarationmap" => opts.declaration_map = b()?,
            "composite" => opts.composite = b()?,
            "isolateddeclarations" => opts.isolated_declarations = b()?,
            "allowimportingtsextensions" => opts.allow_importing_ts_extensions = b()?,
            "rewriterelativeimportextensions" => opts.rewrite_relative_import_extensions = b()?,
            "resolvepackagejsonexports" => opts.resolve_package_json_exports = Some(b()?),
            "resolvepackagejsonimports" => opts.resolve_package_json_imports = Some(b()?),
            "noemit" => opts.no_emit = b()?,
            "emitdeclarationonly" => opts.emit_declaration_only = b()?,
            "resolvejsonmodule" => opts.resolve_json_module = Some(b()?),
            "incremental" => opts.incremental = b()?,
            "isolatedmodules" => opts.isolated_modules = b()?,
            "verbatimmodulesyntax" => opts.verbatim_module_syntax = b()?,
            "preserveconstenums" => opts.preserve_const_enums = Some(b()?),
            "alwaysstrict" => opts.always_strict = Some(b()?),
            "esmoduleinterop" => opts.es_module_interop = Some(b()?),
            "allowsyntheticdefaultimports" => opts.allow_synthetic_default_imports = Some(b()?),
            "downleveliteration" => opts.downlevel_iteration = Some(b()?),
            "importhelpers" => opts.import_helpers = b()?,
            "noimplicitusestrict" => opts.no_implicit_use_strict = b()?,
            "keyofstringsonly" => opts.keyof_strings_only = b()?,
            "suppressexcesspropertyerrors" => opts.suppress_excess_property_errors = b()?,
            "suppressimplicitanyindexerrors" => opts.suppress_implicit_any_index_errors = b()?,
            "nostrictgenericchecks" => opts.no_strict_generic_checks = b()?,
            "preservevalueimports" => opts.preserve_value_imports = b()?,
            "skipdefaultlibcheck" | "skiplibcheck" => {
                let _ = b()?;
            }
            // Harness/compiler switches that tsrs does not model yet. Accept
            // them so directive parsing can still apply the options that do
            // matter for the current checker (notably @filename, @target, and
            // @module) instead of falling back to single-file defaults.
            "allowjs"
            | "checkjs"
            | "notypesandsymbols"
            | "noemithelpers"
            | "lib"
            | "outdir"
            | "noimplicitreferences"
            | "traceresolution"
            | "suppressoutputpathcheck"
            | "currentdirectory"
            | "typeroots"
            | "allowarbitraryextensions"
            | "nolib"
            | "noemitonerror"
            | "nopropertyaccessfromindexsignature"
            | "customconditions"
            | "maxnodemodulejsdepth"
            | "stripinternal"
            | "pretty"
            | "allowumdglobalaccess"
            | "nouncheckedsideeffectimports"
            | "moduledetection"
            | "removecomments"
            | "strictbuiltiniteratorreturn"
            | "libreplacement" => {}
            "declarationdir" => opts.declaration_dir = Some(value.clone()),
            "outfile" => opts.out_file = Some(value.clone()),
            "moduleresolution" => opts.module_resolution = Some(value.to_ascii_lowercase()),
            "tsbuildinfofile" => opts.ts_build_info_file = Some(value.clone()),
            "maproot" => opts.map_root = Some(value.clone()),
            "jsxfactory" => opts.jsx_factory = Some(value.clone()),
            "jsxfragmentfactory" => opts.jsx_fragment_factory = Some(value.clone()),
            "reactnamespace" => opts.react_namespace = Some(value.clone()),
            "jsximportsource" => opts.jsx_import_source = Some(value.clone()),
            "baseurl" => opts.base_url = Some(value.clone()),
            "charset" => opts.charset = Some(value.clone()),
            "out" => opts.out = Some(value.clone()),
            "importsnotusedasvalues" => {
                opts.imports_not_used_as_values = Some(value.to_ascii_lowercase())
            }
            "jsx" => opts.jsx = Some(value.to_ascii_lowercase()),
            "ignoredeprecations" => opts.ignore_deprecations = Some(value.clone()),
            "paths" => {
                opts.paths = Some(
                    crate::options::parse_paths_json(value)
                        .ok_or_else(|| format!("bad @paths JSON: {value}"))?,
                )
            }
            "target" => {
                // tsc's harness expands a comma-separated list into one variant
                // per value ("es5, es2015" ⇒ two runs); we treat the first as
                // the primary variant.
                let first = value.split(',').next().unwrap_or(value).trim();
                opts.target = Some(first.to_ascii_lowercase())
            }
            "module" => {
                // "@module: undefined" = explicit raw-unset (TS6131 branch);
                // overrides the harness's BASE module=commonjs. Comma-lists
                // (e.g. "es2022,esnext,system") pick the first variant, as
                // tsc's harness would.
                opts.module = if value == "undefined" {
                    None
                } else {
                    let first = value.split(',').next().unwrap_or(value).trim();
                    Some(first.to_ascii_lowercase())
                };
            }
            "rootdir" => opts.root_dir = Some(value.clone()),
            "types" => {
                opts.types = Some(
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                )
            }
            other => return Err(format!("unknown directive @{other}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_fixture;

    #[test]
    fn parse_fixture_normalizes_crlf_while_materializing_files() {
        let fixture = parse_fixture("// @target: ES5\r\nlet x = 1;\r\n").unwrap();
        assert_eq!(
            fixture.options,
            vec![("target".to_string(), "ES5".to_string())]
        );
        assert_eq!(
            fixture.files,
            vec![("main.ts".to_string(), "let x = 1;\n".to_string())]
        );
    }

    #[test]
    fn parse_fixture_normalizes_crlf_in_named_file_sections() {
        let fixture =
            parse_fixture("// @filename: a.ts\r\nlet a = 1;\r\n// @filename: b.ts\r\nlet b = 2;")
                .unwrap();
        assert_eq!(
            fixture.files,
            vec![
                ("a.ts".to_string(), "let a = 1;".to_string()),
                ("b.ts".to_string(), "let b = 2;".to_string()),
            ],
        );
    }
}
