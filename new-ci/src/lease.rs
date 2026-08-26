//! Advisory leases with opaque owner tokens and monotonic fencing epochs.
//!
//! Worker clocks never participate in authorization.  The coordinator's
//! monotonic clock confirms expiry, and every protected mutation carries both
//! the owner token and epoch from a lease claim.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// A validated `(namespace, key)` advisory-lock scope.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LeaseScope {
    namespace: String,
    key: String,
}

impl LeaseScope {
    pub fn new(namespace: impl Into<String>, key: impl Into<String>) -> Result<Self, LeaseError> {
        let scope = Self {
            namespace: namespace.into(),
            key: key.into(),
        };
        if scope.namespace.is_empty() {
            return Err(LeaseError::InvalidScope("namespace"));
        }
        if scope.key.is_empty() {
            return Err(LeaseError::InvalidScope("key"));
        }
        Ok(scope)
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

/// An opaque, worker-generated lease owner token.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OwnerToken([u8; 32]);

impl OwnerToken {
    /// Uses the operating system random source. Callers may instead inject
    /// bytes from their own cryptographic source with `from_bytes`.
    pub fn random() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::fs::File;
            use std::io::Read;

            let mut bytes = [0u8; 32];
            File::open("/dev/urandom")?.read_exact(&mut bytes)?;
            Ok(Self(bytes))
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no operating-system random source is implemented on this platform",
            ))
        }
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for OwnerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OwnerToken([redacted])")
    }
}

/// A monotonically increasing fence for one lease scope.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FencingEpoch(u64);

impl FencingEpoch {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for FencingEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Coordinator-owned monotonic time. Units are deliberately opaque, but a
/// lease duration and `now` must use the same unit.
pub trait LeaseClock: Send + Sync {
    fn now(&self) -> u64;
}

/// Process-local monotonic nanosecond clock for the default lease service.
#[derive(Clone, Debug)]
pub struct SystemLeaseClock {
    origin: Instant,
}

impl Default for SystemLeaseClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl LeaseClock for SystemLeaseClock {
    fn now(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// An immutable acquisition or renewal record handed to the owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseClaim {
    scope: LeaseScope,
    owner: OwnerToken,
    epoch: FencingEpoch,
    deadline_tick: u64,
    renewal_sequence: u64,
}

impl LeaseClaim {
    pub fn scope(&self) -> &LeaseScope {
        &self.scope
    }

    pub const fn owner(&self) -> OwnerToken {
        self.owner
    }

    pub const fn epoch(&self) -> FencingEpoch {
        self.epoch
    }

    pub const fn deadline_tick(&self) -> u64 {
        self.deadline_tick
    }

    pub const fn renewal_sequence(&self) -> u64 {
        self.renewal_sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LeaseRecord {
    owner: OwnerToken,
    epoch: FencingEpoch,
    deadline_tick: u64,
    renewal_sequence: u64,
}

impl LeaseRecord {
    fn claim(self, scope: LeaseScope) -> LeaseClaim {
        LeaseClaim {
            scope,
            owner: self.owner,
            epoch: self.epoch,
            deadline_tick: self.deadline_tick,
            renewal_sequence: self.renewal_sequence,
        }
    }
}

/// Coordinator lease service. Locks are advisory; `compare_and_swap` is the
/// enforcement point for protected mutable state.
pub struct LeaseService<C = SystemLeaseClock> {
    clock: C,
    last_observed_tick: AtomicU64,
    records: Mutex<BTreeMap<LeaseScope, LeaseRecord>>,
}

impl Default for LeaseService<SystemLeaseClock> {
    fn default() -> Self {
        Self::new(SystemLeaseClock::default())
    }
}

impl<C: LeaseClock> LeaseService<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            last_observed_tick: AtomicU64::new(0),
            records: Mutex::new(BTreeMap::new()),
        }
    }

    /// Acquires only a never-before-seen scope. Even an expired record must be
    /// reclaimed explicitly so its epoch can never reset to one.
    pub fn acquire(
        &self,
        scope: LeaseScope,
        owner: OwnerToken,
        duration: u64,
    ) -> Result<LeaseClaim, LeaseError> {
        let now = self.observe_now();
        let deadline_tick = deadline(now, duration)?;
        let mut records = self.lock_records()?;
        if let Some(existing) = records.get(&scope) {
            return Err(LeaseError::AlreadyHeld {
                epoch: existing.epoch,
                expired: now >= existing.deadline_tick,
            });
        }
        let record = LeaseRecord {
            owner,
            epoch: FencingEpoch(1),
            deadline_tick,
            renewal_sequence: 0,
        };
        records.insert(scope.clone(), record);
        Ok(record.claim(scope))
    }

