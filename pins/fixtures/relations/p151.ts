// @noLib: true

// relpin p151: assignable source="[number, string, string]" target="[number, ...string[]]"
declare var s: [number, string, string];
var t: [number, ...string[]] = s;
