// @noLib: true

// relpin p289: assignable source="C" target="{ self: C, tag: string }"
interface C { self: this; tag: string }
declare var s: C;
var t: { self: C, tag: string } = s;
