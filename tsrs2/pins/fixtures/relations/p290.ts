// @noLib: true

// relpin p290: assignable source="\"abc\"" target="{ length: number }"
interface String { length: number }
declare var s: "abc";
var t: { length: number } = s;
