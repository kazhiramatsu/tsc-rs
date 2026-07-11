// @noLib: true

// relpin p286: assignable source="A" target="Box<number>"
interface Box<T> { value: T }
type A = Box<number>;
declare var s: A;
var t: Box<number> = s;
