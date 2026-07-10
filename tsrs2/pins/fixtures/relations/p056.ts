// @noLib: true

// relpin p056: assignable source="{ a: number } | { b: string }" target="{ a: number }"
declare var s: { a: number } | { b: string };
var t: { a: number } = s;
