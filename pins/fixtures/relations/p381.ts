// @noLib: true

// relpin p381: assignable source="[string, number]" target="readonly string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: [string, number];
var t: readonly string[] = s;
