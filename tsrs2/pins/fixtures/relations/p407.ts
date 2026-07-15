// @noLib: true

// relpin p407: assignable source="K59B<{ a: number; b: number }>" target="K59B<{ a: number }>"
interface K59B<T> { f(k: keyof T): void }
declare var s: K59B<{ a: number; b: number }>;
var t: K59B<{ a: number }> = s;
