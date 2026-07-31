// @noLib: true

// relpin p370: assignable source="A2<string>" target="B2<string>"
interface A2<T> { next: B2<T> }
interface B2<T> { next: A2<T> }
declare var s: A2<string>;
var t: B2<string> = s;
