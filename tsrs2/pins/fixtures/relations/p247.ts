// @noLib: true

// relpin p247: comparable source="{ a: number }" target="{ a: number, b: string }"
declare var s: { a: number };
var t = s as { a: number, b: string };
