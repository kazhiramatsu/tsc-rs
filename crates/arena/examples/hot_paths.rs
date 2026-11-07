//! Small functional example with stable symbols for instruction inspection.
//! This is not a benchmark: each wrapper is called once and checked.
use tsc_arena::{owner_of, ArenaCell, LocalArena, Ref, SharedArena};

#[unsafe(no_mangle)]
#[inline(never)]
pub fn arena_read_record(r: Ref<u64>) -> u64 {
    *r.get()
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn arena_allocate_record(arena: &SharedArena, value: u64) -> Ref<u64> {
    arena.alloc(value)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn arena_read_cell(cell: &ArenaCell<u64>) -> u64 {
    cell.get()
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn arena_find_owner(raw: u32) -> u32 {
    owner_of(raw).unwrap_or(u32::MAX)
}

fn main() {
    let arena = SharedArena::new(51);
    let r = arena_allocate_record(&arena, std::hint::black_box(42));
    let _holder = arena.freeze();
    assert_eq!(arena_read_record(r), 42);
    assert_eq!(arena_find_owner(r.raw()), 51);
    let local = LocalArena::new(52);
    let cell = local.alloc(ArenaCell::new(std::hint::black_box(17u64)));
    assert_eq!(arena_read_cell(cell.get()), 17);
    println!("hot-path example: record=42, owner=51, cell=17");
}
