// @noLib: true

// relpin p285: assignable source="B<number>" target="A<string>"
interface A<T> { a: T }
interface B<U> extends A<U> { b: U }
declare var s: B<number>;
var t: A<string> = s;
