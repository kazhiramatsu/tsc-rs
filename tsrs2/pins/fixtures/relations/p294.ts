// @noLib: true

// relpin p294: assignable source="C & { x: number }" target="{ self: C, x: number }"
interface C { self: this }
declare var s: C & { x: number };
var t: { self: C, x: number } = s;
