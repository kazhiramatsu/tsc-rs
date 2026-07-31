// @noLib: true

// relpin p387: assignable source="\"abc\"" target="Uppercase<string>"
type Uppercase<S extends string> = intrinsic
declare var s: "abc";
var t: Uppercase<string> = s;
