// @noLib: true

// relpin p327: assignable source="C" target="B"
declare class B { protected x: number }
declare class C extends B { }
declare var s: C;
var t: B = s;
