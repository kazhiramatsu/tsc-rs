// @noLib: true

// relpin p325: assignable source="A" target="{ x: number }"
declare class A { get x(): number; set x(value: number); }
declare var s: A;
var t: { x: number } = s;
