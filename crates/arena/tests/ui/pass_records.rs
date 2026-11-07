#![forbid(unsafe_code)]
use tsc_arena::{record, ArenaCell, LocalArena, Ref, SharedArena, Span};
record! { struct Header { kind: u32, dense_id: u32, next: Option<Ref<Header>>, items: Span<u32> } }
record! { struct Local { cell: ArenaCell<u32> } }
fn main() {
    let arena = SharedArena::new(1);
    let r = arena.alloc(Header {
        kind: 0,
        dense_id: 0,
        next: None,
        items: Span::empty(),
    });
    let holder = arena.freeze();
    assert_eq!(r.get().kind, 0);
    let local = LocalArena::new(2);
    local
        .alloc(Local {
            cell: ArenaCell::new(1),
        })
        .get()
        .cell
        .set(2);
    drop(holder);
}
