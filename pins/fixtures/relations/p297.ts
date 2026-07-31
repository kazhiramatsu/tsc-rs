// @noLib: true

// relpin p297: assignable source="A[1]" target="number"
type A = [string, number];
declare var s: A[1];
var t: number = s;
