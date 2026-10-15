//! tsgo testutil/contentmappertest: content mappers served in-process for
//! tests, chosen by the command their package declares, so a test runs the
//! whole protocol without a process.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tsc_diagnostics::spanmap::{
    Features, Segment, SpanMap, FEATURES_ALL, FEATURES_NONE, FEATURE_DEFINITION, FEATURE_HOVER,
    FEATURE_REFERENCES, FEATURE_RENAME, FEATURE_SIGNATURE_HELP, KIND_ALIAS, KIND_ATOM,
    KIND_VERBATIM,
};

use tsc_host::ipc::jsonrpc::MessageReader;
use tsc_host::ipc::pipe::pipe;
use tsc_host::ipc::AsyncConn;
use tsc_host::ipc::{Handler, Payload};
use tsc_program::content_mapper::host::{MapperProcess, ProcessControl, Spawner};

/// tsgo `VerbatimMapper`: the content as TypeScript, mapped verbatim.
pub const VERBATIM_MAPPER: &str = "verbatim-mapper";
/// tsgo `ModuleVerbatimMapper`: the content as an `.mts` module.
pub const MODULE_VERBATIM_MAPPER: &str = "module-verbatim-mapper";
/// tsgo `FailingMapper`: every transform fails.
pub const FAILING_MAPPER: &str = "failing-mapper";
/// tsgo `SupplementalMapper`: an empty module with the content as a
/// supplemental output.
pub const SUPPLEMENTAL_MAPPER: &str = "supplemental-mapper";
/// tsgo `DynamicVerbatimMapper`: verbatim, with a dynamic configuration.
pub const DYNAMIC_VERBATIM_MAPPER: &str = "dynamic-verbatim-mapper";
/// tsgo `SynthesizingMapper`: code of its own, mapped to nothing.
pub const SYNTHESIZING_MAPPER: &str = "synthesizing-mapper";
/// tsgo `TransformingMapper`: the compiler tests' `.box` mapper (option
/// interpolations, a preamble, `@box-` diagnostic directives).
pub const TRANSFORMING_MAPPER: &str = "compiler-test-mapper";
/// tsgo `LispMapper`: one Lisp expression as a call, with an alias segment.
pub const LISP_MAPPER: &str = "lisp-mapper";
/// tsgo `DiagnosticCodeCollisionMapper`: a mapper diagnostic with a code
/// TypeScript uses too.
pub const DIAGNOSTIC_CODE_COLLISION_MAPPER: &str = "diagnostic-code-collision-mapper";
/// tsgo `SupplementalDiagnosticsMapper`: the content in a supplemental
/// output after a line of its own.
pub const SUPPLEMENTAL_DIAGNOSTICS_MAPPER: &str = "supplemental-diagnostics-mapper";
/// tsgo `SupplementalGlobalsMapper`: supplemental outputs declaring
/// globals the canonical outputs use.
pub const SUPPLEMENTAL_GLOBALS_MAPPER: &str = "supplemental-globals-mapper";
/// tsgo `SupplementalModuleMapper`: a supplemental module with an error.
pub const SUPPLEMENTAL_MODULE_MAPPER: &str = "supplemental-module-mapper";
/// tsgo `PrefixedSupplementalMapper`: an empty module with the content after
/// a generated line as a supplemental output (variants by the file's name).
pub const PREFIXED_SUPPLEMENTAL_MAPPER: &str = "prefixed-supplemental-mapper";
/// tsgo `UnmappedFoldingMapper`: code of its own, mapped to nothing.
pub const UNMAPPED_FOLDING_MAPPER: &str = "unmapped-folding-mapper";
/// tsgo `ComponentMapper`: a component's `<script>` verbatim, the
/// identifiers of its template's `{{ }}` as atoms in a render function, its
/// name as a class and an anchored default export.
pub const COMPONENT_MAPPER: &str = "component-mapper";
/// tsgo `DuplicateMapper`: the content twice in the virtual text, with each
/// copy's features (variants by the file's name).
pub const DUPLICATE_MAPPER: &str = "duplicate-mapper";
/// tsgo `HoistingMapper`: a script's leading imports above a render
/// function that holds the rest of it.
pub const HOISTING_MAPPER: &str = "hoisting-mapper";
/// tsgo `DuplicateProjectionMapper`: the content as the canonical and a
/// supplemental output.
pub const DUPLICATE_PROJECTION_MAPPER: &str = "duplicate-projection-mapper";

/// tsgo `contentmappertest.PackageJSON`: the manifest of the `mapper`
/// package whose command is the given test mapper.
pub fn package_json(mapper: &str) -> String {
    let compiler_options = if mapper == TRANSFORMING_MAPPER {
        r#", "compilerOptions": ["target", "jsx"]"#
    } else {
        ""
    };
    let dynamic_config = if mapper == DYNAMIC_VERBATIM_MAPPER {
        r#", "dynamicConfig": true"#
    } else {
        ""
    };
    format!(
        "{{\n\t\"name\": \"mapper\",\n\t\"version\": \"1.0.0\",\n\t\"typescript\": {{ \"contentMapper\": {{ \"exec\": [\"{mapper}\"]{compiler_options}{dynamic_config} }} }}\n}}"
    )
}

/// tsgo `ProjectLifecycle`: the project protocol calls the dynamic mappers
/// of a spawner received.
#[derive(Debug, Default)]
pub struct ProjectLifecycle {
    pub opens: AtomicI32,
    pub closes: AtomicI32,
}

/// tsgo `NewSpawner`: each mapper process is its handler served over an
/// in-memory pipe.
#[derive(Clone, Debug, Default)]
pub struct TestSpawner {
    lifecycle: Option<Arc<ProjectLifecycle>>,
}

impl TestSpawner {
    /// tsgo `NewSpawnerWithProjectLifecycle`: the spawner's dynamic mappers
    /// record their project protocol calls.
    pub fn with_project_lifecycle(lifecycle: Arc<ProjectLifecycle>) -> Self {
        Self {
            lifecycle: Some(lifecycle),
        }
    }
}

