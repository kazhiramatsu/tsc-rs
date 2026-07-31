// @noLib: true
// @strictFunctionTypes: true

// relpin p122: assignable source="(x: { a: number }) => void" target="(x: { a: number, b: string }) => void"
declare var s: (x: { a: number }) => void;
var t: (x: { a: number, b: string }) => void = s;
