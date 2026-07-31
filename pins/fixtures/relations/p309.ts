// @noLib: true

// relpin p309: assignable source="(() => number) | (() => string)" target="() => number"
declare var s: (() => number) | (() => string);
var t: () => number = s;
