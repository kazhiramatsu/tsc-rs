// @noLib: true

// relpin p319: assignable source="C" target="B"
declare class B { b: string }
declare class C extends B { c: number }
declare var s: C;
var t: B = s;
