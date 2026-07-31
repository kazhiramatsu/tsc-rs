// @noLib: true
// @strictFunctionTypes: true

// relpin p126: assignable source="{ m(x: number): void }" target="{ m(x: 1): void }"
declare var s: { m(x: number): void };
var t: { m(x: 1): void } = s;
