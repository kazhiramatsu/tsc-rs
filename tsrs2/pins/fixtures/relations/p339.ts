// @noLib: true

// relpin p339: assignable source="F.A" target="E"
enum E { A, B }
enum F { A, B }
declare var s: F.A;
var t: E = s;
