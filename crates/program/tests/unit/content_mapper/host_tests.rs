//! tsgo host_test.go's process, project and handshake cases, over mappers
//! served in-process.

use super::*;
use crate::content_mapper::{Definition, Manifest};
use std::sync::atomic::AtomicUsize;
use tsc_host::ipc::jsonrpc::MessageWriter;
use tsc_host::ipc::pipe::{pipe, PipeWriter};
use tsc_host::ipc::Id;

/// A writer the process control can cut, ending the reader's input.
#[derive(Clone)]
struct Cuttable(Arc<Mutex<Option<PipeWriter>>>);

impl Write for Cuttable {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self.0.lock().unwrap().as_mut() {
            Some(writer) => writer.write(bytes),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct FakeControl {
    to_host: Cuttable,
    closes: Arc<AtomicUsize>,
    closed: AtomicBool,
    /// The exit code from the start, or once closed.
    exit_code: Option<i32>,
    exit_on_close: Option<i32>,
}

impl ProcessControl for FakeControl {
    fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.to_host.0.lock().unwrap().take();
        }
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code.or_else(|| {
            self.exit_on_close
                .filter(|_| self.closed.load(Ordering::SeqCst))
        })
    }
}

type Behavior = Arc<dyn Fn(&str, &[u8]) -> Result<String, String> + Send + Sync>;

struct FakeHandler(Behavior);

impl Handler for FakeHandler {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        (self.0)(method, params).map(Payload::Json)
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `fakeSpawner`: serves each spawn with an in-process mapper.
struct FakeSpawner {
    behavior: Behavior,
    spawns: AtomicUsize,
    closes: Arc<AtomicUsize>,
    stderr_text: Option<&'static str>,
}

impl FakeSpawner {
    fn new(behavior: Behavior) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            spawns: AtomicUsize::new(0),
            closes: Arc::new(AtomicUsize::new(0)),
            stderr_text: None,
        })
    }
}

impl Spawner for FakeSpawner {
    fn spawn(
        &self,
        _command: &[String],
        _dir: &str,
        mut stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        self.spawns.fetch_add(1, Ordering::SeqCst);
        if let Some(text) = self.stderr_text {
            stderr.write_all(text.as_bytes()).unwrap();
        }
        let (host_reader, host_writer) = pipe();
        let (mapper_reader, mapper_writer) = pipe();
        let to_host = Cuttable(Arc::new(Mutex::new(Some(host_writer))));
        let server = AsyncConn::new(
            to_host.clone(),
            Box::new(FakeHandler(Arc::clone(&self.behavior))),
        );
        std::thread::spawn(move || {
            let _ = server.run(&mut MessageReader::new(mapper_reader));
        });
        Ok(MapperProcess {
            stdout: Box::new(host_reader),
            stdin: Box::new(mapper_writer),
            control: Box::new(FakeControl {
                to_host,
                closes: Arc::clone(&self.closes),
                closed: AtomicBool::new(false),
                exit_code: None,
                exit_on_close: None,
            }),
        })
    }
}

/// tsgo `fakeMapper` (with `noOpProjectMapper`): transforms verbatim and
/// reports one diagnostic.
fn fake_mapper() -> Behavior {
    Arc::new(|method, params| match method {
        METHOD_INITIALIZE => {
            Ok(r#"{"positionEncoding":"utf-8","diagnosticSource":"vue"}"#.to_owned())
        }
        METHOD_OPEN_PROJECT => Ok("{}".to_owned()),
        METHOD_CLOSE_PROJECT => Ok("null".to_owned()),
        METHOD_TRANSFORM => {
            let params: serde_json::Value = serde_json::from_slice(params).unwrap();
            let content = params["content"].as_str().unwrap();
            let length = content.len();
            Ok(serde_json::json!({
                "text": content,
                "extension": ".ts",
                "mappings": [[0, length, 0, length, 0]],
                "diagnostics": [{"messageText": "boom", "start": 0, "length": length.min(3), "code": 9999}],
            })
            .to_string())
        }
        method => Err(format!("unexpected method {method}")),
    })
}

fn mapper(package: &str, name: &str, version: &str) -> Arc<Mapper> {
    Arc::new(Mapper {
        definition: Definition {
            package: package.to_owned(),
            ..Definition::default()
        },
        manifest: Manifest {
            name: name.to_owned(),
            version: version.to_owned(),
            exec: Some(vec!["mapper".to_owned()]),
            ..Manifest::default()
        },
        ..Mapper::default()
    })
}

fn request() -> Request<'static> {
    Request {
        file_name: "/a.vue",
        content: "export const x = 1;",
    }
}

