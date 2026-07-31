// @noLib: true

// relpin p109: assignable source="() => void" target="(x: number) => void"
declare var s: () => void;
var t: (x: number) => void = s;
