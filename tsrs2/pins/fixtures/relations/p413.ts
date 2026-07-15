// @noLib: true

// relpin p413: assignable source="H59<string[]>" target="H59<[string, string]>"
interface H59<T extends string[]> { f(...args: [...T]): void }
declare var s: H59<string[]>;
var t: H59<[string, string]> = s;