fn simple(file_name: &'static str, content: &'static str) -> Request<'static> {
    Request { file_name, content }
}

#[test]
fn a_transform_runs_through_the_mapper_s_connection() {
    let host = Host::new(FakeSpawner::new(fake_mapper()), "");
    let output = host
        .transform(&mapper("", "vue", "1.0.0"), request())
        .unwrap();
    assert_eq!(output.canonical.text, "export const x = 1;");
    assert_eq!(output.canonical.virtual_extension, ".ts");
    assert_eq!(output.canonical.mappings.segments().len(), 1);
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        (
            output.diagnostics[0].code,
            output.diagnostics[0].source.as_str()
        ),
        (9999, "vue")
    );
    host.close();
}

#[test]
fn logging_records_the_protocol_and_the_mapper_s_stderr() {
    let logs = Arc::new(Mutex::new(Vec::<String>::new()));
    let logger: Logger = {
        let logs = Arc::clone(&logs);
        Arc::new(move |line: &str| logs.lock().unwrap().push(line.to_owned()))
    };
    let spawner = Arc::new(FakeSpawner {
        behavior: fake_mapper(),
        spawns: AtomicUsize::new(0),
        closes: Arc::new(AtomicUsize::new(0)),
        stderr_text: Some("mapper diagnostic\n"),
    });
    let host = Host::with_logger(spawner, "", Some(logger));
    host.transform(&mapper("configured", "resolved", "1.0.0"), request())
        .unwrap();
    host.close();
    let joined = logs.lock().unwrap().join("\n");
    assert!(
        joined.contains(
            r#"[content mapper: resolved] send: {"jsonrpc":"2.0","id":"api1","method":"initialize""#
        ),
        "{joined}"
    );
    assert!(
        joined.contains(
            r#"[content mapper: resolved] receive: {"jsonrpc":"2.0","id":"api1","result":"#
        ),
        "{joined}"
    );
    assert!(
        joined.contains("[content mapper: resolved] stderr: mapper diagnostic"),
        "{joined}"
    );
}

/// A spawner whose process answers nothing: it exits at once with
/// `exit_code`, or reads until it is closed (then exiting with 1).
struct SilentSpawner {
    exit_code: Option<i32>,
}

impl Spawner for SilentSpawner {
    fn spawn(
        &self,
        _command: &[String],
        _dir: &str,
        _stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let (host_reader, host_writer) = pipe();
        let (mut mapper_reader, mapper_writer) = pipe();
        let to_host = Cuttable(Arc::new(Mutex::new(Some(host_writer))));
        if self.exit_code.is_some() {
            to_host.0.lock().unwrap().take();
        } else {
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut mapper_reader, &mut std::io::sink());
            });
        }
        Ok(MapperProcess {
            stdout: Box::new(host_reader),
            stdin: Box::new(mapper_writer),
            control: Box::new(FakeControl {
                to_host,
                closes: Arc::new(AtomicUsize::new(0)),
                closed: AtomicBool::new(false),
                exit_code: self.exit_code,
                exit_on_close: Some(1),
            }),
        })
    }
}

fn initialize_failure(result: Result<TransformOutput, TransformFailure>) -> InitializeErrorKind {
    match result {
        Err(TransformFailure::Initialize(error)) => error.kind,
        result => panic!("expected an initialization failure, got {result:?}"),
    }
}

#[test]
fn a_process_that_exits_before_initializing_reports_its_code() {
    let host = Host::new(
        Arc::new(SilentSpawner {
            exit_code: Some(42),
        }),
        "",
    );
    assert_eq!(
        initialize_failure(host.transform(&mapper("", "mapper", ""), request())),
        InitializeErrorKind::ProcessExit { exit_code: 42 }
    );
}

#[test]
fn an_unanswered_initialize_times_out_before_the_process_is_closed() {
    let host = Host::new(Arc::new(SilentSpawner { exit_code: None }), "");
    assert_eq!(
        initialize_failure(host.transform(&mapper("", "mapper", ""), request())),
        InitializeErrorKind::NoResponse {
            timeout_seconds: INITIALIZE_TIMEOUT_SECONDS
        }
    );
}

/// A spawner whose process initializes, then writes a malformed frame.
struct GarbageSpawner {
    closes: Arc<AtomicUsize>,
}

