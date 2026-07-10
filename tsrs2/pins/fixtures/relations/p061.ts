// @noLib: true

// relpin p061: assignable source="{ k: \"a\" | \"b\", v: number }" target="{ k: \"a\", v: number } | { k: \"b\", v: number }"
declare var s: { k: "a" | "b", v: number };
var t: { k: "a", v: number } | { k: "b", v: number } = s;
