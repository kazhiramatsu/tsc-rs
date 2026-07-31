// @noLib: true
// @strictFunctionTypes: false

// relpin p120: assignable source="(x: number) => void" target="(x: 1) => void"
declare var s: (x: number) => void;
var t: (x: 1) => void = s;
