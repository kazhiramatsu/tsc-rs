// @noLib: true

// relpin p380: assignable source="[string, string]" target="string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: [string, string];
var t: string[] = s;
