// @noLib: true

// relpin p396: assignable source="[string, number]" target="{ length: 3 }"
interface Array<T> { length: number }
declare var s: [string, number];
var t: { length: 3 } = s;
