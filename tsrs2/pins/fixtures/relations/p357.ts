// @noLib: true

// relpin p357: assignable source="Cell<\"a\">" target="Cell<string>"
interface Cell<T> { f: (x: T) => T }
declare var s: Cell<"a">;
var t: Cell<string> = s;
