// @noLib: true

// relpin p355: assignable source="Sink<string>" target="Sink<\"a\">"
interface Sink<T> { f: (x: T) => void }
declare var s: Sink<string>;
var t: Sink<"a"> = s;
