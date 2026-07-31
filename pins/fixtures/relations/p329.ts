// @noLib: true

// relpin p329: assignable source="{ x: number }" target="B"
declare class B { protected x: number }
declare var s: { x: number };
var t: B = s;
