//! B-2 list-copy census. Only compiled with `perf-counters`.
//!
//! Bytes are logical element payload copied, including inline copies. They
//! exclude capacity, allocator metadata and container headers. A handle event
//! has zero copied elements/bytes; its histogram describes the referenced
//! list. Deep payloads (index components and UTF-16 text) have separate sites.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

pub const FIELDS: [&str; 8] = [
    "calls",
    "elements",
    "bytes",
    "len_0_1",
    "len_2_8",
    "len_9_32",
    "len_33_256",
    "len_257_plus",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Group {
    Nodes,
    Declarations,
    Types,
    Signatures,
    Properties,
    IndexInfos,
    Sidecars,
    Scratch,
}

const GROUPS: [Group; 8] = [
    Group::Nodes,
    Group::Declarations,
    Group::Types,
    Group::Signatures,
    Group::Properties,
    Group::IndexInfos,
    Group::Sidecars,
    Group::Scratch,
];

macro_rules! group_names {
    ($name:literal) => {
        [
            concat!("b2.group.", $name, ".calls"),
            concat!("b2.group.", $name, ".elements"),
            concat!("b2.group.", $name, ".bytes"),
            concat!("b2.group.", $name, ".len_0_1"),
            concat!("b2.group.", $name, ".len_2_8"),
            concat!("b2.group.", $name, ".len_9_32"),
            concat!("b2.group.", $name, ".len_33_256"),
            concat!("b2.group.", $name, ".len_257_plus"),
        ]
    };
}

const GROUP_NAMES: [[&str; 8]; 8] = [
    group_names!("nodes"),
    group_names!("declarations"),
    group_names!("types"),
    group_names!("signatures"),
    group_names!("properties"),
    group_names!("index_infos"),
    group_names!("sidecars"),
    group_names!("scratch"),
];

/// One explicit, stable source expression. The first execution registers its
/// counters; subsequent executions only perform relaxed atomic additions.
pub struct Site {
    group: Group,
    names: [&'static str; 8],
    registered: Once,
    values: [AtomicU64; 8],
}

static SITES: Mutex<Vec<&'static Site>> = Mutex::new(Vec::new());

impl Site {
    pub const fn new(group: Group, names: [&'static str; 8]) -> Self {
        Self {
            group,
            names,
            registered: Once::new(),
            values: [const { AtomicU64::new(0) }; 8],
        }
    }

    /// `len` describes this list operation. `elements` and `bytes` describe
    /// actual payload copies, so both are zero for a shared-handle clone.
    #[inline]
    pub fn note(&'static self, len: usize, elements: usize, bytes: usize) {
        self.registered
            .call_once(|| SITES.lock().expect("copy sites poisoned").push(self));
        let bucket = match len {
            0..=1 => 3,
            2..=8 => 4,
            9..=32 => 5,
            33..=256 => 6,
            _ => 7,
        };
        self.values[0].fetch_add(1, Ordering::Relaxed);
        self.values[1].fetch_add(elements as u64, Ordering::Relaxed);
        self.values[2].fetch_add(bytes as u64, Ordering::Relaxed);
        self.values[bucket].fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn slice<T>(&'static self, slice: &[T]) {
        self.note(slice.len(), slice.len(), std::mem::size_of_val(slice));
    }
}

/// Called at CLI exit through the existing performance-counter snapshot.
/// Sorting also makes reports deterministic across checker schedules.
pub fn snapshot() -> Vec<(&'static str, Option<u64>)> {
    let sites = SITES.lock().expect("copy sites poisoned");
    let mut rows = std::collections::BTreeMap::new();
    let mut groups = [[0_u64; 8]; 8];
    for site in sites.iter() {
        for (i, name) in site.names.iter().enumerate() {
            let value = site.values[i].load(Ordering::Relaxed);
            groups[site.group as usize][i] += value;
            // A source macro can expand the same named site more than
            // once. Report its combined work under one stable name.
            *rows.entry(*name).or_insert(0) += value;
        }
    }
    for group in GROUPS {
        for (i, name) in GROUP_NAMES[group as usize].iter().enumerate() {
            rows.insert(*name, groups[group as usize][i]);
        }
    }
    rows.into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect()
}

/// Observe a slice without evaluating or traversing it in a default build.
/// The caller supplies the stable site ID and group; no line-number lookup,
/// formatting, hash lookup or per-event allocation is performed.
#[macro_export]
macro_rules! census_slice {
    ($site:literal, $group:ident, $slice:expr) => {{
        #[cfg(feature = "perf-counters")]
        {
            static SITE: $crate::perf::list_copies::Site = $crate::perf::list_copies::Site::new(
                $crate::perf::list_copies::Group::$group,
                $crate::census_names!($site),
            );
            SITE.slice($slice);
        }
    }};
}

#[macro_export]
macro_rules! census_names {
    ($site:literal) => {
        [
            concat!("b2.site.", $site, ".calls"),
            concat!("b2.site.", $site, ".elements"),
            concat!("b2.site.", $site, ".bytes"),
            concat!("b2.site.", $site, ".len_0_1"),
            concat!("b2.site.", $site, ".len_2_8"),
            concat!("b2.site.", $site, ".len_9_32"),
            concat!("b2.site.", $site, ".len_33_256"),
            concat!("b2.site.", $site, ".len_257_plus"),
        ]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_payload_and_handle_accounting() {
        static SITE: Site = Site::new(Group::Types, crate::census_names!("test.boundaries"));
        let lengths = [0, 1, 2, 8, 9, 32, 33, 256, 257];
        for len in lengths {
            SITE.note(len, len, len * 4);
        }
        SITE.note(257, 0, 0);
        let values: Vec<_> = SITE
            .values
            .iter()
            .map(|v| v.load(Ordering::Relaxed))
            .collect();
        assert_eq!(values, [10, 598, 2392, 2, 2, 2, 2, 2]);
        let rows = snapshot();
        for (name, value) in SITE.names.iter().zip(values) {
            assert!(rows.contains(&(*name, Some(value))));
        }
    }

    #[test]
    fn expanded_source_macros_share_one_report_row() {
        static FIRST: Site = Site::new(Group::Types, crate::census_names!("test.expanded"));
        static SECOND: Site = Site::new(Group::Types, crate::census_names!("test.expanded"));
        FIRST.slice(&[1_u32, 2]);
        SECOND.slice(&[3_u32]);
        let rows = snapshot();
        assert_eq!(
            rows.iter()
                .filter(|(name, _)| *name == FIRST.names[0])
                .count(),
            1
        );
        assert!(rows.contains(&(FIRST.names[0], Some(2))));
        assert!(rows.contains(&(FIRST.names[1], Some(3))));
        assert!(rows.contains(&(FIRST.names[2], Some(12))));
    }
}
