// @noLib: true

// relpin p166: assignable source="P" target="X"
interface P { p: Q }
interface Q { p: R }
interface R { p: P }
interface X { p: X }
declare var s: P;
var t: X = s;
