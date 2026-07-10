// @noLib: true

// relpin p268: assignable source="{ q: number }" target="({ kind: \"a\" } & { kind: \"b\" }) | { q: number }"
declare var s: { q: number };
var t: ({ kind: "a" } & { kind: "b" }) | { q: number } = s;
