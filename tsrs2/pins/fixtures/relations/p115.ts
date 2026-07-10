// @noLib: true
// @strictNullChecks: true

// relpin p115: assignable source="() => void" target="() => undefined"
declare var s: () => void;
var t: () => undefined = s;
