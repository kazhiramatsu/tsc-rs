// @noLib: true

// relpin p262: assignable source="({ a: string } | { b: number }) & { c: boolean }" target="{ c: boolean }"
declare var s: ({ a: string } | { b: number }) & { c: boolean };
var t: { c: boolean } = s;
