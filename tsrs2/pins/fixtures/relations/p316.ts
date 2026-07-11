// @noLib: true

// relpin p316: assignable source="((...args: number[]) => void) | ((a: number) => void)" target="(a: number) => void"
interface Array<T> { length: number }
declare var s: ((...args: number[]) => void) | ((a: number) => void);
var t: (a: number) => void = s;
