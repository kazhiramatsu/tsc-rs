// @noLib: true

// relpin p093: assignable source="{ a: number | string }" target="{ a: number }"
declare var s: { a: number | string };
var t: { a: number } = s;
