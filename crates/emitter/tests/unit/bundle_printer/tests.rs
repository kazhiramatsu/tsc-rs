use std::path::{Path, PathBuf};

use serde_json::Value;
use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, ParseOptions, SourceFile};
use tsc_types::CompilerOptions;

use super::*;

struct Host {
    options: CompilerOptions,
    files: Vec<SourceFile>,
    ids: Vec<SourceFileId>,
    common: PathBuf,
    collision_queries: Vec<Value>,
    referenced_collision_queries: Vec<Value>,
}

impl Host {
    fn from_case(case: &Value) -> Self {
        let mut options = CompilerOptions::default();
        for (key, value) in case["options"].as_object().unwrap() {
            match key.as_str() {
                "target" => options.target = Some(value.as_i64().unwrap() as i32),
                "module" => options.module = Some(value.as_i64().unwrap() as i32),
                "outFile" => options.out_file = value.as_str().map(str::to_owned),
                "strict" => options.strict = value.as_bool(),
                "alwaysStrict" => options.always_strict = value.as_bool(),
                "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                "removeComments" => options.remove_comments = value.as_bool(),
                "noEmitHelpers" => options.no_emit_helpers = value.as_bool(),
                "importHelpers" => options.import_helpers = value.as_bool(),
                "allowJs" => options.allow_js = value.as_bool().unwrap(),
                "emitBOM" => options.emit_bom = value.as_bool(),
                "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
                other => panic!("unprojected option {other}"),
            }
        }
        let files = case["typescript_observation"]["source_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                let input = case["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|file| file["path"] == *name)
                    .unwrap();
                parse_source_file(
                    name.as_str().unwrap(),
                    input["text"].as_str().unwrap(),
                    ParseOptions {
                        script_target: options.emit_script_target(),
                        ..Default::default()
                    },
                    None,
                )
            })
            .collect::<Vec<_>>();
        let ids = (0..files.len())
            .map(|index| SourceFileId::from_raw(index as u32))
            .collect();
        Self {
            options,
            files,
            ids,
            common: PathBuf::from(
                case["typescript_observation"]["common_source_directory"]
                    .as_str()
                    .unwrap(),
            ),
            collision_queries: case["typescript_observation"]["collision_queries"]
                .as_array()
                .unwrap()
                .clone(),
            referenced_collision_queries: case["typescript_observation"]
                ["referenced_collision_queries"]
                .as_array()
                .unwrap()
                .clone(),
        }
    }
}

impl crate::EmitHost for Host {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }
    fn current_directory(&self) -> &Path {
        Path::new("/project")
    }
    fn common_source_directory(&self) -> &Path {
        &self.common
    }
    fn config_file_path(&self) -> Option<&Path> {
        None
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }
    fn source_file(&self, id: SourceFileId) -> Option<crate::EmitSource<'_>> {
        let syntax = self.files.get(id.index())?;
        let path = Path::new(&syntax.file_name);
        Some(crate::EmitSource::new(
            id,
            path,
            path,
            true,
            None,
            Some(syntax),
        ))
    }
}

impl crate::EmitResolver for Host {
    fn get_referenced_declaration_with_colliding_name(
        &self,
        node: crate::EmitResolverNode,
    ) -> Result<Option<crate::EmitResolverNode>, crate::EmitResolverError> {
        let syntax = &self.files[node.source().index()];
        let record = syntax.arena.node(node.node());
        let observed = self
            .referenced_collision_queries
            .iter()
            .find(|query| {
                query["path"] == syntax.file_name
                    && query["kind"] == format!("{:?}", record.kind)
                    && query["pos"] == record.pos
                    && query["end"] == record.end
            })
            .expect("referenced collision query must have a pinned checker observation");
        assert!(
            observed["value"].is_null(),
            "focused inputs have no colliding reference"
        );
        Ok(None)
    }

