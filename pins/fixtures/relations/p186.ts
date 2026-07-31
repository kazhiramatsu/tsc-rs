// @noLib: true

// relpin p186: assignable source="{ a: number }" target="{ [k: string]: number }"
declare var s: { a: number };
var t: { [k: string]: number } = s;
