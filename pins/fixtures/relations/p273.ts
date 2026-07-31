// @noLib: true

// relpin p273: assignable source="A" target="[number]"
type A = [string];
declare var s: A;
var t: [number] = s;
