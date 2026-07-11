// @noLib: true

// relpin p386: assignable source="\"ABC\"" target="Uppercase<string>"
type Uppercase<S extends string> = intrinsic
declare var s: "ABC";
var t: Uppercase<string> = s;
