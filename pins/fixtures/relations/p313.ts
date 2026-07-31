// @noLib: true

// relpin p313: assignable source="{ [k: string]: number } | { [k: number]: number }" target="{ [k: string]: number }"
declare var s: { [k: string]: number } | { [k: number]: number };
var t: { [k: string]: number } = s;
