// @noLib: true

// relpin p291: assignable source="\"abc\"" target="{ length: string }"
interface String { length: number }
declare var s: "abc";
var t: { length: string } = s;
