// @noLib: true

// relpin p411: assignable source="G59<[number | boolean]>" target="G59<[number]>"
interface G59<T extends unknown[]> { f(...args: [string, ...T]): void }
declare var s: G59<[number | boolean]>;
var t: G59<[number]> = s;
