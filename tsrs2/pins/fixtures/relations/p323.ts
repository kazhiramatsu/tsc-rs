// @noLib: true

// relpin p323: assignable source="C" target="I"
declare class C { x: number }
interface I { x: number }
declare var s: C;
var t: I = s;
