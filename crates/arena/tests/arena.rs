use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tsc_arena::{
    owner_of, record, report, ArenaCell, FrozenArena, LocalArena, Owned, Ref, SharedArena, Span,
    BLOCK_BYTES,
};

record! { #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Eight { pub a: u64 } }
record! { #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct TwentyFour { pub a: [u64; 3] } }
record! { #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Forty { pub a: [u64; 5] } }
record! { pub struct CellRecord { pub value: ArenaCell<u32>, pub dense_id: u32 } }
record! { pub struct Edges { pub next: Option<Ref<Eight>>, pub items: Span<Eight> } }
record! {
    pub struct SyncButNotSend {
        pub value: u64,
        pub marker: std::marker::PhantomData<std::sync::MutexGuard<'static, ()>>,
    }
}

const _: () = assert!(std::mem::size_of::<Ref<Eight>>() == 4);
const _: () = assert!(std::mem::size_of::<Option<Ref<Eight>>>() == 4);
const _: () = assert!(std::mem::size_of::<Span<Eight>>() == 8);

fn send<T: Send>() {}
fn sync<T: Sync>() {}

// Ambiguous inference would fail compilation if one of these types gained
// Send/Sync. No extra dev-dependency or unsafe negative impl is necessary.
const _: fn() = || {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: ?Sized + Send> Ambiguous<u8> for T {}
    let _ = <Ref<CellRecord> as Ambiguous<_>>::check;
    let _ = <SyncButNotSend as Ambiguous<_>>::check;
};
const _: fn() = || {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: ?Sized + Sync> Ambiguous<u8> for T {}
    let _ = <Ref<CellRecord> as Ambiguous<_>>::check;
    let _ = <SharedArena as Ambiguous<_>>::check;
    let _ = <LocalArena as Ambiguous<_>>::check;
    let _ = <tsc_arena::ScratchArena as Ambiguous<_>>::check;
};

#[test]
fn round_trips_layout_alignment_and_handles() {
    send::<Ref<Eight>>();
    send::<Ref<SyncButNotSend>>();
    sync::<Ref<SyncButNotSend>>();
    sync::<Ref<Eight>>();
    send::<SharedArena>();
    send::<LocalArena>();
    send::<FrozenArena>();
    sync::<FrozenArena>();
    send::<Owned<Ref<CellRecord>>>();
    let arena = SharedArena::new(1);
    let a = arena.alloc(Eight { a: 18 });
    let b = arena.alloc(TwentyFour { a: [1, 2, 3] });
    let c = arena.alloc(Forty { a: [4, 5, 6, 7, 8] });
    assert_eq!(a.get().a, 18);
    assert_eq!(b.get().a, [1, 2, 3]);
    assert_eq!(c.get().a, [4, 5, 6, 7, 8]);
    for address in [
        a.get() as *const _ as usize,
        b.get() as *const _ as usize,
        c.get() as *const _ as usize,
    ] {
        assert_eq!(address % 8, 0);
    }
    assert_eq!(Ref::<Eight>::from_raw(a.raw()), Some(a));
    assert_eq!(Ref::<Eight>::from_raw(0), None);
    let mut h1 = DefaultHasher::new();
    let mut h2 = DefaultHasher::new();
    a.hash(&mut h1);
    a.raw().hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
    let another = arena.alloc(Eight { a: 19 });
    assert!(a < another); // Within one block only; never a semantic ordering.
    let edge = arena.alloc(Edges {
        next: Some(a),
        items: Span::empty(),
    });
    assert_eq!(edge.get().next.unwrap().get().a, 18);
    assert!(edge.get().items.as_slice().is_empty());
}

#[test]
fn slices_lists_bytes_empty_and_multiple_blocks() {
    let arena = SharedArena::new(2);
    let values = vec![Eight { a: 73 }; BLOCK_BYTES / 8 * 3 + 1];
    let span = arena.alloc_slice(&values);
    assert_eq!(span.len(), values.len());
    assert_eq!(span.as_slice(), values.as_slice());
    assert_eq!(span.as_slice().as_ptr() as usize % 8, 0);
    let list = arena.alloc_list(&values);
    assert_eq!(list.header().len as usize, values.len());
    assert_eq!(list.header().extra, 0);
    assert_eq!(list.header() as *const _ as usize % 8, 0);
    assert_eq!(list.items().as_ptr() as usize % 8, 0);
    assert_eq!(list.items(), values.as_slice());
    let bytes = arena.alloc_bytes(b"literal\0\xff\n");
    assert_eq!(bytes.as_bytes(), b"literal\0\xff\n");
    assert_eq!(bytes.get() as *const _ as usize % 8, 0);
    assert!(arena.alloc_slice::<Eight>(&[]).as_slice().is_empty());
    assert!(arena.alloc_list::<Eight>(&[]).items().is_empty());
    assert!(arena.alloc_bytes(&[]).as_bytes().is_empty());
    assert!(Span::<Eight>::empty().start().is_none());
    // Check first, middle, last block of the contiguous multi-block span.
    let start = span.start().unwrap().raw();
    for delta in [0, (BLOCK_BYTES / 8) as u32, (BLOCK_BYTES * 3 / 8) as u32] {
        assert_eq!(owner_of(start + delta), Some(2));
    }
    drop(arena);
    for delta in [0, (BLOCK_BYTES / 8) as u32, (BLOCK_BYTES * 3 / 8) as u32] {
        assert_eq!(owner_of(start + delta), None);
    }
}

#[test]
fn exclusive_mutation_freeze_and_four_readers() {
    let mut arena = SharedArena::new(3);
    let r = arena.alloc(Forty { a: [1; 5] });
    arena.get_mut(r).a = [2, 3, 4, 5, 6];
    let arena = Arc::new(arena.freeze());
    let mut readers = Vec::new();
    for _ in 0..4 {
        let holder = arena.clone();
        readers.push(std::thread::spawn(move || {
            for _ in 0..100 {
                assert_eq!(r.get().a, [2, 3, 4, 5, 6]);
            }
            assert!(holder.committed_bytes() >= BLOCK_BYTES);
            assert_eq!(holder.used_bytes(), 40);
        }));
    }
    drop(arena);
    for thread in readers {
        thread.join().unwrap();
    }
    assert_eq!(owner_of(r.raw()), None);
}

#[test]
fn drop_diagnostics_reuse_and_stale_scalar() {
    let arena = LocalArena::new(4);
    let stale = arena.alloc(Eight { a: u64::MAX });
    let raw = stale.raw();
    drop(arena);
    assert_eq!(owner_of(raw), None);
    #[cfg(debug_assertions)]
    assert!(std::panic::catch_unwind(|| stale.get()).is_err());
    #[cfg(not(debug_assertions))]
    assert_eq!(stale.get().a, 0); // Deliberate sequential contract violation probe.
    let other = LocalArena::new(5);
    let reused = other.alloc(Eight { a: 27 });
    assert_eq!(reused.raw(), raw);
    assert_eq!(owner_of(raw), Some(5));
    assert_eq!(stale.get().a, 27); // Demonstrate valid bytes after owner change.
}

#[test]
fn reports_live_arenas_transitions_and_stats() {
    let arena = SharedArena::new(6);
    assert_eq!(arena.used_bytes(), 0);
    assert_eq!(arena.committed_bytes(), 0);
    let first = report().into_iter().find(|s| s.owner == 6).unwrap();
    assert_eq!(
        (first.kind, first.used, first.committed, first.released),
        ("shared", 0, 0, false)
    );
    arena.alloc(Eight { a: 10 });
    arena.alloc_bytes(b"abc");
    assert_eq!(arena.used_bytes(), 24);
    assert_eq!(arena.committed_bytes(), BLOCK_BYTES);
    let frozen = arena.freeze();
    let stats = report().into_iter().find(|s| s.owner == 6).unwrap();
    assert_eq!(
        (stats.kind, stats.used, stats.committed),
        ("frozen", 24, BLOCK_BYTES)
    );
    assert_eq!(frozen.used_bytes(), 24);
    frozen.release();
    assert!(!report().iter().any(|s| s.owner == 6));
    let local = LocalArena::new(6);
    local.alloc(3u64);
    assert_eq!(local.used_bytes(), 8);
    assert_eq!(local.committed_bytes(), BLOCK_BYTES);
    assert_eq!(
        report().into_iter().find(|s| s.owner == 6).unwrap().kind,
        "local"
    );
}

struct LocalThing {
    arena: LocalArena,
    r: Ref<CellRecord>,
}
impl LocalThing {
    fn enter(&self, adopt: bool) {
        if adopt {
            self.arena.adopt_current_thread();
        }
        self.r.get().value.set(42);
        assert_eq!(self.r.get().value.get(), 42);
    }
}

#[test]
fn owned_checker_transfer_and_thread_adoption() {
    send::<Owned<LocalThing>>();
    let arena = LocalArena::new(7);
    let r = arena.alloc(CellRecord {
        value: ArenaCell::new(1),
        dense_id: 0,
    });
    r.get().value.set(2);
    assert_eq!(r.get().value.get(), 2);
    let owned = Owned(LocalThing { arena, r });
    let owned = std::thread::spawn(move || {
        #[cfg(debug_assertions)]
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owned.enter(false))).is_err()
        );
        owned.enter(true);
        let next = owned.arena.alloc(CellRecord {
            value: ArenaCell::new(17),
            dense_id: 1,
        });
        assert_eq!(next.get().value.get(), 17);
        owned
    })
    .join()
    .unwrap();
    owned.enter(true);
}

