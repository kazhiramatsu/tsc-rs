// @noLib: true

// relpin p378: assignable source="string[]" target="readonly string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: string[];
var t: readonly string[] = s;
