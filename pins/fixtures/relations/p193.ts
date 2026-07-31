// @noLib: true

// relpin p193: assignable source="{ [k: number]: number }" target="{ [k: string]: number }"
declare var s: { [k: number]: number };
var t: { [k: string]: number } = s;
