// @noLib: true

// relpin p399: assignable source="(a: string, b: string) => void" target="(...args: string[]) => void"
interface Array<T> { length: number }
declare var s: (a: string, b: string) => void;
var t: (...args: string[]) => void = s;
