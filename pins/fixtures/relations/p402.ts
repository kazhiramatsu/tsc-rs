// @noLib: true

// relpin p402: assignable source="(...args: [string, number]) => void" target="(a: string, b: string) => void"
interface Array<T> { length: number }
declare var s: (...args: [string, number]) => void;
var t: (a: string, b: string) => void = s;
