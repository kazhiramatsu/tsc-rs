// @noLib: true

// relpin p403: assignable source="(...args: [a: string, b: number]) => void" target="(x: string, y: number) => void"
interface Array<T> { length: number }
declare var s: (...args: [a: string, b: number]) => void;
var t: (x: string, y: number) => void = s;
