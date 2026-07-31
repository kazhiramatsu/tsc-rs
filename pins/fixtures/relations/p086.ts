// @noLib: true

// relpin p086: assignable source="{ a: { b: number, c: string } }" target="{ a: { b: number } }"
declare var s: { a: { b: number, c: string } };
var t: { a: { b: number } } = s;
