#!/usr/bin/env python3
"""r11 patch script, part 2 (after part 1): EF7-PRESERVE-CJS-HELPERS — transformECMAScriptModule's
import-equals helpers form for CommonJS-format files (`module: preserve` + `.cts`/`.cjs`), with the
emit-time `tslib_1.<helper>` substitution. usage: python3 r11-patches-2.py <tree>"""
import sys, os
ROOT = sys.argv[1]
def patch(rel, old, new, count=1):
    path = os.path.join(ROOT, rel)
    s = open(path).read()
    if new in s:
        print(f"  already applied: {rel}: {old.strip().splitlines()[0][:60]}")
        return
    n = s.count(old)
    assert n == count, f"{rel}: expected {count} occurrence(s) of anchor, found {n}: {old[:80]!r}"
    open(path, "w").write(s.replace(old, new))
    print(f"  patched {rel}: {old.strip().splitlines()[0][:60]}")

B = "crates/emitter/src/builtins.rs"
patch(B,
"""struct EcmaScriptModuleTransformer {
    preserve_jsx: bool,
    rewrite_calls: relative_imports::ImportCallRewrites,
    module_kind: i32,
    rewrite_relative_import_extensions: bool,
    import_helpers: bool,
""",
"""struct EcmaScriptModuleTransformer<'host> {
    preserve_jsx: bool,
    rewrite_calls: relative_imports::ImportCallRewrites,
    module_kind: i32,
    rewrite_relative_import_extensions: bool,
    import_helpers: bool,
    /// `context.getEmitHost().getEmitModuleFormatOfFile(file)`: selects the
    /// import-equals helpers form for a CommonJS-format file
    /// (EF7-PRESERVE-CJS-HELPERS). Hosts without a Program see the compiler
    /// module kind only.
    host: Option<&'host dyn EmitHost>,
    /// The source whose helper references are being qualified at print time.
    current_source: Option<TransformSourceId>,
""")
patch(B,
"""impl Transformer for EcmaScriptModuleTransformer {
    fn name(&self) -> &'static str {
        "transformECMAScriptModule"
    }
""",
"""impl Transformer for EcmaScriptModuleTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformECMAScriptModule"
    }
""")
patch(B,
"""pub fn transform_ecmascript_module(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(EcmaScriptModuleTransformer {
        module_kind: options.emit_module_kind(),
        preserve_jsx: options.jsx == Some(1),
        rewrite_calls: relative_imports::ImportCallRewrites::default(),
        rewrite_relative_import_extensions: options
            .rewrite_relative_import_extensions
            .unwrap_or(false),
        import_helpers: options.import_helpers == Some(true),
        target: options.emit_script_target(),
    })
}
""",
"""pub fn transform_ecmascript_module(options: &CompilerOptions) -> Box<dyn Transformer> {
    transform_ecmascript_module_with_host(options, None)
}

/// transformECMAScriptModule with the emit host that decides a file's helpers
/// import form (EF7-PRESERVE-CJS-HELPERS); the host-less entry above keeps the
/// H1 omission-inventory anchor.
pub fn transform_ecmascript_module_with_host<'host>(
    options: &CompilerOptions,
    host: Option<&'host dyn EmitHost>,
) -> Box<dyn Transformer + 'host> {
    Box::new(EcmaScriptModuleTransformer {
        module_kind: options.emit_module_kind(),
        preserve_jsx: options.jsx == Some(1),
        rewrite_calls: relative_imports::ImportCallRewrites::default(),
        rewrite_relative_import_extensions: options
            .rewrite_relative_import_extensions
            .unwrap_or(false),
        import_helpers: options.import_helpers == Some(true),
        host,
        current_source: None,
        target: options.emit_script_target(),
    })
}
""")
patch(B,
"""            activity.construct_transform_ecmascript_module();
            transform_ecmascript_module(options)
        }
""",
"""            activity.construct_transform_ecmascript_module();
            transform_ecmascript_module_with_host(options, host.map(|(host, _)| host))
        }
""")
patch(B,
"""    activity.construct_transform_ecmascript_module();
    let esm = transform_ecmascript_module(options);
""",
"""    activity.construct_transform_ecmascript_module();
    let esm = transform_ecmascript_module_with_host(options, Some(host));
""")
patch(B,
"""        if was_external {
            let current_root = context.arena().root(source)?;
            let mut visitor =
                EcmaScriptModuleEqualsVisitor::new(context, source, self.module_kind, self.target);
            let rewritten = visitor.transform_source_file(current_root)?;
            visitor
                .context
                .arena_mut()?
                .replace_root(source, rewritten)?;
        }
        if was_external && self.import_helpers {
            insert_external_helpers_import_declaration(context, source)?;
        }
""",
"""        if was_external && self.import_helpers {
            // createExternalHelpersImportDeclarationIfNeeded
            // (_tsc.js:27636-27692): the import-equals form for a
            // CommonJS-format file (`impliedModuleKind === CommonJS`), the
            // named-import form otherwise (`moduleKind` ES2015..ESNext, an
            // ESNext-format file, or `module: preserve` without an implied
            // format). The declaration is visited by the module visitor below
            // (`visitArray([declaration], visitor)`, _tsc.js:113396), which
            // turns the import-equals form into `const tslib_1 =
            // require("tslib")`.
            let format = self.host.and_then(|host| {
                context
                    .arena()
                    .source(source)
                    .ok()?
                    .program_source()
                    .and_then(|program_source| host.get_emit_module_format_of_file(program_source))
            });
            let named_imports = format != Some(MODULE_COMMON_JS)
                && ((MODULE_ES2015..=MODULE_ES_NEXT).contains(&self.module_kind)
                    || format == Some(MODULE_ES_NEXT)
                    || self.module_kind == MODULE_PRESERVE);
            if named_imports {
                insert_external_helpers_import_declaration(context, source)?;
            } else {
                insert_external_helpers_import_equals_declaration(context, source)?;
            }
        }
        if was_external {
            let current_root = context.arena().root(source)?;
            let mut visitor =
                EcmaScriptModuleEqualsVisitor::new(context, source, self.module_kind, self.target);
            let rewritten = visitor.transform_source_file(current_root)?;
            visitor
                .context
                .arena_mut()?
                .replace_root(source, rewritten)?;
        }
""")
patch(B,
"""    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }
}

struct EcmaScriptModuleEqualsVisitor<'context> {
""",
"""    /// substituteHelperName (transformECMAScriptModule): a helper reference
    /// in a file that imports `tslib_1 = require("tslib")` prints as
    /// `tslib_1.<helper>`.
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if hint != EmitHint::Expression
            || context.arena().node(node)?.kind != SyntaxKind::Identifier
            || !context
                .arena()
                .metadata(node)
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::HELPER_NAME))
        {
            return Ok(node);
        }
        let Some(source) = self.current_source else {
            return Ok(node);
        };
        let Some(namespace) = get_external_helpers_module_name(context.arena(), source)? else {
            return Ok(node);
        };
        let final_name = context
            .arena()
            .metadata(namespace)
            .and_then(crate::EmitMetadata::generated_binding_id)
            .and_then(|binding| context.generated_binding_name(binding))
            .map(str::to_owned);
        if let Some(final_name) = final_name {
            context
                .arena_mut()?
                .set_generated_identifier_text(namespace, &final_name)?;
        }
        context
            .substitution_factory()?
            .create_property_access_expression(source, namespace, node)
    }

    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if context.arena().node(node)?.kind == SyntaxKind::SourceFile {
            self.current_source = Some(node.source());
        }
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if context.arena().node(node)?.kind == SyntaxKind::SourceFile {
            self.current_source = None;
        }
        Ok(())
    }

    fn dispose(&mut self) {
        self.current_source = None;
    }
}

struct EcmaScriptModuleEqualsVisitor<'context> {
""")
patch(B,
"""    context
        .arena_mut()?
        .metadata_mut(declaration)
        .set_internal_flags(InternalEmitFlags::NEVER_APPLY_IMPORT_HELPER);
    let root_node = context.arena().root(source)?;
    let NodeData::SourceFile(mut source_data) = context.arena().node(root_node)?.data.clone()
    else {
""",
"""    context
        .arena_mut()?
        .metadata_mut(declaration)
        .set_internal_flags(InternalEmitFlags::NEVER_APPLY_IMPORT_HELPER);
    insert_external_helpers_statement(context, source, declaration)
}

/// The import-equals arm of createExternalHelpersImportDeclarationIfNeeded
/// (_tsc.js:27680-27692) for a CommonJS-format file that
/// transformECMAScriptModule emits (`module: preserve` with a `.cts`/`.cjs`
/// file): `import tslib_1 = require("tslib")`, which the module visitor
/// prints as `const tslib_1 = require("tslib")`, and helper references
/// become `tslib_1.<helper>` (EF7-PRESERVE-CJS-HELPERS).
fn insert_external_helpers_import_equals_declaration(
    context: &mut TransformationContext,
    source: TransformSourceId,
) -> Result<(), TransformError> {
    let has_helpers = context
        .requested_emit_helpers()
        .iter()
        .any(|helper| !helper.scoped());
    if !has_helpers {
        return Ok(());
    }
    let namespace = match get_external_helpers_module_name(context.arena(), source)? {
        Some(namespace) => namespace,
        None => {
            let namespace = context.factory()?.create_unique_name(
                source,
                "tslib",
                crate::GeneratedIdentifierFlags::NONE,
            )?;
            let original = original_source_file_node(context.arena(), source)?;
            context
                .arena_mut()?
                .metadata_mut(original)
                .external_helpers_module_name = Some(namespace);
            namespace
        }
    };
    let specifier = context
        .factory()?
        .create_string_literal(source, "tslib", false)?;
    let reference = context
        .factory()?
        .create_external_module_reference(source, specifier)?;
    let declaration = context
        .factory()?
        .create_import_equals_declaration(source, None, false, namespace, reference)?;
    context
        .arena_mut()?
        .metadata_mut(declaration)
        .set_internal_flags(InternalEmitFlags::NEVER_APPLY_IMPORT_HELPER);
    insert_external_helpers_statement(context, source, declaration)
}

/// `factory2.copyPrologue(node.statements, statements)` + the helpers import,
/// then the remaining statements (updateExternalModule, _tsc.js:113392-113402).
fn insert_external_helpers_statement(
    context: &mut TransformationContext,
    source: TransformSourceId,
    declaration: TransformNode,
) -> Result<(), TransformError> {
    let root_node = context.arena().root(source)?;
    let NodeData::SourceFile(mut source_data) = context.arena().node(root_node)?.data.clone()
    else {
""")
print("r11 part 2 applied")
