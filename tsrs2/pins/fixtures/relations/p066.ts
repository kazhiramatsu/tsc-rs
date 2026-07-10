// @noLib: true

// relpin p066: assignable source="{ a: number } & { b: string }" target="{ b: string } & { a: number }"
declare var s: { a: number } & { b: string };
var t: { b: string } & { a: number } = s;
