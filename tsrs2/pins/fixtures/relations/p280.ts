// @noLib: true

// relpin p280: assignable source="I<\"x\">" target="I<string>"
interface I<T> { a: T }
declare var s: I<"x">;
var t: I<string> = s;
