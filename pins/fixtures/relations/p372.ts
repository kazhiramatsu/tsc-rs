// @noLib: true

// relpin p372: assignable source="Rec<string>" target="Rec<number>"
interface Rec<T> { next: Rec<T> }
declare var s: Rec<string>;
var t: Rec<number> = s;