    /// Reclaims only the exact observed, coordinator-confirmed expired epoch.
    /// The winning owner always receives the next greater fencing epoch.
    pub fn reclaim(
        &self,
        scope: &LeaseScope,
        observed_epoch: FencingEpoch,
        new_owner: OwnerToken,
        duration: u64,
    ) -> Result<LeaseClaim, LeaseError> {
        let now = self.observe_now();
        let deadline_tick = deadline(now, duration)?;
        let mut records = self.lock_records()?;
        let existing = records.get_mut(scope).ok_or(LeaseError::LeaseMissing)?;
        if existing.epoch != observed_epoch {
            return Err(LeaseError::StaleEpoch {
                claimed: observed_epoch,
                current: existing.epoch,
            });
        }
        if now < existing.deadline_tick {
            return Err(LeaseError::NotExpired {
                epoch: existing.epoch,
                deadline_tick: existing.deadline_tick,
                now,
            });
        }
        let next_epoch = existing
            .epoch
            .get()
            .checked_add(1)
            .and_then(FencingEpoch::new)
            .ok_or(LeaseError::EpochExhausted)?;
        *existing = LeaseRecord {
            owner: new_owner,
            epoch: next_epoch,
            deadline_tick,
            renewal_sequence: 0,
        };
        Ok((*existing).claim(scope.clone()))
    }

    pub fn renew(&self, claim: &LeaseClaim, duration: u64) -> Result<LeaseClaim, LeaseError> {
        let now = self.observe_now();
        let deadline_tick = deadline(now, duration)?;
        let mut records = self.lock_records()?;
        let existing = records
            .get_mut(&claim.scope)
            .ok_or(LeaseError::LeaseMissing)?;
        validate_owner(existing, claim, now)?;
        existing.deadline_tick = deadline_tick;
        existing.renewal_sequence = existing
            .renewal_sequence
            .checked_add(1)
            .ok_or(LeaseError::RenewalSequenceExhausted)?;
        Ok((*existing).claim(claim.scope.clone()))
    }

    /// Ends a lease without deleting its epoch history. A following owner must
    /// reclaim the tombstoned record at a greater epoch.
    pub fn release(&self, claim: &LeaseClaim) -> Result<LeaseClaim, LeaseError> {
        let now = self.observe_now();
        let mut records = self.lock_records()?;
        let existing = records
            .get_mut(&claim.scope)
            .ok_or(LeaseError::LeaseMissing)?;
        if existing.epoch != claim.epoch {
            return Err(LeaseError::StaleEpoch {
                claimed: claim.epoch,
                current: existing.epoch,
            });
        }
        if existing.owner != claim.owner {
            return Err(LeaseError::OwnerMismatch {
                epoch: existing.epoch,
            });
        }
        existing.deadline_tick = now;
        existing.renewal_sequence = existing
            .renewal_sequence
            .checked_add(1)
            .ok_or(LeaseError::RenewalSequenceExhausted)?;
        Ok((*existing).claim(claim.scope.clone()))
    }

    pub fn current(&self, scope: &LeaseScope) -> Result<Option<LeaseClaim>, LeaseError> {
        let records = self.lock_records()?;
        Ok(records
            .get(scope)
            .copied()
            .map(|record| record.claim(scope.clone())))
    }

