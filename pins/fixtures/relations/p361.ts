// @noLib: true

// relpin p361: assignable source="Unused<\"a\">" target="Unused<string>"
interface Unused<T> { x: number }
declare var s: Unused<"a">;
var t: Unused<string> = s;
