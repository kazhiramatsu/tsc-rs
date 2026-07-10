// @noLib: true

// relpin p265: assignable source="{ kind: \"a\", x: number, y: number }" target="{ kind: \"a\", x: number } | { kind: \"b\", y: number }"
var t: { kind: "a", x: number } | { kind: "b", y: number } = { kind: "a", x: 1, y: 2 };
