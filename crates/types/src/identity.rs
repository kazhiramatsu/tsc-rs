//! Collision-safe identity ranges shared by parsed and bound documents.
//!
//! A domain is intentionally opaque: numeric ids are meaningful only while
//! their lease is alive. Every domain [`new`](IdentityDomain::new) makes
//! allocates from one process-wide state, so an identity names one record of
//! the process: the stores (`tsc_store`) keep a record at the position of its
//! identity, in any domain. A domain is the owner a document is checked
//! against. Released intervals are reclaimed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

/// Persistent symbols occupy the low half of `SymbolId`. The checker owns the
/// tagged high half for session-local transient symbols.
pub const TRANSIENT_SYMBOL_BIT: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum IdentitySpace {
    Node = 0,
    NodeArray = 1,
    Symbol = 2,
    PrivateNameSerial = 3,
    NodeArrayItem = 4,
    /// A checker's types, built in the type store at the leased positions
    /// (a type's identity stays the index in its checker's list).
    Type = 5,
    /// The words of a tree's literal texts (each a length word and the
    /// text's bytes), a text's identity being its first word.
    NodeText = 6,
}

impl IdentitySpace {
    pub const ALL: [Self; 7] = [
        Self::Node,
        Self::NodeArray,
        Self::Symbol,
        Self::PrivateNameSerial,
        Self::NodeArrayItem,
        Self::Type,
        Self::NodeText,
    ];

    const fn index(self) -> usize {
        self as usize
    }

    const fn default_start(self) -> u32 {
        match self {
            Self::PrivateNameSerial => 1,
            _ => 0,
        }
    }
}

impl fmt::Display for IdentitySpace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Node => "node",
            Self::NodeArray => "node-array",
            Self::Symbol => "persistent-symbol",
            Self::PrivateNameSerial => "private-name-serial",
            Self::NodeArrayItem => "node-array-item",
            Self::Type => "type",
            Self::NodeText => "node-text",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityLimits {
    pub node_end: u32,
    pub node_array_end: u32,
    pub persistent_symbol_end: u32,
    pub private_name_serial_end: u32,
    pub node_array_item_end: u32,
    pub type_end: u32,
    pub node_text_end: u32,
}

impl IdentityLimits {
    const fn end(self, space: IdentitySpace) -> u32 {
        match space {
            IdentitySpace::Node => self.node_end,
            IdentitySpace::NodeArray => self.node_array_end,
            IdentitySpace::Symbol => self.persistent_symbol_end,
            IdentitySpace::PrivateNameSerial => self.private_name_serial_end,
            IdentitySpace::NodeArrayItem => self.node_array_item_end,
            IdentitySpace::Type => self.type_end,
            IdentitySpace::NodeText => self.node_text_end,
        }
    }
}

impl Default for IdentityLimits {
    fn default() -> Self {
        Self {
            // The exclusive end is representable in every arena today, so
            // the all-ones raw value remains an exhaustion sentinel.
            node_end: u32::MAX,
            node_array_end: u32::MAX,
            persistent_symbol_end: TRANSIENT_SYMBOL_BIT,
            private_name_serial_end: u32::MAX,
            node_array_item_end: u32::MAX,
            type_end: u32::MAX,
            // One below the others: the last index is the syntax crate's
            // empty text, which takes no words.
            node_text_end: u32::MAX - 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityRange {
    start: u32,
    end: u32,
}

impl IdentityRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn end(self) -> u32 {
        self.end
    }

    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn contains(self, value: u32) -> bool {
        value >= self.start && value < self.end
    }

    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityError {
    InvalidLimits {
        space: IdentitySpace,
        start: u32,
        end: u32,
    },
    EmptyReservation,
    DuplicateSpace(IdentitySpace),
    ReservationMismatch,
    Exhausted {
        space: IdentitySpace,
        requested: u32,
        limit: u32,
    },
    InvalidLease {
        space: IdentitySpace,
        detail: &'static str,
    },
    Poisoned,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits { space, start, end } => {
                write!(formatter, "invalid {space} identity limits {start}..{end}")
            }
            Self::EmptyReservation => formatter.write_str("identity reservation is empty"),
            Self::DuplicateSpace(space) => {
                write!(formatter, "identity reservation repeats {space}")
            }
            Self::ReservationMismatch => {
                formatter.write_str("identity reservation spaces and sealed counts disagree")
            }
            Self::Exhausted {
                space,
                requested,
                limit,
            } => write!(
                formatter,
                "{space} identity space exhausted while reserving {requested} values below {limit}"
            ),
            Self::InvalidLease { space, detail } => {
                write!(formatter, "invalid {space} identity lease: {detail}")
            }
            Self::Poisoned => formatter.write_str("identity-domain lock was poisoned"),
        }
    }
}

impl std::error::Error for IdentityError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveRange {
    end: u32,
    lease_id: u64,
}

#[derive(Clone, Debug)]
struct RangeAllocator {
    start: u32,
    limit: u32,
    bump: u32,
    free: BTreeMap<u32, u32>,
    active: BTreeMap<u32, ActiveRange>,
}

impl RangeAllocator {
    fn new(start: u32, limit: u32) -> Self {
        Self {
            start,
            limit,
            bump: start,
            free: BTreeMap::new(),
            active: BTreeMap::new(),
        }
    }

