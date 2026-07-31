// @noLib: true

// relpin p161: assignable source="A" target="B"
interface A { next: B; x: number }
interface B { next: A; x: string }
declare var s: A;
var t: B = s;
