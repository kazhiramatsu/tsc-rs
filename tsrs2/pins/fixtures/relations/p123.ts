// @noLib: true
// @strictFunctionTypes: true

// relpin p123: assignable source="(x: { a: number, b: string }) => void" target="(x: { a: number }) => void"
declare var s: (x: { a: number, b: string }) => void;
var t: (x: { a: number }) => void = s;
