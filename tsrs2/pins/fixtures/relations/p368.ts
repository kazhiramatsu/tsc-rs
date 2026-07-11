// @noLib: true

// relpin p368: assignable source="AliasBox<string>" target="AliasBox<\"a\">"
type AliasBox<T> = { x: T }
declare var s: AliasBox<string>;
var t: AliasBox<"a"> = s;
