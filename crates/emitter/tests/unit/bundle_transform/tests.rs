//! Pinned chainBundle phase ordering and per-source built-in helper ownership.
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use serde_json::{json, Value};
use tsc_program::SourceFileId;
use tsc_syntax::parse_source_file;
use tsc_types::CompilerOptions;

use super::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../fixtures/bundle-transform.json")).unwrap()
}

struct TraceTransformer {
    label: &'static str,
    log: Rc<RefCell<Vec<String>>>,
    replace_source: Option<String>,
    replace_phase: Option<String>,
}

impl Transformer for TraceTransformer {
    fn name(&self) -> &'static str {
        self.label
    }

    fn initialize(&mut self, _: &mut TransformationContext) -> Result<(), TransformError> {
        self.log
            .borrow_mut()
            .push(format!("{}:initialize", self.label));
        Ok(())
    }

    fn transform_root(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            unreachable!()
        };
        let path = cx.arena().source(source)?.syntax().file_name.clone();
        let names = cx
            .requested_emit_helpers()
            .iter()
            .map(EmitHelper::name)
            .collect::<Vec<_>>()
            .join(",");
        self.log
            .borrow_mut()
            .push(format!("{}:{path}:{names}", self.label));
        cx.request_emit_helper(EmitHelper::new(
            "shared",
            false,
            vec![EmitHelper::new("dependency", false, vec![])],
        ))?;
        cx.request_emit_helper(EmitHelper::new(
            format!("{}:{path}", self.label),
            false,
            vec![],
        ))?;
        let source = if self.replace_phase.as_deref() == Some(self.label)
            && self.replace_source.as_deref() == Some(&path)
        {
            add_source(
                cx.arena_mut()?,
                "replacement.ts",
                "const value: number = 1;\n",
            )
        } else {
            source
        };
        Ok(TransformRoot::SourceFile(source))
    }
}

fn add_source(arena: &mut TransformArena, name: &str, text: &str) -> TransformSourceId {
    let syntax = parse_source_file(name, text, Default::default(), None);
    arena.add_source(&syntax, None)
}

fn helper_names<'a>(
    result: &'a TransformationResult<'_>,
    source: TransformSourceId,
) -> Vec<&'a str> {
    result
        .emit_helpers_for_source(source)
        .iter()
        .map(EmitHelper::name)
        .collect()
}

fn source_projection(result: &TransformationResult<'_>, source: TransformSourceId) -> Value {
    let syntax = result.arena().source(source).unwrap().syntax();
    json!({"path": syntax.file_name, "helpers": helper_names(result, source), "is_declaration_file": syntax.is_declaration_file})
}

struct BuiltinHost {
    options: CompilerOptions,
    files: Vec<tsc_syntax::SourceFile>,
    ids: Vec<SourceFileId>,
    collision_queries: Vec<Value>,
}

