// @noLib: true

// relpin p312: assignable source="{ [k: string]: number } | { [k: string]: string }" target="{ [k: string]: number | string }"
declare var s: { [k: string]: number } | { [k: string]: string };
var t: { [k: string]: number | string } = s;
