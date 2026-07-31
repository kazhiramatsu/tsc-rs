// @noLib: true

// relpin p071: assignable source="(\"a\" | \"b\") & string" target="\"a\" | \"b\""
declare var s: ("a" | "b") & string;
var t: "a" | "b" = s;
