// @noLib: true

// relpin p058: assignable source="{ a: number | string }" target="{ a: number } | { a: string }"
declare var s: { a: number | string };
var t: { a: number } | { a: string } = s;
