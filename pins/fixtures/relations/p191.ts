// @noLib: true

// relpin p191: assignable source="{ [k: string]: \"a\" }" target="{ [k: string]: string }"
declare var s: { [k: string]: "a" };
var t: { [k: string]: string } = s;
