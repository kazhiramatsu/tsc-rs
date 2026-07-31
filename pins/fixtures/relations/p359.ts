// @noLib: true

// relpin p359: assignable source="Meth<string>" target="Meth<\"a\">"
interface Meth<T> { f(x: T): void }
declare var s: Meth<string>;
var t: Meth<"a"> = s;
