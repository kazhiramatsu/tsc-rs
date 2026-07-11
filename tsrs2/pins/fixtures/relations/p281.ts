// @noLib: true

// relpin p281: assignable source="I<string>" target="I<\"x\">"
interface I<T> { a: T }
declare var s: I<string>;
var t: I<"x"> = s;
