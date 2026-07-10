// @noLib: true

// relpin p182: assignable source="`a${string}`" target="`a${string}b`"
declare var s: `a${string}`;
var t: `a${string}b` = s;
