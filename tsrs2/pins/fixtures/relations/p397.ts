// @noLib: true

// relpin p397: assignable source="{ 0: string, 1: number, length: 2 }" target="[string, number]"
interface Array<T> { length: number }
declare var s: { 0: string, 1: number, length: 2 };
var t: [string, number] = s;
