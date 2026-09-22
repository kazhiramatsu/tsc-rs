use std::path::Path;
use tsc_emitter::{create_printer, get_script_transformers_for_source, transform_nodes, EmitHost, EmitSource, EmitResolver, EmitResolverError, EmitResolverNode, EmitConstantValue, EmitExportContainerMode, EmitTypeReferenceSerializationKind, TransformArena, TransformRoot, NewLineKind, PrinterOptions, PrintRequest};
use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, ParseOptions};
use tsc_types::{CompilerOptions, ScriptTarget, ModuleKind};
struct TransformContractHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    source_ids: [SourceFileId; 1],
}

impl EmitHost for TransformContractHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn common_source_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn config_file_path(&self) -> Option<tsc_diagnostics::JsStr<'_>> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        (id == self.source_ids[0]).then(|| {
            let path = self.syntax.file_name.as_js();
            EmitSource::new(id, path, path, true, None, Some(self.syntax))
        })
    }
}

struct SystemContractResolver;

impl EmitResolver for SystemContractResolver {
    fn is_referenced_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn is_value_alias_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(true)
    }

    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_import_declaration_at_location(
        &self,
        _node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_value_declaration(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn get_type_reference_serialization_kind(
        &self,
        _node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<EmitTypeReferenceSerializationKind, EmitResolverError> {
        Ok(EmitTypeReferenceSerializationKind::Unknown)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}


fn main() {
 let text=std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
 for module in [ModuleKind::COMMON_JS, ModuleKind::AMD, ModuleKind::UMD, ModuleKind::SYSTEM] {
  let mut parsed=parse_source_file("main.ts",text.clone(),ParseOptions{script_target:ScriptTarget::ES5,..Default::default()},None);
  println!("MODULE {module:?} BEFORE_CLEAR {:?}", parsed.parse_recovery());
  parsed.discard_parse_recovery_for_harness();
  let mut arena=TransformArena::new();let id=SourceFileId::from_raw(0);let source=arena.add_source(&parsed,Some(id));
  let options=CompilerOptions{target:Some(ScriptTarget::ES5.bits()),module:Some(module.bits()),always_strict:Some(true),..Default::default()};
  let host=TransformContractHost{options:&options,syntax:&parsed,source_ids:[id]};
  let f=get_script_transformers_for_source(&options,&SystemContractResolver,&host,id).unwrap();
  match transform_nodes(arena,vec![TransformRoot::SourceFile(source)],f,false){
   Ok(mut result)=>match create_printer(PrinterOptions::new(NewLineKind::CarriageReturnLineFeed).with_target(ScriptTarget::ES5)).print(&mut result,PrintRequest::SourceFile(source),None){
    Ok(o)=>println!("MOCK_HARNESS_OUTPUT_BEGIN\n{}\nMOCK_HARNESS_OUTPUT_END",o.text()),Err(e)=>println!("PRINT_ERROR {e:?}")},
   Err(e)=>println!("TRANSFORM_ERROR {e:?}")
  }
 }
}
