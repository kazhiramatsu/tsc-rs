use tsc_arena::ScratchArena;
fn main() {
    let mut arena = ScratchArena::new();
    let value;
    {
        let scope = arena.scope();
        value = scope.alloc(1u32);
    }
    println!("{}", value.get());
}
