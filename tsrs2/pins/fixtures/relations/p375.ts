// @noLib: true

// relpin p375: assignable source="string[]" target="string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: string[];
var t: string[] = s;
