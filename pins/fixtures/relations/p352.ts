// @noLib: true

// relpin p352: assignable source="E.B" target="4"
const x = 3
enum E { A = x, B }
declare var s: E.B;
var t: 4 = s;
