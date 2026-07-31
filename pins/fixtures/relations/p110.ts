// @noLib: true

// relpin p110: assignable source="(a: number) => void" target="(a: number, b: string) => void"
declare var s: (a: number) => void;
var t: (a: number, b: string) => void = s;
