// @noLib: true

// relpin p345: assignable source="S.X" target="\"x\""
enum S { X = "x", Y = "y" }
declare var s: S.X;
var t: "x" = s;
