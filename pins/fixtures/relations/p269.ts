// @noLib: true

// relpin p269: assignable source="\"é\"" target="`${string}${string}`"
declare var s: "é";
var t: `${string}${string}` = s;
