// @noLib: true

// relpin p296: assignable source="A" target="J"
interface A { self: this; a: number }
interface J extends A { }
declare var s: A;
var t: J = s;
