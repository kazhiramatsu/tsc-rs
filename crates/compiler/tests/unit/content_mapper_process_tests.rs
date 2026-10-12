use super::*;
use std::sync::Arc;
use std::time::Duration;

use tsc_program::content_mapper::errors::{InitializeErrorKind, TransformFailure};
use tsc_program::content_mapper::host::{Host, Request};
use tsc_program::content_mapper::{Manifest, Mapper};

fn mapper(exec: Vec<String>) -> Arc<Mapper> {
    Arc::new(Mapper {
        manifest: Manifest {
            name: "shell".to_owned(),
            exec: Some(exec),
            ..Manifest::default()
        },
        package_directory: std::env::temp_dir().to_string_lossy().into_owned(),
        ..Mapper::default()
    })
}

#[test]
fn a_command_not_on_the_path_fails_to_start_as_go_reports_it() {
    let error = NativeSpawner
        .spawn(
            &["tsc-rs-no-such-mapper".to_owned()],
            &std::env::temp_dir().to_string_lossy(),
            Box::new(std::io::sink()),
        )
        .err()
        .expect("the command does not exist");
    assert_eq!(
        error,
        r#"exec: "tsc-rs-no-such-mapper": executable file not found in $PATH"#
    );
}

/// A mapper process written in POSIX shell: it answers `initialize`,
/// `openProject`, `transform` and `closeProject` in turn (the host's calls
/// are sequential here, so their IDs are `api1`…`api4`).
#[cfg(unix)]
#[test]
fn a_native_process_speaks_the_protocol_over_its_standard_streams() {
    const SCRIPT: &str = r#"
read_message() {
  length=0
  while IFS= read -r line; do
    line=$(printf '%s' "$line" | tr -d '\r')
    [ -z "$line" ] && break
    case "$line" in Content-Length:*) length=${line#Content-Length: } ;; esac
  done
  [ "$length" -gt 0 ] || exit 0
  dd bs=1 count="$length" 2>/dev/null >/dev/null
}
respond() {
  body="{\"jsonrpc\":\"2.0\",\"id\":\"api$1\",\"result\":$2}"
  printf 'Content-Length: %s\r\n\r\n%s' "${#body}" "$body"
}
echo "shell mapper started" >&2
read_message; respond 1 '{"positionEncoding":"utf-8","diagnosticSource":"shell"}'
read_message; respond 2 '{}'
read_message; respond 3 '{"text":"export {};","extension":".ts"}'
read_message; respond 4 'null'
read_message
"#;
    let logs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let host = Host::with_logger(Arc::new(NativeSpawner), "", {
        let logs = Arc::clone(&logs);
        Some(Arc::new(move |line: &str| {
            logs.lock().unwrap().push(line.to_owned())
        }))
    });
    let mapper = mapper(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        SCRIPT.to_owned(),
    ]);
    let output = host
        .transform(
            &mapper,
            Request {
                file_name: "/a.box",
                content: "anything",
            },
        )
        .unwrap();
    assert_eq!(output.canonical.text, "export {};");
    assert_eq!(output.canonical.virtual_extension, ".ts");
    host.close();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !logs
        .lock()
        .unwrap()
        .iter()
        .any(|line| line == "[content mapper: shell] stderr: shell mapper started")
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|line| line == "[content mapper: shell] stderr: shell mapper started"));
}

/// A process that exits before it answers `initialize` reports its exit
/// code when the host sees the exit before the failed call returns, else
/// the failed request (tsgo's `ExitCode` has the same race).
#[cfg(unix)]
#[test]
fn a_native_process_that_exits_before_initializing_reports_its_code() {
    let host = Host::new(Arc::new(NativeSpawner), "");
    let mapper = mapper(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "exit 3".to_owned(),
    ]);
    match host.transform(
        &mapper,
        Request {
            file_name: "/a.box",
            content: "",
        },
    ) {
        Err(TransformFailure::Initialize(error)) => assert!(
            matches!(
                error.kind,
                InitializeErrorKind::ProcessExit { exit_code: 3 }
                    | InitializeErrorKind::Request { .. }
            ),
            "{error:?}"
        ),
        result => panic!("expected an initialization failure, got {result:?}"),
    }
}
