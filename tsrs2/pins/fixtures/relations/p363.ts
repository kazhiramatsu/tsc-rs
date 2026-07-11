// @noLib: true

// relpin p363: assignable source="I2<string>" target="I2<\"a\">"
interface I2<in T> { f: (x: T) => void }
declare var s: I2<string>;
var t: I2<"a"> = s;
