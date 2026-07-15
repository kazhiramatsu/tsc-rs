// @noLib: true

// relpin p412: assignable source="H59<[string, string]>" target="H59<string[]>"
interface H59<T extends string[]> { f(...args: [...T]): void }
declare var s: H59<[string, string]>;
var t: H59<string[]> = s;
