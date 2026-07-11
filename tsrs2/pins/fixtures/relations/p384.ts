// @noLib: true

// relpin p384: assignable source="Uppercase<S2>" target="string"
type Uppercase<S extends string> = intrinsic
type S2 = string
declare var s: Uppercase<S2>;
var t: string = s;
