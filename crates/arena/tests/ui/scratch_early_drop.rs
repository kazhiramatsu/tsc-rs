use tsc_arena::ScratchArena;
fn main() {
    let mut arena = ScratchArena::new();
    let scope = arena.scope();
    let value = scope.alloc(1u32);
    drop(scope);
    println!("{}", value.get());
}
