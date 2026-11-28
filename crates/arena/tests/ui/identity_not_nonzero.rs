#![forbid(unsafe_code)]
#[derive(Clone, Copy)]
pub struct Plain(u32);
tsc_arena::identity_element!(Plain, 0);
fn main() {}
