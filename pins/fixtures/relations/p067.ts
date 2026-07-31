// @noLib: true

// relpin p067: assignable source="{ a: number } & { b: string }" target="{ a: number } | { b: string }"
declare var s: { a: number } & { b: string };
var t: { a: number } | { b: string } = s;
