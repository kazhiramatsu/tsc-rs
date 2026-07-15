// @noLib: true

// relpin p415: comparable source="`a${string}` & { x: 1 }" target="number"
declare var s: `a${string}` & { x: 1 };
var t = s as number;