impl Spawner for GarbageSpawner {
    fn spawn(
        &self,
        _command: &[String],
        _dir: &str,
        _stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let (host_reader, host_writer) = pipe();
        let (mapper_reader, mapper_writer) = pipe();
        let to_host = Cuttable(Arc::new(Mutex::new(Some(host_writer))));
        let mut writer = to_host.clone();
        std::thread::spawn(move || {
            let mut reader = MessageReader::new(mapper_reader);
            let message = reader.read_message().unwrap().unwrap();
            MessageWriter::new(writer.clone())
                .write_response(
                    message.id.as_ref().unwrap_or(&Id::Int(0)),
                    &Payload::Json(
                        r#"{"positionEncoding":"utf-8","diagnosticSource":"mapper"}"#.to_owned(),
                    ),
                )
                .unwrap();
            let _ = writer.write_all(b"oops\n");
        });
        Ok(MapperProcess {
            stdout: Box::new(host_reader),
            stdin: Box::new(mapper_writer),
            control: Box::new(FakeControl {
                to_host,
                closes: Arc::clone(&self.closes),
                closed: AtomicBool::new(false),
                exit_code: None,
                exit_on_close: None,
            }),
        })
    }
}

#[test]
fn a_failed_read_loop_closes_the_process() {
    let closes = Arc::new(AtomicUsize::new(0));
    let host = Host::new(
        Arc::new(GarbageSpawner {
            closes: Arc::clone(&closes),
        }),
        "",
    );
    assert!(host
        .transform(&mapper("", "mapper", ""), request())
        .is_err());
    let deadline = Instant::now() + Duration::from_secs(1);
    while closes.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

fn spec(
    config_file_name: &str,
    mappers: &[&Arc<Mapper>],
    options: &Arc<GoJson>,
) -> Arc<ProjectSpec> {
    Arc::new(ProjectSpec {
        config_file_name: config_file_name.to_owned(),
        mappers: mappers.iter().map(|mapper| Arc::clone(mapper)).collect(),
        compiler_options: Arc::clone(options),
    })
}

fn no_options() -> Arc<GoJson> {
    Arc::new(GoJson::Object(Vec::new()))
}

#[test]
fn mappers_with_one_identity_share_a_process() {
    let spawner = FakeSpawner::new(fake_mapper());
    let host = Host::new(Arc::clone(&spawner) as Arc<dyn Spawner>, "");
    let vue_a = mapper("a", "vue", "1.0.0");
    let vue_b = mapper("b", "vue", "1.0.0");
    let svelte = mapper("", "svelte", "2.0.0");
    let project = host
        .project(spec("", &[&vue_a, &vue_b, &svelte], &no_options()))
        .unwrap();
    for mapper in [&vue_a, &vue_b, &vue_a, &svelte] {
        project.transform(mapper, simple("/x", "y")).unwrap();
    }
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 2);
    project.close().unwrap();
    host.close();
}

#[test]
fn acquisitions_keep_a_process_until_its_last_release() {
    let spawner = FakeSpawner::new(fake_mapper());
    let host = Host::new(Arc::clone(&spawner) as Arc<dyn Spawner>, "");
    let vue_a = mapper("a", "vue", "1.0.0");
    let vue_b = mapper("b", "vue", "1.0.0");
    let svelte = mapper("", "svelte", "2.0.0");
    let release_vue_a = host.acquire(&[Arc::clone(&vue_a), Arc::clone(&vue_a)]);
    let release_vue_b = host.acquire(&[Arc::clone(&vue_b)]);
    let release_svelte = host.acquire(&[Arc::clone(&svelte)]);
    for mapper in [&vue_a, &svelte] {
        host.transform(mapper, simple("/x", "y")).unwrap();
    }
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 2);
    let closes = || spawner.closes.load(Ordering::SeqCst);
    drop(release_vue_a);
    assert_eq!(closes(), 0, "the shared vue process stays owned");
    drop(release_svelte);
    assert_eq!(closes(), 1, "the last release closes the process");
    drop(release_vue_b);
    assert_eq!(closes(), 2, "the last vue owner closes it once");
    let release_new = host.acquire(&[Arc::clone(&vue_a)]);
    host.transform(&vue_a, simple("/x", "y")).unwrap();
    assert_eq!(
        spawner.spawns.load(Ordering::SeqCst),
        3,
        "a fresh process starts lazily"
    );
    drop(release_new);
    assert_eq!(closes(), 3);
}

