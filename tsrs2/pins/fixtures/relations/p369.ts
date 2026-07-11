// @noLib: true

// relpin p369: assignable source="AliasSink<string>" target="AliasSink<\"a\">"
type AliasSink<T> = { f: (x: T) => void }
declare var s: AliasSink<string>;
var t: AliasSink<"a"> = s;
