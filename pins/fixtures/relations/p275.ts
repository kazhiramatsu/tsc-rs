// @noLib: true

// relpin p275: assignable source="A" target="B"
type A = [number, A];
type B = [number, B];
declare var s: A;
var t: B = s;
