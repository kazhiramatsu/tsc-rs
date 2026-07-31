// @noLib: true
// @strictNullChecks: true

// relpin p080: assignable source="{ a?: number }" target="{ a: number }"
declare var s: { a?: number };
var t: { a: number } = s;