impl Spawner for TestSpawner {
    fn spawn(
        &self,
        command: &[String],
        _dir: &str,
        _stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let (mut handler, lifecycle) = handler_for_mapper(command, self.lifecycle.as_ref())?;
        if command[0] != DYNAMIC_VERBATIM_MAPPER {
            handler = Box::new(StaticProject {
                inner: handler,
                lifecycle,
            });
        }
        let (host_reader, host_writer) = pipe();
        let (mapper_reader, mapper_writer) = pipe();
        let server = AsyncConn::new(host_writer, handler);
        std::thread::Builder::new()
            .name("tsc-rs-test-content-mapper".to_owned())
            .spawn(move || {
                let _ = server.run(&mut MessageReader::new(mapper_reader));
            })
            .map_err(|error| error.to_string())?;
        Ok(MapperProcess {
            stdout: Box::new(host_reader),
            stdin: Box::new(mapper_writer),
            control: Box::new(InProcess::default()),
        })
    }
}

/// tsgo `recordingContentMapperSpawner`: the harness's in-process mappers,
/// counting the processes started and closed, and the projects the dynamic
/// mappers opened and closed (tsgo `ProjectLifecycle`).
pub struct RecordingSpawner {
    inner: TestSpawner,
    pub lifecycle: Arc<ProjectLifecycle>,
    pub spawns: AtomicUsize,
    pub closes: Arc<AtomicUsize>,
}

impl RecordingSpawner {
    pub fn new() -> Self {
        let lifecycle = Arc::new(ProjectLifecycle::default());
        Self {
            inner: TestSpawner::with_project_lifecycle(Arc::clone(&lifecycle)),
            lifecycle,
            spawns: AtomicUsize::new(0),
            closes: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Default for RecordingSpawner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spawner for RecordingSpawner {
    fn spawn(
        &self,
        command: &[String],
        dir: &str,
        stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let process = self.inner.spawn(command, dir, stderr)?;
        self.spawns.fetch_add(1, Ordering::SeqCst);
        Ok(MapperProcess {
            stdout: process.stdout,
            stdin: process.stdin,
            control: Box::new(RecordingControl {
                inner: process.control,
                closes: Arc::clone(&self.closes),
                closed: AtomicBool::new(false),
            }),
        })
    }
}

/// A test mapper process whose first close is counted.
struct RecordingControl {
    inner: Box<dyn ProcessControl>,
    closes: Arc<AtomicUsize>,
    closed: AtomicBool,
}

impl ProcessControl for RecordingControl {
    fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.close();
    }

    fn exit_code(&self) -> Option<i32> {
        self.inner.exit_code()
    }
}

/// A mapper's handler, and its project lifecycle when it keeps state per
/// project.
type MapperHandler = (Box<dyn Handler>, Option<Arc<dyn ProjectLifecycleHandler>>);

/// tsgo `handlerForMapper`.
fn handler_for_mapper(
    command: &[String],
    lifecycle: Option<&Arc<ProjectLifecycle>>,
) -> Result<MapperHandler, String> {
    let Some(name) = command.first() else {
        return Err("contentmappertest: empty mapper command".to_owned());
    };
    let handler: Box<dyn Handler> = match name.as_str() {
        TRANSFORMING_MAPPER => {
            let transforming = Arc::new(Transforming::default());
            return Ok((
                Box::new(Shared(Arc::clone(&transforming))),
                Some(transforming),
            ));
        }
        VERBATIM_MAPPER => Box::new(Verbatim { extension: ".ts" }),
        MODULE_VERBATIM_MAPPER => Box::new(Verbatim { extension: ".mts" }),
        FAILING_MAPPER => Box::new(Failing),
        SUPPLEMENTAL_MAPPER => Box::new(Supplemental),
        DYNAMIC_VERBATIM_MAPPER => Box::new(DynamicVerbatim {
            lifecycle: lifecycle.cloned(),
        }),
        SYNTHESIZING_MAPPER => Box::new(Synthesizing),
        LISP_MAPPER => Box::new(Lisp),
        DIAGNOSTIC_CODE_COLLISION_MAPPER => Box::new(DiagnosticCodeCollision),
        SUPPLEMENTAL_DIAGNOSTICS_MAPPER => Box::new(SupplementalDiagnostics),
        SUPPLEMENTAL_GLOBALS_MAPPER => Box::new(SupplementalGlobals),
        SUPPLEMENTAL_MODULE_MAPPER => Box::new(SupplementalModule),
        PREFIXED_SUPPLEMENTAL_MAPPER => Box::new(Transform(prefixed_supplemental)),
        UNMAPPED_FOLDING_MAPPER => Box::new(Transform(unmapped_folding)),
        COMPONENT_MAPPER => Box::new(Transform(component)),
        DUPLICATE_MAPPER => Box::new(Transform(duplicate)),
        HOISTING_MAPPER => Box::new(Transform(hoisting)),
        DUPLICATE_PROJECTION_MAPPER => Box::new(Transform(duplicate_projection)),
        _ => {
            return Err(format!(
                "contentmappertest: unknown mapper command {}",
                go_string_slice(command)
            ))
        }
    };
    Ok((handler, None))
}

/// tsgo `projectLifecycleHandler`: a mapper that keeps state per project.
trait ProjectLifecycleHandler: Send + Sync {
    fn open_project(&self, params: &serde_json::Value) -> Result<(), String>;
    fn close_project(&self, params: &serde_json::Value);
}

/// A handler shared with its project lifecycle.
struct Shared<T>(Arc<T>);

impl<T: Handler> Handler for Shared<T> {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        self.0.handle_request(method, params)
    }

    fn handle_notification(&self, method: &str, params: &[u8]) {
        self.0.handle_notification(method, params);
    }
}

/// Go's `%v` of a string slice.
fn go_string_slice(values: &[String]) -> String {
    format!("[{}]", values.join(" "))
}

/// The control of a mapper served in-process: closing ends nothing more
/// than the pipes do, and it has no exit code.
#[derive(Default)]
struct InProcess {
    closed: Mutex<bool>,
}

impl ProcessControl for InProcess {
    fn close(&self) {
        *self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }

    fn exit_code(&self) -> Option<i32> {
        None
    }
}

/// tsgo `initializeResult`.
fn initialize_result(source: &str) -> Payload {
    Payload::Json(format!(
        r#"{{"positionEncoding":"utf-8","diagnosticSource":{}}}"#,
        serde_json::to_string(source).expect("a string serializes")
    ))
}

/// The `content` of a transform request.
fn transform_content(params: &[u8]) -> Result<String, String> {
    let params: serde_json::Value =
        serde_json::from_slice(params).map_err(|error| error.to_string())?;
    params
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "contentmappertest: a transform without content".to_owned())
}

