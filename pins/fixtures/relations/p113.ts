// @noLib: true

// relpin p113: assignable source="(a: number) => void" target="(a?: number) => void"
declare var s: (a: number) => void;
var t: (a?: number) => void = s;
