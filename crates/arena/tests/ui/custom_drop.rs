use tsc_arena::record;
record! { struct HasDrop { word: u32 } }
impl Drop for HasDrop {
    fn drop(&mut self) {}
}
fn main() {}
