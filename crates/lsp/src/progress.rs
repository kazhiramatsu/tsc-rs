//! tsgo `lsp/progress.go`: the editor's work done progress for long
//! operations (loading a project, installing types). The indicator shows
//! only once an operation outlasts a delay, reports the operations still
//! running, and ends when the last one finishes.

use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tsc_diagnostics::DiagnosticMessage;

/// tsgo `progressReporter`: what the progress asks of the transport.
pub trait ProgressReporter: Send + Sync {
    /// tsgo `localize`: a message in the server's locale.
    fn localize(&self, message: &'static DiagnosticMessage, args: &[String]) -> String;
    /// tsgo `createWorkDoneProgress`: `window/workDoneProgress/create`.
    fn create_work_done_progress(&self, token: &str);
    /// tsgo `sendProgress`: `$/progress`.
    fn send_progress(&self, token: &str, value: ProgressValue);
}

/// tsgo `WorkDoneProgressBeginOrReportOrEnd`, as the progress sends it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressValue {
    Begin { title: String, message: String },
    Report { message: String },
    End,
}

/// tsgo `progressEvent`.
struct ProgressEvent {
    message: &'static DiagnosticMessage,
    args: Vec<String>,
    finish: bool,
}

/// tsgo `projectLoadingProgress`: events go to one thread, which owns the
/// state and sends the progress in order. `start` and `finish` may block
/// while 64 events wait, unless the thread is gone.
pub struct ProjectLoadingProgress {
    events: SyncSender<ProgressEvent>,
}

impl ProjectLoadingProgress {
    /// tsgo `newProjectLoadingProgressFromReporter`: the indicator shows
    /// `delay` after the first start.
    pub fn new(reporter: Arc<dyn ProgressReporter>, delay: Duration) -> Self {
        let (events, receiver) = mpsc::sync_channel::<ProgressEvent>(64);
        std::thread::Builder::new()
            .name("tsc-rs-progress".to_owned())
            .spawn(move || {
                let mut state = ProgressState::new(delay);
                let mut deadline: Option<Instant> = None;
                loop {
                    let event = match deadline {
                        Some(at) => match receiver
                            .recv_timeout(at.saturating_duration_since(Instant::now()))
                        {
                            Ok(event) => Some(event),
                            Err(RecvTimeoutError::Timeout) => None,
                            Err(RecvTimeoutError::Disconnected) => return,
                        },
                        None => match receiver.recv() {
                            Ok(event) => Some(event),
                            Err(_) => return,
                        },
                    };
                    let calls = match event {
                        Some(event) => {
                            let text = reporter.localize(event.message, &event.args);
                            if event.finish {
                                state.finish(&text)
                            } else {
                                state.start(&text)
                            }
                        }
                        None => state.fire_delay(),
                    };
                    for call in calls {
                        match call {
                            ProgressCall::Create(token) => {
                                reporter.create_work_done_progress(&token)
                            }
                            ProgressCall::Send(token, value) => {
                                let value = match value {
                                    ProgressValue::Begin { message, .. } => ProgressValue::Begin {
                                        title: reporter
                                            .localize(&tsc_diagnostics::gen::Loading, &[]),
                                        message,
                                    },
                                    value => value,
                                };
                                reporter.send_progress(&token, value);
                            }
                        }
                    }
                    deadline = match (state.timer, deadline) {
                        (true, Some(at)) => Some(at),
                        (true, None) => Some(Instant::now() + delay),
                        (false, _) => None,
                    };
                }
            })
            .expect("start the progress thread");
        Self { events }
    }

    /// tsgo `start`.
    pub fn start(&self, message: &'static DiagnosticMessage, args: Vec<String>) {
        // A gone thread (the server stopping) drops the event.
        let _ = self.events.send(ProgressEvent {
            message,
            args,
            finish: false,
        });
    }

