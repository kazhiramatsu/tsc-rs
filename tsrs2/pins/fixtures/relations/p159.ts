// @noLib: true

// relpin p159: assignable source="B" target="A"
interface A { next: B }
interface B { next: A }
declare var s: B;
var t: A = s;
