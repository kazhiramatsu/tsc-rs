// @noLib: true

// relpin p088: assignable source="{ readonly a: number }" target="{ a: number }"
declare var s: { readonly a: number };
var t: { a: number } = s;
