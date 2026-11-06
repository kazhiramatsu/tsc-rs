//! Streaming memory inventory. No growing observer collections: shared backing
//! addresses are emitted for exact, global deduplication by mem_account.py.
//! All byte fields are logical quantities unless explicitly named resident.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::sync::OnceLock;

static ALLOCATOR: OnceLock<fn(&mut Account)> = OnceLock::new();

pub fn set_allocator_probe(probe: fn(&mut Account)) {
    let _ = ALLOCATOR.set(probe);
}

pub struct Account {
    file: BufWriter<File>,
    instant: &'static str,
    failed: bool,
    region_ends: [usize; crate::REGIONS],
}

impl Account {
    /// Append, allowing several Programs to share a sidecar. Each begin/end
    /// pair is a separate sample; the reducer never combines their pointers.
    pub fn open(instant: &'static str) -> Option<Self> {
        let path = std::env::var_os("TSRS_MEM_ACCOUNT")?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let mut this = Self {
            file: BufWriter::with_capacity(2048, file),
            instant,
            failed: false,
            region_ends: [0; crate::REGIONS],
        };
        this.write(format_args!("{{\"kind\":\"begin\",\"schema\":1,\"instant\":\"{instant}\",\"pid\":{},\"residency_method\":\"mincore; whole intersecting pages; null on failure/unsupported\"}}\n", std::process::id()));
        this.metric("observer.path_capacity_bytes", Some(path.capacity()));
        this.process(false);
        if let Some(probe) = ALLOCATOR.get() {
            probe(&mut this);
        }
        EXTRA.with(|extra| {
            if let Some(extra) = extra.get() {
                extra(&mut this);
            }
        });
        Some(this)
    }

    pub fn write(&mut self, args: std::fmt::Arguments<'_>) {
        if self.file.write_fmt(args).is_err() {
            self.failed = true;
        }
    }

    fn number(&mut self, n: Option<usize>) {
        match n {
            Some(n) => self.write(format_args!("{n}")),
            None => self.write(format_args!("null")),
        }
    }

    pub fn metric(&mut self, name: &str, bytes: Option<usize>) {
        self.write(format_args!(
            "{{\"kind\":\"metric\",\"name\":\"{name}\",\"bytes\":"
        ));
        match bytes {
            Some(n) => self.write(format_args!("{n}")),
            None => self.write(format_args!("null")),
        }
        self.write(format_args!("}}\n"));
    }

    pub fn unknown(&mut self, owner: &str) {
        self.write(format_args!(
            "{{\"kind\":\"unknown\",\"owner\":\"{owner}\"}}\n"
        ));
    }

    /// Each backing reference has a stable address within this stopped sample.
    /// The reducer charges its capacity and initialized bytes to its first owner.
    /// A null unique_backing_bytes is deliberately unresolved, never zero.
    #[allow(clippy::too_many_arguments)]
    pub fn backing(
        &mut self,
        owner: &str,
        address: usize,
        count: usize,
        width: usize,
        initialized: impl Into<Option<usize>>,
        capacity: usize,
        reserved: usize,
        occupied: Option<usize>,
        slots: Option<usize>,
    ) {
        let resident = if capacity >= 65536 {
            resident_bytes(address, capacity)
        } else {
            None
        };
        self.backing_inner(
            owner,
            address,
            count,
            width,
            initialized.into(),
            capacity,
            reserved,
            occupied,
            slots,
            resident,
        );
    }

