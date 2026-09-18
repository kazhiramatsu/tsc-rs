#!/usr/bin/env python3
"""EF6-UMD-FACTORY: emit-resolver import/export declarations are projected with their own source file
(project_resolver_node), never stamped with the referencing node's source."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/checker/src/emit.rs"
s = open(p).read()
def rep(old, new, label):
    global s
    assert s.count(old) == 1, label
    s = s.replace(old, new)
rep("""        self.with_resolver_node(
            EmitResolverMethod::GetReferencedExportContainer,
            node,
            |state, reference| state.emit_get_referenced_export_container(reference, mode),
        )
        .map(|container| container.map(|container| EmitResolverNode::new(node.source(), container)))
    }
""", """        // A container or declaration may live in another file (a UMD
        // `export as namespace` alias resolved from a JSX factory, an alias
        // imported through a different source): project it with its own
        // source so the emitter's same-source guards see the truth.
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedExportContainer,
            node,
            |state, reference| {
                let container = state.emit_get_referenced_export_container(reference, mode)?;
                Ok(container.map(|container| project_resolver_node(state, container)))
            },
        )
    }
""", "export container")
rep("""        self.with_resolver_node(
            EmitResolverMethod::GetReferencedImportDeclaration,
            node,
            CheckerState::emit_get_referenced_import_declaration,
        )
        .map(|declaration| {
            declaration.map(|declaration| EmitResolverNode::new(node.source(), declaration))
        })
    }
""", """        self.with_resolver_node(
            EmitResolverMethod::GetReferencedImportDeclaration,
            node,
            |state, reference| {
                let declaration = state.emit_get_referenced_import_declaration(reference)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }
""", "import declaration")
rep("""        self.with_resolver_node_and_location(
            EmitResolverMethod::GetReferencedImportDeclarationAtLocation,
            node,
            location,
            CheckerState::emit_get_referenced_import_declaration_at_location,
        )
        .map(|declaration| {
            declaration.map(|declaration| EmitResolverNode::new(node.source(), declaration))
        })
    }
""", """        self.with_resolver_node_and_location(
            EmitResolverMethod::GetReferencedImportDeclarationAtLocation,
            node,
            location,
            |state, reference, location| {
                let declaration =
                    state.emit_get_referenced_import_declaration_at_location(reference, location)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }
""", "at location")
rep("""        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryImportDeclaration,
            node,
            |state, location| state.emit_get_jsx_factory_import_declaration(location, name),
        )
        .map(|declaration| {
            declaration.map(|declaration| EmitResolverNode::new(node.source(), declaration))
        })
    }
""", """        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryImportDeclaration,
            node,
            |state, location| {
                let declaration = state.emit_get_jsx_factory_import_declaration(location, name)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }
""", "jsx import")
rep("""        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryExportContainer,
            node,
            |state, location| state.emit_get_jsx_factory_export_container(location, name),
        )
        .map(|container| container.map(|container| EmitResolverNode::new(node.source(), container)))
    }
""", """        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryExportContainer,
            node,
            |state, location| {
                let container = state.emit_get_jsx_factory_export_container(location, name)?;
                Ok(container.map(|container| project_resolver_node(state, container)))
            },
        )
    }
""", "jsx export container")
open(p, "w").write(s)
# Same-source guards in the module transformers' import-binding lookups (node ids are per-source).
for path, old, new in [
 ("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins.rs",
  """        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?;
        Ok(declaration.and_then(|declaration| {
            if let Some(export) = self
                .info
                .imports""",
  """        // A declaration in another source (a UMD `export as namespace`
        // alias) is not an import binding of this module.
        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?
            .filter(|declaration| declaration.source() == resolver_node.source());
        Ok(declaration.and_then(|declaration| {
            if let Some(export) = self
                .info
                .imports"""),
 ("/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/system.rs",
  """        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?;
        Ok(declaration.and_then(|declaration| {
            self.info
                .common
                .import_bindings""",
  """        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?
            .filter(|declaration| declaration.source() == resolver_node.source());
        Ok(declaration.and_then(|declaration| {
            self.info
                .common
                .import_bindings"""),
]:
    t = open(path).read(); assert t.count(old) == 1, path; open(path, "w").write(t.replace(old, new))
print("EF6-UMD patch applied")
