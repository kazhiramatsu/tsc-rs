// @noLib: true

// relpin p398: assignable source="(...args: string[]) => void" target="(a: string, b: string) => void"
interface Array<T> { length: number }
declare var s: (...args: string[]) => void;
var t: (a: string, b: string) => void = s;
