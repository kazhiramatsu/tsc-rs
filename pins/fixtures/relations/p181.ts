// @noLib: true

// relpin p181: assignable source="`a${string}b`" target="`a${string}`"
declare var s: `a${string}b`;
var t: `a${string}` = s;
