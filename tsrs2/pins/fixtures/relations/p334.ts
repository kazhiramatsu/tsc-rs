// @noLib: true

// relpin p334: assignable source="E" target="E.A"
enum E { A, B }
declare var s: E;
var t: E.A = s;