#[test]
fn foreign_mutation_rejected_and_reserved_owner_values_rejected() {
    let mut first = SharedArena::new(8);
    let second = SharedArena::new(8); // Same logical owner still has separate leases.
    let r = second.alloc(8u64);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        first.get_mut(r);
    }))
    .is_err());
    for owner in [u32::MAX - 1, u32::MAX] {
        assert!(std::panic::catch_unwind(|| LocalArena::new(owner)).is_err());
    }
    assert_eq!(owner_of(0), None);
    let cell = ArenaCell::new(4u32);
    cell.set(5);
    assert_eq!(cell.get(), 5);
}

#[test]
fn concurrent_owners_keep_disjoint_live_blocks() {
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let mut threads = Vec::new();
    for owner in 20..24 {
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let arena = SharedArena::new(owner);
            let mut handles = Vec::new();
            for _ in 0..4 {
                handles.push(arena.alloc([owner as u64; BLOCK_BYTES / 8]));
            }
            barrier.wait();
            for r in &handles {
                assert_eq!(owner_of(r.raw()), Some(owner));
                assert!(r.get().iter().all(|&v| v == owner as u64));
            }
            barrier.wait();
            handles.into_iter().map(Ref::raw).collect::<Vec<_>>()
        }));
    }
    let handles: Vec<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    for raw in handles {
        assert_eq!(owner_of(raw), None);
    }
}

