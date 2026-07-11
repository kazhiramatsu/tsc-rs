// @noLib: true

// relpin p354: assignable source="Box<string>" target="Box<\"a\">"
interface Box<T> { x: T }
declare var s: Box<string>;
var t: Box<"a"> = s;
