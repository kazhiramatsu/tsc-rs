//! Collision-safe identity ranges shared by parsed and bound documents.
//!
//! A domain is intentionally opaque: numeric ids are meaningful only while
//! their lease is alive and only beside other leases from the same domain.
//! Long-lived domains reclaim released intervals. One-shot programs use the
//! bump policy and may reserve the current tail provisionally so parsing and
//! binding can construct directly at their final bases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

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
}

impl IdentitySpace {
    pub const ALL: [Self; 4] = [
        Self::Node,
        Self::NodeArray,
        Self::Symbol,
        Self::PrivateNameSerial,
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
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityAllocationPolicy {
    EphemeralBump,
    Reclaiming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityLimits {
    pub node_end: u32,
    pub node_array_end: u32,
    pub persistent_symbol_end: u32,
    pub private_name_serial_end: u32,
}

impl IdentityLimits {
    const fn end(self, space: IdentitySpace) -> u32 {
        match space {
            IdentitySpace::Node => self.node_end,
            IdentitySpace::NodeArray => self.node_array_end,
            IdentitySpace::Symbol => self.persistent_symbol_end,
            IdentitySpace::PrivateNameSerial => self.private_name_serial_end,
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
    ProvisionalAllocationActive(IdentitySpace),
    ProvisionalAllocationUnsupported,
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
            Self::ProvisionalAllocationActive(space) => {
                write!(
                    formatter,
                    "a provisional {space} allocation is already active"
                )
            }
            Self::ProvisionalAllocationUnsupported => formatter.write_str(
                "provisional tail allocation is only available in an ephemeral identity domain",
            ),
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
    provisional: Option<u64>,
}

impl RangeAllocator {
    fn new(start: u32, limit: u32) -> Self {
        Self {
            start,
            limit,
            bump: start,
            free: BTreeMap::new(),
            active: BTreeMap::new(),
            provisional: None,
        }
    }

    fn allocate(
        &mut self,
        space: IdentitySpace,
        count: u32,
        lease_id: u64,
        policy: IdentityAllocationPolicy,
    ) -> Result<IdentityRange, IdentityError> {
        if self.provisional.is_some() {
            return Err(IdentityError::ProvisionalAllocationActive(space));
        }
        if count == 0 {
            return Ok(IdentityRange::new(self.bump, self.bump));
        }

        if policy == IdentityAllocationPolicy::Reclaiming {
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

    fn seal_tail(
        &mut self,
        space: IdentitySpace,
        token: u64,
        count: u32,
        lease_id: u64,
    ) -> Result<IdentityRange, IdentityError> {
        if self.provisional != Some(token) {
            return Err(IdentityError::ReservationMismatch);
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
        self.provisional = None;
        if count != 0 {
            self.insert_active(space, range, lease_id)?;
        }
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

    fn release(&mut self, range: IdentityRange, lease_id: u64, policy: IdentityAllocationPolicy) {
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
        if active.is_none() || policy == IdentityAllocationPolicy::EphemeralBump {
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
        loop {
            let Some((&start, &end)) = self.free.iter().next_back() else {
                break;
            };
            if end != self.bump {
                break;
            }
            self.free.remove(&start);
            self.bump = start;
        }
        debug_assert!(self.bump >= self.start && self.bump <= self.limit);
    }
}

#[derive(Clone, Debug)]
struct DomainState {
    allocators: [RangeAllocator; 4],
    next_lease_id: u64,
    next_token: u64,
}

struct IdentityDomainInner {
    policy: IdentityAllocationPolicy,
    state: Mutex<DomainState>,
}

#[derive(Clone)]
pub struct IdentityDomain {
    inner: Arc<IdentityDomainInner>,
}

impl fmt::Debug for IdentityDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityDomain")
            .field("policy", &self.inner.policy)
            .finish_non_exhaustive()
    }
}

impl IdentityDomain {
    pub fn ephemeral() -> Self {
        Self::with_limits(
            IdentityAllocationPolicy::EphemeralBump,
            IdentityLimits::default(),
        )
        .expect("default ephemeral identity limits are valid")
    }

    pub fn reclaiming() -> Self {
        Self::with_limits(
            IdentityAllocationPolicy::Reclaiming,
            IdentityLimits::default(),
        )
        .expect("default reclaiming identity limits are valid")
    }

    pub fn with_limits(
        policy: IdentityAllocationPolicy,
        limits: IdentityLimits,
    ) -> Result<Self, IdentityError> {
        for space in IdentitySpace::ALL {
            let start = space.default_start();
            let end = limits.end(space);
            if start >= end || space == IdentitySpace::Symbol && end > TRANSIENT_SYMBOL_BIT {
                return Err(IdentityError::InvalidLimits { space, start, end });
            }
        }
        Ok(Self {
            inner: Arc::new(IdentityDomainInner {
                policy,
                state: Mutex::new(DomainState {
                    allocators: IdentitySpace::ALL
                        .map(|space| RangeAllocator::new(space.default_start(), limits.end(space))),
                    next_lease_id: 1,
                    next_token: 1,
                }),
            }),
        })
    }

    pub fn policy(&self) -> IdentityAllocationPolicy {
        self.inner.policy
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
        let mut trial = state.clone();
        let mut allocated = Vec::with_capacity(requests.len());
        for &(space, count) in requests {
            let lease_id = trial.next_lease_id;
            trial.next_lease_id =
                trial
                    .next_lease_id
                    .checked_add(1)
                    .ok_or(IdentityError::InvalidLease {
                        space,
                        detail: "lease serial exhausted",
                    })?;
            let range = trial.allocators[space.index()].allocate(
                space,
                count,
                lease_id,
                self.inner.policy,
            )?;
            allocated.push((space, range, lease_id));
        }
        *state = trial;
        drop(state);
        Ok(allocated
            .into_iter()
            .map(|(space, range, lease_id)| self.make_lease(space, range, lease_id))
            .collect())
    }

    /// Opens the current bump tail without advancing it. Only one one-shot
    /// publisher may hold a provisional reservation for a given space.
    pub fn reserve_provisional(
        &self,
        spaces: &[IdentitySpace],
    ) -> Result<IdentityReservation, IdentityError> {
        if self.inner.policy != IdentityAllocationPolicy::EphemeralBump {
            return Err(IdentityError::ProvisionalAllocationUnsupported);
        }
        validate_spaces(spaces.iter().copied())?;
        let mut state = self.lock()?;
        let mut trial = state.clone();
        let token = trial.next_token;
        trial.next_token = trial
            .next_token
            .checked_add(1)
            .ok_or(IdentityError::ReservationMismatch)?;
        let mut bases = Vec::with_capacity(spaces.len());
        for &space in spaces {
            let allocator = &mut trial.allocators[space.index()];
            if allocator.provisional.is_some() {
                return Err(IdentityError::ProvisionalAllocationActive(space));
            }
            allocator.provisional = Some(token);
            bases.push((space, allocator.bump));
        }
        *state = trial;
        Ok(IdentityReservation {
            domain: self.clone(),
            token,
            bases,
            active: true,
        })
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
                provisional: allocator.provisional.is_some(),
            }
        });
        Ok(IdentityDomainStats {
            policy: self.inner.policy,
            spaces,
        })
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
}

impl Drop for IdentityLeaseInner {
    fn drop(&mut self) {
        let mut state = self
            .domain
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.allocators[self.space.index()].release(self.range, self.lease_id, self.domain.policy);
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

pub struct IdentityReservation {
    domain: IdentityDomain,
    token: u64,
    bases: Vec<(IdentitySpace, u32)>,
    active: bool,
}

impl fmt::Debug for IdentityReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityReservation")
            .field("bases", &self.bases)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl IdentityReservation {
    pub fn base(&self, space: IdentitySpace) -> Result<u32, IdentityError> {
        self.bases
            .iter()
            .find_map(|&(candidate, base)| (candidate == space).then_some(base))
            .ok_or(IdentityError::ReservationMismatch)
    }

    /// Atomically seals the observed counts and turns the provisional tails
    /// into ordinary RAII leases.
    pub fn seal(
        mut self,
        counts: &[(IdentitySpace, u32)],
    ) -> Result<Vec<IdentityLease>, IdentityError> {
        validate_spaces(counts.iter().map(|(space, _)| *space))?;
        let requested: BTreeSet<_> = counts.iter().map(|(space, _)| *space).collect();
        let reserved: BTreeSet<_> = self.bases.iter().map(|(space, _)| *space).collect();
        if requested != reserved {
            return Err(IdentityError::ReservationMismatch);
        }

        let mut state = self.domain.lock()?;
        let mut trial = state.clone();
        let mut allocated = Vec::with_capacity(counts.len());
        for &(space, count) in counts {
            let lease_id = trial.next_lease_id;
            trial.next_lease_id =
                trial
                    .next_lease_id
                    .checked_add(1)
                    .ok_or(IdentityError::InvalidLease {
                        space,
                        detail: "lease serial exhausted",
                    })?;
            let range =
                trial.allocators[space.index()].seal_tail(space, self.token, count, lease_id)?;
            allocated.push((space, range, lease_id));
        }
        *state = trial;
        drop(state);
        self.active = false;
        Ok(allocated
            .into_iter()
            .map(|(space, range, lease_id)| self.domain.make_lease(space, range, lease_id))
            .collect())
    }
}

impl Drop for IdentityReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self
            .domain
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for &(space, _) in &self.bases {
            let provisional = &mut state.allocators[space.index()].provisional;
            if *provisional == Some(self.token) {
                *provisional = None;
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityDomainStats {
    pub policy: IdentityAllocationPolicy,
    pub spaces: [IdentitySpaceStats; 4],
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
    pub provisional: bool,
}

#[cfg(test)]
#[path = "../tests/unit/identity/tests.rs"]
mod tests;
