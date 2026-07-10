// @noLib: true
// @strictFunctionTypes: false

// relpin p121: assignable source="(x: 1) => void" target="(x: number) => void"
declare var s: (x: 1) => void;
var t: (x: number) => void = s;
