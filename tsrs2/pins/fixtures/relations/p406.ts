// @noLib: true

// relpin p406: assignable source="K59B<{ a: number }>" target="K59B<{ a: number; b: number }>"
interface K59B<T> { f(k: keyof T): void }
declare var s: K59B<{ a: number }>;
var t: K59B<{ a: number; b: number }> = s;
