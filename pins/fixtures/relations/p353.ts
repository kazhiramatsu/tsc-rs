// @noLib: true

// relpin p353: assignable source="Box<\"a\">" target="Box<string>"
interface Box<T> { x: T }
declare var s: Box<"a">;
var t: Box<string> = s;
