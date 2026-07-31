// @noLib: true

// relpin p388: assignable source="Uppercase<string>" target="Lowercase<string>"
type Uppercase<S extends string> = intrinsic
type Lowercase<S extends string> = intrinsic
declare var s: Uppercase<string>;
var t: Lowercase<string> = s;