    /// Fenced CAS for state protected by this lease service. Validation and
    /// mutation share the service mutex, so reclamation cannot interleave
    /// between the owner/epoch check and the state update.
    pub fn compare_and_swap<T>(
        &self,
        claim: &LeaseClaim,
        value: &FencedValue<T>,
        expected_revision: u64,
        next: T,
    ) -> Result<u64, LeaseError> {
        let now = self.observe_now();
        let records = self.lock_records()?;
        let existing = records.get(&claim.scope).ok_or(LeaseError::LeaseMissing)?;
        validate_owner(existing, claim, now)?;

        let mut state = value
            .state
            .lock()
            .map_err(|_| LeaseError::LockPoisoned("fenced value"))?;
        if claim.epoch.get() < state.highest_epoch {
            return Err(LeaseError::Fenced {
                claimed: claim.epoch,
                highest: FencingEpoch(state.highest_epoch),
            });
        }
        if state.revision != expected_revision {
            return Err(LeaseError::CompareAndSwapLost {
                expected: expected_revision,
                actual: state.revision,
            });
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(LeaseError::RevisionExhausted)?;
        state.highest_epoch = claim.epoch.get();
        state.value = next;
        Ok(state.revision)
    }

    fn observe_now(&self) -> u64 {
        let observed = self.clock.now();
        let previous = self
            .last_observed_tick
            .fetch_max(observed, Ordering::SeqCst);
        observed.max(previous)
    }

    fn lock_records(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<LeaseScope, LeaseRecord>>, LeaseError> {
        self.records
            .lock()
            .map_err(|_| LeaseError::LockPoisoned("lease table"))
    }
}

fn validate_owner(existing: &LeaseRecord, claim: &LeaseClaim, now: u64) -> Result<(), LeaseError> {
    if existing.epoch != claim.epoch {
        return Err(LeaseError::StaleEpoch {
            claimed: claim.epoch,
            current: existing.epoch,
        });
    }
    if existing.owner != claim.owner {
        return Err(LeaseError::OwnerMismatch {
            epoch: existing.epoch,
        });
    }
    if now >= existing.deadline_tick {
        return Err(LeaseError::Expired {
            epoch: existing.epoch,
        });
    }
    Ok(())
}

fn deadline(now: u64, duration: u64) -> Result<u64, LeaseError> {
    if duration == 0 {
        return Err(LeaseError::ZeroDuration);
    }
    now.checked_add(duration)
        .ok_or(LeaseError::DeadlineOverflow)
}

/// Mutable state whose updates remember the highest fencing epoch observed.
pub struct FencedValue<T> {
    state: Mutex<FencedState<T>>,
}

impl<T> FencedValue<T> {
    pub fn new(value: T) -> Self {
        Self {
            state: Mutex::new(FencedState {
                value,
                revision: 0,
                highest_epoch: 0,
            }),
        }
    }
}

impl<T: Clone> FencedValue<T> {
    pub fn snapshot(&self) -> Result<FencedSnapshot<T>, LeaseError> {
        let state = self
            .state
            .lock()
            .map_err(|_| LeaseError::LockPoisoned("fenced value"))?;
        Ok(FencedSnapshot {
            value: state.value.clone(),
            revision: state.revision,
            highest_epoch: FencingEpoch::new(state.highest_epoch),
        })
    }
}

struct FencedState<T> {
    value: T,
    revision: u64,
    highest_epoch: u64,
}

/// Read-only snapshot of a fenced value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FencedSnapshot<T> {
    pub value: T,
    pub revision: u64,
    pub highest_epoch: Option<FencingEpoch>,
}

/// Lease acquisition, fencing, and CAS failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LeaseError {
    InvalidScope(&'static str),
    ZeroDuration,
    DeadlineOverflow,
    AlreadyHeld {
        epoch: FencingEpoch,
        expired: bool,
    },
    LeaseMissing,
    NotExpired {
        epoch: FencingEpoch,
        deadline_tick: u64,
        now: u64,
    },
    StaleEpoch {
        claimed: FencingEpoch,
        current: FencingEpoch,
    },
    OwnerMismatch {
        epoch: FencingEpoch,
    },
    Expired {
        epoch: FencingEpoch,
    },
    Fenced {
        claimed: FencingEpoch,
        highest: FencingEpoch,
    },
    CompareAndSwapLost {
        expected: u64,
        actual: u64,
    },
    EpochExhausted,
    RenewalSequenceExhausted,
    RevisionExhausted,
    LockPoisoned(&'static str),
}

impl fmt::Display for LeaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScope(field) => write!(formatter, "lease {field} is empty"),
            Self::ZeroDuration => formatter.write_str("lease duration is zero"),
            Self::DeadlineOverflow => formatter.write_str("lease deadline overflows"),
            Self::AlreadyHeld { epoch, expired } => write!(
                formatter,
                "lease epoch {epoch} already exists (expired={expired}); reclaim it explicitly"
            ),
            Self::LeaseMissing => formatter.write_str("lease scope is absent"),
            Self::NotExpired {
                epoch,
                deadline_tick,
                now,
            } => write!(
                formatter,
                "lease epoch {epoch} has not expired: deadline {deadline_tick}, now {now}"
            ),
            Self::StaleEpoch { claimed, current } => {
                write!(
                    formatter,
                    "stale lease epoch {claimed}; current epoch is {current}"
                )
            }
            Self::OwnerMismatch { epoch } => {
                write!(formatter, "owner token does not own lease epoch {epoch}")
            }
            Self::Expired { epoch } => write!(formatter, "lease epoch {epoch} is expired"),
            Self::Fenced { claimed, highest } => {
                write!(formatter, "epoch {claimed} is fenced by epoch {highest}")
            }
            Self::CompareAndSwapLost { expected, actual } => write!(
                formatter,
                "CAS expected revision {expected}, but current revision is {actual}"
            ),
            Self::EpochExhausted => formatter.write_str("fencing epoch is exhausted"),
            Self::RenewalSequenceExhausted => {
                formatter.write_str("lease renewal sequence is exhausted")
            }
            Self::RevisionExhausted => formatter.write_str("fenced value revision is exhausted"),
            Self::LockPoisoned(name) => write!(formatter, "{name} mutex is poisoned"),
        }
    }
}

