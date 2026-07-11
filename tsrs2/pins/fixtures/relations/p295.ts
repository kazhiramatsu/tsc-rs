// @noLib: true

// relpin p295: assignable source="J" target="A"
interface A { self: this; a: number }
interface J extends A { }
declare var s: J;
var t: A = s;
