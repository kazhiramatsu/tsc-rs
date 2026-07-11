// @noLib: true

// relpin p298: assignable source="A[0]" target="number"
type A = [string, number];
declare var s: A[0];
var t: number = s;
