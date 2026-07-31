// @noLib: true

// relpin p089: assignable source="{ a: number }" target="{ readonly a: number }"
declare var s: { a: number };
var t: { readonly a: number } = s;
