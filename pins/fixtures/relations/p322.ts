// @noLib: true

// relpin p322: assignable source="Box<string>" target="Box<\"x\">"
declare class Box<T> { value: T }
declare var s: Box<string>;
var t: Box<"x"> = s;
