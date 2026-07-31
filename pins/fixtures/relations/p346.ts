// @noLib: true

// relpin p346: assignable source="\"x\"" target="S.X"
enum S { X = "x", Y = "y" }
declare var s: "x";
var t: S.X = s;
