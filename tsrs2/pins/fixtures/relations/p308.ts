// @noLib: true

// relpin p308: assignable source="(() => number) | (() => string)" target="() => number | string"
declare var s: (() => number) | (() => string);
var t: () => number | string = s;
