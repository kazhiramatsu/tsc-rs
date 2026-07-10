// @noLib: true

// relpin p197: assignable source="{ readonly [k: string]: number }" target="{ [k: string]: number }"
declare var s: { readonly [k: string]: number };
var t: { [k: string]: number } = s;
