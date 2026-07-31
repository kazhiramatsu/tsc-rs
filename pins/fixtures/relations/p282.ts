// @noLib: true

// relpin p282: assignable source="B" target="A"
interface A { a: string }
interface B extends A { b: number }
declare var s: B;
var t: A = s;
