// @noLib: true

// relpin p172: assignable source="\"abc\"" target="`a${string}`"
declare var s: "abc";
var t: `a${string}` = s;
