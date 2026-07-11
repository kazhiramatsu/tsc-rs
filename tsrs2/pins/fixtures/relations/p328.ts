// @noLib: true

// relpin p328: assignable source="D" target="B"
declare class B { protected x: number }
declare class D { protected x: number }
declare var s: D;
var t: B = s;
