// @noLib: true

// relpin p184: assignable source="`a${number}`" target="`a${string}`"
declare var s: `a${number}`;
var t: `a${string}` = s;
