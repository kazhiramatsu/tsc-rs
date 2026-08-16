use std::collections::VecDeque;

use crate::{EffectPhase, InfraError};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourcePolicyV1 {
    control_cpu_millis: u64,
    control_rss_bytes: u64,
    child_cpu_millis: u64,
    child_rss_bytes: u64,
    child_output_bytes: u64,
    max_children: usize,
    max_queue_items: usize,
}

impl ResourcePolicyV1 {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        control_cpu_millis: u64,
        control_rss_bytes: u64,
        child_cpu_millis: u64,
        child_rss_bytes: u64,
        child_output_bytes: u64,
        max_children: usize,
        max_queue_items: usize,
    ) -> Result<Self, InfraError> {
        if control_cpu_millis == 0
            || control_rss_bytes == 0
            || child_cpu_millis == 0
            || child_rss_bytes == 0
            || child_output_bytes == 0
            || max_children == 0
            || max_queue_items == 0
        {
            return Err(InfraError::Quota {
                phase: EffectPhase::Acquire,
            });
        }
        Ok(Self {
            control_cpu_millis,
            control_rss_bytes,
            child_cpu_millis,
            child_rss_bytes,
            child_output_bytes,
            max_children,
            max_queue_items,
        })
    }

    pub const fn control_cpu_millis(&self) -> u64 {
        self.control_cpu_millis
    }

    pub const fn control_rss_bytes(&self) -> u64 {
        self.control_rss_bytes
    }

    pub const fn child_cpu_millis(&self) -> u64 {
        self.child_cpu_millis
    }

    pub const fn child_rss_bytes(&self) -> u64 {
        self.child_rss_bytes
    }

    pub const fn child_output_bytes(&self) -> u64 {
        self.child_output_bytes
    }

    pub const fn max_children(&self) -> usize {
        self.max_children
    }

    pub const fn max_queue_items(&self) -> usize {
        self.max_queue_items
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourceClaimV1 {
    child_cpu_millis: u64,
    child_rss_bytes: u64,
    child_output_bytes: u64,
    children: usize,
    queue_items: usize,
}

impl ResourceClaimV1 {
    pub const fn new(
        child_cpu_millis: u64,
        child_rss_bytes: u64,
        child_output_bytes: u64,
        children: usize,
        queue_items: usize,
    ) -> Self {
        Self {
            child_cpu_millis,
            child_rss_bytes,
            child_output_bytes,
            children,
            queue_items,
        }
    }

    pub const fn admitted_by(self, policy: ResourcePolicyV1) -> bool {
        self.child_cpu_millis <= policy.child_cpu_millis
            && self.child_rss_bytes <= policy.child_rss_bytes
            && self.child_output_bytes <= policy.child_output_bytes
            && self.children <= policy.max_children
            && self.queue_items <= policy.max_queue_items
    }
}

#[derive(Debug)]
pub struct BoundedQueue<T> {
    entries: VecDeque<T>,
    limit: usize,
}

impl<T> BoundedQueue<T> {
    pub fn new(limit: usize) -> Result<Self, InfraError> {
        if limit == 0 {
            return Err(InfraError::Quota {
                phase: EffectPhase::Acquire,
            });
        }
        Ok(Self {
            entries: VecDeque::with_capacity(limit),
            limit,
        })
    }

    pub fn push(&mut self, value: T) -> Result<(), InfraError> {
        if self.entries.len() >= self.limit {
            return Err(InfraError::Quota {
                phase: EffectPhase::Execute,
            });
        }
        self.entries.push_back(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        self.entries.pop_front()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
