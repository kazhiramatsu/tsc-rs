// @noLib: true

// relpin p188: assignable source="{ a: number, b: string }" target="{ [k: string]: number | string }"
declare var s: { a: number, b: string };
var t: { [k: string]: number | string } = s;
