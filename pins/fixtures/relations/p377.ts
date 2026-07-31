// @noLib: true

// relpin p377: assignable source="string[]" target="number[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: string[];
var t: number[] = s;
