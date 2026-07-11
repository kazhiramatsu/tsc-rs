// @noLib: true

// relpin p360: assignable source="Meth<\"a\">" target="Meth<string>"
interface Meth<T> { f(x: T): void }
declare var s: Meth<"a">;
var t: Meth<string> = s;