/// tsgo `identityMappedOutput`: the content with one verbatim segment over
/// it (in UTF-8 bytes).
fn identity_mapped_output(content: &str, extension: &str) -> serde_json::Value {
    let length = content.len();
    serde_json::json!({
        "text": content,
        "extension": extension,
        "mappings": [[0, length, 0, length, 0]],
    })
}

/// tsgo `verbatimHandler` and `moduleVerbatimHandler`.
struct Verbatim {
    extension: &'static str,
}

impl Handler for Verbatim {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let content = transform_content(params)?;
                Ok(Payload::Json(
                    identity_mapped_output(&content, self.extension).to_string(),
                ))
            }
            method => Err(format!("contentmappertest: unexpected method {method:?}")),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `failingHandler`.
struct Failing;

impl Handler for Failing {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => Err("content mapper failed to transform the file".to_owned()),
            method => Err(format!("contentmappertest: unexpected method {method:?}")),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `supplementalHandler`.
struct Supplemental;

impl Handler for Supplemental {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let content = transform_content(params)?;
                Ok(Payload::Json(
                    serde_json::json!({
                        "text": "export {};",
                        "extension": ".ts",
                        "supplemental": [identity_mapped_output(&content, ".ts")],
                    })
                    .to_string(),
                ))
            }
            method => Err(format!("contentmappertest: unexpected method {method:?}")),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `dynamicVerbatimHandler`: its configuration identity is the config
/// file and the options, it watches `mapper.config.json` beside the config,
/// and it transforms only in a project.
struct DynamicVerbatim {
    lifecycle: Option<Arc<ProjectLifecycle>>,
}

impl Handler for DynamicVerbatim {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "openProject" => {
                if let Some(lifecycle) = &self.lifecycle {
                    lifecycle.opens.fetch_add(1, Ordering::SeqCst);
                }
                let params: serde_json::Value =
                    serde_json::from_slice(params).map_err(|error| error.to_string())?;
                let config_file_name = params
                    .get("configFileName")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let options = params
                    .get("options")
                    .map(serde_json::Value::to_string)
                    .unwrap_or_default();
                let mut result = serde_json::json!({
                    "configIdentity": format!("{config_file_name}:{options}"),
                    "watchedFiles": [watched_config(config_file_name)],
                });
                if options == r#"{"plugins":[{"name":1}]}"# {
                    result["optionDiagnostics"] = option_name_diagnostic();
                }
                Ok(Payload::Json(result.to_string()))
            }
            "closeProject" => {
                if let Some(lifecycle) = &self.lifecycle {
                    lifecycle.closes.fetch_add(1, Ordering::SeqCst);
                }
                Ok(Payload::Json("null".to_owned()))
            }
            "transform" => {
                let params: serde_json::Value =
                    serde_json::from_slice(params).map_err(|error| error.to_string())?;
                if params
                    .get("projectHandle")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    return Err("content mapper transform requires a project handle".to_owned());
                }
                Verbatim { extension: ".ts" }
                    .handle_request(method, &params.to_string().into_bytes())
            }
            _ => Verbatim { extension: ".ts" }.handle_request(method, params),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// `mapper.config.json` in the config's directory (tsgo
/// `GetDirectoryPath`, `/` when there is none).
fn watched_config(config_file_name: &str) -> String {
    let directory = config_file_name
        .rfind('/')
        .map_or("", |index| &config_file_name[..index]);
    let directory = if directory.is_empty() { "/" } else { directory };
    if directory.ends_with('/') {
        format!("{directory}mapper.config.json")
    } else {
        format!("{directory}/mapper.config.json")
    }
}

/// The option diagnostic the test mappers report for
/// `{"plugins":[{"name":1}]}`.
fn option_name_diagnostic() -> serde_json::Value {
    serde_json::json!([{
        "path": ["plugins", 0, "name"],
        "messageText": "Option 'name' requires a string.",
        "code": 123,
    }])
}

/// tsgo `synthesizingHandler`.
struct Synthesizing;

impl Handler for Synthesizing {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => Ok(Payload::Json(
                serde_json::json!({
                    "text": "export const el = jsxRuntime(Widget);\n",
                    "extension": ".ts",
                    "mappings": [],
                })
                .to_string(),
            )),
            method => Err(format!("contentmappertest: unexpected method {method:?}")),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `staticProjectHandler`: a mapper without dynamic configuration
/// opens and closes projects with nothing to say (its lifecycle, if any,
/// told), but for the options the option diagnostics test passes.
struct StaticProject {
    inner: Box<dyn Handler>,
    lifecycle: Option<Arc<dyn ProjectLifecycleHandler>>,
}

impl Handler for StaticProject {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "openProject" => {
                let params: serde_json::Value =
                    serde_json::from_slice(params).map_err(|error| error.to_string())?;
                if let Some(lifecycle) = &self.lifecycle {
                    lifecycle.open_project(&params)?;
                }
                let options = params.get("options").map(serde_json::Value::to_string);
                let result = if options.as_deref() == Some(r#"{"plugins":[{"name":1}]}"#) {
                    serde_json::json!({ "optionDiagnostics": option_name_diagnostic() })
                } else {
                    serde_json::json!({})
                };
                Ok(Payload::Json(result.to_string()))
            }
            "closeProject" => {
                let params: serde_json::Value =
                    serde_json::from_slice(params).map_err(|error| error.to_string())?;
                if let Some(lifecycle) = &self.lifecycle {
                    lifecycle.close_project(&params);
                }
                Ok(Payload::Json("null".to_owned()))
            }
            _ => self.inner.handle_request(method, params),
        }
    }

    fn handle_notification(&self, method: &str, params: &[u8]) {
        self.inner.handle_notification(method, params);
    }
}

