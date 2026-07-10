// @noLib: true

// relpin p263: assignable source="({ a: string } | { b: number }) & { c: boolean }" target="{ a: string }"
declare var s: ({ a: string } | { b: number }) & { c: boolean };
var t: { a: string } = s;
