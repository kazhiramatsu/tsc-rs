// @noLib: true

// relpin p278: assignable source="[number, string]" target="A"
type B = [number];
type A = [...B, string];
declare var s: [number, string];
var t: A = s;
