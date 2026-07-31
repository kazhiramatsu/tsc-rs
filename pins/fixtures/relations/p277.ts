// @noLib: true

// relpin p277: assignable source="A" target="[number, string]"
type B = [number];
type A = [...B, string];
declare var s: A;
var t: [number, string] = s;