#[test]
fn bump_stats_exclude_closed_run_slack_and_freeze_exactly() {
    let arena = SharedArena::new(81);
    let snapshot = || report().into_iter().find(|s| s.owner == 81).unwrap();
    arena.alloc(1u64);
    assert_eq!(snapshot().used, 8);
    arena.alloc(2u64);
    assert_eq!(arena.used_bytes(), 16);
    assert_eq!(snapshot().used, 8); // Only run additions publish mutable stats.
    arena.alloc([0u8; BLOCK_BYTES - 24]);
    assert_eq!(arena.used_bytes(), BLOCK_BYTES - 8);
    arena.alloc(TwentyFour { a: [3; 3] }); // Eight-byte slack stays in the old run.
    assert_eq!(arena.used_bytes(), BLOCK_BYTES + 16);
    assert_eq!(arena.committed_bytes(), 2 * BLOCK_BYTES);
    assert_eq!(snapshot().used, BLOCK_BYTES + 16);
    arena.alloc(4u64);
    assert_eq!(arena.used_bytes(), BLOCK_BYTES + 24);
    assert_eq!(snapshot().used, BLOCK_BYTES + 16);
    let frozen = arena.freeze();
    assert_eq!(frozen.used_bytes(), BLOCK_BYTES + 24);
    assert_eq!(snapshot().used, frozen.used_bytes());
    assert_eq!(snapshot().committed, frozen.committed_bytes());
}

struct SeveralLocalBlocks {
    arena: LocalArena,
    cells: Vec<Ref<ArenaCell<u64>>>,
}
impl SeveralLocalBlocks {
    fn check(&self, adopt: bool) {
        if adopt {
            self.arena.adopt_current_thread();
        }
        for (i, cell) in self.cells.iter().enumerate() {
            cell.get().set(i as u64);
            assert_eq!(cell.get().get(), i as u64);
        }
    }
}

#[test]
fn adoption_updates_every_block_in_every_run() {
    let arena = LocalArena::new(82);
    let mut cells = Vec::new();
    for _ in 0..3 {
        let span = arena.alloc_slice(&[const { ArenaCell::new(0u64) }; 2 * BLOCK_BYTES / 8]);
        let raw = span.start().unwrap().raw();
        // Decode actual cell elements in both blocks of this allocation.
        cells.push(Ref::from_raw(raw).unwrap());
        cells.push(Ref::from_raw(raw + (BLOCK_BYTES / 8) as u32).unwrap());
    }
    let moved = Owned(SeveralLocalBlocks { arena, cells });
    let moved = std::thread::spawn(move || {
        #[cfg(debug_assertions)]
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| moved.check(false))).is_err()
        );
        moved.check(true);
        moved
    })
    .join()
    .unwrap();
    moved.check(true);
}
