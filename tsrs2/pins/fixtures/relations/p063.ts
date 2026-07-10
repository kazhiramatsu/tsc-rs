// @noLib: true

// relpin p063: assignable source="{ a: number } & { b: string }" target="{ a: number, b: string }"
declare var s: { a: number } & { b: string };
var t: { a: number, b: string } = s;
