// @noLib: true

// relpin p303: assignable source="(...args: [number, string?]) => void" target="(a: number, b?: string) => void"
declare var s: (...args: [number, string?]) => void;
var t: (a: number, b?: string) => void = s;
