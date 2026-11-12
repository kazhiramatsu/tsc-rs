//! Scheduler-independent dense rooms over a list of spaces.
//! A room owns its upper bound; source slices retain that owner without a
//! domain lock. Only the room's sequential cursor may grant the next slice.
use super::*;

pub struct Rooms {
    spaces: Vec<(IdentityLease, u32)>,
    sealed: bool,
}

impl IdentityDomain {
    /// Lease an ordered prefix of stripes atomically under one domain lock.
    /// Requests and order must be computed from input sizes, never workers.
    pub fn lease_rooms(
        &self,
        stripes: &[Vec<(IdentitySpace, u32)>],
    ) -> Result<Vec<Rooms>, IdentityError> {
        for requests in stripes {
            validate_spaces(requests.iter().map(|&(space, _)| space))?;
        }
        let mut state = self.lock()?;
        let mut allocated = Vec::new();
        for (stripe, requests) in stripes.iter().enumerate() {
            for &(space, count) in requests {
                let id = state.next_lease_id;
                state.next_lease_id += 1;
                match state.allocators[space.index()].allocate_bump(space, count, id) {
                    Ok(range) => allocated.push((stripe, space, range, id)),
                    Err(error) => {
                        for (_, space, range, id) in allocated.into_iter().rev() {
                            state.allocators[space.index()].release(range, id);
                        }
                        return Err(error);
                    }
                }
            }
        }
        drop(state);
        let mut result: Vec<_> = stripes
            .iter()
            .map(|_| Rooms {
                spaces: Vec::new(),
                sealed: false,
            })
            .collect();
        for (stripe, space, range, id) in allocated {
            result[stripe]
                .spaces
                .push((self.make_lease(space, range, id), 0));
        }
        Ok(result)
    }
}

impl Rooms {
    pub fn remaining(&self) -> Vec<(IdentitySpace, IdentityRange)> {
        assert!(!self.sealed);
        self.spaces
            .iter()
            .map(|(lease, used)| {
                (
                    lease.space(),
                    IdentityRange::new(lease.range().start() + used, lease.range().end()),
                )
            })
            .collect()
    }

    /// Advance all spaces together after their store vectors have been sealed.
    /// The returned exact slices keep the entire stripe pinned. No allocator
    /// state is touched, including for a zero-length source space.
    pub fn finish_entry(&mut self, counts: &[u32]) -> Result<Vec<IdentityLease>, IdentityError> {
        assert!(!self.sealed);
        if counts.len() != self.spaces.len()
            || self
                .spaces
                .iter()
                .zip(counts)
                .any(|((lease, used), count)| *count > lease.range().len() - used)
        {
            return Err(IdentityError::ReservationMismatch);
        }
        Ok(self
            .spaces
            .iter_mut()
            .zip(counts)
            .map(|((parent, used), count)| {
                let start = parent.range().start() + *used;
                *used += count;
                let range = IdentityRange::new(start, start + count);
                IdentityLease {
                    inner: Arc::new(IdentityLeaseInner {
                        domain: Arc::clone(&parent.inner.domain),
                        space: parent.space(),
                        range,
                        lease_id: parent.inner.lease_id,
                        claimed: AtomicBool::new(false),
                        parent: Some(parent.clone()),
                        retained_end: std::sync::atomic::AtomicU32::new(range.end()),
                    }),
                }
            })
            .collect())
    }

    pub fn unused(&self) -> Vec<(IdentitySpace, IdentityRange)> {
        self.spaces
            .iter()
            .map(|(lease, used)| {
                (
                    lease.space(),
                    IdentityRange::new(lease.range().start() + used, lease.range().end()),
                )
            })
            .collect()
    }

