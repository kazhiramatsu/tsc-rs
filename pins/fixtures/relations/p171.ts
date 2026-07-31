// @noLib: true
// @strictNullChecks: true

// relpin p171: assignable source="H" target="G"
interface G { next: G }
interface H { next?: H }
declare var s: H;
var t: G = s;