impl crate::EmitHost for BuiltinHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }
    fn current_directory(&self) -> &Path {
        Path::new("/")
    }
    fn common_source_directory(&self) -> &Path {
        Path::new("/")
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

impl crate::EmitResolver for BuiltinHost {
    fn is_declaration_with_colliding_name(
        &self,
        node: crate::EmitResolverNode,
    ) -> Result<bool, crate::EmitResolverError> {
        // Project the pinned checker's exact parse-tree query. Undefined and
        // false both become false at this existing bool-valued Rust seam.
        // All other resolver methods retain their typed unavailable result.
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

#[test]
fn bundle_transform_traces_match_pinned_typescript_twice() {
    for case in fixture()["cases"].as_array().unwrap() {
        for _ in 0..2 {
            let mut arena = TransformArena::new();
            let roots = case["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|root| {
                    let mut add = |name: &Value| {
                        let name = name.as_str().unwrap();
                        let text = if name.ends_with(".d.ts") {
                            "declare const value: number;\n"
                        } else {
                            "const value: number = 1;\n"
                        };
                        add_source(&mut arena, name, text)
                    };
                    if let Some(names) = root.as_array() {
                        TransformRoot::Bundle(TransformBundle::new(
                            names.iter().map(&mut add).collect(),
                        ))
                    } else {
                        TransformRoot::SourceFile(add(root))
                    }
                })
                .collect();
            let log = Rc::new(RefCell::new(Vec::new()));
            let transformers = ["first", "second"]
                .map(|label| {
                    Box::new(TraceTransformer {
                        label,
                        log: Rc::clone(&log),
                        replace_source: case["replace_source"].as_str().map(str::to_owned),
                        replace_phase: case["replace_phase"].as_str().map(str::to_owned),
                    }) as Box<dyn Transformer>
                })
                .into_iter()
                .collect();
            let mut result = transform_nodes(
                arena,
                roots,
                transformers,
                case["allow_declaration_files"].as_bool().unwrap(),
            )
            .unwrap();
            let roots = result.roots().iter().map(|root| match root {
                TransformRoot::SourceFile(source) => json!({"kind": "source-file", "sources": [source_projection(&result, *source)]}),
                TransformRoot::Bundle(bundle) => json!({"kind": "bundle", "sources": bundle.sources().iter().map(|source| source_projection(&result, *source)).collect::<Vec<_>>()})
            }).collect::<Vec<_>>();
            assert!(result.diagnostics().is_empty());
            assert_eq!(
                json!({"log": *log.borrow(), "roots": roots, "diagnostics": []}),
                case["observation"],
                "{}",
                case["case_id"]
            );
            assert_eq!(result.state(), TransformationState::Completed);
            result.dispose();
            assert!(result.context.source_emit_helpers.is_empty());
        }
    }
}

#[test]
fn builtin_bundle_helpers_belong_to_the_requesting_source() {
    for case in fixture()["builtin_cases"].as_array().unwrap() {
        for _ in 0..2 {
            let files = case["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| {
                    parse_source_file(
                        file[0].as_str().unwrap(),
                        file[1].as_str().unwrap(),
                        Default::default(),
                        None,
                    )
                })
                .collect::<Vec<_>>();
            let options = CompilerOptions {
                target: Some(case["options"]["target"].as_i64().unwrap() as i32),
                module: Some(case["options"]["module"].as_i64().unwrap() as i32),
                out_file: case["options"]["outFile"].as_str().map(str::to_owned),
                always_strict: Some(false),
                strict: Some(false),
                ..Default::default()
            };
            let ids = (0..files.len())
                .map(|index| SourceFileId::from_raw(index as u32))
                .collect::<Vec<_>>();
            let host = BuiltinHost {
                options,
                files,
                ids,
                collision_queries: case["observation"]["collision_queries"]
                    .as_array()
                    .unwrap()
                    .clone(),
            };
            let mut arena = TransformArena::new();
            let sources = host
                .files
                .iter()
                .zip(&host.ids)
                .map(|(file, id)| arena.add_source(file, Some(*id)))
                .collect();
            let transformers =
                crate::get_script_transformers_for_source(&host.options, &host, &host, host.ids[0])
                    .unwrap();
            let result = transform_nodes(
                arena,
                vec![TransformRoot::Bundle(TransformBundle::new(sources))],
                transformers,
                false,
            )
            .unwrap();
            let TransformRoot::Bundle(bundle) = &result.roots()[0] else {
                panic!("bundle root retained")
            };
            let actual = bundle.sources().iter().map(|source| json!({"path": result.arena().source(*source).unwrap().syntax().file_name, "helpers": helper_names(&result, *source)})).collect::<Vec<_>>();
            assert_eq!(
                json!(actual),
                case["observation"]["roots"],
                "{}",
                case["case_id"]
            );
            assert!(result.diagnostics().is_empty());
        }
    }
}

struct FailingBundleTransformer {
    disposed: Rc<RefCell<usize>>,
}
impl Transformer for FailingBundleTransformer {
    fn name(&self) -> &'static str {
        "failing-bundle-member"
    }
    fn transform_root(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            unreachable!()
        };
        cx.request_emit_helper(EmitHelper::new("requested-before-failure", false, vec![]))?;
        if cx.arena().source(source)?.syntax().file_name == "b.ts" {
            return Err(TransformError::BlockScopeRequired);
        }
        Ok(TransformRoot::SourceFile(source))
    }
    fn dispose(&mut self) {
        *self.disposed.borrow_mut() += 1;
    }
}

#[test]
fn failed_bundle_member_disposes_the_transformer_once() {
    let mut arena = TransformArena::new();
    let a = add_source(&mut arena, "a.ts", "a;");
    let b = add_source(&mut arena, "b.ts", "b;");
    let disposed = Rc::new(RefCell::new(0));
    let result = transform_nodes(
        arena,
        vec![TransformRoot::Bundle(TransformBundle::new(vec![a, b]))],
        vec![Box::new(FailingBundleTransformer {
            disposed: Rc::clone(&disposed),
        })],
        false,
    );
    assert_eq!(result.err(), Some(TransformError::BlockScopeRequired));
    assert_eq!(*disposed.borrow(), 1);
}