    fn is_declaration_with_colliding_name(
        &self,
        node: crate::EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        let syntax = &self.files[node.source().index()];
        let record = syntax.arena.node(node.node());
        let observed = self
            .collision_queries
            .iter()
            .find(|query| {
                query["path"] == syntax.file_name
                    && query["kind"] == format!("{:?}", record.kind)
                    && query["pos"] == record.pos
                    && query["end"] == record.end
            })
            .expect("collision query must have a pinned checker observation");
        Ok(observed["truthy"].as_bool().unwrap())
    }
}

fn compare_case(case: &Value) -> Result<(), String> {
    let host = Host::from_case(case);
    let mut arena = crate::TransformArena::new();
    let sources = host
        .files
        .iter()
        .zip(&host.ids)
        .map(|(file, id)| arena.add_source(file, Some(*id)))
        .collect();
    let transformers =
        crate::get_script_transformers_for_source(&host.options, &host, &host, host.ids[0])
            .map_err(|error| error.to_string())?;
    let mut transformation = crate::transform_nodes(
        arena,
        vec![crate::TransformRoot::Bundle(TransformBundle::new(sources))],
        transformers,
        false,
    )
    .map_err(|error| error.to_string())?;
    let crate::TransformRoot::Bundle(bundle) = transformation.roots()[0].clone() else {
        panic!("bundle root retained")
    };
    let newline = if host.options.new_line == Some(0) {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let options = PrinterOptions::new(newline)
        .with_target(host.options.emit_script_target())
        .with_module_kind(host.options.emit_module_kind())
        .with_remove_comments(host.options.remove_comments == Some(true))
        .with_no_emit_helpers(host.options.no_emit_helpers == Some(true))
        .with_import_helpers(host.options.import_helpers == Some(true));
    let printed = create_printer(options)
        .print(&mut transformation, PrintRequest::Bundle(bundle), None)
        .map_err(|error| error.to_string())?;
    let expected = &case["typescript_observation"]["writes"][0];
    if crate::execute::base64_encode(printed.text().as_bytes()) != expected["callback_utf8_base64"]
    {
        return Err(format!(
            "printed bytes differ: actual={:?}; expected_base64={}",
            printed.text(),
            expected["callback_utf8_base64"]
        ));
    }
    assert_eq!(
        printed.text().len() as u64,
        expected["callback_utf8_bytes"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(printed.end().position().value()),
        expected["end"]["position"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(printed.end().line()),
        expected["end"]["line"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(printed.end().column()),
        expected["end"]["character"].as_u64().unwrap()
    );
    assert!(printed.source_map().is_none());
    assert!(transformation.diagnostics().is_empty());
    Ok(())
}

#[test]
fn global_script_bundles_match_complete_typescript_output_bytes_twice() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/bundle-printer.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 28);
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        for repetition in 0..2 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare_case(case))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failures.push(format!("{} #{repetition}: {error}", case["case_id"]))
                }
                Err(_) => failures.push(format!(
                    "{} #{repetition}: unexpected resolver/input contract panic",
                    case["case_id"]
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn declaration_external_module_and_json_bundle_members_remain_typed_controls() {
    for (name, text) in [
        ("types.d.ts", "declare const value: number;"),
        ("external.ts", "export const value = 1;"),
        ("data.json", "{\"value\":1}"),
    ] {
        let source = parse_source_file(name, text, Default::default(), None);
        let mut arena = crate::TransformArena::new();
        let source = arena.add_source(&source, None);
        let bundle = TransformBundle::new(vec![source]);
        let mut result = crate::transform_nodes(
            arena,
            vec![crate::TransformRoot::Bundle(bundle.clone())],
            Vec::new(),
            false,
        )
        .unwrap();
        assert_eq!(
            create_printer(PrinterOptions::new(NewLineKind::LineFeed)).print(
                &mut result,
                PrintRequest::Bundle(bundle),
                None
            ),
            Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::BundleRoot
            ))
        );
    }
}
