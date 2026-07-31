// @noLib: true

// relpin p320: assignable source="B" target="C"
declare class B { b: string }
declare class C extends B { c: number }
declare var s: B;
var t: C = s;
