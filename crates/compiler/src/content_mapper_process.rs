//! The native system's content mapper processes (tsgo cmd/tsc/sys.go
//! `spawnProcess`): a mapper's command runs as a child process in its
//! package directory, speaking the protocol on its standard streams.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError};

use tsc_program::content_mapper::host::{MapperProcess, ProcessControl, Spawner};

/// tsgo `osSys.Spawn`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSpawner;

impl Spawner for NativeSpawner {
    fn spawn(
        &self,
        command: &[String],
        dir: &str,
        mut stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let (program, arguments) = command
            .split_first()
            .ok_or_else(|| "no command to run".to_owned())?;
        let mut child = Command::new(program)
            .args(arguments)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| spawn_error(program, &error))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut child_stderr = child.stderr.take().expect("stderr is piped");
        let _ = std::thread::Builder::new()
            .name("tsc-rs-content-mapper-stderr".to_owned())
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                while let Ok(count) = child_stderr.read(&mut buffer) {
                    if count == 0 || stderr.write_all(&buffer[..count]).is_err() {
                        break;
                    }
                }
            });
        Ok(MapperProcess {
            stdout: Box::new(stdout),
            stdin: Box::new(stdin),
            control: Box::new(ChildControl {
                child: Mutex::new(ChildState {
                    child,
                    exit_code: None,
                }),
            }),
        })
    }
}

/// Go `exec.Command`'s start errors: a bare name not found on `PATH` is
/// `exec: "name": executable file not found in $PATH`.
fn spawn_error(program: &str, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound && !program.contains(['/', '\\']) {
        format!("exec: {program:?}: executable file not found in $PATH")
    } else {
        error.to_string()
    }
}

struct ChildState {
    child: Child,
    exit_code: Option<i32>,
}

/// tsgo `childProcess`: closing kills and reaps the process; its exit code
/// is known once it has exited.
struct ChildControl {
    child: Mutex<ChildState>,
}

impl ProcessControl for ChildControl {
    fn close(&self) {
        let mut state = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        if state.exit_code.is_some() {
            return;
        }
        let _ = state.child.kill();
        if let Ok(status) = state.child.wait() {
            state.exit_code = Some(status.code().unwrap_or(-1));
        }
    }

    fn exit_code(&self) -> Option<i32> {
        let mut state = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        if state.exit_code.is_none() {
            if let Ok(Some(status)) = state.child.try_wait() {
                state.exit_code = Some(status.code().unwrap_or(-1));
            }
        }
        state.exit_code
    }
}

#[cfg(test)]
#[path = "../tests/unit/content_mapper_process_tests.rs"]
mod content_mapper_process_tests;
