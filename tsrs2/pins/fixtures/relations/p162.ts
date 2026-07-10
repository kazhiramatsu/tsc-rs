// @noLib: true

// relpin p162: assignable source="D1" target="E1"
interface D1 { next: D2 }
interface D2 { next: D1; x: number }
interface E1 { next: E2 }
interface E2 { next: E1; x: string }
declare var s: D1;
var t: E1 = s;
