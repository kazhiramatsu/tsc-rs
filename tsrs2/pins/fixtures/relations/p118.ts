// @noLib: true
// @strictFunctionTypes: true

// relpin p118: assignable source="(x: number) => void" target="(x: 1) => void"
declare var s: (x: number) => void;
var t: (x: 1) => void = s;
