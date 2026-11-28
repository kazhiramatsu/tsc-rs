#![forbid(unsafe_code)]
use tsc_arena::LocalArena;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Id(std::num::NonZeroU32);
tsc_arena::identity_element!(Id, 0);
fn main() {
    let arena = LocalArena::new(3);
    let id = Id(std::num::NonZeroU32::new(7).unwrap());
    assert_eq!(arena.alloc_slice(&[id, id]).as_slice(), &[id, id]);
    assert_eq!(arena.alloc_list(&[id]).items(), &[id]);
}
