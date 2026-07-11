// @noLib: true

// relpin p379: assignable source="readonly string[]" target="string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: readonly string[];
var t: string[] = s;
