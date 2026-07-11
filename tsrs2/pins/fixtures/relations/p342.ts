// @noLib: true

// relpin p342: assignable source="C" target="E"
const enum C { A }
enum E { A, B }
declare var s: C;
var t: E = s;
