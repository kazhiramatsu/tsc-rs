// @noLib: true

// relpin p108: assignable source="(x: number) => void" target="() => void"
declare var s: (x: number) => void;
var t: () => void = s;
