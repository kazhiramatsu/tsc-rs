#![forbid(unsafe_code)]
#[derive(Clone, Copy)]
pub struct Wide(std::num::NonZeroU32, u32);
tsc_arena::identity_element!(Wide, 0);
fn main() {}
