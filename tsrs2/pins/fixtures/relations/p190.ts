// @noLib: true

// relpin p190: assignable source="{ [k: string]: number }" target="{ [k: string]: string }"
declare var s: { [k: string]: number };
var t: { [k: string]: string } = s;
