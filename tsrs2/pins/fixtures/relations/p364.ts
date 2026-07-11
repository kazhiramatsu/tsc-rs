// @noLib: true

// relpin p364: assignable source="IO2<\"a\">" target="IO2<string>"
interface IO2<in out T> { x: T }
declare var s: IO2<"a">;
var t: IO2<string> = s;
