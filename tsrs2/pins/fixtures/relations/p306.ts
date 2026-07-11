// @noLib: true

// relpin p306: assignable source="(...args: [number, ...string[]]) => void" target="(a: number, b: string, c: string) => void"
interface Array<T> { length: number }
declare var s: (...args: [number, ...string[]]) => void;
var t: (a: number, b: string, c: string) => void = s;
