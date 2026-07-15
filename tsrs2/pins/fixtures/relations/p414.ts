// @noLib: true

// relpin p414: comparable source="`a${string}` & { x: 1 }" target="\"abc\""
declare var s: `a${string}` & { x: 1 };
var t = s as "abc";
