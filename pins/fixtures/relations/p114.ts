// @noLib: true
// @strictNullChecks: true

// relpin p114: assignable source="() => undefined" target="() => void"
declare var s: () => undefined;
var t: () => void = s;
