// @noLib: true

// relpin p192: assignable source="{ [k: string]: number }" target="{ [k: number]: number }"
declare var s: { [k: string]: number };
var t: { [k: number]: number } = s;
