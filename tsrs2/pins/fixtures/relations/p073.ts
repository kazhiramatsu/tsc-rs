// @noLib: true

// relpin p073: assignable source="{ a: number } & unknown" target="{ a: number }"
declare var s: { a: number } & unknown;
var t: { a: number } = s;
