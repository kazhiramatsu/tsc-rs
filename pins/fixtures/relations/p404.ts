// @noLib: true

// relpin p404: assignable source="K59A<{ a: number }>" target="K59A<{ a: number; b: number }>"
interface K59A<T> { k: keyof T }
declare var s: K59A<{ a: number }>;
var t: K59A<{ a: number; b: number }> = s;
