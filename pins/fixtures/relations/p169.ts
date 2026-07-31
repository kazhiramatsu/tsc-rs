// @noLib: true
// @strictNullChecks: true

// relpin p169: assignable source="J" target="K"
interface J { v: number; next: J | null }
interface K { v: number; next: K | null }
declare var s: J;
var t: K = s;
