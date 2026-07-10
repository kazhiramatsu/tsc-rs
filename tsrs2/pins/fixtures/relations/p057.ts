// @noLib: true

// relpin p057: assignable source="{ a: number } | { a: string }" target="{ a: number | string }"
declare var s: { a: number } | { a: string };
var t: { a: number | string } = s;
