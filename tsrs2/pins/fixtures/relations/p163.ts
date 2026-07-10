// @noLib: true

// relpin p163: assignable source="L" target="M"
interface L { v: number; next: L }
interface M { v: number; next: M }
declare var s: L;
var t: M = s;
