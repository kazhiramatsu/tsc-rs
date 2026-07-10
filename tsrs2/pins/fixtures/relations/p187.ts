// @noLib: true

// relpin p187: assignable source="{ a: string }" target="{ [k: string]: number }"
declare var s: { a: string };
var t: { [k: string]: number } = s;
