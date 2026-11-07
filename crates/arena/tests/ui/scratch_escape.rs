use tsc_arena::ScratchArena;
struct Holder<'a> {
    values: &'a [u32],
}
fn main() {
    let mut arena = ScratchArena::new();
    let holder;
    {
        let scope = arena.scope();
        let mut vector = scope.vec();
        vector.push(1u32);
        holder = Holder {
            values: vector.as_slice(),
        };
    }
    println!("{}", holder.values[0]);
}
