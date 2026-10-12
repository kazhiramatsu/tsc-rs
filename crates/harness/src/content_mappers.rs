//! tsgo testutil/contentmappertest: content mappers served in-process for
//! tests, chosen by the command their package declares, so a test runs the
//! whole protocol without a process.

use std::io::Write;
use std::sync::Mutex;

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

/// tsgo `NewSpawner`: each mapper process is its handler served over an
/// in-memory pipe.
#[derive(Clone, Copy, Debug, Default)]
pub struct TestSpawner;

impl Spawner for TestSpawner {
    fn spawn(
        &self,
        command: &[String],
        _dir: &str,
        _stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let mut handler = handler_for_mapper(command)?;
        if command[0] != DYNAMIC_VERBATIM_MAPPER {
            handler = Box::new(StaticProject(handler));
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

/// tsgo `handlerForMapper`.
fn handler_for_mapper(command: &[String]) -> Result<Box<dyn Handler>, String> {
    let Some(name) = command.first() else {
        return Err("contentmappertest: empty mapper command".to_owned());
    };
    match name.as_str() {
        VERBATIM_MAPPER => Ok(Box::new(Verbatim { extension: ".ts" })),
        MODULE_VERBATIM_MAPPER => Ok(Box::new(Verbatim { extension: ".mts" })),
        FAILING_MAPPER => Ok(Box::new(Failing)),
        SUPPLEMENTAL_MAPPER => Ok(Box::new(Supplemental)),
        DYNAMIC_VERBATIM_MAPPER => Ok(Box::new(DynamicVerbatim)),
        SYNTHESIZING_MAPPER => Ok(Box::new(Synthesizing)),
        _ => Err(format!(
            "contentmappertest: unknown mapper command {}",
            go_string_slice(command)
        )),
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
struct DynamicVerbatim;

impl Handler for DynamicVerbatim {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "openProject" => {
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
            "closeProject" => Ok(Payload::Json("null".to_owned())),
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
/// opens and closes projects with nothing to say, but for the options the
/// option diagnostics test passes.
struct StaticProject(Box<dyn Handler>);

impl Handler for StaticProject {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "openProject" => {
                let params: serde_json::Value =
                    serde_json::from_slice(params).map_err(|error| error.to_string())?;
                let options = params.get("options").map(serde_json::Value::to_string);
                let result = if options.as_deref() == Some(r#"{"plugins":[{"name":1}]}"#) {
                    serde_json::json!({ "optionDiagnostics": option_name_diagnostic() })
                } else {
                    serde_json::json!({})
                };
                Ok(Payload::Json(result.to_string()))
            }
            "closeProject" => Ok(Payload::Json("null".to_owned())),
            _ => self.0.handle_request(method, params),
        }
    }

    fn handle_notification(&self, method: &str, params: &[u8]) {
        self.0.handle_notification(method, params);
    }
}
