//! tsgo `lsp/progress_test.go`: the work done progress of long
//! operations. tsgo's fake time (`synctest`) becomes explicit steps of the
//! progress's state: the delay fires where tsgo's clock passes it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tsc_diagnostics::DiagnosticMessage;
use tsc_lsp::progress::{
    ProgressCall, ProgressReporter, ProgressState, ProgressValue, ProjectLoadingProgress,
};

const PROJECT: &str = "Project 'proj'";

fn create(token: &str) -> ProgressCall {
    ProgressCall::Create(token.to_owned())
}

fn begin(token: &str, message: &str) -> ProgressCall {
    ProgressCall::Send(
        token.to_owned(),
        ProgressValue::Begin {
            title: String::new(),
            message: message.to_owned(),
        },
    )
}

fn report(token: &str, message: &str) -> ProgressCall {
    ProgressCall::Send(
        token.to_owned(),
        ProgressValue::Report {
            message: message.to_owned(),
        },
    )
}

fn end(token: &str) -> ProgressCall {
    ProgressCall::Send(token.to_owned(), ProgressValue::End)
}

fn delay() -> ProgressState {
    ProgressState::new(Duration::from_millis(500))
}

/// tsgo `StartFinishBeforeDelay`: nothing for a fast operation.
#[test]
fn start_finish_before_delay() {
    let mut progress = delay();
    assert_eq!(progress.start(PROJECT), []);
    assert_eq!(progress.finish(PROJECT), []);
    // The timer is stopped: the delay never fires.
    assert!(!progress.timer_running());
}

/// tsgo `ShowsAfterDelay`.
#[test]
fn shows_after_delay() {
    let mut progress = delay();
    assert_eq!(progress.start(PROJECT), []);
    assert!(progress.timer_running());
    assert_eq!(
        progress.fire_delay(),
        [create("tsgo-loading-1"), begin("tsgo-loading-1", PROJECT)]
    );
    assert_eq!(progress.finish(PROJECT), [end("tsgo-loading-1")]);
}

/// tsgo `ReportsMultipleOperations`.
#[test]
fn reports_multiple_operations() {
    let mut progress = delay();
    progress.start("Project 'projA'");
    progress.start("Project 'projB'");
    assert_eq!(
        progress.fire_delay(),
        [
            create("tsgo-loading-1"),
            begin("tsgo-loading-1", "Project 'projA'")
        ]
    );
    // One finished: a report of the other.
    assert_eq!(
        progress.finish("Project 'projA'"),
        [report("tsgo-loading-1", "Project 'projB'")]
    );
    assert_eq!(progress.finish("Project 'projB'"), [end("tsgo-loading-1")]);
}

/// tsgo `RefCounting`.
#[test]
fn ref_counting() {
    let mut progress = delay();
    progress.start(PROJECT);
    progress.start(PROJECT);
    progress.fire_delay();
    let calls = progress.finish(PROJECT);
    assert!(!calls.contains(&end("tsgo-loading-1")), "{calls:?}");
    assert_eq!(progress.finish(PROJECT), [end("tsgo-loading-1")]);
}

/// tsgo `NewTokenAfterEnd`.
#[test]
fn new_token_after_end() {
    let mut progress = delay();
    progress.start(PROJECT);
    assert_eq!(progress.fire_delay()[0], create("tsgo-loading-1"));
    progress.finish(PROJECT);
    progress.start("Project 'proj2'");
    assert_eq!(progress.fire_delay()[0], create("tsgo-loading-2"));
    assert_eq!(progress.finish("Project 'proj2'"), [end("tsgo-loading-2")]);
}

/// tsgo `StartBeforeDelayThenMoreAfterDelay`: a start after the delay
/// reports at once.
#[test]
fn start_before_delay_then_more_after_delay() {
    let mut progress = ProgressState::new(Duration::from_millis(200));
    progress.start("Project 'projA'");
    assert_eq!(progress.fire_delay().len(), 2);
    assert_eq!(
        progress.start("Project 'projB'"),
        [report("tsgo-loading-1", "Project 'projB'")]
    );
    progress.finish("Project 'projA'");
    assert_eq!(progress.finish("Project 'projB'"), [end("tsgo-loading-1")]);
}

/// tsgo `FinishWithNoActiveToken`.
#[test]
fn finish_with_no_active_token() {
    let mut progress = delay();
    assert_eq!(progress.finish(PROJECT), []);
}

/// tsgo `ZeroDelay`: the progress begins at once.
#[test]
fn zero_delay() {
    let mut progress = ProgressState::new(Duration::ZERO);
    assert_eq!(
        progress.start(PROJECT),
        [create("tsgo-loading-1"), begin("tsgo-loading-1", PROJECT)]
    );
    assert!(!progress.timer_running());
    assert_eq!(progress.finish(PROJECT), [end("tsgo-loading-1")]);
}

/// tsgo `FinishBeforeDelayNoBegun`: no end without a begin.
#[test]
fn finish_before_delay_no_begun() {
    let mut progress = delay();
    progress.start(PROJECT);
    assert_eq!(progress.finish(PROJECT), []);
}

/// tsgo's fake reporter.
#[derive(Default)]
struct Reporter {
    calls: Mutex<Vec<(String, String, ProgressValue)>>,
}

impl ProgressReporter for Reporter {
    fn localize(&self, message: &'static DiagnosticMessage, args: &[String]) -> String {
        message.format_in(None, args).to_string_lossy().into_owned()
    }

    fn create_work_done_progress(&self, token: &str) {
        self.calls.lock().unwrap().push((
            "create".to_owned(),
            token.to_owned(),
            ProgressValue::End,
        ));
    }

    fn send_progress(&self, token: &str, value: ProgressValue) {
        self.calls
            .lock()
            .unwrap()
            .push(("send".to_owned(), token.to_owned(), value));
    }
}

/// tsgo `ZeroDelay` through the progress's thread: the begin is titled
/// tsgo's `Loading`, its message the localized operation.
#[test]
fn the_thread_titles_and_localizes_the_progress() {
    let reporter = Arc::new(Reporter::default());
    let progress = ProjectLoadingProgress::new(
        Arc::clone(&reporter) as Arc<dyn ProgressReporter>,
        Duration::ZERO,
    );
    progress.start(&tsc_diagnostics::gen::Project_0, vec!["proj".to_owned()]);
    progress.finish(&tsc_diagnostics::gen::Project_0, vec!["proj".to_owned()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while reporter.calls.lock().unwrap().len() < 3 {
        assert!(Instant::now() < deadline, "the progress's calls");
        std::thread::sleep(Duration::from_millis(5));
    }
    let calls = reporter.calls.lock().unwrap().clone();
    assert_eq!(calls[0].0, "create");
    assert_eq!(
        calls[1].2,
        ProgressValue::Begin {
            title: "Loading".to_owned(),
            message: "Project 'proj'".to_owned(),
        }
    );
    assert_eq!(calls[2].2, ProgressValue::End);
}
