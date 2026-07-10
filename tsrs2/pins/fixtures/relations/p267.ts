// @noLib: true

// relpin p267: assignable source="{ kind: \"a\" } & { kind: \"b\" }" target="{ q: number }"
declare var s: { kind: "a" } & { kind: "b" };
var t: { q: number } = s;
