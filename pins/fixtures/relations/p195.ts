// @noLib: true

// relpin p195: assignable source="{ a: string }" target="{ [k: number]: string }"
declare var s: { a: string };
var t: { [k: number]: string } = s;
