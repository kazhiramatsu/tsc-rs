// @noLib: true

// relpin p307: assignable source="(...args: string[]) => void" target="(a: string) => void"
interface Array<T> { length: number }
declare var s: (...args: string[]) => void;
var t: (a: string) => void = s;
