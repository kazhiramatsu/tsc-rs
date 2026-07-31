// @noLib: true

// relpin p287: assignable source="A<string>" target="B<string>"
interface A<T> { next: B<T> }
interface B<T> { next: A<T> }
declare var s: A<string>;
var t: B<string> = s;
