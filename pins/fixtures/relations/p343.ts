// @noLib: true

// relpin p343: assignable source="S" target="string"
enum S { X = "x", Y = "y" }
declare var s: S;
var t: string = s;
