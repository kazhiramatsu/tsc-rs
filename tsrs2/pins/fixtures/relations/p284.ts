// @noLib: true

// relpin p284: assignable source="B<number>" target="A<number>"
interface A<T> { a: T }
interface B<U> extends A<U> { b: U }
declare var s: B<number>;
var t: A<number> = s;
