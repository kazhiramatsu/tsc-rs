// @noLib: true

// relpin p264: assignable source="{ a: number, b: number }" target="({ a: string } | { b: number }) & { a: number }"
declare var s: { a: number, b: number };
var t: ({ a: string } | { b: number }) & { a: number } = s;
