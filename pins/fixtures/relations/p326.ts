// @noLib: true

// relpin p326: assignable source="C<number>" target="{ b: number, c: number }"
declare class B<T> { b: T }
declare class C<U> extends B<U> { c: U }
declare var s: C<number>;
var t: { b: number, c: number } = s;
