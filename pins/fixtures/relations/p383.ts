// @noLib: true

// relpin p383: assignable source="Uppercase<\"abc\">" target="\"ABC\""
type Uppercase<S extends string> = intrinsic
declare var s: Uppercase<"abc">;
var t: "ABC" = s;
