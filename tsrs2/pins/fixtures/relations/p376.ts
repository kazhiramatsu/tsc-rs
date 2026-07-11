// @noLib: true

// relpin p376: assignable source="\"a\"[]" target="string[]"
interface Array<T> { length: number }
interface ReadonlyArray<T> { length: number }
declare var s: "a"[];
var t: string[] = s;
