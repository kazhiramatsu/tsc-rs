use tsc_arena::ScratchArena;

#[test]
fn scratch_cells_vectors_growth_and_reuse() {
    let mut arena = ScratchArena::new();
    let address;
    {
        let scope = arena.scope();
        let cell = scope.alloc(17u64);
        address = cell as *const _ as usize;
        cell.set(23);
        assert_eq!(cell.get(), 23);
        assert_eq!(address % 8, 0);
        let mut vector = scope.vec();
        for i in 0..10_000u32 {
            vector.push(i);
        }
        assert_eq!(vector.len(), 10_000);
        assert_eq!(vector.as_slice(), (0..10_000).collect::<Vec<_>>());
        vector.clear();
        assert!(vector.is_empty());
        vector.extend(&[2, 4, 6]);
        assert_eq!(vector.as_slice(), [2, 4, 6]);
    }
    assert_eq!(arena.used_bytes(), 0);
    let committed = arena.committed_bytes();
    assert!(committed > 0);
    {
        let scope = arena.scope();
        let cell = scope.alloc(99u64);
        assert_eq!(cell as *const _ as usize, address);
        assert_eq!(cell.get(), 99);
    }
    assert_eq!(arena.committed_bytes(), committed);
}

#[test]
fn nested_scopes_reset_and_keep_parent_storage() {
    let mut arena = ScratchArena::new();
    let scope = arena.scope();
    let parent = scope.alloc(12u64);
    let child_address;
    {
        let child = scope.child();
        let child_cell = child.alloc(19u64);
        child_address = child_cell as *const _ as usize;
        let grandchild = child.child();
        assert_eq!(grandchild.alloc(25u64).get(), 25);
        assert_eq!(parent.get(), 12);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scope.alloc(100u32))).is_err()
        );
    }
    let reused = scope.alloc(22u64);
    assert_eq!(reused as *const _ as usize, child_address);
    assert_eq!(parent.get(), 12);
}

#[test]
fn over_aligned_zero_sized_and_borrowed_values() {
    #[repr(align(64))]
    #[derive(Clone, Copy)]
    struct Aligned(u32);
    let outside = 31;
    let mut arena = ScratchArena::new();
    let scope = arena.scope();
    let aligned = scope.alloc(Aligned(14));
    assert_eq!(aligned as *const _ as usize % 64, 0);
    assert_eq!(aligned.get().0, 14);
    assert_eq!(*scope.alloc(&outside).get(), 31);
    scope.alloc(()).set(());
    let mut vector = scope.vec::<()>();
    vector.extend(&[(); 32]);
    assert_eq!(vector.len(), 32);
    let mut empty = scope.vec::<u64>();
    empty.extend(&[]);
    assert!(empty.as_slice().is_empty());
}

#[test]
fn unwind_and_forgotten_child_reset_with_parent() {
    let mut arena = ScratchArena::new();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let scope = arena.scope();
        scope.alloc(11u32);
        let child = scope.child();
        child.alloc(12u32);
        panic!("test unwind");
    }))
    .is_err());
    assert_eq!(arena.used_bytes(), 0);
    {
        let scope = arena.scope();
        let child = scope.child();
        child.alloc(33u32);
        std::mem::forget(child);
    }
    assert_eq!(arena.used_bytes(), 0);
}

#[test]
fn scratch_report_tracks_scope_reset_and_drop() {
    let mut arena = ScratchArena::with_owner(77);
    assert_eq!(
        tsc_arena::report()
            .into_iter()
            .find(|s| s.owner == 77)
            .unwrap()
            .kind,
        "scratch"
    );
    {
        let scope = arena.scope();
        scope.alloc(14u64);
        let stats = tsc_arena::report()
            .into_iter()
            .find(|s| s.owner == 77)
            .unwrap();
        assert_eq!(stats.used, 8);
        assert!(stats.committed >= tsc_arena::BLOCK_BYTES);
    }
    let stats = tsc_arena::report()
        .into_iter()
        .find(|s| s.owner == 77)
        .unwrap();
    assert_eq!(stats.used, 0);
    assert_eq!(stats.committed, arena.committed_bytes());
    drop(arena);
    assert!(!tsc_arena::report().iter().any(|s| s.owner == 77));
}
