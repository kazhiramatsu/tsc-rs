// @noLib: true

// relpin p401: assignable source="(a: string, ...rest: number[]) => void" target="(...args: [string, ...number[]]) => void"
interface Array<T> { length: number }
declare var s: (a: string, ...rest: number[]) => void;
var t: (...args: [string, ...number[]]) => void = s;
