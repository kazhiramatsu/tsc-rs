// @noLib: true

// relpin p317: assignable source="C" target="{ b: string, c: number }"
declare class B { b: string }
declare class C extends B { c: number }
declare var s: C;
var t: { b: string, c: number } = s;
