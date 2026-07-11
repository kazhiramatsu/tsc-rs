// @noLib: true

// relpin p315: assignable source="(a: string) => void" target="((a: number) => void) & ((a: string) => void)"
declare var s: (a: string) => void;
var t: ((a: number) => void) & ((a: string) => void) = s;
