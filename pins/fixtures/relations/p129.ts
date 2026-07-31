// @noLib: true

// relpin p129: assignable source="(x: any) => void" target="(x: number) => void"
declare var s: (x: any) => void;
var t: (x: number) => void = s;
