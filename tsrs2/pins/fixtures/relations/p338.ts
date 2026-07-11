// @noLib: true

// relpin p338: assignable source="E" target="F"
enum E { A, B }
enum F { A, B }
declare var s: E;
var t: F = s;
