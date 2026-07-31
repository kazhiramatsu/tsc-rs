// @noLib: true

// relpin p248: comparable source="{ a: number }" target="{ b: string }"
declare var s: { a: number };
var t = s as { b: string };
