// @noLib: true

// relpin p351: comparable source="E" target="F"
enum E { A, B }
enum F { A, B }
declare var s: E;
var t = s as F;
