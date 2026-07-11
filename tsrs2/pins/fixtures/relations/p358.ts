// @noLib: true

// relpin p358: assignable source="Cell<string>" target="Cell<\"a\">"
interface Cell<T> { f: (x: T) => T }
declare var s: Cell<string>;
var t: Cell<"a"> = s;
