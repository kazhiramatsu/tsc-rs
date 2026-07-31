// @noLib: true

// relpin p261: assignable source="{ a: string, b: string }" target="({ a: string } & { b: string }) | { c: string }"
var t: ({ a: string } & { b: string }) | { c: string } = { a: "x", b: "y" };
