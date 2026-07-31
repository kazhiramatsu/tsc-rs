// @noLib: true

// relpin p373: assignable source="Deep<string>" target="Deep<number>"
interface Deep<T> { next: Deep<Deep<T>> }
declare var s: Deep<string>;
var t: Deep<number> = s;