    fn allocate(
        &mut self,
        space: IdentitySpace,
        count: u32,
        lease_id: u64,
    ) -> Result<IdentityRange, IdentityError> {
        if count == 0 {
            return Ok(IdentityRange::new(self.bump, self.bump));
        }

        let candidate = self
            .free
            .iter()
            .find_map(|(&start, &end)| (end - start >= count).then_some((start, end)));
        if let Some((start, free_end)) = candidate {
            let end = start + count;
            self.free.remove(&start);
            if end < free_end {
                self.free.insert(end, free_end);
            }
            self.insert_active(space, IdentityRange::new(start, end), lease_id)?;
            return Ok(IdentityRange::new(start, end));
        }

        self.allocate_bump(space, count, lease_id)
    }

    fn allocate_bump(
        &mut self,
        space: IdentitySpace,
        count: u32,
        lease_id: u64,
    ) -> Result<IdentityRange, IdentityError> {
        if count == 0 {
            return Ok(IdentityRange::new(self.bump, self.bump));
        }
        let end = self
            .bump
            .checked_add(count)
            .filter(|end| *end <= self.limit)
            .ok_or(IdentityError::Exhausted {
                space,
                requested: count,
                limit: self.limit,
            })?;
        let range = IdentityRange::new(self.bump, end);
        self.bump = end;
        self.insert_active(space, range, lease_id)?;
        Ok(range)
    }

    fn insert_active(
        &mut self,
        space: IdentitySpace,
        range: IdentityRange,
        lease_id: u64,
    ) -> Result<(), IdentityError> {
        if let Some((&start, previous)) = self.active.range(..=range.start).next_back() {
            if start != range.start && previous.end > range.start {
                return Err(IdentityError::InvalidLease {
                    space,
                    detail: "range overlaps its predecessor",
                });
            }
        }
        if let Some((&start, _)) = self.active.range(range.start..).next() {
            if start < range.end {
                return Err(IdentityError::InvalidLease {
                    space,
                    detail: "range overlaps its successor",
                });
            }
        }
        if self
            .active
            .insert(
                range.start,
                ActiveRange {
                    end: range.end,
                    lease_id,
                },
            )
            .is_some()
        {
            return Err(IdentityError::InvalidLease {
                space,
                detail: "range duplicates an active start",
            });
        }
        Ok(())
    }

    fn release(&mut self, range: IdentityRange, lease_id: u64) {
        if range.is_empty() {
            return;
        }
        let active = self.active.remove(&range.start);
        debug_assert_eq!(
            active,
            Some(ActiveRange {
                end: range.end,
                lease_id,
            }),
            "released identity range was not the active lease"
        );
        if active.is_none() {
            return;
        }

        let mut start = range.start;
        let mut end = range.end;
        if let Some((&previous_start, &previous_end)) = self.free.range(..start).next_back() {
            if previous_end == start {
                self.free.remove(&previous_start);
                start = previous_start;
            }
        }
        if let Some((&next_start, &next_end)) = self.free.range(end..).next() {
            if next_start == end {
                self.free.remove(&next_start);
                end = next_end;
            }
        }
        self.free.insert(start, end);
        self.trim_free_tail();
    }

    fn trim_free_tail(&mut self) {
        while let Some((&start, &end)) = self.free.iter().next_back() {
            if end != self.bump {
                break;
            }
            self.free.remove(&start);
            self.bump = start;
        }
        debug_assert!(self.bump >= self.start && self.bump <= self.limit);
    }
}

#[derive(Debug)]
struct DomainState {
    allocators: [RangeAllocator; IdentitySpace::ALL.len()],
    next_lease_id: u64,
}

impl DomainState {
    fn new(limits: IdentityLimits) -> Self {
        Self {
            allocators: IdentitySpace::ALL
                .map(|space| RangeAllocator::new(space.default_start(), limits.end(space))),
            next_lease_id: 1,
        }
    }
}

struct IdentityDomainInner {
    state: Arc<Mutex<DomainState>>,
}

#[derive(Clone)]
pub struct IdentityDomain {
    inner: Arc<IdentityDomainInner>,
}

impl fmt::Debug for IdentityDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityDomain")
            .finish_non_exhaustive()
    }
}

