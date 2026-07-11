// @noLib: true

// relpin p344: assignable source="string" target="S"
enum S { X = "x", Y = "y" }
declare var s: string;
var t: S = s;
