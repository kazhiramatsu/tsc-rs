use tsc_arena::{record, ArenaCell, LocalArena};
record! { struct CellRecord { cell: ArenaCell<u32> } }
fn main() {
    let arena = LocalArena::new(1);
    let r = arena.alloc(CellRecord {
        cell: ArenaCell::new(1),
    });
    std::thread::spawn(move || r.get().cell.get())
        .join()
        .unwrap();
}
