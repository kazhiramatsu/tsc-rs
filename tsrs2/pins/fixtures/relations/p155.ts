// @noLib: true

// relpin p155: assignable source="[number, string, boolean]" target="[number, ...[string, boolean]]"
declare var s: [number, string, boolean];
var t: [number, ...[string, boolean]] = s;
