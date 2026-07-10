// @noLib: true

// relpin p199: assignable source="{ [k: string]: number, a: number }" target="{ [k: string]: number }"
declare var s: { [k: string]: number, a: number };
var t: { [k: string]: number } = s;
