// @noLib: true

// relpin p087: assignable source="{ a: { b: string } }" target="{ a: { b: number } }"
declare var s: { a: { b: string } };
var t: { a: { b: number } } = s;
