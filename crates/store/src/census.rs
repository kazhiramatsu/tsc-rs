//! Allocation census site vocabulary. No heap allocation or formatting here.
//! Backing sites describe distinct requests; the reservation is a virtual
//! address-space envelope, never added to room claims as physical memory.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Site {
    StoreReservation,
    StoreMapping,
    StoreRange,
    ClaimedRoom,
    FrozenRoom,
    RecycleRoom,
    NodeList,
    SymbolDeclarations,
    TypeList,
    Signature,
    Members,
    Diagnostic,
    Type,
}
pub const NAMES: [&str; 13] = [
    "store.reservation",
    "store.mapping",
    "store.range",
    "claimed.room",
    "frozen.room",
    "recycle.room",
    "node.list",
    "symbol.declarations",
    "type.list",
    "signature.created",
    "members.created",
    "diagnostic.accepted",
    "type.created",
];
pub const BACKING_SITES: usize = 6;
pub type Hook = fn(Site, u64, usize, usize);
#[cfg(feature = "alloc-census")]
static HOOK: std::sync::OnceLock<Hook> = std::sync::OnceLock::new();
/// Installed by the observation executable, before compiler work.
pub fn install(hook: Hook) {
    #[cfg(feature = "alloc-census")]
    {
        let _ = HOOK.set(hook);
    }
    #[cfg(not(feature = "alloc-census"))]
    let _ = hook;
}
/// `calls` is nonzero only at the unique request/publication point. Later
/// initialization adds used bytes with calls=0. Logical length is passed in
/// `reserved`, initialized payload bytes in `used`.
#[inline(always)]
pub fn note(site: Site, calls: u64, reserved: usize, used: usize) {
    #[cfg(feature = "alloc-census")]
    if let Some(hook) = HOOK.get() {
        hook(site, calls, reserved, used);
    }
    #[cfg(not(feature = "alloc-census"))]
    let _ = (site, calls, reserved, used);
}

#[cfg(feature = "alloc-census")]
thread_local! {
    static SITE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// A lexical producer tag for sampled GlobalAlloc events. Zero is unknown;
/// tags are separate from exact logical publications, and never infer counts.
pub struct Scope {
    #[cfg(feature = "alloc-census")]
    previous: usize,
}
#[inline(always)]
pub fn scope(site: Site) -> Scope {
    #[cfg(feature = "alloc-census")]
    {
        Scope {
            previous: SITE.with(|current| current.replace(site as usize + 1)),
        }
    }
    #[cfg(not(feature = "alloc-census"))]
    {
        let _ = site;
        Scope {}
    }
}
impl Drop for Scope {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(feature = "alloc-census")]
        SITE.with(|current| current.set(self.previous));
    }
}
#[inline(always)]
pub fn current_site() -> usize {
    #[cfg(feature = "alloc-census")]
    {
        SITE.try_with(std::cell::Cell::get).unwrap_or(0)
    }
    #[cfg(not(feature = "alloc-census"))]
    {
        0
    }
}

// The executable snapshots its fixed pointer table at the checker's M7
// after_check boundary. The callback and all call sites compile away off.
#[cfg(feature = "alloc-census")]
static PEAK_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();
pub fn install_peak(hook: fn()) {
    #[cfg(feature = "alloc-census")]
    let _ = PEAK_HOOK.set(hook);
    #[cfg(not(feature = "alloc-census"))]
    let _ = hook;
}
#[inline(always)]
pub fn after_check() {
    #[cfg(feature = "alloc-census")]
    if let Some(hook) = PEAK_HOOK.get() {
        hook();
    }
}

#[cfg(feature = "alloc-census")]
static THREAD_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();
pub fn install_thread(hook: fn()) {
    #[cfg(feature = "alloc-census")]
    let _ = THREAD_HOOK.set(hook);
    #[cfg(not(feature = "alloc-census"))]
    let _ = hook;
}
/// Prepare bounds outside allocator hooks, including config-prefetch workers
/// which do not use the loader/checker worker-budget start hook.
#[inline(always)]
pub fn warm_thread() {
    #[cfg(feature = "alloc-census")]
    if let Some(hook) = THREAD_HOOK.get() {
        hook();
    }
}
