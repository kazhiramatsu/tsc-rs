// @noLib: true

// relpin p100: assignable source="{ b: number, c: number }" target="{ a?: number, x?: string }"
declare var s: { b: number, c: number };
var t: { a?: number, x?: string } = s;
