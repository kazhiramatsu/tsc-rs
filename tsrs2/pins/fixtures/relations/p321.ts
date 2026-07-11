// @noLib: true

// relpin p321: assignable source="Box<\"x\">" target="Box<string>"
declare class Box<T> { value: T }
declare var s: Box<"x">;
var t: Box<string> = s;
