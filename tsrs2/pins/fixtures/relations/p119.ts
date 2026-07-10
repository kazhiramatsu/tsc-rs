// @noLib: true
// @strictFunctionTypes: true

// relpin p119: assignable source="(x: 1) => void" target="(x: number) => void"
declare var s: (x: 1) => void;
var t: (x: number) => void = s;
