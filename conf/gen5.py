import json
C=[]
def add(n,s): C.append({"name":n,"src":s})

# ── class inheritance / override ──
add("b5_override_method", "class A { f(): number { return 1; } } class B extends A { f(): number { return 2; } } const b = new B(); const n: number = b.f();")
add("b5_override_incompatible", "class A { f(): number { return 1; } } class B extends A { f(): string { return 'x'; } }")
add("b5_super_call", "class A { greet(): string { return 'a'; } } class B extends A { greet(): string { return super.greet() + 'b'; } }")
add("b5_abstract_impl", "abstract class A { abstract f(): number; } class B extends A { f(): number { return 1; } } const b = new B();")
add("b5_abstract_not_impl", "abstract class A { abstract f(): number; } class B extends A {}")
add("b5_abstract_instantiate", "abstract class A {} const a = new A();")
add("b5_protected_access", "class A { protected x = 1; } class B extends A { get(): number { return this.x; } }")
add("b5_protected_external", "class A { protected x = 1; } const a = new A(); const n = a.x;")
add("b5_private_external", "class A { private x = 1; } const a = new A(); const n = a.x;")
add("b5_static_member", "class A { static count = 0; static inc(): void { A.count++; } } A.inc(); const n: number = A.count;")
add("b5_static_inherit", "class A { static f(): number { return 1; } } class B extends A {} const n: number = B.f();")

# ── generic defaults / constraints ──
add("b5_generic_default", "function f<T = string>(x?: T): T { return x as T; } const s = f<number>(1);")
add("b5_generic_constraint_ok", "function f<T extends { length: number }>(x: T): number { return x.length; } f([1, 2]); f('abc');")
add("b5_generic_constraint_bad", "function f<T extends { length: number }>(x: T): number { return x.length; } f(42);")
add("b5_generic_class", "class Box<T> { constructor(public value: T) {} get(): T { return this.value; } } const b = new Box<number>(1); const n: number = b.get();")
add("b5_generic_class_infer", "class Box<T> { constructor(public value: T) {} } const b = new Box('hello'); const s: string = b.value;")
add("b5_generic_method_chain", "class List<T> { items: T[] = []; add(x: T): this { this.items.push(x); return this; } } const l = new List<number>(); l.add(1).add(2);")
add("b5_fbound", "interface Comparable<T> { compareTo(other: T): number; } function max<T extends Comparable<T>>(a: T, b: T): T { return a.compareTo(b) > 0 ? a : b; }")

# ── enums advanced ──
add("b5_enum_computed", "enum E { A = 1, B = A * 2, C = B + 1 } const x: number = E.C;")
add("b5_enum_string_member_type", "enum Dir { Up = 'UP', Down = 'DOWN' } function move(d: Dir): void {} move(Dir.Up);")
add("b5_enum_const_inline", "const enum E { A = 10, B = 20 } const x = E.A + E.B;")
add("b5_enum_keyof", "enum E { A, B, C } type Keys = keyof typeof E; const k: Keys = 'A';")
add("b5_enum_index_value", "enum E { A = 'a', B = 'b' } const v = E['A']; const s: string = v;")

# ── control flow narrowing ──
add("b5_narrow_typeof", "function f(x: string | number): string { if (typeof x === 'number') return x.toString(); return x; }")
add("b5_narrow_instanceof", "class Cat { meow(): void {} } class Dog { bark(): void {} } function f(a: Cat | Dog): void { if (a instanceof Cat) a.meow(); else a.bark(); }")
add("b5_narrow_in_operator", "type A = { a: number }; type B = { b: string }; function f(x: A | B): void { if ('a' in x) { const n: number = x.a; } }")
add("b5_narrow_discriminated", "type Shape = { kind: 'circle'; radius: number } | { kind: 'square'; side: number }; function area(s: Shape): number { if (s.kind === 'circle') return s.radius; return s.side; }")
add("b5_narrow_exhaustive_bad", "type T = 'a' | 'b'; function f(x: T): number { if (x === 'a') return 1; }")
add("b5_narrow_truthy", "function f(x: string | null): string { if (x) return x; return ''; }")
add("b5_narrow_assign_widen", "let x: string | number = 'a'; x = 5; const n: number = x;")

