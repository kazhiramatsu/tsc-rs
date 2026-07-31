// @noLib: true

// relpin p288: assignable source="A<string>" target="B<number>"
interface A<T> { next: B<T> }
interface B<T> { next: A<T> }
declare var s: A<string>;
var t: B<number> = s;
