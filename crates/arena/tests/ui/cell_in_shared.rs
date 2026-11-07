use tsc_arena::{record, ArenaCell, SharedArena};
record! { struct CellRecord { cell: ArenaCell<u32> } }
fn main() {
    SharedArena::new(1).alloc(CellRecord {
        cell: ArenaCell::new(1),
    });
}
