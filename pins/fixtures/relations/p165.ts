// @noLib: true
// @strictNullChecks: true

// relpin p165: assignable source="L2" target="M2"
interface L2 { v: number; next?: L2 }
interface M2 { v: number; next?: M2 }
declare var s: L2;
var t: M2 = s;
