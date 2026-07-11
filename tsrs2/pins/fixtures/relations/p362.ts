// @noLib: true

// relpin p362: assignable source="O2<\"a\">" target="O2<string>"
interface O2<out T> { x: T }
declare var s: O2<"a">;
var t: O2<string> = s;
