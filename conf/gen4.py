import json
C=[]
def add(n,s): C.append({"name":n,"src":s})

# ── enums ──
add("b4_enum_numeric", "enum E { A, B, C } const x: E = E.A; const n: number = E.B;")
add("b4_enum_string", "enum E { A = 'a', B = 'b' } const x: E = E.A; const s: string = E.A;")
add("b4_enum_explicit", "enum E { A = 1, B = 2, C = 4 } const x: E.A = E.A; const y: E.A = E.B;")
add("b4_enum_as_type", "enum Color { Red, Green } function f(c: Color): number { return c; } f(Color.Red);")
add("b4_enum_reverse", "enum E { A, B } const name = E[0]; const s: string = name;")
add("b4_enum_const", "const enum E { A, B } const x: E = E.A;")
add("b4_enum_member_init_bad", "enum E { A = 'a', B } ")
add("b4_enum_assign_number_bad", "enum E { A, B } const x: E = 5;")

# ── function overloads ──
add("b4_overload_basic", "function f(x: number): number; function f(x: string): string; function f(x: any): any { return x; } const n: number = f(1); const s: string = f('a');")
add("b4_overload_resolve_bad", "function f(x: number): number; function f(x: string): string; function f(x: any): any { return x; } const n: number = f('a');")
add("b4_overload_no_match", "function f(x: number): number; function f(x: string): string; function f(x: any): any { return x; } f(true);")
add("b4_overload_optional", "function f(x: number, y?: string): void; function f(x: number): void { } f(1); f(1, 'a');")

# ── this types ──
add("b4_this_polymorphic", "class C { val = 1; self(): this { return this; } } class D extends C { extra = 2; } const d = new D().self(); const n: number = d.extra;")
add("b4_this_param", "function f(this: { x: number }): number { return this.x; }")
add("b4_this_param_call_bad", "function f(this: { x: number }): number { return this.x; } f();")
add("b4_this_in_interface", "interface Chain { next(): this; } declare const c: Chain; const r: Chain = c.next();")

# ── assertion functions ──
add("b4_asserts_is", "function assert(x: unknown): asserts x is string {} declare const v: unknown; assert(v); const s: string = v;")
add("b4_asserts_cond", "function assert(c: unknown): asserts c {} declare const x: string | null; assert(x); const s: string = x;")
add("b4_asserts_nonnull", "function assertDef<T>(x: T): asserts x is NonNullable<T> {} declare const v: string | undefined; assertDef(v); const s: string = v;")

# ── user-defined type guards ──
add("b4_guard_basic", "function isStr(x: unknown): x is string { return typeof x === 'string'; } declare const v: unknown; if (isStr(v)) { const s: string = v; }")
add("b4_guard_generic", "function isArr<T>(x: T | T[]): x is T[] { return Array.isArray(x); } declare const v: number | number[]; if (isArr(v)) { const a: number[] = v; }")
add("b4_guard_narrow_else", "function isStr(x: unknown): x is string { return typeof x === 'string'; } declare const v: string | number; if (!isStr(v)) { const n: number = v; }")

# ── getters/setters ──
add("b4_getter_only_readonly", "class C { get x(): number { return 1; } } const c = new C(); c.x = 2;")
add("b4_getter_setter_types", "class C { private _v = 0; get v(): number { return this._v; } set v(n: number) { this._v = n; } } const c = new C(); c.v = 5; const n: number = c.v;")
add("b4_getter_return_bad", "class C { get x(): number { return 'a'; } }")

# ── const assertions ──
add("b4_as_const_literal", "const x = 'hello' as const; const y: 'hello' = x;")
add("b4_as_const_obj", "const o = { a: 1, b: 'x' } as const; const a: 1 = o.a;")
add("b4_as_const_arr", "const a = [1, 2, 3] as const; const t: readonly [1, 2, 3] = a;")
add("b4_as_const_mutate_bad", "const o = { a: 1 } as const; o.a = 2;")

# ── index signatures ──
add("b4_index_string", "interface Dict { [k: string]: number; } const d: Dict = { a: 1, b: 2 };")
add("b4_index_string_bad", "interface Dict { [k: string]: number; } const d: Dict = { a: 'x' };")
add("b4_index_number", "interface Arr { [i: number]: string; } const a: Arr = { 0: 'a', 1: 'b' };")
add("b4_index_access", "interface Dict { [k: string]: number; } declare const d: Dict; const n: number = d['key'];")
add("b4_index_prop_conflict", "interface I { [k: string]: number; name: string; }")

# ── unique symbol ──
add("b4_unique_symbol", "declare const sym: unique symbol; const obj = { [sym]: 1 }; const n: number = obj[sym];")
add("b4_symbol_key", "const s = Symbol(); const o = { [s]: 'v' };")

