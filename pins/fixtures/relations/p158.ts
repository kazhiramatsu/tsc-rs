// @noLib: true

// relpin p158: assignable source="A" target="B"
interface A { next: B }
interface B { next: A }
declare var s: A;
var t: B = s;
