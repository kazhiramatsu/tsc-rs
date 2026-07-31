// @noLib: true
// @strictFunctionTypes: true

// relpin p125: assignable source="{ m(x: 1): void }" target="{ m(x: number): void }"
declare var s: { m(x: 1): void };
var t: { m(x: number): void } = s;
