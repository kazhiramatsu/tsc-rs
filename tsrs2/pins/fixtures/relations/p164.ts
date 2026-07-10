// @noLib: true

// relpin p164: assignable source="L" target="N"
interface L { v: number; next: L }
interface N { v: string; next: N }
declare var s: L;
var t: N = s;