/// tsgo `recordingMapper`: records the project protocol.
#[derive(Default)]
struct Recorded {
    project_handles: Vec<String>,
    closed_handles: Vec<String>,
    transform_handle: String,
    transform_params: String,
    compiler_options: String,
    options: String,
    locale: String,
}

#[derive(Clone, Default)]
struct Recording {
    recorded: Arc<Mutex<Recorded>>,
    dynamic_config: bool,
    watched_files: Option<Vec<String>>,
    config_identity: Option<String>,
    option_diagnostics: Option<String>,
}

impl Recording {
    fn behavior(&self) -> Behavior {
        let recording = self.clone();
        Arc::new(move |method, params| {
            let params: serde_json::Value = serde_json::from_slice(params).unwrap_or_default();
            let mut recorded = recording.recorded.lock().unwrap();
            match method {
                METHOD_INITIALIZE => {
                    recorded.locale = params["locale"].as_str().unwrap_or_default().to_owned();
                    Ok(r#"{"positionEncoding":"utf-8","diagnosticSource":"mapper"}"#.to_owned())
                }
                METHOD_OPEN_PROJECT => {
                    recorded
                        .project_handles
                        .push(params["projectHandle"].as_str().unwrap().to_owned());
                    recorded.compiler_options = params["compilerOptions"].to_string();
                    recorded.options = params
                        .get("options")
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let mut watched = recording.watched_files.clone();
                    if recording.dynamic_config && watched.is_none() {
                        let config = params["configFileName"].as_str().unwrap();
                        let directory = &config[..config.rfind('/').unwrap()];
                        watched = Some(vec![format!("{directory}/mapper.config.js")]);
                    }
                    let mut config_identity = if recording.dynamic_config {
                        format!("config:{}", recorded.options)
                    } else {
                        String::new()
                    };
                    if let Some(identity) = &recording.config_identity {
                        config_identity = identity.clone();
                    }
                    Ok(format!(
                        r#"{{"configIdentity":{},"watchedFiles":{},"optionDiagnostics":{}}}"#,
                        serde_json::to_string(&config_identity).unwrap(),
                        serde_json::to_string(&watched.unwrap_or_default()).unwrap(),
                        recording.option_diagnostics.as_deref().unwrap_or("[]")
                    ))
                }
                METHOD_CLOSE_PROJECT => {
                    recorded
                        .closed_handles
                        .push(params["projectHandle"].as_str().unwrap().to_owned());
                    Ok("null".to_owned())
                }
                METHOD_TRANSFORM => {
                    recorded.transform_handle =
                        params["projectHandle"].as_str().unwrap().to_owned();
                    recorded.transform_params = params.to_string();
                    Ok(
                        serde_json::json!({"text": params["content"], "extension": ".ts"})
                            .to_string(),
                    )
                }
                method => Err(format!("unexpected method {method}")),
            }
        })
    }
}

fn dynamic(options: &str, compiler_options: Option<Vec<String>>) -> Arc<Mapper> {
    Arc::new(Mapper {
        definition: Definition {
            options: Some(options.to_owned()),
            ..Definition::default()
        },
        manifest: Manifest {
            name: "dynamic".to_owned(),
            version: "1.0.0".to_owned(),
            exec: Some(vec!["mapper".to_owned()]),
            compiler_options,
            dynamic_config: true,
        },
        ..Mapper::default()
    })
}

#[test]
fn projects_open_lazily_share_processes_and_close_with_their_last_view() {
    let recording = Recording {
        dynamic_config: true,
        ..Recording::default()
    };
    let spawner = FakeSpawner::new(recording.behavior());
    let host = Host::new(Arc::clone(&spawner) as Arc<dyn Spawner>, "");
    let spawns = || spawner.spawns.load(Ordering::SeqCst);

    let static_mapper = Arc::new(Mapper {
        definition: Definition {
            options: Some(r#"{"mode":"static"}"#.to_owned()),
            ..Definition::default()
        },
        manifest: Manifest {
            name: "static".to_owned(),
            version: "1.0.0".to_owned(),
            exec: Some(vec!["mapper".to_owned()]),
            ..Manifest::default()
        },
        ..Mapper::default()
    });
    let static_project = host
        .project(spec(
            "/repo/tsconfig.json",
            &[&static_mapper],
            &no_options(),
        ))
        .unwrap();
    assert_eq!(static_project.identities().unwrap().len(), 1);
    assert_eq!(spawns(), 0, "a static identity starts no mapper");
    static_project.close().unwrap();

    let dynamic_a = dynamic(r#"{"mode":"a"}"#, Some(vec!["jsx".to_owned()]));
    let dynamic_b = dynamic(r#"{"mode":"b"}"#, None);
    let options_a = no_options();
    let project_a = host
        .project(spec(
            "/repo/a/tsconfig.json",
            &[&dynamic_a, &dynamic_b],
            &options_a,
        ))
        .unwrap();
    let project_a_reversed = host
        .project(spec(
            "/repo/reversed/tsconfig.json",
            &[&dynamic_b, &dynamic_a],
            &options_a,
        ))
        .unwrap();
    let project_jsx = host
        .project(spec(
            "/repo/options/tsconfig.json",
            &[&dynamic_a],
            &Arc::new(GoJson::Object(vec![("jsx".to_owned(), GoJson::number(3))])),
        ))
        .unwrap();
    let project_b = host
        .project(spec("/repo/b/tsconfig.json", &[&dynamic_a], &no_options()))
        .unwrap();
    let project_a_again = host
        .project(spec(
            "/repo/a/tsconfig.json",
            &[&dynamic_a, &dynamic_b],
            &options_a,
        ))
        .unwrap();
    assert_eq!(spawns(), 0, "views of dynamic projects start no mapper");
    let a = project_a.identities().unwrap();
    let reversed = project_a_reversed.identities().unwrap();
    let jsx = project_jsx.identities().unwrap();
    let b = project_b.identities().unwrap();
    assert_eq!((a.len(), reversed.len(), b.len()), (2, 2, 1));
    assert_eq!((&a[0], &a[1]), (&reversed[1], &reversed[0]));
    assert_ne!(a[0], jsx[0]);
    assert_eq!(spawns(), 1, "dynamic projects share one mapper process");
    assert_eq!(
        project_a.watched_files().unwrap(),
        ["/repo/a/mapper.config.js"]
    );
    assert_eq!(
        project_b.watched_files().unwrap(),
        ["/repo/b/mapper.config.js"]
    );

    project_a
        .transform(&dynamic_b, simple("/repo/a/file.ext", "x"))
        .unwrap();
    {
        let recorded = recording.recorded.lock().unwrap();
        assert!(recorded.project_handles[..2].contains(&recorded.transform_handle));
    }
    for project in [
        project_a_again,
        project_a,
        project_a_reversed,
        project_jsx,
        project_b,
    ] {
        project.close().unwrap();
    }
    let timings = host.timings();
    let dynamic = timings.mappers[&dynamic_a.identity()];
    assert_eq!(
        (
            dynamic.spawn.count,
            dynamic.initialize.count,
            dynamic.open_project.count,
            dynamic.transform.count,
            dynamic.close_project.count
        ),
        (1, 1, 6, 1, 6)
    );
    {
        let recorded = recording.recorded.lock().unwrap();
        assert_eq!(
            (
                recorded.project_handles.len(),
                recorded.closed_handles.len()
            ),
            (6, 6)
        );
    }
    host.close();
}

#[test]
fn project_views_after_the_host_closes_are_empty() {
    let recording = Recording {
        dynamic_config: true,
        ..Recording::default()
    };
    let host = Host::new(FakeSpawner::new(recording.behavior()), "");
    let mapper = dynamic("{}", None);
    let project = host
        .project(spec("/repo/tsconfig.json", &[&mapper], &no_options()))
        .unwrap();
    project
        .transform(&mapper, simple("/repo/file.ext", "x"))
        .unwrap();
    assert_eq!(project.identity(&Arc::new(Mapper::default())).unwrap(), "");
    host.close();
    project.refresh().unwrap();
    assert!(project.identities().unwrap().is_empty());
    assert_eq!(project.identity(&mapper).unwrap(), "");
    assert!(project.watched_files().unwrap().is_empty());
    assert!(project.diagnostics().is_empty());
    let failure = project
        .transform(&mapper, simple("/repo/file.ext", "x"))
        .unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("content mapper project is closed"),
        "{failure}"
    );
}

fn project_failure(recording: Recording, mapper: &Arc<Mapper>) -> ProjectErrorKind {
    let host = Host::new(FakeSpawner::new(recording.behavior()), "");
    let project = host
        .project(spec("/repo/tsconfig.json", &[mapper], &no_options()))
        .unwrap();
    match project.transform(mapper, simple("/repo/file.ext", "x")) {
        Err(TransformFailure::Project(ProjectFailure::Project(kind))) => kind,
        result => panic!("expected a project failure, got {result:?}"),
    }
}

#[test]
fn project_responses_that_break_the_protocol_are_rejected() {
    let dynamic_mapper = dynamic("{}", None);
    let static_mapper = mapper("", "static", "1.0.0");
    assert_eq!(
        project_failure(
            Recording {
                dynamic_config: true,
                watched_files: Some(vec!["mapper.config.js".to_owned()]),
                ..Recording::default()
            },
            &dynamic_mapper
        ),
        ProjectErrorKind::NonAbsoluteWatchedFile
    );
    assert_eq!(
        project_failure(
            Recording {
                dynamic_config: true,
                config_identity: Some(String::new()),
                ..Recording::default()
            },
            &dynamic_mapper
        ),
        ProjectErrorKind::MissingConfigIdentity
    );
    assert_eq!(
        project_failure(
            Recording {
                config_identity: Some("dynamic".to_owned()),
                ..Recording::default()
            },
            &static_mapper
        ),
        ProjectErrorKind::UnexpectedConfigIdentity
    );
    assert_eq!(
        project_failure(
            Recording {
                watched_files: Some(vec!["/repo/mapper.config.js".to_owned()]),
                ..Recording::default()
            },
            &static_mapper
        ),
        ProjectErrorKind::UnexpectedWatchedFiles
    );
    assert_eq!(
        project_failure(
            Recording {
                option_diagnostics: Some(
                    r#"[{"path":[null],"messageText":"Invalid option.","code":123}]"#.to_owned()
                ),
                ..Recording::default()
            },
            &static_mapper
        ),
        ProjectErrorKind::MalformedResponse
    );
}

#[test]
fn projects_forward_their_options_and_the_locale() {
    let recording = Recording::default();
    let host = Host::new(FakeSpawner::new(recording.behavior()), "cs-CZ");
    let mapper = Arc::new(Mapper {
        definition: Definition {
            options: Some(r#"{"strictTemplates":true}"#.to_owned()),
            ..Definition::default()
        },
        manifest: Manifest {
            name: "vue".to_owned(),
            version: "1.0.0".to_owned(),
            exec: Some(vec!["vue-mapper".to_owned()]),
            compiler_options: Some(vec!["target".to_owned(), "jsx".to_owned()]),
            dynamic_config: false,
        },
        ..Mapper::default()
    });
    let options = Arc::new(GoJson::Object(vec![
        ("strict".to_owned(), GoJson::Bool(true)),
        ("target".to_owned(), GoJson::number(7)),
    ]));
    let project = host.project(spec("", &[&mapper], &options)).unwrap();
    project.transform(&mapper, simple("/a.vue", "x")).unwrap();
    let recorded = recording.recorded.lock().unwrap();
    assert_eq!(recorded.compiler_options, r#"{"strict":true,"target":7}"#);
    assert_eq!(recorded.options, r#"{"strictTemplates":true}"#);
    assert_eq!(recorded.locale, "cs-CZ");
    assert!(!recorded.transform_params.contains(r#""options""#));
    assert!(!recorded.transform_params.contains(r#""compilerOptions""#));
}

#[test]
fn the_handshake_rejects_unusable_initialize_results() {
    let mut cases = vec![
        (
            r#"{"positionEncoding":"utf-32","diagnosticSource":"mapper"}"#.to_owned(),
            "unsupported position encoding",
        ),
        (
            r#"{"positionEncoding":"utf-8","diagnosticSource":" "}"#.to_owned(),
            "diagnostic source must not be empty",
        ),
    ];
    for source in [
        "ts",
        "TS",
        "d.ts",
        "json",
        "typescript",
        "TypeScript",
        "tsc",
        "TSC",
    ] {
        cases.push((
            format!(r#"{{"positionEncoding":"utf-8","diagnosticSource":"{source}"}}"#),
            "is reserved by TypeScript",
        ));
    }
    for (result, expected) in cases {
        let answer = result.clone();
        let behavior: Behavior = Arc::new(move |method, _| match method {
            METHOD_INITIALIZE => Ok(answer.clone()),
            _ => Ok("{}".to_owned()),
        });
        let host = Host::new(FakeSpawner::new(behavior), "");
        let failure = host
            .transform(&mapper("", "invalid", ""), simple("/a.vue", "x"))
            .unwrap_err();
        assert!(
            failure.to_string().contains(expected),
            "{result}: {failure}"
        );
    }
}
