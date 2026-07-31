// @noLib: true

// relpin p276: assignable source="A" target="B"
type A = [number, A];
type B = [string, B];
declare var s: A;
var t: B = s;
