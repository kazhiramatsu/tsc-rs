// @noLib: true

// relpin p274: assignable source="A" target="[A]"
type A = [A];
declare var s: A;
var t: [A] = s;
