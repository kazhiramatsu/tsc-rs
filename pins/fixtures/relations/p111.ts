// @noLib: true

// relpin p111: assignable source="(a: number, b: string) => void" target="(a: number) => void"
declare var s: (a: number, b: string) => void;
var t: (a: number) => void = s;
