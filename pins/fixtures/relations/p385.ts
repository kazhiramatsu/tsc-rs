// @noLib: true

// relpin p385: assignable source="string" target="Uppercase<string>"
type Uppercase<S extends string> = intrinsic
declare var s: string;
var t: Uppercase<string> = s;
