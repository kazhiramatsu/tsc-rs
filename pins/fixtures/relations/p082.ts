// @noLib: true

// relpin p082: assignable source="{ a: number }" target="{ a?: number }"
declare var s: { a: number };
var t: { a?: number } = s;
