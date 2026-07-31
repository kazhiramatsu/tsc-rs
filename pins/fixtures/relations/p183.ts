// @noLib: true

// relpin p183: assignable source="\"a1\" | \"a2\"" target="`a${number}`"
declare var s: "a1" | "a2";
var t: `a${number}` = s;
