// @noLib: true

// relpin p365: assignable source="OutOnly<string>" target="OutOnly<\"a\">"
interface OutOnly<out T> { f: (x: T) => void }
declare var s: OutOnly<string>;
var t: OutOnly<"a"> = s;