    /// Return the unused suffix after every writer has finished and the
    /// scheduler has leased all stripes. Exact fallback leases follow this
    /// call in input order.
    pub fn seal(&mut self) -> Result<(), IdentityError> {
        assert!(!self.sealed);
        self.sealed = true;
        if self.spaces.is_empty() {
            return Ok(());
        }
        let domain = self.spaces[0].0.domain();
        let mut state = domain.lock()?;
        for (lease, used) in &self.spaces {
            let old = lease.range();
            let end = old.start() + used;
            if end == old.end() {
                continue;
            }
            let allocator = &mut state.allocators[lease.space().index()];
            if !old.is_empty() {
                let active = allocator.active.get(&old.start()).expect("live room");
                assert_eq!(active.lease_id, lease.inner.lease_id);
                if *used == 0 {
                    allocator.active.remove(&old.start());
                } else {
                    allocator
                        .active
                        .get_mut(&old.start())
                        .expect("live room")
                        .end = end;
                }
                if old.end() == allocator.bump {
                    allocator.bump = end;
                    allocator.trim_free_tail();
                } else {
                    let mut free_start = end;
                    let mut free_end = old.end();
                    if let Some((&start, &previous_end)) =
                        allocator.free.range(..free_start).next_back()
                    {
                        if previous_end == free_start {
                            allocator.free.remove(&start);
                            free_start = start;
                        }
                    }
                    if let Some((&start, &next_end)) = allocator.free.range(free_end..).next() {
                        if start == free_end {
                            allocator.free.remove(&start);
                            free_end = next_end;
                        }
                    }
                    allocator.free.insert(free_start, free_end);
                }
            }
            lease.inner.retained_end.store(end, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_keep_the_parent_live_and_claim_independently() {
        let domain = IdentityDomain::with_limits(IdentityLimits::default()).unwrap();
        let mut rooms = domain
            .lease_rooms(&[vec![
                (IdentitySpace::Node, 12),
                (IdentitySpace::Symbol, 8),
                (IdentitySpace::PrivateNameSerial, 8),
            ]])
            .unwrap()
            .pop()
            .unwrap();
        let first = rooms.finish_entry(&[3, 2, 1]).unwrap();
        let second = rooms.finish_entry(&[4, 0, 3]).unwrap();
        assert_eq!(first[0].range().end(), second[0].range().start());
        assert!(!first[0].range().overlaps(second[0].range()));
        let claim = first[0].claim().unwrap();
        assert!(first[0].claim().is_none());
        assert!(second[0].claim().is_some());
        rooms.seal().unwrap();
        drop(rooms);
        drop(second);
        drop(first);
        assert_eq!(
            domain
                .stats()
                .unwrap()
                .space(IdentitySpace::Node)
                .active_values,
            7
        );
        drop(claim);
        assert_eq!(
            domain
                .stats()
                .unwrap()
                .space(IdentitySpace::Node)
                .active_values,
            0
        );
    }

    #[test]
    fn releasing_suffixes_keeps_slices_and_returns_only_unused_ids() {
        let domain = IdentityDomain::with_limits(IdentityLimits::default()).unwrap();
        let mut rooms = domain
            .lease_rooms(&[
                vec![(IdentitySpace::Node, 12)],
                vec![(IdentitySpace::Node, 8)],
            ])
            .unwrap();
        let first = rooms[0].finish_entry(&[3]).unwrap();
        let second = rooms[1].finish_entry(&[4]).unwrap();
        for room in &mut rooms {
            room.seal().unwrap();
        }
        let recycled = domain.lease(IdentitySpace::Node, 9).unwrap();
        assert_eq!(recycled.range(), IdentityRange::new(3, 12));
        assert_eq!(second[0].range(), IdentityRange::new(12, 16));
        drop(rooms);
        drop(first);
        drop(second);
        drop(recycled);
        assert_eq!(domain.stats().unwrap().space(IdentitySpace::Node).bump, 0);
    }

    #[test]
    fn failed_prefix_is_atomic_and_failed_entry_does_not_advance() {
        let domain = IdentityDomain::with_limits(IdentityLimits {
            node_end: 16,
            ..IdentityLimits::default()
        })
        .unwrap();
        assert!(domain
            .lease_rooms(&[
                vec![(IdentitySpace::Node, 8)],
                vec![(IdentitySpace::Node, 9)]
            ])
            .is_err());
        assert_eq!(domain.stats().unwrap().space(IdentitySpace::Node).bump, 0);
        let mut room = domain
            .lease_rooms(&[vec![(IdentitySpace::Node, 8)]])
            .unwrap()
            .pop()
            .unwrap();
        let before = room.remaining();
        assert!(room.finish_entry(&[9]).is_err());
        assert_eq!(before, room.remaining());
        assert!(room.finish_entry(&[]).is_err());
        assert_eq!(before, room.remaining());
    }
}