    /// tsgo `finish`.
    pub fn finish(&self, message: &'static DiagnosticMessage, args: Vec<String>) {
        let _ = self.events.send(ProgressEvent {
            message,
            args,
            finish: true,
        });
    }
}

/// What a step of the progress sends (the begin's title is the thread's
/// to localize).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressCall {
    Create(String),
    Send(String, ProgressValue),
}

/// tsgo `projectLoadingProgress.run`'s state: the operations running, by
/// their text (counted, in the order they first started), the current
/// token, whether its begin was sent and whether the delay has passed.
#[derive(Debug)]
pub struct ProgressState {
    delay: Duration,
    loading: Vec<(String, usize)>,
    token: Option<String>,
    token_id: u64,
    begun: bool,
    delay_fired: bool,
    /// The delay's timer runs.
    timer: bool,
}

impl ProgressState {
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            loading: Vec::new(),
            token: None,
            token_id: 0,
            begun: false,
            delay_fired: false,
            timer: false,
        }
    }

    /// Whether the delay's timer runs (the thread fires it when due).
    pub fn timer_running(&self) -> bool {
        self.timer
    }

    /// An operation started: a new token with the first one (created at once
    /// without a delay, else after it), and a begin or report once the delay
    /// has passed.
    pub fn start(&mut self, text: &str) -> Vec<ProgressCall> {
        let mut calls = Vec::new();
        match self.loading.iter_mut().find(|(known, _)| known == text) {
            Some((_, count)) => *count += 1,
            None => self.loading.push((text.to_owned(), 1)),
        }
        if self.token.is_none() {
            self.token_id += 1;
            let token = format!("tsgo-loading-{}", self.token_id);
            self.begun = false;
            if self.delay.is_zero() {
                self.delay_fired = true;
                calls.push(ProgressCall::Create(token.clone()));
            } else {
                self.delay_fired = false;
                self.timer = true;
            }
            self.token = Some(token);
        }
        if self.delay_fired {
            self.begin_or_report(text.to_owned(), &mut calls);
        }
        calls
    }

    /// An operation finished: the end when it was the last (after a begin),
    /// else a report of the first still running.
    pub fn finish(&mut self, text: &str) -> Vec<ProgressCall> {
        let mut calls = Vec::new();
        if let Some(index) = self.loading.iter().position(|(known, _)| known == text) {
            if self.loading[index].1 <= 1 {
                self.loading.remove(index);
            } else {
                self.loading[index].1 -= 1;
            }
        }
        let Some(token) = self.token.clone() else {
            return calls;
        };
        if self.loading.is_empty() {
            if self.begun {
                calls.push(ProgressCall::Send(token, ProgressValue::End));
            }
            self.timer = false;
            self.token = None;
        } else if self.delay_fired {
            calls.push(ProgressCall::Send(
                token,
                ProgressValue::Report {
                    message: self.loading[0].0.clone(),
                },
            ));
        }
        calls
    }

    /// The delay passed: the token created and the first operation still
    /// running begun.
    pub fn fire_delay(&mut self) -> Vec<ProgressCall> {
        let mut calls = Vec::new();
        self.delay_fired = true;
        self.timer = false;
        if let (Some(token), Some((first, _))) = (self.token.clone(), self.loading.first()) {
            let first = first.clone();
            calls.push(ProgressCall::Create(token));
            self.begin_or_report(first, &mut calls);
        }
        calls
    }

    /// tsgo `beginOrReport`.
    fn begin_or_report(&mut self, text: String, calls: &mut Vec<ProgressCall>) {
        let token = self.token.clone().expect("a progress token");
        if self.begun {
            calls.push(ProgressCall::Send(
                token,
                ProgressValue::Report { message: text },
            ));
        } else {
            calls.push(ProgressCall::Send(
                token,
                ProgressValue::Begin {
                    title: String::new(),
                    message: text,
                },
            ));
        }
        self.begun = true;
    }
}