# ── functions ──
add("b5_default_param", "function greet(name: string = 'World'): string { return 'Hello ' + name; } greet(); greet('Bob');")
add("b5_rest_after_params", "function f(first: string, ...rest: number[]): void {} f('a', 1, 2, 3);")
add("b5_func_overload_this", "function f(x: number): number; function f(x: string): string; function f(x: number | string): number | string { return x; } const r = f(1);")
add("b5_callback_contextual", "[1, 2, 3].forEach((x, i) => { const n: number = x; const j: number = i; });")
add("b5_func_type_assign", "type Fn = (a: number) => number; const f: Fn = (a) => a * 2; f(5);")
add("b5_func_arity_bad", "type Fn = (a: number) => number; const f: Fn = (a, b) => a + b;")
add("b5_void_callback", "function each<T>(arr: T[], cb: (x: T) => void): void {} each([1, 2], x => x);")

# ── type operators ──
add("b5_indexed_access", "type T = { a: number; b: string }; type A = T['a']; const x: A = 1;")
add("b5_indexed_union", "type T = { a: number; b: string }; type V = T['a' | 'b']; const x: V = 1; const y: V = 's';")
add("b5_typeof_function", "function f(): number { return 1; } type FT = typeof f; const g: FT = () => 2;")
add("b5_keyof_array", "type K = keyof number[]; const k: K = 'length';")
add("b5_mapped_partial", "type T = { a: number; b: string }; type P = { [K in keyof T]?: T[K] }; const x: P = { a: 1 };")
add("b5_mapped_readonly", "type T = { a: number }; type R = { readonly [K in keyof T]: T[K] }; const r: R = { a: 1 }; r.a = 2;")
add("b5_conditional_extract", "type Ext<T> = T extends string ? T : never; type R = Ext<'a' | 1 | 'b'>; const x: R = 'a';")

# ── async / promise ──
add("b5_async_return", "async function f(): Promise<number> { return 1; } f().then(n => { const x: number = n; });")
add("b5_async_await", "async function f(): Promise<number> { return 1; } async function g(): Promise<string> { const n = await f(); return n.toString(); }")
add("b5_async_return_bad", "async function f(): Promise<number> { return 'x'; }")
add("b5_promise_all", "async function f(): Promise<void> { const [a, b] = await Promise.all([Promise.resolve(1), Promise.resolve('x')]); const n: number = a; const s: string = b; }")

# ── object / array methods ──
add("b5_object_keys", "const o = { a: 1, b: 2 }; const keys = Object.keys(o); const k: string = keys[0];")
add("b5_array_find", "const a = [1, 2, 3]; const f = a.find(x => x > 1); const n: number | undefined = f;")
add("b5_array_includes", "const a = [1, 2, 3]; const b: boolean = a.includes(2);")
add("b5_spread_array", "const a = [1, 2]; const b = [...a, 3]; const n: number = b[0];")
add("b5_spread_object", "const a = { x: 1 }; const b = { ...a, y: 2 }; const n: number = b.x; const m: number = b.y;")
add("b5_destructure_default", "function f({ a = 1, b = 'x' }: { a?: number; b?: string }): void {} f({});")

# ── misc ──
add("b5_optional_chain_call", "type T = { f?: () => number }; declare const t: T; const n = t.f?.();")
add("b5_nullish_default", "function f(x: number | null): number { return x ?? 0; }")
add("b5_template_type", "type Greeting = `Hello ${string}`; const g: Greeting = 'Hello World';")
add("b5_template_type_bad", "type Greeting = `Hello ${string}`; const g: Greeting = 'Goodbye';")
add("b5_tuple_destructure", "const [a, b]: [number, string] = [1, 'x']; const n: number = a; const s: string = b;")
add("b5_readonly_tuple", "const t: readonly [number, string] = [1, 'x']; const n: number = t[0];")

json.dump(C, open("/tmp/cases5.json","w"))
print(f"generated {len(C)} cases")
