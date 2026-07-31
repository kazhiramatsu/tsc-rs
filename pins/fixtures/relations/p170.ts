// @noLib: true
// @strictNullChecks: true

// relpin p170: assignable source="G" target="H"
interface G { next: G }
interface H { next?: H }
declare var s: G;
var t: H = s;
