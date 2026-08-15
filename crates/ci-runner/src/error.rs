use core::fmt;
use std::io;

/// The only blocking-effect phases recognized by the v1 runner boundary.
///
/// Later packets may add operations inside these phases, but they may not
/// invent a second failure channel or an ambient executor boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EffectPhase {
    Acquire,
    Read,
    Spawn,
    Execute,
    Join,
    Commit,
}

impl fmt::Display for EffectPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Acquire => "acquire",
            Self::Read => "read",
            Self::Spawn => "spawn",
            Self::Execute => "execute",
            Self::Join => "join",
            Self::Commit => "commit",
        };
        formatter.write_str(name)
    }
}

/// A stable, payload-free projection of `std::io::ErrorKind`.
///
/// Error text and OS-specific payloads are intentionally not retained. They
/// belong to a nonsemantic execution receipt, never to a reusable result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IoKind {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    AddrNotAvailable,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    InvalidInput,
    InvalidData,
    TimedOut,
    Interrupted,
    WriteZero,
    UnexpectedEof,
    Unsupported,
    OutOfMemory,
    Other,
}

impl From<io::ErrorKind> for IoKind {
    fn from(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::ConnectionRefused => Self::ConnectionRefused,
            io::ErrorKind::ConnectionReset => Self::ConnectionReset,
            io::ErrorKind::ConnectionAborted => Self::ConnectionAborted,
            io::ErrorKind::NotConnected => Self::NotConnected,
            io::ErrorKind::AddrInUse => Self::AddrInUse,
            io::ErrorKind::AddrNotAvailable => Self::AddrNotAvailable,
            io::ErrorKind::BrokenPipe => Self::BrokenPipe,
            io::ErrorKind::AlreadyExists => Self::AlreadyExists,
            io::ErrorKind::WouldBlock => Self::WouldBlock,
            io::ErrorKind::InvalidInput => Self::InvalidInput,
            io::ErrorKind::InvalidData => Self::InvalidData,
            io::ErrorKind::TimedOut => Self::TimedOut,
            io::ErrorKind::Interrupted => Self::Interrupted,
            io::ErrorKind::WriteZero => Self::WriteZero,
            io::ErrorKind::UnexpectedEof => Self::UnexpectedEof,
            io::ErrorKind::Unsupported => Self::Unsupported,
            io::ErrorKind::OutOfMemory => Self::OutOfMemory,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for IoKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::NotFound => "not-found",
            Self::PermissionDenied => "permission-denied",
            Self::ConnectionRefused => "connection-refused",
            Self::ConnectionReset => "connection-reset",
            Self::ConnectionAborted => "connection-aborted",
            Self::NotConnected => "not-connected",
            Self::AddrInUse => "address-in-use",
            Self::AddrNotAvailable => "address-not-available",
            Self::BrokenPipe => "broken-pipe",
            Self::AlreadyExists => "already-exists",
            Self::WouldBlock => "would-block",
            Self::InvalidInput => "invalid-input",
            Self::InvalidData => "invalid-data",
            Self::TimedOut => "timed-out",
            Self::Interrupted => "interrupted",
            Self::WriteZero => "write-zero",
            Self::UnexpectedEof => "unexpected-eof",
            Self::Unsupported => "unsupported",
            Self::OutOfMemory => "out-of-memory",
            Self::Other => "other",
        };
        formatter.write_str(name)
    }
}

/// Explicit host-to-runner cancellation reasons.
///
/// The runner never reads a global signal, clock, or provider singleton. The
/// host translates those events into one of these values at a checked safe
/// point in a later packet.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RunCancellation {
    UserRequested,
    ProviderRequested,
    DeadlineExpired,
}

impl fmt::Display for RunCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::UserRequested => "user-requested",
            Self::ProviderRequested => "provider-requested",
            Self::DeadlineExpired => "deadline-expired",
        };
        formatter.write_str(name)
    }
}

/// Closed family names for infrastructure failures.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InfraErrorFamily {
    Io,
    Transport,
    Spawn,
    Signal,
    Timeout,
    Cancelled,
    OutOfMemory,
    Panic,
    Quota,
    Guard,
    Race,
    Durability,
}

impl fmt::Display for InfraErrorFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Io => "io",
            Self::Transport => "transport",
            Self::Spawn => "spawn",
            Self::Signal => "signal",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::OutOfMemory => "out-of-memory",
            Self::Panic => "panic",
            Self::Quota => "quota",
            Self::Guard => "guard",
            Self::Race => "race",
            Self::Durability => "durability",
        };
        formatter.write_str(name)
    }
}

/// Infrastructure failure before semantic completion or authority commit.
///
/// This is intentionally `Copy`, payload-free, and closed. A backend may keep
/// rich diagnostics in an execution receipt, but cannot smuggle them into a
/// semantic observation or reinterpret an infrastructure failure as a miss.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InfraError {
    Io {
        phase: EffectPhase,
        kind: IoKind,
    },
    Transport {
        phase: EffectPhase,
    },
    Spawn {
        phase: EffectPhase,
    },
    Signal {
        phase: EffectPhase,
    },
    Timeout {
        phase: EffectPhase,
    },
    Cancelled {
        phase: EffectPhase,
        reason: RunCancellation,
    },
    OutOfMemory {
        phase: EffectPhase,
    },
    Panic {
        phase: EffectPhase,
    },
    Quota {
        phase: EffectPhase,
    },
    Guard {
        phase: EffectPhase,
    },
    Race {
        phase: EffectPhase,
    },
    Durability {
        phase: EffectPhase,
    },
}

impl InfraError {
    /// Converts an I/O error while retaining only its stable kind and phase.
    pub fn from_io(phase: EffectPhase, error: io::Error) -> Self {
        Self::Io {
            phase,
            kind: error.kind().into(),
        }
    }

    /// Converts an unwinding boundary into infrastructure failure without
    /// retaining panic text or payload data.
    pub const fn from_panic(phase: EffectPhase) -> Self {
        Self::Panic { phase }
    }

    pub const fn family(self) -> InfraErrorFamily {
        match self {
            Self::Io { .. } => InfraErrorFamily::Io,
            Self::Transport { .. } => InfraErrorFamily::Transport,
            Self::Spawn { .. } => InfraErrorFamily::Spawn,
            Self::Signal { .. } => InfraErrorFamily::Signal,
            Self::Timeout { .. } => InfraErrorFamily::Timeout,
            Self::Cancelled { .. } => InfraErrorFamily::Cancelled,
            Self::OutOfMemory { .. } => InfraErrorFamily::OutOfMemory,
            Self::Panic { .. } => InfraErrorFamily::Panic,
            Self::Quota { .. } => InfraErrorFamily::Quota,
            Self::Guard { .. } => InfraErrorFamily::Guard,
            Self::Race { .. } => InfraErrorFamily::Race,
            Self::Durability { .. } => InfraErrorFamily::Durability,
        }
    }

    pub const fn phase(self) -> EffectPhase {
        match self {
            Self::Io { phase, .. }
            | Self::Transport { phase }
            | Self::Spawn { phase }
            | Self::Signal { phase }
            | Self::Timeout { phase }
            | Self::Cancelled { phase, .. }
            | Self::OutOfMemory { phase }
            | Self::Panic { phase }
            | Self::Quota { phase }
            | Self::Guard { phase }
            | Self::Race { phase }
            | Self::Durability { phase } => phase,
        }
    }

    pub const fn is_cancelled(self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}

impl fmt::Display for InfraError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} infrastructure failure during {}",
            self.family(),
            self.phase()
        )
    }
}

impl std::error::Error for InfraError {}
