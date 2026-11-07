//! Compile only. These safe-looking programs violate the legacy caller
//! contract. They must never run. Compilation demonstrates the soundness gap
//! in the submitted API independently of the runtime tests passing.
#![allow(dead_code)]
use tsc_arena::{Ref, SharedArena};

fn aliasing_views() {
    let mut arena = SharedArena::new(1);
    let r = arena.alloc(1u64);
    let shared = r.get();
    *arena.get_mut(r) = 2;
    std::hint::black_box(*shared);
}

fn view_outlives_owner() -> &'static u64 {
    let arena = SharedArena::new(1);
    arena.alloc(1u64).get()
}

fn forged_handle() -> &'static [u64; 3] {
    Ref::from_raw(u32::MAX).unwrap().get()
}

fn main() {}