# ── namespaces ──
add("b4_namespace_basic", "namespace N { export const x = 1; export type T = number; } const v: N.T = N.x;")
add("b4_namespace_nested", "namespace A { export namespace B { export const y = 2; } } const v: number = A.B.y;")
add("b4_namespace_merge", "namespace N { export const a = 1; } namespace N { export const b = 2; } const v: number = N.a + N.b;")

# ── intersections ──
add("b4_intersect_basic", "type A = { a: number }; type B = { b: string }; type C = A & B; const x: C = { a: 1, b: 's' };")
add("b4_intersect_missing", "type A = { a: number }; type B = { b: string }; type C = A & B; const x: C = { a: 1 };")
add("b4_intersect_method", "type A = { f(): number }; type B = { g(): string }; const x: A & B = { f: () => 1, g: () => 's' };")
add("b4_intersect_conflict", "type A = { x: number }; type B = { x: string }; declare const v: A & B; const n: number = v.x;")

# ── optional / rest params ──
add("b4_optional_param", "function f(a: number, b?: string): string { return b ?? 'd'; } f(1); f(1, 'x');")
add("b4_rest_param", "function sum(...nums: number[]): number { return nums.reduce((a, b) => a + b, 0); } sum(1, 2, 3);")
add("b4_rest_param_bad", "function sum(...nums: number[]): number { return 0; } sum(1, 'a');")
add("b4_optional_before_required_bad", "function f(a?: number, b: string): void {}")

# ── literal widening ──
add("b4_let_widen", "let x = 'a'; x = 'b'; const y: string = x;")
add("b4_const_no_widen", "const x = 'a'; const y: 'a' = x;")
add("b4_obj_prop_widen", "const o = { s: 'hello' }; o.s = 'world';")
add("b4_array_widen", "const a = [1, 2]; a.push(3); const n: number = a[0];")

# ── keyof / typeof ──
add("b4_keyof_basic", "type K = keyof { a: number; b: string }; const x: K = 'a'; const y: K = 'c';")
add("b4_typeof_var", "const obj = { a: 1, b: 'x' }; type T = typeof obj; const v: T = { a: 2, b: 'y' };")
add("b4_keyof_typeof", "const obj = { a: 1, b: 2 }; type K = keyof typeof obj; const k: K = 'a';")
add("b4_indexed_keyof", "type T = { a: number; b: string }; type V = T[keyof T]; const x: V = 1; const y: V = 's';")

# ── recursive types ──
add("b4_recursive_json", "type Json = string | number | boolean | null | Json[] | { [k: string]: Json }; const x: Json = { a: [1, 'b', { c: true }] };")
add("b4_recursive_tree", "interface Tree { value: number; children: Tree[]; } const t: Tree = { value: 1, children: [{ value: 2, children: [] }] };")
add("b4_recursive_linkedlist", "type List<T> = { head: T; tail: List<T> | null }; const l: List<number> = { head: 1, tail: { head: 2, tail: null } };")

# ── distributive conditional edge ──
add("b4_cond_distribute_arr", "type ToArr<T> = T extends any ? T[] : never; type R = ToArr<number | string>; const x: R = [1]; const y: R = ['a'];")
add("b4_cond_infer_union", "type Unpack<T> = T extends (infer U)[] ? U : T; type R = Unpack<number[] | string[]>; const x: R = 1; const y: R = 's';")
add("b4_cond_boolean_distribute", "type Not<T> = T extends true ? false : true; type R = Not<boolean>; const x: R = true; const y: R = false;")

# ── excess property ──
add("b4_excess_obj", "interface P { a: number; } const x: P = { a: 1, b: 2 };")
add("b4_excess_nested", "interface P { o: { a: number } } const x: P = { o: { a: 1, b: 2 } };")
add("b4_excess_via_var_ok", "interface P { a: number; } const tmp = { a: 1, b: 2 }; const x: P = tmp;")

# ── misc ──
add("b4_void_return_ignore", "function f(cb: () => void): void { cb(); } f(() => 42);")
add("b4_never_in_union", "type T = string | never; const x: T = 's';")
add("b4_tuple_length", "const t: [number, string] = [1, 'a']; const len: 2 = t.length;")
add("b4_readonly_array_method", "const a: readonly number[] = [1, 2]; const b = a.map(x => x * 2); const n: number = b[0];")
add("b4_string_method_chain", "const s = 'hello'.toUpperCase().slice(1); const r: string = s;")

json.dump(C, open("/tmp/cases4.json","w"))
print(f"generated {len(C)} cases")
