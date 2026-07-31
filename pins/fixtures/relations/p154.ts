// @noLib: true

// relpin p154: assignable source="[number, ...[string, boolean]]" target="[number, string, boolean]"
declare var s: [number, ...[string, boolean]];
var t: [number, string, boolean] = s;