/// Go's `%q` of a string (`strconv.Quote` over the text these mappers see).
fn go_quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if (character as u32) < 0x20 || character == '\u{7f}' => {
                quoted.push_str(&format!("\\x{:02x}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// tsgo `spanmap.Marshal`: each segment as `[virtualStart, virtualLength,
/// originalStart, originalLength, kind]`, with its features after them
/// when they are not every feature.
fn marshal_span_map(map: &SpanMap) -> serde_json::Value {
    serde_json::Value::Array(
        map.segments()
            .iter()
            .map(|segment| {
                let mut tuple = vec![
                    segment.virtual_start,
                    segment.virtual_end - segment.virtual_start,
                    segment.original_start,
                    segment.original_end - segment.original_start,
                    segment.kind,
                ];
                if segment.features != FEATURES_ALL {
                    tuple.push(segment.features);
                }
                serde_json::json!(tuple)
            })
            .collect(),
    )
}

fn segment(
    virtual_start: usize,
    virtual_end: usize,
    original_start: usize,
    original_end: usize,
    kind: i32,
) -> Segment {
    Segment {
        virtual_start: virtual_start as i32,
        virtual_end: virtual_end as i32,
        original_start: original_start as i32,
        original_end: original_end as i32,
        kind,
        features: FEATURES_ALL,
    }
}

/// The parameters of a transform request.
fn transform_params(params: &[u8]) -> Result<serde_json::Value, String> {
    serde_json::from_slice(params).map_err(|error| error.to_string())
}

fn param_text<'a>(params: &'a serde_json::Value, name: &str) -> &'a str {
    params
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

/// tsgo `Handler` (`TransformingMapper`): the compiler tests' `.box`
/// mapper, with each project's compiler options.
#[derive(Default)]
struct Transforming {
    compiler_options: Mutex<HashMap<String, Option<serde_json::Map<String, serde_json::Value>>>>,
}

impl Transforming {
    fn options(
        &self,
    ) -> std::sync::MutexGuard<
        '_,
        HashMap<String, Option<serde_json::Map<String, serde_json::Value>>>,
    > {
        self.compiler_options
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ProjectLifecycleHandler for Transforming {
    fn open_project(&self, params: &serde_json::Value) -> Result<(), String> {
        let options = match params.get("compilerOptions") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Object(options)) => Some(options.clone()),
            Some(_) => {
                return Err("contentmappertest: compiler options are not an object".to_owned())
            }
        };
        self.options()
            .insert(param_text(params, "projectHandle").to_owned(), options);
        Ok(())
    }

    fn close_project(&self, params: &serde_json::Value) {
        self.options().remove(param_text(params, "projectHandle"));
    }
}

const PREAMBLE: &str = "const __VERSION = \"1.0.0\";\n";
const UNCLOSED_INTERPOLATION_CODE: i32 = 1000;

impl Handler for Transforming {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("box")),
            "transform" => {
                let params = transform_params(params)?;
                let handle = param_text(&params, "projectHandle");
                let options = self.options().get(handle).cloned().flatten();
                let Some(options) = options else {
                    return Err(format!(
                        "contentmappertest: project {} is not open",
                        go_quote(handle)
                    ));
                };
                let content = param_text(&params, "content");
                let (text, map, diagnostics) = transforming_output(content, &options);
                let mut result = serde_json::Map::new();
                result.insert("text".to_owned(), serde_json::json!(text));
                result.insert(
                    "extension".to_owned(),
                    serde_json::json!(mapped_extension(content)),
                );
                result.insert("mappings".to_owned(), marshal_span_map(&map));
                if let Some(directives) = diagnostic_directives(content, &map) {
                    result.insert("diagnosticDirectives".to_owned(), directives);
                }
                if !diagnostics.is_empty() {
                    result.insert(
                        "diagnostics".to_owned(),
                        serde_json::Value::Array(diagnostics),
                    );
                }
                Ok(Payload::Json(serde_json::Value::Object(result).to_string()))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `mappedExtension`: a first line `// @box-extension: .x` names the
/// output's extension.
fn mapped_extension(content: &str) -> String {
    const PREFIX: &str = "// @box-extension:";
    match content.split_once('\n') {
        Some((first_line, _)) if first_line.starts_with(PREFIX) => {
            first_line[PREFIX.len()..].trim().to_owned()
        }
        _ => ".ts".to_owned(),
    }
}

/// tsgo `transform`: the preamble, then the content with each `#{name}` an
/// atom of the option's JSON (`undefined` without it); an interpolation
/// unclosed on its line is an atom `undefined` over the rest of the line
/// and a diagnostic.
fn transforming_output(
    content: &str,
    options: &serde_json::Map<String, serde_json::Value>,
) -> (String, SpanMap, Vec<serde_json::Value>) {
    let mut virtual_text = String::from(PREAMBLE);
    let mut segments = Vec::new();
    let mut diagnostics = Vec::new();
    let write_verbatim =
        |virtual_text: &mut String, segments: &mut Vec<Segment>, from: usize, to: usize| {
            if to <= from {
                return;
            }
            let virtual_start = virtual_text.len();
            virtual_text.push_str(&content[from..to]);
            segments.push(segment(
                virtual_start,
                virtual_text.len(),
                from,
                to,
                KIND_VERBATIM,
            ));
        };
    let write_atom = |virtual_text: &mut String,
                      segments: &mut Vec<Segment>,
                      value: &str,
                      from: usize,
                      to: usize| {
        let virtual_start = virtual_text.len();
        virtual_text.push_str(value);
        segments.push(segment(
            virtual_start,
            virtual_text.len(),
            from,
            to,
            KIND_ATOM,
        ));
    };
    let mut pos = 0;
    while pos < content.len() {
        let Some(relative) = content[pos..].find("#{") else {
            write_verbatim(&mut virtual_text, &mut segments, pos, content.len());
            break;
        };
        let token_start = pos + relative;
        let line_end = content[token_start..]
            .find('\n')
            .map_or(content.len(), |index| token_start + index);
        let Some(close) = content[token_start..line_end].find('}') else {
            write_verbatim(&mut virtual_text, &mut segments, pos, token_start);
            write_atom(
                &mut virtual_text,
                &mut segments,
                "undefined",
                token_start,
                line_end,
            );
            diagnostics.push(serde_json::json!({
                "messageText": "Unclosed interpolation.",
                "start": token_start,
                "length": line_end - token_start,
                "code": UNCLOSED_INTERPOLATION_CODE,
            }));
            pos = line_end;
            continue;
        };
        let token_end = token_start + close + 1;
        let name = &content[token_start + 2..token_end - 1];
        write_verbatim(&mut virtual_text, &mut segments, pos, token_start);
        let rendered = render_option(options, name);
        write_atom(
            &mut virtual_text,
            &mut segments,
            &rendered,
            token_start,
            token_end,
        );
        pos = token_end;
    }
    (virtual_text, SpanMap::new(segments), diagnostics)
}

/// tsgo `renderOption`: the option's JSON text, or `undefined`.
fn render_option(options: &serde_json::Map<String, serde_json::Value>, name: &str) -> String {
    options
        .get(name)
        .map(serde_json::Value::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "undefined".to_owned())
}

/// A directive tuple: `[originalStart, originalLength, virtualStart,
/// virtualEnd, policy, unusedExpectDirectiveIndex?]`.
fn directive(
    original: (i64, i64),
    virtual_range: (i64, i64),
    policy: i64,
    unused_index: Option<usize>,
) -> Vec<i64> {
    let mut tuple = vec![
        original.0,
        original.1,
        virtual_range.0,
        virtual_range.1,
        policy,
    ];
    if let Some(index) = unused_index {
        tuple.push(index as i64);
    }
    tuple
}

/// tsgo `diagnosticDirectives`: `// @box-ignore` (policy 0) and
/// `// @box-expect-error: <message>` (policy 1, with the message as its
/// unused diagnostic) over the next line's virtual span, or a malformed set
/// named by a first line `// @box-invalid-directive: <case>`.
fn diagnostic_directives(content: &str, map: &SpanMap) -> Option<serde_json::Value> {
    const IGNORE: i64 = 0;
    const EXPECT: i64 = 1;
    let wrap = |directives: Vec<Vec<i64>>, unused: Vec<serde_json::Value>| {
        serde_json::json!({
            "unusedExpectDirectiveDiagnostics": if unused.is_empty() { serde_json::Value::Null } else { serde_json::Value::Array(unused) },
            "directives": if directives.is_empty() { serde_json::Value::Null } else { serde_json::json!(directives) },
        })
    };
    let empty_unused = || serde_json::json!({ "code": 0, "messageText": "" });
    const INVALID_PREFIX: &str = "// @box-invalid-directive:";
    if let Some(rest) = content.strip_prefix(INVALID_PREFIX) {
        let case = rest.split('\n').next().unwrap_or_default().trim();
        let invalid = match case {
            "invalid-range" => Some(wrap(
                vec![directive((0, 0), (-1, 0), IGNORE, None)],
                Vec::new(),
            )),
            "original-range-out-of-bounds" => Some(wrap(
                vec![directive(
                    (content.len() as i64 + 1, 0),
                    (0, 0),
                    IGNORE,
                    None,
                )],
                Vec::new(),
            )),
            "virtual-range-out-of-bounds" => Some(wrap(
                vec![directive((0, 0), (1 << 20, 1 << 20), IGNORE, None)],
                Vec::new(),
            )),
            "invalid-policy" => Some(wrap(vec![directive((0, 0), (0, 0), 2, None)], Vec::new())),
            "ignore-with-unused-diagnostic" => Some(wrap(
                vec![directive((0, 0), (0, 0), IGNORE, None)],
                vec![empty_unused()],
            )),
            "expect-without-unused-diagnostic" => Some(wrap(
                vec![directive((0, 0), (0, 0), EXPECT, None)],
                Vec::new(),
            )),
            "invalid-unused-diagnostic-index" => Some(wrap(
                vec![directive((0, 0), (0, 0), EXPECT, Some(1))],
                vec![empty_unused()],
            )),
            "overlap" => Some(wrap(
                vec![
                    directive((0, 0), (0, 2), IGNORE, None),
                    directive((0, 0), (1, 3), IGNORE, None),
                ],
                Vec::new(),
            )),
            _ => None,
        };
        if invalid.is_some() {
            return invalid;
        }
    }
    const IGNORE_PREFIX: &str = "// @box-ignore";
    const EXPECT_PREFIX: &str = "// @box-expect-error:";
    let mut directives: Vec<(Vec<i64>, Option<usize>)> = Vec::new();
    let mut unused = Vec::new();
    let mut line_start = 0;
    while line_start < content.len() {
        let line_end = content[line_start..]
            .find('\n')
            .map_or(content.len(), |index| line_start + index);
        let trimmed = content[line_start..line_end].trim();
        let policy = if trimmed == IGNORE_PREFIX {
            Some((IGNORE, None))
        } else if let Some(message) = trimmed.strip_prefix(EXPECT_PREFIX) {
            let index = unused.len();
            unused.push(serde_json::json!({ "code": 2578, "messageText": message.trim() }));
            Some((EXPECT, Some(index)))
        } else {
            None
        };
        if let (Some((policy, unused_index)), true) = (policy, line_end < content.len()) {
            let affected_start = line_end + 1;
            let affected_end = content[affected_start..]
                .find('\n')
                .map_or(content.len(), |index| affected_start + index);
            let spans = map.original_to_virtual_spans(
                affected_start as i32,
                affected_end as i32,
                FEATURES_ALL,
            );
            if let [span] = spans.as_slice() {
                directives.push((
                    directive(
                        (line_start as i64, (line_end - line_start) as i64),
                        (i64::from(span.pos), i64::from(span.end)),
                        policy,
                        None,
                    ),
                    unused_index,
                ));
            }
        }
        if line_end == content.len() {
            break;
        }
        line_start = line_end + 1;
    }
    if directives.is_empty() && unused.is_empty() {
        return None;
    }
    // A single unused diagnostic is every directive's: the index is left
    // out.
    let single = unused.len() == 1;
    let directives = directives
        .into_iter()
        .map(|(mut tuple, index)| {
            if let (Some(index), false) = (index, single) {
                tuple.push(index as i64);
            }
            tuple
        })
        .collect();
    Some(wrap(directives, unused))
}

/// tsgo `identityMappedOutput` as a JSON object.
fn identity_output(content: &str) -> serde_json::Map<String, serde_json::Value> {
    match identity_mapped_output(content, ".ts") {
        serde_json::Value::Object(output) => output,
        _ => unreachable!("an output is an object"),
    }
}

/// tsgo `lispHandler`.
struct Lisp;

impl Handler for Lisp {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("lisp")),
            "transform" => {
                let params = transform_params(params)?;
                let content = param_text(&params, "content");
                if content.strip_suffix('\n').unwrap_or(content) != "(+ 1 2 \"oops\")" {
                    return Err(format!(
                        "contentmappertest: unsupported Lisp expression {}",
                        go_quote(content)
                    ));
                }
                let map = SpanMap::new(vec![
                    segment(0, 3, 1, 2, KIND_ALIAS),
                    segment(4, 5, 3, 4, KIND_VERBATIM),
                    segment(7, 8, 5, 6, KIND_VERBATIM),
                    segment(10, 16, 7, 13, KIND_VERBATIM),
                ]);
                Ok(Payload::Json(
                    serde_json::json!({
                        "text": "add(1, 2, \"oops\");",
                        "extension": ".ts",
                        "mappings": marshal_span_map(&map),
                    })
                    .to_string(),
                ))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `diagnosticCodeCollisionHandler`: TS9007's code on a mapper
/// diagnostic over the content's first `foo`.
struct DiagnosticCodeCollision;

impl Handler for DiagnosticCodeCollision {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let params = transform_params(params)?;
                let content = param_text(&params, "content");
                let mut output = identity_output(content);
                let start = content.find("foo").map_or(-1, |index| index as i64);
                output.insert(
                    "diagnostics".to_owned(),
                    serde_json::json!([{
                        "messageText": "Mapper diagnostic with a colliding code.",
                        "start": start,
                        "length": 3,
                        "code": tsc_diagnostics::gen::Function_must_have_an_explicit_return_type_annotation_with_isolatedDeclarations.code,
                    }]),
                );
                Ok(Payload::Json(serde_json::Value::Object(output).to_string()))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `supplementalDiagnosticsHandler`.
struct SupplementalDiagnostics;

impl Handler for SupplementalDiagnostics {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let params = transform_params(params)?;
                let content = param_text(&params, "content");
                const PREFIX: &str = "missingSupplementalGlobal;\n";
                let map = SpanMap::new(vec![segment(
                    PREFIX.len(),
                    PREFIX.len() + content.len(),
                    0,
                    content.len(),
                    KIND_VERBATIM,
                )]);
                Ok(Payload::Json(
                    serde_json::json!({
                        "text": "export {};",
                        "extension": ".ts",
                        "supplemental": [{
                            "text": format!("{PREFIX}{content}"),
                            "extension": ".ts",
                            "mappings": marshal_span_map(&map),
                        }],
                    })
                    .to_string(),
                ))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `supplementalGlobalsHandler`.
struct SupplementalGlobals;

impl Handler for SupplementalGlobals {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let params = transform_params(params)?;
                let file_name = param_text(&params, "fileName");
                let supplemental = if file_name.ends_with("/a.vue") {
                    "/// <reference path=\"./extra.d.ts\" />\ninterface Shared extends Extra { value: string }"
                } else if file_name.ends_with("/b.vue") {
                    "declare const shared: Shared;"
                } else {
                    return Err(format!(
                        "contentmappertest: unexpected supplemental global input {}",
                        go_quote(file_name)
                    ));
                };
                Ok(Payload::Json(
                    serde_json::json!({
                        "text": "export default shared.value;",
                        "extension": ".ts",
                        "supplemental": [{ "text": supplemental, "extension": ".ts" }],
                    })
                    .to_string(),
                ))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `supplementalModuleHandler`.
struct SupplementalModule;

impl Handler for SupplementalModule {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => Ok(Payload::Json(
                serde_json::json!({
                    "text": "export default 1;",
                    "extension": ".ts",
                    "supplemental": [{
                        "text": "export const privateValue: number = \"wrong\";",
                        "extension": ".ts",
                    }],
                })
                .to_string(),
            )),
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// A mapper of the language service tests: `initialize` as tsgo's test
/// mappers answer it, and `transform` by a function of its parameters'
/// file name and content.
struct Transform(fn(&str, &str) -> Result<serde_json::Value, String>);

impl Handler for Transform {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "initialize" => Ok(initialize_result("mapper")),
            "transform" => {
                let params = transform_params(params)?;
                let result = (self.0)(
                    param_text(&params, "fileName"),
                    param_text(&params, "content"),
                )?;
                Ok(Payload::Json(result.to_string()))
            }
            method => Err(format!(
                "contentmappertest: unexpected method {}",
                go_quote(method)
            )),
        }
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

fn segment_with(
    virtual_start: usize,
    virtual_end: usize,
    original_start: usize,
    original_end: usize,
    kind: i32,
    features: Features,
) -> Segment {
    Segment {
        features,
        ..segment(
            virtual_start,
            virtual_end,
            original_start,
            original_end,
            kind,
        )
    }
}

/// tsgo `prefixedSupplementalHandler`: an empty module and, as a
/// supplemental output, the content after `/* generated */` (its features
/// none for a `folding-disabled`, `codelens-disabled` or
/// `formatting-disabled` file); a `formatting-split` file's supplemental
/// output has a generated declaration before `function second`; a
/// `formatting-overlap` file's canonical output is the content up to
/// `function third` and its supplemental one the content from `function
/// second` in an `if` block; a `folding-duplicate`, `codelens-disabled` or
/// `codelens-duplicate` file's canonical output is the content.
fn prefixed_supplemental(file_name: &str, content: &str) -> Result<serde_json::Value, String> {
    const PREFIX: &str = "/* generated */\n";
    let features = if [
        "folding-disabled",
        "codelens-disabled",
        "formatting-disabled",
    ]
    .iter()
    .any(|variant| file_name.contains(variant))
    {
        FEATURES_NONE
    } else {
        FEATURES_ALL
    };
    let mut supplemental_text = format!("{PREFIX}{content}");
    let mut segments = vec![segment_with(
        PREFIX.len(),
        PREFIX.len() + content.len(),
        0,
        content.len(),
        KIND_VERBATIM,
        features,
    )];
    if file_name.contains("formatting-split") {
        let second_start = content.find("function second").ok_or_else(|| {
            "contentmappertest: formatting-split input is missing function second".to_owned()
        })?;
        const GENERATED: &str = "const generated={x:1};\n";
        supplemental_text = format!(
            "{PREFIX}{}{GENERATED}{}",
            &content[..second_start],
            &content[second_start..]
        );
        segments = vec![
            segment(
                PREFIX.len(),
                PREFIX.len() + second_start,
                0,
                second_start,
                KIND_VERBATIM,
            ),
            segment(
                PREFIX.len() + second_start + GENERATED.len(),
                supplemental_text.len(),
                second_start,
                content.len(),
                KIND_VERBATIM,
            ),
        ];
    }
    if file_name.contains("formatting-overlap") {
        let (Some(second_start), Some(third_start)) = (
            content.find("function second"),
            content.find("function third"),
        ) else {
            return Err(
                "contentmappertest: formatting-overlap input is missing function second or third"
                    .to_owned(),
            );
        };
        const WRAPPER_START: &str = "if (true) {\n";
        const WRAPPER_END: &str = "}\n";
        let supplemental_text = format!("{WRAPPER_START}{}{WRAPPER_END}", &content[second_start..]);
        let segments = vec![segment(
            WRAPPER_START.len(),
            WRAPPER_START.len() + content.len() - second_start,
            second_start,
            content.len(),
            KIND_VERBATIM,
        )];
        let canonical = SpanMap::new(vec![segment(0, third_start, 0, third_start, KIND_VERBATIM)]);
        return Ok(serde_json::json!({
            "text": &content[..third_start],
            "extension": ".ts",
            "mappings": marshal_span_map(&canonical),
            "supplemental": [{
                "text": supplemental_text,
                "extension": ".ts",
                "mappings": marshal_span_map(&SpanMap::new(segments)),
            }],
        }));
    }
    let supplemental = serde_json::json!({
        "text": supplemental_text,
        "extension": ".ts",
        "mappings": marshal_span_map(&SpanMap::new(segments)),
    });
    if [
        "folding-duplicate",
        "codelens-disabled",
        "codelens-duplicate",
    ]
    .iter()
    .any(|variant| file_name.contains(variant))
    {
        let canonical = SpanMap::new(vec![segment(
            0,
            content.len(),
            0,
            content.len(),
            KIND_VERBATIM,
        )]);
        return Ok(serde_json::json!({
            "text": content,
            "extension": ".ts",
            "mappings": marshal_span_map(&canonical),
            "supplemental": [supplemental],
        }));
    }
    Ok(serde_json::json!({
        "text": "export {};",
        "extension": ".ts",
        "supplemental": [supplemental],
    }))
}

/// tsgo `unmappedFoldingHandler`: imports and a comment of its own, mapped
/// to nothing.
fn unmapped_folding(_file_name: &str, _content: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "text": "import \"a\";\nimport \"b\";\n/*\n * generated\n */\nexport {};",
        "extension": ".ts",
        "mappings": [],
    }))
}

/// tsgo `duplicateProjectionHandler`: the content as the canonical output
/// and as a supplemental one, so an original span has a counterpart in two
/// virtual files of the same original file.
fn duplicate_projection(_file_name: &str, content: &str) -> Result<serde_json::Value, String> {
    let mut result = identity_mapped_output(content, ".ts");
    result["supplemental"] = serde_json::json!([identity_mapped_output(content, ".ts")]);
    Ok(result)
}

/// The virtual text a test mapper writes, as Go's byte builder writes it,
/// and its segments.
#[derive(Default)]
struct VirtualText {
    bytes: Vec<u8>,
    segments: Vec<Segment>,
}

impl VirtualText {
    fn synthesized(&mut self, text: &[u8]) {
        self.bytes.extend_from_slice(text);
    }

    fn mapped(
        &mut self,
        text: &[u8],
        original_start: usize,
        original_end: usize,
        kind: i32,
        features: Features,
    ) {
        let virtual_start = self.bytes.len();
        self.bytes.extend_from_slice(text);
        self.segments.push(segment_with(
            virtual_start,
            self.bytes.len(),
            original_start,
            original_end,
            kind,
            features,
        ));
    }

    fn output(self) -> (String, SpanMap) {
        (
            String::from_utf8(self.bytes).expect("a test mapper writes whole characters"),
            SpanMap::new(self.segments),
        )
    }
}

fn is_identifier_start_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_identifier_part_byte(byte: u8) -> bool {
    is_identifier_start_byte(byte) || byte.is_ascii_digit()
}

/// tsgo `componentHandler` (`transformComponent`).
fn component(_file_name: &str, content: &str) -> Result<serde_json::Value, String> {
    let bytes = content.as_bytes();
    let mut text = VirtualText::default();
    if let Some(script_open) = content.find("<script") {
        let open_end = content[script_open..]
            .find('>')
            .ok_or_else(|| "contentmappertest: unclosed <script> tag".to_owned())?;
        let script_start = script_open + open_end + 1;
        let close = content[script_start..]
            .find("</script>")
            .ok_or_else(|| "contentmappertest: missing </script> tag".to_owned())?;
        let script_end = script_start + close;
        text.mapped(
            &bytes[script_start..script_end],
            script_start,
            script_end,
            KIND_VERBATIM,
            FEATURES_ALL,
        );
    }
    text.synthesized(b"\nfunction __render() {\n");
    let mut search_start = 0;
    while search_start < content.len() {
        let Some(open) = content[search_start..].find("{{") else {
            break;
        };
        let expression_start = search_start + open + "{{".len();
        let close = content[expression_start..]
            .find("}}")
            .ok_or_else(|| "contentmappertest: unclosed template expression".to_owned())?;
        let expression_end = expression_start + close;
        text.synthesized(b"  void (");
        let mut position = expression_start;
        while position < expression_end {
            if !is_identifier_start_byte(bytes[position]) {
                text.synthesized(&bytes[position..position + 1]);
                position += 1;
                continue;
            }
            let mut end = position + 1;
            while end < expression_end && is_identifier_part_byte(bytes[end]) {
                end += 1;
            }
            text.mapped(
                &bytes[position..end],
                position,
                end,
                KIND_ATOM,
                FEATURES_ALL,
            );
            position = end;
        }
        text.synthesized(b");\n");
        search_start = expression_end + "}}".len();
    }
    text.synthesized(b"}\n");
    if let Some((name_start, name_end)) = component_name_range(content) {
        text.synthesized(b"export class ");
        text.mapped(
            &bytes[name_start..name_end],
            name_start,
            name_end,
            KIND_ATOM,
            FEATURES_ALL,
        );
        text.synthesized(b" {}\n");
    }
    // An anchor: the default export at the file's start.
    text.mapped(
        b"export default {};\n",
        0,
        0,
        KIND_ATOM,
        FEATURE_DEFINITION | FEATURE_REFERENCES,
    );
    let (text, map) = text.output();
    Ok(serde_json::json!({
        "text": text,
        "extension": ".ts",
        "mappings": marshal_span_map(&map),
    }))
}

/// tsgo `componentNameRange`: the `name` attribute's value of the
/// `<component>` tag.
fn component_name_range(content: &str) -> Option<(usize, usize)> {
    let component_start = content.find("<component")?;
    let tag_end = content[component_start..].find('>')?;
    let tag = &content[component_start..component_start + tag_end];
    let name = tag.find("name=\"")?;
    let start = component_start + name + "name=\"".len();
    let end = content[start..].find('"')?;
    Some((start, start + end))
}

/// tsgo `duplicateHandler`: the content twice (three times for a rename
/// conflict) in the virtual text, each copy a verbatim segment of the whole
/// content with the variant's features.
fn duplicate(file_name: &str, content: &str) -> Result<serde_json::Value, String> {
    let length = content.len();
    let output = |text: String, starts: &[usize], features: &[Features]| {
        let segments = starts
            .iter()
            .zip(features)
            .map(|(&start, &features)| {
                segment_with(start, start + length, 0, length, KIND_VERBATIM, features)
            })
            .collect();
        serde_json::json!({
            "text": text,
            "extension": ".ts",
            "mappings": marshal_span_map(&SpanMap::new(segments)),
        })
    };
    if file_name.contains("hover-fallback") {
        let text = format!("// {content}\nconst {content} = 1;\n");
        let first = "// ".len();
        let second = first + length + "\nconst ".len();
        return Ok(output(
            text,
            &[first, second],
            &[FEATURE_HOVER, FEATURE_HOVER],
        ));
    }
    if file_name.contains("hover-concat") {
        let text = format!(
            "namespace A {{ export const {content} = 1; }}\nnamespace B {{ export const {content} = \"text\"; }}\n"
        );
        let first = text.find(content).unwrap_or(0);
        let second = text.rfind(content).unwrap_or(0);
        return Ok(output(
            text,
            &[first, second],
            &[FEATURE_HOVER, FEATURE_HOVER],
        ));
    }
    if file_name.contains("signature-fallback") {
        let text = format!("// {content}\nfunction use(value: number): void {{}}\n{content};\n");
        let first = "// ".len();
        let second = text.rfind(content).unwrap_or(0);
        return Ok(output(
            text,
            &[first, second],
            &[FEATURE_SIGNATURE_HELP, FEATURE_SIGNATURE_HELP],
        ));
    }
    if file_name.contains("rename-conflict") {
        let text =
            format!("export const {content} = 1;\nconst object = {{ {content} }};\n{content};\n");
        let first = text.find(content).unwrap_or(0);
        let second = text[first + length..].find(content).unwrap_or(0) + first + length;
        let third = text[second + length..].find(content).unwrap_or(0) + second + length;
        return Ok(output(
            text,
            &[first, second, third],
            &[FEATURE_RENAME, FEATURE_RENAME, FEATURE_RENAME],
        ));
    }
    let text = format!("export const {content} = 1;\n{content};\n");
    let first = "export const ".len();
    let second = first + length + " = 1;\n".len();
    let (semantic, navigation) = if file_name.contains("disabled") {
        (FEATURES_NONE, FEATURES_NONE)
    } else {
        (
            FEATURE_HOVER | FEATURE_DEFINITION | FEATURE_REFERENCES | FEATURE_RENAME,
            FEATURE_DEFINITION | FEATURE_REFERENCES | FEATURE_RENAME,
        )
    };
    Ok(output(text, &[first, second], &[semantic, navigation]))
}

/// tsgo `hoistingHandler` (`transformHoisting`): the script's leading
/// imports lifted above a render function holding the rest of the script,
/// as svelte2tsx lays out a component.
fn hoisting(_file_name: &str, content: &str) -> Result<serde_json::Value, String> {
    let (script_start, script_end) = script_range(content)?;
    let (imports_start, imports_end) = leading_import_range(content, script_start, script_end);
    let bytes = content.as_bytes();
    let mut text = VirtualText::default();
    let mapped = |text: &mut VirtualText, start: usize, end: usize| {
        text.mapped(&bytes[start..end], start, end, KIND_VERBATIM, FEATURES_ALL);
    };
    text.synthesized(b"///<reference types=\"svelte\" />\n;\n");
    mapped(&mut text, imports_start, imports_end);
    text.synthesized(b"\nfunction $$render() {");
    mapped(&mut text, script_start, imports_start);
    mapped(&mut text, imports_end, script_end);
    text.synthesized(b"\n;\nreturn { props: {} as Record<string, never> }}\n");
    let (text, map) = text.output();
    Ok(serde_json::json!({
        "text": text,
        "extension": ".ts",
        "mappings": marshal_span_map(&map),
    }))
}

/// tsgo `scriptRange`: the body of the `<script>` tag.
fn script_range(content: &str) -> Result<(usize, usize), String> {
    let script_open = content
        .find("<script")
        .ok_or_else(|| "contentmappertest: missing <script> tag".to_owned())?;
    let open_end = content[script_open..]
        .find('>')
        .ok_or_else(|| "contentmappertest: unclosed <script> tag".to_owned())?;
    let start = script_open + open_end + 1;
    let close = content[start..]
        .find("</script>")
        .ok_or_else(|| "contentmappertest: missing </script> tag".to_owned())?;
    Ok((start, start + close))
}

/// tsgo `leadingImportRange`: the run of import declarations at the start
/// of the script's body, without the white space around it.
fn leading_import_range(content: &str, script_start: usize, script_end: usize) -> (usize, usize) {
    let bytes = content.as_bytes();
    let is_space = |byte: u8| matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
    let mut start = script_start;
    while start < script_end && is_space(bytes[start]) {
        start += 1;
    }
    let mut end = start;
    while end < script_end && content[end..script_end].starts_with("import ") {
        match content[end..script_end].find('\n') {
            None => {
                end = script_end;
                break;
            }
            Some(line_end) => end += line_end,
        }
        while end < script_end && is_space(bytes[end]) {
            end += 1;
        }
    }
    while end > start && is_space(bytes[end - 1]) {
        end -= 1;
    }
    (start, end)
}
