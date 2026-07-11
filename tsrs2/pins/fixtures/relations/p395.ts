// @noLib: true

// relpin p395: assignable source="[string, number]" target="{ length: 2 }"
interface Array<T> { length: number }
declare var s: [string, number];
var t: { length: 2 } = s;
