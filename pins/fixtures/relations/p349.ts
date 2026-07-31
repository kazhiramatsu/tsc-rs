// @noLib: true

// relpin p349: assignable source="E.C" target="14"
enum E { A = 3, B, C = (A | B) * 2 }
declare var s: E.C;
var t: 14 = s;
