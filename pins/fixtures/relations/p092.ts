// @noLib: true

// relpin p092: assignable source="{ a: number }" target="{ a: number | string }"
declare var s: { a: number };
var t: { a: number | string } = s;
