// @noLib: true

// relpin p374: assignable source="Deep<string>" target="Deep2<string>"
interface Deep<T> { next: Deep<Deep<T>> }
interface Deep2<T> { next: Deep2<Deep2<T>> }
declare var s: Deep<string>;
var t: Deep2<string> = s;
