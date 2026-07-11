// @noLib: true

// relpin p304: assignable source="(a: number, b?: string) => void" target="(...args: [number, string?]) => void"
declare var s: (a: number, b?: string) => void;
var t: (...args: [number, string?]) => void = s;
