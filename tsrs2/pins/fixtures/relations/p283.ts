// @noLib: true

// relpin p283: assignable source="A" target="B"
interface A { a: string }
interface B extends A { b: number }
declare var s: A;
var t: B = s;
