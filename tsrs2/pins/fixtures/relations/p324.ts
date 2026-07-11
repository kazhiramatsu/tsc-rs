// @noLib: true

// relpin p324: assignable source="C" target="D"
declare class C { private x: number }
declare class D { private x: number }
declare var s: C;
var t: D = s;