impl Error for LeaseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[derive(Clone)]
    struct ManualClock(Arc<AtomicU64>);

    impl ManualClock {
        fn new(now: u64) -> Self {
            Self(Arc::new(AtomicU64::new(now)))
        }

        fn set(&self, now: u64) {
            self.0.store(now, Ordering::SeqCst);
        }
    }

    impl LeaseClock for ManualClock {
        fn now(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn token(byte: u8) -> OwnerToken {
        OwnerToken::from_bytes([byte; 32])
    }

    fn scope() -> LeaseScope {
        LeaseScope::new("trusted-main", "receipt-key").expect("valid scope")
    }

    #[test]
    fn concurrent_lease_cas_mint_has_one_winner() {
        const RACERS: usize = 24;
        let service = Arc::new(LeaseService::new(ManualClock::new(7)));
        let barrier = Arc::new(Barrier::new(RACERS));
        let mut threads = Vec::new();
        for index in 0..RACERS {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                barrier.wait();
                service.acquire(scope(), token(index as u8 + 1), 100)
            }));
        }
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("racer did not panic"))
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results.iter().filter_map(|result| result.as_ref().err()).all(
            |error| matches!(error, LeaseError::AlreadyHeld { epoch, expired: false } if epoch.get() == 1)
        ));
        assert_eq!(
            service
                .current(&scope())
                .expect("current lease")
                .expect("lease exists")
                .epoch()
                .get(),
            1
        );
    }

    #[test]
    fn stale_owner_loses_even_when_its_clock_is_wrong() {
        let clock = ManualClock::new(100);
        let service = LeaseService::new(clock.clone());
        let scope = scope();
        let first = service
            .acquire(scope.clone(), token(1), 10)
            .expect("first owner");
        let value = FencedValue::new("initial");
        assert_eq!(
            service
                .compare_and_swap(&first, &value, 0, "first-owner")
                .expect("first mutation"),
            1
        );

        let stale_worker_clock = 101; // The worker still believes epoch one is live.
        assert!(stale_worker_clock < first.deadline_tick());
        clock.set(111); // Only coordinator monotonic time confirms expiry.
        let second = service
            .reclaim(&scope, first.epoch(), token(2), 20)
            .expect("higher-epoch reclamation");
        assert_eq!(second.epoch().get(), 2);
        assert_eq!(
            service
                .compare_and_swap(&second, &value, 1, "second-owner")
                .expect("second owner mutation"),
            2
        );

        // Even a backwards/faulty raw clock cannot unwind the coordinator's
        // last observed monotonic tick or restore the stale token and epoch.
        clock.set(0);
        assert!(matches!(
            service.compare_and_swap(&first, &value, 2, "stale-write"),
            Err(LeaseError::StaleEpoch { claimed, current })
                if claimed.get() == 1 && current.get() == 2
        ));
        assert_eq!(
            value.snapshot().expect("snapshot"),
            FencedSnapshot {
                value: "second-owner",
                revision: 2,
                highest_epoch: FencingEpoch::new(2),
            }
        );
    }

    #[test]
    fn reclamation_requires_expiry_and_strictly_higher_epoch() {
        let clock = ManualClock::new(10);
        let service = LeaseService::new(clock.clone());
        let scope = scope();
        let first = service
            .acquire(scope.clone(), token(1), 5)
            .expect("initial acquire");
        assert!(matches!(
            service.reclaim(&scope, first.epoch(), token(2), 5),
            Err(LeaseError::NotExpired { .. })
        ));
        clock.set(15);
        assert!(matches!(
            service.acquire(scope.clone(), token(2), 5),
            Err(LeaseError::AlreadyHeld {
                epoch,
                expired: true
            }) if epoch.get() == 1
        ));
        let second = service
            .reclaim(&scope, first.epoch(), token(2), 5)
            .expect("reclaim expired epoch");
        assert_eq!(second.epoch().get(), first.epoch().get() + 1);
        assert!(matches!(
            service.reclaim(&scope, first.epoch(), token(3), 5),
            Err(LeaseError::StaleEpoch { .. })
        ));
    }
}
