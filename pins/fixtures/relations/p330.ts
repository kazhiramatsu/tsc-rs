// @noLib: true

// relpin p330: assignable source="B" target="{ x: number }"
declare class B { protected x: number }
declare var s: B;
var t: { x: number } = s;