impl Default for IdentityDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityDomain {
    /// A new owner over the process's identities.
    pub fn new() -> Self {
        static STATE: OnceLock<Arc<Mutex<DomainState>>> = OnceLock::new();
        Self {
            inner: Arc::new(IdentityDomainInner {
                state: Arc::clone(STATE.get_or_init(|| {
                    Arc::new(Mutex::new(DomainState::new(IdentityLimits::default())))
                })),
            }),
        }
    }

    /// A domain with identities of its own, for testing the allocator: what
    /// it leases collides with the process's identities, so no document is
    /// ever published from it.
    pub fn with_limits(limits: IdentityLimits) -> Result<Self, IdentityError> {
        for space in IdentitySpace::ALL {
            let start = space.default_start();
            let end = limits.end(space);
            if start >= end || space == IdentitySpace::Symbol && end > TRANSIENT_SYMBOL_BIT {
                return Err(IdentityError::InvalidLimits { space, start, end });
            }
        }
        Ok(Self {
            inner: Arc::new(IdentityDomainInner {
                state: Arc::new(Mutex::new(DomainState::new(limits))),
            }),
        })
    }

    pub fn same_domain(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn owns(&self, lease: &IdentityLease) -> bool {
        Arc::ptr_eq(&self.inner, &lease.inner.domain)
    }

    pub fn lease(&self, space: IdentitySpace, count: u32) -> Result<IdentityLease, IdentityError> {
        self.lease_batch(&[(space, count)])
            .map(|mut leases| leases.pop().expect("one requested lease"))
    }

    /// Atomically reserves exact intervals. A failed member leaves every
    /// identity space unchanged.
    pub fn lease_batch(
        &self,
        requests: &[(IdentitySpace, u32)],
    ) -> Result<Vec<IdentityLease>, IdentityError> {
        validate_spaces(requests.iter().map(|(space, _)| *space))?;
        let mut state = self.lock()?;
        let mut allocated = Vec::with_capacity(requests.len());
        for &(space, count) in requests {
            let lease_id = state.next_lease_id;
            state.next_lease_id += 1;
            match state.allocators[space.index()].allocate(space, count, lease_id) {
                Ok(range) => allocated.push((space, range, lease_id)),
                Err(error) => {
                    for (space, range, lease_id) in allocated {
                        state.allocators[space.index()].release(range, lease_id);
                    }
                    return Err(error);
                }
            }
        }
        drop(state);
        Ok(allocated
            .into_iter()
            .map(|(space, range, lease_id)| self.make_lease(space, range, lease_id))
            .collect())
    }

    pub fn stats(&self) -> Result<IdentityDomainStats, IdentityError> {
        let state = self.lock()?;
        let spaces = IdentitySpace::ALL.map(|space| {
            let allocator = &state.allocators[space.index()];
            IdentitySpaceStats {
                space,
                active_ranges: allocator.active.len(),
                active_values: allocator
                    .active
                    .iter()
                    .map(|(&start, active)| u64::from(active.end - start))
                    .sum(),
                bump: allocator.bump,
                free_ranges: allocator.free.len(),
            }
        });
        Ok(IdentityDomainStats { spaces })
    }

    pub fn active_ranges(&self, space: IdentitySpace) -> Result<Vec<IdentityRange>, IdentityError> {
        let state = self.lock()?;
        Ok(state.allocators[space.index()]
            .active
            .iter()
            .map(|(&start, active)| IdentityRange::new(start, active.end))
            .collect())
    }

    fn lock(&self) -> Result<MutexGuard<'_, DomainState>, IdentityError> {
        self.inner.state.lock().map_err(|_| IdentityError::Poisoned)
    }

    fn make_lease(
        &self,
        space: IdentitySpace,
        range: IdentityRange,
        lease_id: u64,
    ) -> IdentityLease {
        IdentityLease {
            inner: Arc::new(IdentityLeaseInner {
                domain: Arc::clone(&self.inner),
                space,
                range,
                lease_id,
                claimed: AtomicBool::new(false),
                parent: None,
                retained_end: std::sync::atomic::AtomicU32::new(range.end()),
            }),
        }
    }
}

