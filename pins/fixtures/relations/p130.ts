// @noLib: true

// relpin p130: assignable source="(x: number) => void" target="(x: any) => void"
declare var s: (x: number) => void;
var t: (x: any) => void = s;
