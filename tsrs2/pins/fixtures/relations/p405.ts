// @noLib: true

// relpin p405: assignable source="K59A<{ a: number; b: number }>" target="K59A<{ a: number }>"
interface K59A<T> { k: keyof T }
declare var s: K59A<{ a: number; b: number }>;
var t: K59A<{ a: number }> = s;
