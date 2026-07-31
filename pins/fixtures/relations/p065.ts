// @noLib: true

// relpin p065: assignable source="{ a: number }" target="{ a: number } & { b: string }"
declare var s: { a: number };
var t: { a: number } & { b: string } = s;
