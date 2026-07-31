// @noLib: true

// relpin p356: assignable source="Sink<\"a\">" target="Sink<string>"
interface Sink<T> { f: (x: T) => void }
declare var s: Sink<"a">;
var t: Sink<string> = s;
