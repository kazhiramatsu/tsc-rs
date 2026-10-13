//! tsgo `lsp/logger.go`: the server's log, sent as `window/logMessage` at
//! the client's verbosity (to the standard error until the client starts
//! initializing the server).

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};

use tsc_lsproto::{self as lsproto, LogVerbosity, MessageType};

use crate::server::Server;

pub struct Logger {
    verbosity: Mutex<LogVerbosity>,
}

impl Default for Logger {
    fn default() -> Self {
        Self {
            verbosity: Mutex::new(LogVerbosity::INFO),
        }
    }
}

/// tsgo `maxVerbosityForMessageType`.
fn max_verbosity_for_message_type(message_type: MessageType) -> LogVerbosity {
    match message_type {
        MessageType::ERROR => LogVerbosity::ERROR,
        MessageType::WARNING => LogVerbosity::WARNING,
        MessageType::INFO => LogVerbosity::INFO,
        MessageType::DEBUG => LogVerbosity::DEBUG,
        _ => LogVerbosity::INFO,
    }
}

/// tsgo `isValidLogVerbosity`.
pub fn is_valid_log_verbosity(verbosity: LogVerbosity) -> bool {
    verbosity >= LogVerbosity::OFF && verbosity <= LogVerbosity::ERROR
}

impl Logger {
    fn verbosity(&self) -> LogVerbosity {
        *self
            .verbosity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// tsgo `SetVerbosity`.
    pub fn set_verbosity(&self, verbosity: LogVerbosity) {
        *self
            .verbosity
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = verbosity;
    }

    /// tsgo `IsTracing`.
    pub fn is_tracing(&self) -> bool {
        self.verbosity() == LogVerbosity::TRACE
    }

    /// tsgo `sendLogMessage`.
    fn send(&self, server: &Server, message_type: MessageType, message: &str) {
        if !server.init_started.load(Ordering::SeqCst) {
            let mut stderr = server.stderr.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(stderr, "{message}");
            return;
        }
        let verbosity = self.verbosity();
        if verbosity == LogVerbosity::OFF
            || verbosity > max_verbosity_for_message_type(message_type)
        {
            return;
        }
        let params = lsproto::LogMessageParams {
            r#type: message_type,
            message: message.to_owned(),
        };
        if server
            .send_notification::<lsproto::WindowLogMessageInfo>(&params)
            .is_err()
        {
            let mut stderr = server.stderr.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(stderr, "{message}");
        }
    }

    /// tsgo `Log`.
    pub fn log(&self, server: &Server, message: &str) {
        self.send(server, MessageType::INFO, message);
    }

    /// tsgo `Info`.
    pub fn info(&self, server: &Server, message: &str) {
        self.send(server, MessageType::INFO, message);
    }

    /// tsgo `Warn`.
    pub fn warn(&self, server: &Server, message: &str) {
        self.send(server, MessageType::WARNING, message);
    }

    /// tsgo `Error`.
    pub fn error(&self, server: &Server, message: &str) {
        self.send(server, MessageType::ERROR, message);
    }
}