    /// One deduplication unit comprising several discontiguous allocations.
    /// Residency cannot be queried as a single address interval.
    pub fn composite(
        &mut self,
        owner: &str,
        identity: usize,
        count: usize,
        width: usize,
        initialized: usize,
        capacity: usize,
    ) {
        self.backing_inner(
            owner,
            identity,
            count,
            width,
            Some(initialized),
            capacity,
            0,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn backing_inner(
        &mut self,
        owner: &str,
        address: usize,
        count: usize,
        width: usize,
        initialized: Option<usize>,
        capacity: usize,
        reserved: usize,
        occupied: Option<usize>,
        slots: Option<usize>,
        resident: Option<usize>,
    ) {
        if capacity == 0 {
            return;
        }
        if (crate::FIRST_REGION..crate::FIRST_REGION + crate::REGIONS * crate::REGION_BYTES)
            .contains(&address)
        {
            let region = (address - crate::FIRST_REGION) / crate::REGION_BYTES;
            let end = address - (crate::FIRST_REGION + region * crate::REGION_BYTES) + capacity;
            self.region_ends[region] = self.region_ends[region].max(end);
        }
        self.write(format_args!("{{\"kind\":\"backing\",\"owner\":\"{owner}\",\"backing_id\":{address},\"count\":{count},\"record_width\":{width},\"initialized_bytes\":"));
        self.number(initialized);
        self.write(format_args!(",\"capacity_bytes\":{capacity},\"unique_backing_bytes\":null,\"retired_versions_bytes\":null,\"reserved_virtual_bytes\":{reserved},\"resident_bytes\":"));
        match resident {
            Some(n) => self.write(format_args!("{n}")),
            None => self.write(format_args!("null")),
        }
        self.write(format_args!(",\"non_default_entries\":"));
        self.number(occupied);
        self.write(format_args!(",\"slots\":"));
        self.number(slots);
        self.write(format_args!("}}\n"));
    }

    /// A separately owned Box slice, or a slice whose backing is identified
    /// elsewhere with exactly this extent. Never use for a Vec sub-slice.
    pub fn slice<T>(&mut self, owner: &str, values: &[T]) {
        let bytes = std::mem::size_of_val(values);
        self.backing(
            owner,
            values.as_ptr() as usize,
            values.len(),
            size_of::<T>(),
            bytes,
            bytes,
            0,
            None,
            None,
        );
    }

    pub fn vector<T>(&mut self, owner: &str, values: &Vec<T>) {
        self.backing(
            owner,
            values.as_ptr() as usize,
            values.len(),
            size_of::<T>(),
            values.len() * size_of::<T>(),
            values.capacity() * size_of::<T>(),
            0,
            None,
            None,
        );
    }

    pub fn store<T: crate::StoreRecord>(&mut self, owner: &str, values: &crate::StoreVec<T>) {
        let capacity = values.capacity() * size_of::<T>();
        self.backing(
            owner,
            values.as_ptr() as usize,
            values.len(),
            size_of::<T>(),
            values.len() * size_of::<T>(),
            capacity,
            if values.is_published() { capacity } else { 0 },
            None,
            None,
        );
    }

    /// An exclusively owned allocation whose address is unavailable. Callers
    /// must not pass shared backing here. The bytes are exact or explicitly
    /// labeled estimates by the owner name.
    pub fn owned(
        &mut self,
        owner: &str,
        count: usize,
        width: usize,
        initialized: usize,
        capacity: usize,
    ) {
        self.write(format_args!("{{\"kind\":\"owner\",\"owner\":\"{owner}\",\"count\":{count},\"record_width\":{width},\"initialized_bytes\":{initialized},\"capacity_bytes\":{capacity},\"unique_backing_bytes\":{capacity},\"retired_versions_bytes\":null,\"reserved_virtual_bytes\":0,\"resident_bytes\":null}}\n"));
    }

    /// Exact initialized entry payload, with unknown bucket/control capacity.
    /// std::HashMap exposes usable capacity, not its allocation layout.
    pub fn map<K, V, S>(&mut self, owner: &str, map: &std::collections::HashMap<K, V, S>) {
        self.entries(owner, map.len(), size_of::<(K, V)>(), map.capacity());
    }

    /// Hashbrown exposes allocation size and raw bucket count. Count occupied
    /// record slots plus all control bytes; uninitialized empty buckets are
    /// capacity, not initialized records.
    pub fn hash_table(
        &mut self,
        owner: &str,
        count: usize,
        width: usize,
        buckets: usize,
        allocation: usize,
    ) {
        let initialized = count * width + allocation.saturating_sub(buckets * width);
        self.owned(owner, count, width, initialized, allocation);
    }

    pub fn entries(
        &mut self,
        owner: &str,
        count: usize,
        width: usize,
        slots: impl Into<Option<usize>>,
    ) {
        let initialized = count * width;
        let slots = slots.into();
        self.write(format_args!("{{\"kind\":\"owner\",\"owner\":\"{owner}\",\"count\":{count},\"record_width\":{width},\"initialized_bytes\":{initialized},\"capacity_bytes\":null,\"unique_backing_bytes\":null,\"retired_versions_bytes\":null,\"reserved_virtual_bytes\":0,\"resident_bytes\":null,\"non_default_entries\":{count},\"slots\":"));
        self.number(slots);
        self.write(format_args!("}}\n"));
    }

    fn process(&mut self, after_observation: bool) {
        let names = if after_observation {
            [
                "os.after_observation.physical_footprint",
                "os.after_observation.lifetime_peak_footprint",
                "os.after_observation.rss",
            ]
        } else {
            [
                "os.physical_footprint",
                "os.lifetime_peak_footprint",
                "os.rss",
            ]
        };
        #[cfg(target_os = "macos")]
        {
            let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
            // SAFETY: V4 writes the SDK-defined structure to this aligned buffer.
            let ok = unsafe {
                libc::proc_pid_rusage(
                    libc::getpid(),
                    libc::RUSAGE_INFO_V4,
                    info.as_mut_ptr().cast(),
                )
            } == 0;
            if ok {
                // SAFETY: a successful call initialized the complete structure.
                let info = unsafe { info.assume_init() };
                self.metric(names[0], Some(info.ri_phys_footprint as usize));
                self.metric(names[1], Some(info.ri_lifetime_max_phys_footprint as usize));
                self.metric(names[2], Some(info.ri_resident_size as usize));
                return;
            }
        }
        self.metric(names[0], None);
        self.metric(names[1], None);
        self.metric(names[2], None);
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        if crate::RESERVE.is_completed() {
            for region in 0..crate::REGIONS {
                let extent = self.region_ends[region];
                let base = crate::FIRST_REGION + region * crate::REGION_BYTES;
                self.write(format_args!("{{\"kind\":\"mapping\",\"region\":{region},\"reserved_virtual_bytes\":{},\"observed_extent_bytes\":{extent},\"resident_bytes\":", crate::REGION_BYTES));
                self.number(if extent == 0 {
                    None
                } else {
                    resident_bytes(base, extent)
                });
                self.write(format_args!(",\"resident_scope\":\"base through highest observed live owner end; excludes unobserved retired ranges\"}}\n"));
            }
        }
        // Owner/fill walks can fault existing pages. Keep a second OS reading
        // so this effect is visible rather than attributed to compiler work.
        self.process(true);
        let instant = self.instant;
        let failed = self.failed;
        self.write(format_args!(
            "{{\"kind\":\"end\",\"instant\":\"{instant}\",\"write_failed\":{failed}}}\n"
        ));
    }
}

/// Does not read or fault in the measured range. Each call needs 256 bytes of
/// stack, regardless of mapping size. Intersecting boundary pages may belong
/// to several allocations, so resident_bytes is not an additive owner total.
pub fn resident_bytes(address: usize, bytes: usize) -> Option<usize> {
    if bytes == 0 {
        return Some(0);
    }
    #[cfg(unix)]
    {
        // SAFETY: sysconf has no pointer arguments.
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        let first = address / page * page;
        let last = address.checked_add(bytes)?.checked_add(page - 1)? / page * page;
        let mut bits = [0u8; 256];
        let mut cursor = first;
        let mut resident = 0;
        while cursor < last {
            let pages = ((last - cursor) / page).min(bits.len());
            // SAFETY: mincore only queries mappings; the output holds one
            // byte per page. Invalid/unmapped input returns an error.
            if unsafe {
                libc::mincore(
                    cursor as *mut libc::c_void,
                    pages * page,
                    bits.as_mut_ptr().cast(),
                )
            } != 0
            {
                return None;
            }
            resident += bits[..pages].iter().filter(|&&b| b & 1 != 0).count() * page;
            cursor += pages * page;
        }
        Some(resident)
    }
    #[cfg(not(unix))]
    {
        let _ = (address, bytes);
        None
    }
}

type ExtraVisitor = &'static dyn Fn(&mut Account);

thread_local! {
    static EXTRA: std::cell::Cell<Option<ExtraVisitor>> = const { std::cell::Cell::new(None) };
}

/// Register a borrowed, thread-local owner visitor for a synchronous operation.
/// Neither Account nor any public API retains the borrow past this scope.
/// Nested scopes and unwinding restore the previous visitor.
pub fn with_extra<R>(extra: &dyn Fn(&mut Account), work: impl FnOnce() -> R) -> R {
    struct Restore(Option<ExtraVisitor>);
    impl Drop for Restore {
        fn drop(&mut self) {
            EXTRA.with(|slot| slot.set(self.0));
        }
    }
    // SAFETY: the visitor is read only on this thread, during `work`, while
    // its original borrow is alive. Restore removes it on return or unwind.
    // No API exposes this erased reference or transfers it to another thread.
    let erased = unsafe {
        std::mem::transmute::<&dyn Fn(&mut Account), &'static dyn Fn(&mut Account)>(extra)
    };
    let _restore = Restore(EXTRA.with(|slot| slot.replace(Some(erased))));
    work()
}

pub fn store_reservation_bytes() -> usize {
    if crate::RESERVE.is_completed() {
        crate::REGIONS * crate::REGION_BYTES
    } else {
        0
    }
}
