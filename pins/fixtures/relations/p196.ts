// @noLib: true

// relpin p196: assignable source="{ a: number }" target="{ readonly [k: string]: number }"
declare var s: { a: number };
var t: { readonly [k: string]: number } = s;
