// @noLib: true

// relpin p101: assignable source="{ a: number, b: number }" target="{ a?: number, x?: string }"
declare var s: { a: number, b: number };
var t: { a?: number, x?: string } = s;
