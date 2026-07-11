// @noLib: true

// relpin p394: assignable source="[string, number]" target="{ 0: string, 1: boolean }"
interface Array<T> { length: number }
declare var s: [string, number];
var t: { 0: string, 1: boolean } = s;
