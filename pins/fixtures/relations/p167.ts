// @noLib: true

// relpin p167: assignable source="X" target="P"
interface P { p: Q }
interface Q { p: R }
interface R { p: P }
interface X { p: X }
declare var s: X;
var t: P = s;
