// @noLib: true

// relpin p410: assignable source="G59<[number]>" target="G59<[number | boolean]>"
interface G59<T extends unknown[]> { f(...args: [string, ...T]): void }
declare var s: G59<[number]>;
var t: G59<[number | boolean]> = s;
