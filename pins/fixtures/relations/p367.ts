// @noLib: true

// relpin p367: assignable source="AliasBox<\"a\">" target="AliasBox<string>"
type AliasBox<T> = { x: T }
declare var s: AliasBox<"a">;
var t: AliasBox<string> = s;
