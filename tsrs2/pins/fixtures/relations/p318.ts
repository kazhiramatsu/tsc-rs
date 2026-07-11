// @noLib: true

// relpin p318: assignable source="{ b: string, c: number }" target="C"
declare class B { b: string }
declare class C extends B { c: number }
declare var s: { b: string, c: number };
var t: C = s;