fn validate_spaces(spaces: impl IntoIterator<Item = IdentitySpace>) -> Result<(), IdentityError> {
    let mut seen = BTreeSet::new();
    let mut any = false;
    for space in spaces {
        any = true;
        if !seen.insert(space) {
            return Err(IdentityError::DuplicateSpace(space));
        }
    }
    if !any {
        return Err(IdentityError::EmptyReservation);
    }
    Ok(())
}

struct IdentityLeaseInner {
    domain: Arc<IdentityDomainInner>,
    space: IdentitySpace,
    range: IdentityRange,
    lease_id: u64,
    /// Whether an [`IdentityClaim`] is live.
    claimed: AtomicBool,
    parent: Option<IdentityLease>,
    retained_end: std::sync::atomic::AtomicU32,
}

impl Drop for IdentityLeaseInner {
    fn drop(&mut self) {
        if self.parent.is_some() {
            return;
        }
        let range = IdentityRange::new(
            self.range.start(),
            self.retained_end.load(Ordering::Relaxed),
        );
        let mut state = self
            .domain
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.allocators[self.space.index()].release(range, self.lease_id);
    }
}

#[derive(Clone)]
pub struct IdentityLease {
    inner: Arc<IdentityLeaseInner>,
}

impl IdentityLease {
    pub fn space(&self) -> IdentitySpace {
        self.inner.space
    }

    pub fn range(&self) -> IdentityRange {
        self.inner.range
    }

    pub fn same_domain(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner.domain, &other.inner.domain)
    }

    /// A view of the lease that does not hold it: whether it is still held
    /// by a document, a clone of one or a claim.
    pub fn watch(&self) -> IdentityLeaseWatch {
        IdentityLeaseWatch {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// The exclusive use of what the lease's identities name beyond the
    /// records themselves: the tables a bind keeps at a tree's node
    /// positions. One claim is live at a time; `None` while another is.
    pub fn claim(&self) -> Option<IdentityClaim> {
        self.inner
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| IdentityClaim {
                lease: self.clone(),
            })
    }

    pub fn belongs_to(&self, domain: &IdentityDomain) -> bool {
        domain.owns(self)
    }

    /// Clone the opaque allocator capability without exposing a numeric
    /// domain/revision identity.
    pub fn domain(&self) -> IdentityDomain {
        IdentityDomain {
            inner: Arc::clone(&self.inner.domain),
        }
    }
}

impl fmt::Debug for IdentityLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityLease")
            .field("space", &self.space())
            .field("range", &self.range())
            .finish_non_exhaustive()
    }
}

impl PartialEq for IdentityLease {
    fn eq(&self, other: &Self) -> bool {
        self.space() == other.space() && self.range() == other.range() && self.same_domain(other)
    }
}

impl Eq for IdentityLease {}

/// An [`IdentityLease::watch`].
#[derive(Clone)]
pub struct IdentityLeaseWatch {
    inner: Weak<IdentityLeaseInner>,
}

impl IdentityLeaseWatch {
    /// Whether the watched lease is still held.
    pub fn is_held(&self) -> bool {
        self.inner.strong_count() > 0
    }

    /// Whether `lease` is the watched one.
    pub fn watches(&self, lease: &IdentityLease) -> bool {
        std::ptr::eq(self.inner.as_ptr(), Arc::as_ptr(&lease.inner))
    }
}

/// A live [`IdentityLease::claim`]; dropping it lets the lease be claimed
/// again.
pub struct IdentityClaim {
    lease: IdentityLease,
}

impl IdentityClaim {
    pub fn lease(&self) -> &IdentityLease {
        &self.lease
    }
}

impl Drop for IdentityClaim {
    fn drop(&mut self) {
        self.lease.inner.claimed.store(false, Ordering::Release);
    }
}

impl fmt::Debug for IdentityClaim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityClaim")
            .field("lease", &self.lease)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityDomainStats {
    pub spaces: [IdentitySpaceStats; IdentitySpace::ALL.len()],
}

impl IdentityDomainStats {
    pub fn space(&self, space: IdentitySpace) -> &IdentitySpaceStats {
        &self.spaces[space.index()]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentitySpaceStats {
    pub space: IdentitySpace,
    pub active_ranges: usize,
    pub active_values: u64,
    pub bump: u32,
    pub free_ranges: usize,
}

#[path = "direct_rooms.rs"]
pub mod direct;

#[cfg(test)]
#[path = "../tests/unit/identity/tests.rs"]
mod tests;
