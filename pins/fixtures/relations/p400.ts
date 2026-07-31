// @noLib: true

// relpin p400: assignable source="(...args: [string, ...number[]]) => void" target="(a: string, ...rest: number[]) => void"
interface Array<T> { length: number }
declare var s: (...args: [string, ...number[]]) => void;
var t: (a: string, ...rest: number[]) => void = s;
