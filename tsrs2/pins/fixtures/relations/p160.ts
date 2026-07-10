// @noLib: true

// relpin p160: assignable source="A" target="B"
interface A { next: B; tag: number }
interface B { next: A; tag: number }
declare var s: A;
var t: B = s;
